# Combined DOCX Review Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the split preview/page-review Python flow with one validating `review(path, plan)` call that returns bounded edit context and a compact commit key consumed by `commit(key)`.

**Architecture:** Keep the core typed-plan execution and commit-time atomic safety checks. Move review presentation into the single host callback: project each core operation report into structured edit context, issue a short opaque key, and retain only the plan/path/source authorization needed for a later commit. Remove review-page state and make commit use the reviewed stored plan.

**Tech Stack:** TypeScript, Node.js, Monty Python REPL, Rust `docxdriver-core`, Node built-in test runner, npm build/test scripts.

## Global Constraints

- The default experimental V1 Python-plan surface changes; the separate five-tool `docx_edit` API and archived V2/V3 surfaces remain unchanged.
- The visible commit key is `k1:` plus 32 lowercase hexadecimal characters (128 random bits).
- Review must not write the target; commit must retain source/path revalidation, candidate verification, atomic write, read-back verification, cancellation, and lifecycle invalidation.
- Review context is bounded and must never expose the deterministic core preview key.
- Existing unrelated worktree modifications must not be overwritten or included in commits.

---

### Task 1: Replace the receipt protocol with combined review and compact commit-key host behavior

**Files:**
- Modify: `packages/docxdriver-pi/src/python-host.ts`
- Test: `packages/docxdriver-pi/test/python-host.test.mjs`

**Interfaces:**
- Produces private callbacks `_docx_review(path, state) -> { commit_key, edits } | null` and `_docx_commit(commit_key) -> null`.
- `PythonHostNotice` uses `review` for successful review and `commit` for commit outcomes; its capability field is `commitKey`/`key` consistently, never the core preview key.
- `ReceiptStore` becomes a compact `CommitKeyStore`; active records no longer contain review pages, page digests, delivered-page sets, or review-completion state.

- [ ] **Step 1: Add failing host tests for the new one-call contract.**

Add tests near the existing preview/review gate tests that:

```js
const result = await host.externalLookup._docx_review('demo.docx', planState([replaceTextOp(id)]));
assert.match(result.commit_key, /^k1:[0-9a-f]{32}$/);
assert.equal(result.edits.length, 1);
assert.equal(result.edits[0].index, 1);
assert.equal(result.edits[0].outcome, 'applied');
assert.match(JSON.stringify(result.edits[0].context), /new/);

await repl.execute(`commit("${result.commit_key}")`, host.externalLookup);
```

Also add assertions that a failed review returns `null` and creates no active key, that same-execution commit is rejected, and that a second commit reports consumed/unknown-key failure.

- [ ] **Step 2: Run the focused tests and verify they fail for the old API.**

Run:

```bash
cd packages/docxdriver-pi
npm test -- --test-name-pattern='combined review|compact commit|edit context'
```

Expected: FAIL because `_docx_review` currently accepts a receipt/page and `_docx_commit` currently requires a plan state.

- [ ] **Step 3: Introduce compact key types and remove page-review state.**

In `python-host.ts`:

- Replace `RECEIPT_ID_RE` with `COMMIT_KEY_RE = /^k1:[0-9a-f]{32}$/`.
- Change `newReceiptId()` to `newCommitKey()` using `randomBytes(16)`.
- Rename `ReceiptId`/`ReceiptRecord`/`ReceiptStore` identifiers to commit-key equivalents, preserving tombstone, generation, FIFO, and byte-cap behavior.
- Reduce the active record to key, state, generation, cwd root, canonical path, source hash, canonical plan, core preview key, issued execution, and retry/commit state.
- Remove `ReviewPage`, `buildReviewPages`, `reviewPageBytes`, `maxReviewPages`, page digests, `deliveredPages`, and review state transitions.
- Update byte accounting and store tests to account for exactly the retained fields.

- [ ] **Step 4: Project core operation reports into bounded edit context.**

Extend the host-side operation projection to preserve `affected` entries from the core report rather than only `(op, outcome, summary)`. Use a bounded shape:

```ts
export type ReviewEdit = {
  index: number;
  op: string;
  outcome: string;
  summary: string;
  context: Array<Record<string, unknown>>;
  context_truncated?: boolean;
};
```

Each context entry must preserve `para_id` and rendered `markup` when present. Limit per-summary/context bytes and total review response bytes using named host limits; set `context_truncated` when the limit removes data. Do not include core preview keys in the returned object or notices.

- [ ] **Step 5: Convert `_docx_preview` into `_docx_review(path, state)`.**

Move the existing source read, derivation, canonicalization, core `runPlan`, complete-report validation, and key-record creation into `_docx_review`. After `runPlan` succeeds:

- Build the bounded `edits` array from `output.report.ops`.
- Mint the key only after all validations and size checks pass.
- Store the reviewed canonical plan and private core preview key.
- Push one successful `review` notice containing the compact key and operation count.
- Return `{ commit_key: key, edits }`.
- On any planned validation rejection, push a blocked review notice and return `null` without storing a key.
- Preserve cancellation/deadline checkpoints before store insertion.

- [ ] **Step 6: Make `_docx_commit` accept only the compact key and use the stored plan.**

Change the callback signature to `_docx_commit(key)`. Keep the later-execution gate and all existing source/path/core-key/candidate/atomic-write checks. Read the stored canonical plan from the record, parse it into the core plan shape, and run commit validation against the stored core preview key. Remove current plan-state digest comparison because the key authorizes the immutable reviewed plan. Update blocked messages from receipt/preview/review terminology to commit-key/review terminology.

- [ ] **Step 7: Run focused host tests and repair regressions.**

Run:

```bash
cd packages/docxdriver-pi
npm test -- --test-name-pattern='python-host|combined review|commit key|edit context'
```

Expected: all updated host protocol, lifecycle, atomic-write, cancellation, race, and context tests pass.

- [ ] **Step 8: Commit the host protocol change.**

```bash
git add packages/docxdriver-pi/src/python-host.ts packages/docxdriver-pi/test/python-host.test.mjs
git commit -m "feat(pi): combine docx review and issue compact commit keys"
```

---

### Task 2: Update the Python prelude, stubs, prompt guidance, and API reference

**Files:**
- Modify: `packages/docxdriver-pi/src/python-plan-prelude.ts`
- Modify: `packages/docxdriver-pi/src/python-plan-extension.ts`
- Test: `packages/docxdriver-pi/test/python-plan-extension.test.mjs`

**Interfaces:**
- Public `ReviewResult` has `commit_key: str` and `edits: list`.
- Public wrappers are `docx_review(path: str, plan: Plan) -> ReviewResult | None` and `docx_commit(commit_key: str) -> None`.
- Private wrappers are `_docx_review(path: str, state: dict)` and `_docx_commit(commit_key: str)`.

- [ ] **Step 1: Replace old prelude tests with failing one-call API tests.**

Update the standard helper to execute:

```python
review = docx_review("contract.docx", plan)
print("KEY=" + review.commit_key)
print(review.edits)
```

Then in a later call execute:

```python
docx_commit(review.commit_key)
print("committed")
```

Assert the key matches `k1:[0-9a-f]{32}`, edit context contains the changed text, and old `docx_preview`/page-based `docx_review` calls are rejected by the type checker or absent from the prelude.

- [ ] **Step 2: Run the focused extension tests to verify old API failures.**

```bash
cd packages/docxdriver-pi
npm test -- --test-name-pattern='plan authoring|review|commit signature|help'
```

Expected: FAIL until prelude and host callback signatures are changed.

- [ ] **Step 3: Update prelude dataclasses and wrappers.**

Replace `PreviewResult` and paginated `ReviewResult` with:

```python
@dataclass
class ReviewResult:
    commit_key: str
    edits: list
```

Implement:

```python
def docx_review(path: str, plan: Plan) -> ReviewResult:
    result = _docx_review(path, {"plan": plan.to_dict()})
    if result is None:
        return None
    return ReviewResult(commit_key=result["commit_key"], edits=result["edits"])

def docx_commit(commit_key: str):
    _docx_commit(commit_key)
    return None
```

Update Python type stubs for the same signatures and remove old preview/page-review/private callback declarations.

- [ ] **Step 4: Update the API reference and prompt guidelines.**

Teach the agent to read first, build a plan, call `docx_review(path, plan)`, inspect `review.edits`, and call `docx_commit(review.commit_key)` in a later Python execution. Explain that a new review is required after changing the plan. Remove every instruction about preview receipts, review pages, and passing a plan to commit.

- [ ] **Step 5: Run extension tests and repair output/rendering expectations.**

```bash
cd packages/docxdriver-pi
npm test -- --test-name-pattern='python-plan-extension|prompt|help|review context'
```

Expected: updated one-call review/commit tests pass, including same-execution rejection and core-key non-leak checks.

- [ ] **Step 6: Commit the Python surface change.**

```bash
git add packages/docxdriver-pi/src/python-plan-prelude.ts packages/docxdriver-pi/src/python-plan-extension.ts packages/docxdriver-pi/test/python-plan-extension.test.mjs
git commit -m "feat(pi): expose combined review in Python plan surface"
```

---

### Task 3: Update live harnesses, package documentation, and protocol tests

**Files:**
- Modify: `packages/docxdriver-pi/README.md`
- Modify: `packages/docxdriver-pi/test/repl-live.test.mjs`
- Modify: `packages/docxdriver-pi/test/pi-e2e/repl-live.mjs`
- Modify: `packages/docxdriver-pi/test/pi-e2e/repl-tasks.json`
- Modify: `packages/docxdriver-pi/test/pi-e2e/manifest.json` if protocol text is present

**Interfaces:**
- Live task protocol is `review -> later commit`, with no page delivery loop.
- Metrics count one review and one consumed compact key per successful task.

- [ ] **Step 1: Add/update live protocol assertions before implementation.**

Change event fixtures and metrics to expect a successful review notice with `k1:` and edit context, followed by a later `commit(k1)` call. Remove assertions for `preview`, `docx_review(receipt, page)`, page counts, and complete-review delivery.

- [ ] **Step 2: Run the live-related offline tests to identify all stale assumptions.**

```bash
cd packages/docxdriver-pi
npm test -- --test-name-pattern='repl-live|pi-e2e|protocol|receipt'
```

Expected: failures identify remaining old protocol strings and event fixtures.

- [ ] **Step 3: Update harness prompts and metrics.**

Teach the live agent prompt to call `docx_review(path, plan)`, inspect the returned edit neighborhoods, then call `docx_commit(review.commit_key)` later. Change success criteria from preview/review-page ordering to review/commit ordering and compact-key consumption. Preserve source-race and plan-mutation tasks by making them mutate the source or require a fresh review before commit.

- [ ] **Step 4: Update README and inline host comments.**

Document the one-call review result, compact key format, bounded edit context, later-execution commit gate, and immutable reviewed plan. Remove stale receipt-store/page-pagination explanations. Keep the five-tool `docx_edit` documentation unchanged.

- [ ] **Step 5: Run offline package tests.**

```bash
cd packages/docxdriver-pi
npm test
```

Expected: PASS for build, deterministic tests, protocol rendering, extension behavior, and host lifecycle tests.

- [ ] **Step 6: Commit documentation and harness changes.**

```bash
git add packages/docxdriver-pi/README.md packages/docxdriver-pi/test/repl-live.test.mjs packages/docxdriver-pi/test/pi-e2e/repl-live.mjs packages/docxdriver-pi/test/pi-e2e/repl-tasks.json packages/docxdriver-pi/test/pi-e2e/manifest.json
git commit -m "docs(test): document combined docx review protocol"
```

---

### Task 4: Full verification and independent review

**Files:**
- Modify only if verification finds a defect in files from Tasks 1–3.

- [ ] **Step 1: Inspect the diff and worktree boundary.**

```bash
git status --short
git diff --stat HEAD~3..HEAD
git diff --check HEAD~3..HEAD
```

Confirm no unrelated pre-existing modifications were staged or committed.

- [ ] **Step 2: Run the package test suite from a clean build.**

```bash
cd packages/docxdriver-pi
npm test
```

- [ ] **Step 3: Run targeted core tests if context extraction changed Rust code.**

```bash
cargo test -p docxdriver-core
```

- [ ] **Step 4: Run the type/build checks and inspect generated-surface drift.**

```bash
cd packages/docxdriver-pi
npm run build
node --test test/python-host.test.mjs test/python-plan-extension.test.mjs test/repl-live.test.mjs
```

- [ ] **Step 5: Review security and resource invariants.**

Verify that no `p1:sha256:` core key appears in returned values, notices, protocol lines, or docs; the commit key is exactly `k1:` plus 32 hex characters; review context is bounded; same-execution commit remains blocked; and source/path/candidate races preserve the target and consume or retain the key according to the existing rules.

- [ ] **Step 6: Request independent code review before declaring completion.**

Use a fresh read-only reviewer against the final diff, asking specifically for protocol regressions, key leakage, missing lifecycle gates, unbounded context, and stale public API references. Apply any verified fixes, rerun the affected tests, and report residual risks.
