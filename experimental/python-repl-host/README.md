# Python REPL host (unpublished)

Host glue for Python processes running docxdriver plans in a Monty sandbox,
formerly `docxdriver.repl` in the PyPI package. It is no longer published:
hosts such as Open WebUI vendor what they need.

- `docxdriver_repl/host.py`: `NativeBridge`, `PlanHost`, commit-key store,
  workspace confinement, atomic commit.
- `docxdriver_repl/quotes.py`: structured quotation validation and audit.
- `docxdriver_repl/prelude.py`: generated; `PLAN` is read from the installed
  `docxdriver` package, followed by async host wrappers and `REFERENCE`.

Tests need the `docxdriver` wheel installed:

```sh
PYTHONPATH=experimental/python-repl-host python -m unittest discover -s experimental/python-repl-host/tests
```
