# docxdriver-monty-repl

The [docxdriver](https://github.com/jhnoel/docxdriver) plan vocabulary as
Python source, for Rust hosts that run a
[Monty](https://github.com/pydantic/monty) sandbox:

- `PLAN`: `Plan`, the typed op dataclasses (`ReplaceText`, `FormatText`, ...),
  `Revise`, `AcceptRevision`/`RejectRevision`. Same text as the
  `docxdriver.plan` module of the `docxdriver` PyPI package.
- `PLAN_STUBS`: Monty type-checker stubs for `PLAN`.
- `PLAN_REFERENCE`: agent-facing Markdown reference.

Preload `PLAN` into the sandbox. `Plan(author=..., ops=[...]).to_dict()`
returns `{"author", "change_mode", "ops"}`; add `base` (`sha256:<hex>` of the
source document) and pass the JSON to `docxdriver_core::parse_plan_json`.
Host functions such as reading documents and committing plans are the host's
own.

## Development

`sandbox/plan.py` links to `crates/docxdriver-python/python/docxdriver/plan.py`,
the single file. After changing it, the core schema, or
`sandbox/plan-capabilities.json`, regenerate from the repository root:

```sh
python scripts/generate-python-plan.py
```
