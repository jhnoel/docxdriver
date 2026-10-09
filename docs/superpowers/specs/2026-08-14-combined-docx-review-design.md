# Combined DOCX Review and Compact Commit Key

## Goal

Replace the two-step preview/page-review authoring flow with one `review(path, plan)` operation that validates a typed DOCX plan, returns bounded per-edit context, and issues a compact commit capability. Commit consumes only that capability.

## Current Problem

The Python host currently exposes:

1. `preview(path, state)`, which executes the plan and stores a full paginated report behind a 256-bit `r1:` receipt.
2. `review(receipt, page)`, which requires the agent to request every page in later executions.
3. `commit(receipt, state)`, which requires the agent to pass the plan state again.

The core already attaches affected markup to operation reports, but the host projects only operation summaries into review pages. The visible capability is longer than necessary for an in-process opaque handle.

## Public Python API

```python
review_result = review("contract.docx", plan)
# {
#   "commit_key": "k1:<32 lowercase hex characters>",
#   "edits": [
#     {
#       "index": 1,
#       "op": "replace_text",
#       "outcome": "applied",
#       "summary": "...",
#       "context": [
#         {"para_id": "2673269E", "markup": "<p ...>...</p>"}
#       ],
#       "context_truncated": false
#     }
#   ]
# }

commit(review_result["commit_key"])
```

The old public preview plus page-by-page review flow is removed from the default Python surface. A reviewed plan is immutable: changing the plan requires calling `review` again.

## Review Semantics

`review(path, state)` performs the existing preview-time safety work in one callback:

- Validate and digest the supplied plan state.
- Resolve and read the target under the preview working-directory boundary.
- Derive the core typed plan and canonicalize it.
- Execute the plan against an in-memory source.
- Require a successful, complete core report.
- Capture bounded per-operation context from the core report, preserving affected markup and adding nearby rendered blocks when available.
- Mint and retain a compact commit key only after all validation succeeds.

Review does not write the target. It returns the review data directly instead of creating paginated pages that must be delivered separately.

The existing commit-time protections remain mandatory: source/path revalidation, plan/core-key semantic checks, candidate rendering and report verification, atomic write, read-back verification, cancellation handling, and truthful consumption/retry behavior.

## Edit Context

The host must preserve operation metadata needed by an agent:

- one-based operation index;
- operation name;
- outcome;
- existing summary;
- affected paragraph IDs and rendered markup;
- bounded neighboring rendered blocks where the core can provide them;
- a truncation flag when the context budget is exceeded.

Context is returned in the review result and is not retained in the commit capability after the review response is produced. Per-operation and aggregate limits must prevent a large plan or document from defeating host output/resource bounds. Existing core `affected` data should be reused rather than reparsing DOCX markup in TypeScript.

## Commit Key and Record

Replace the externally visible random receipt with a `k1:` key containing 128 random bits encoded as 32 lowercase hexadecimal characters. The key is an opaque store lookup handle, not a source/plan digest and not the core preview key.

The retained record keeps only authorization and commit data:

- key and active/terminal state;
- runtime generation;
- preview working-directory root and canonical target path;
- source hash;
- canonical plan/core preview key needed to reproduce the reviewed candidate;
- issued execution number and any commit retry state.

Remove review-page arrays, page digests, delivered-page tracking, and review-completion state. The record becomes commit-ready immediately after successful review, subject to the existing later-execution gate.

`commit(key)` uses the stored reviewed plan. It no longer requires a duplicate plan-state argument. A key authorizes exactly the plan and target captured by its review.

## Error and Lifecycle Behavior

- Invalid plans and incomplete/rejected operations return a normal blocked review result and create no key.
- A malformed, unknown, expired, consumed, or wrong-generation key is rejected with stable diagnostics.
- Commit in the same Python execution as review remains blocked.
- Source, path, or stored-plan/core semantic mismatches consume the key as before.
- Infrastructure failure before the write keeps the key retryable.
- A completed write consumes the key and prevents replay.
- Reset, shutdown, generation changes, and eviction expire active keys.

## Testing

Update unit and integration tests to cover:

- one-call review returns a key and structured edit context;
- failed review returns no key;
- compact key format and no core-key leakage;
- context includes affected markup and the bounded context representation selected for the implementation;
- commit accepts only the returned key and uses the reviewed plan;
- same-execution commit rejection;
- source/path/plan semantic races and lifecycle invalidation;
- retry behavior around pre-write infrastructure failures;
- atomic write and read-back verification;
- resource limits for review context and retained key records;
- Python prelude/help/docs and live-surface ordering.

## Compatibility and Scope

This change targets the default experimental V1 Python-plan surface. The separate five-tool `docx_edit` API is unchanged. Historical V2/V3 surfaces are unchanged. Existing unrelated worktree modifications must not be overwritten or included in this change.
