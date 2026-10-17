# docxdriver

Python SDK for the Rust DOCX engine. It supports Python 3.10 and later and has no
Python runtime dependencies.

Install from PyPI:

```sh
python -m pip install docxdriver
```

The core API exposes `execute_request`, `canonical_plan_json`, and the convenience
functions `run_command`, `run_plan`, and `run_request`. Results preserve document
contents as Python `bytes`.

Typed plan builders live in `docxdriver.plan`:

```python
import hashlib
from docxdriver import run_plan
from docxdriver.plan import Plan, ReplaceText

data = open("contract.docx", "rb").read()

plan = Plan(author="Reviewer", ops=[
    ReplaceText(at="0F537164", select="thirty", with_="sixty"),
]).to_dict()                      # {"author", "change_mode", "ops"}
plan["base"] = "sha256:" + hashlib.sha256(data).hexdigest()
preview = run_plan(data, plan)
```

`plan.py` stays within Monty's Python subset. Sandbox hosts can preload its
text:

```python
from importlib.resources import files
PLAN = files("docxdriver").joinpath("plan.py").read_text(encoding="utf-8")
```

Rust hosts get the same text as `docxdriver_monty_repl::PLAN`. The op
dataclasses are generated from the core schema; regenerate from the repository
root with `python scripts/generate-python-plan.py`.

Version 0.4.0 removed the `docxdriver.repl` sandbox host from this package. Its
source remains in `experimental/python-repl-host` for hosts to vendor.

Build and install a wheel from the repository root:

```sh
python -m pip install "maturin>=1.9,<2"
maturin build --profile python --locked --manifest-path crates/docxdriver-python/Cargo.toml --out /tmp/docxdriver-wheels
python -m pip install /tmp/docxdriver-wheels/*.whl
```

## Publish to PyPI

The `Publish Python SDK` GitHub Actions workflow builds a source distribution
and wheels for Linux x86_64 and aarch64, macOS universal2, and Windows x86_64
when a `v*` tag is pushed. The tag must match the version in the root
`Cargo.toml` workspace package.

Before the first release, configure a PyPI trusted publisher for the
`docxdriver` project with these values:

- Owner: `jhnoel`
- Repository: `docxdriver`
- Workflow filename: `pypi.yml`
- GitHub environment: `pypi`

Create the `pypi` GitHub environment under the repository settings if you want
to add release approvals. Then update the workspace version, commit the change,
and push the matching tag (for example, `v0.2.7`). GitHub Actions publishes the
built distributions using PyPI's trusted publishing; no long-lived PyPI token is
needed.

The `python` Cargo profile keeps release optimizations and enables panic unwinding
so PyO3 can turn Rust panics into Python exceptions. ABI3 wheels support Python
3.10 and later on the built operating system and architecture. To run the Python
SDK and host tests after installing the wheel:

```sh
python -m unittest discover -s crates/docxdriver-python/tests
```
