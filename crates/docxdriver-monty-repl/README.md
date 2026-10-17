# docxdriver-monty-repl

Python sources for Rust hosts that run
[docxdriver](https://github.com/jhnoel/docxdriver) plans in a
[Monty](https://github.com/pydantic/monty) sandbox:

- `PLAN`: plan vocabulary (`Plan`, op dataclasses, `Revise`, `Quote`, `Term`,
  `Inline`, `AcceptRevision`/`RejectRevision`). Same text as
  the `docxdriver.plan` module of the `docxdriver` PyPI package.
- `HOST_WRAPPERS`: `docx_*`/`quote_*` functions calling private
  `_docx_*`/`_quote_*`/`_term_*`/`_inline_*` callbacks the host implements,
  and their result values (`ReadResult`, `ReviewResult`, `QuoteResult`, ...).
- `PRELUDE`, `STUBS`, `REFERENCE`: combined source, Monty type stubs, and the
  agent-facing reference.

`Plan(...).to_dict()` returns `{"author", "change_mode", "ops"}`. Add `base`
(`sha256:<hex>` of the source document) before
`docxdriver_core::parse_plan_json`.

## Development

`sandbox/` is the single source. After changing it, the core schema, or
`sandbox/plan-capabilities.json`, regenerate from the repository root:

```sh
python scripts/generate-python-plan.py
```
