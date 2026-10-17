# docxdriver-monty-repl

The [docxdriver](https://github.com/jhnoel/docxdriver) Python API as source,
for Rust hosts that run a [Monty](https://github.com/pydantic/monty) sandbox.
Same text as the `docxdriver` PyPI package's modules, so sandbox code and
native code are interchangeable:

```python
doc = Document(Path("a.docx").read_bytes())
review = doc.review(Plan(ops=[ReplaceText(at=..., select="thirty", with_="sixty")]))
Path("a.docx").write_bytes(review.commit().data)
```

- `PLAN`: `Plan`, the typed op dataclasses (`ReplaceText`, `FormatText`, ...),
  `Revise`, `AcceptRevision`/`RejectRevision` (`docxdriver.plan`).
- `DOCUMENT`: `Document`, `Review`, `create` (`docxdriver.document`).
- `ENGINE`: the external functions `DOCUMENT` calls. Define each as a host
  function over `docxdriver_core::Document`; see its docs for signatures.
- `PLAN_STUBS`: Monty type-checker stubs for `PLAN`.
- `PLAN_REFERENCE`: agent-facing Markdown reference.

Preload `PLAN`, then `DOCUMENT`.

## Development

`sandbox/plan.py` and `sandbox/document.py` link to
`crates/docxdriver-python/python/docxdriver/`, the single copies. After
changing `plan.py`, the core schema, or `sandbox/plan-capabilities.json`,
regenerate from the repository root:

```sh
python scripts/generate-python-plan.py
```
