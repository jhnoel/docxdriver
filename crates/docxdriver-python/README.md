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

The optional sandbox REPL harness is available as a subpackage in the same
distribution:

```python
from docxdriver.repl import NativeBridge, PRELUDE, REFERENCE
```

`NativeBridge` provides async `init`, `call`, and `reset` requests for a host
running a Python sandbox. The sandbox receives authoring classes and async
wrappers from `PRELUDE`; `REFERENCE` provides their API documentation. The
sandbox itself does not import the native extension. Native work runs through
host callbacks outside the server event loop.

The host confines reads and writes to the provided workspace, verifies source
hashes and candidate output before commit, and performs atomic file replacement
with read-back verification. Commit keys are bounded and single-use. Cancellation
is drained before another host call so a write callback cannot be abandoned.

Set `DOCXDRIVER_QUOTE_AUDIT_COMMENTS=1` to enable structured Quote/Term/Inline plan
values and protected quotation audit comments. The provenance policy defaults to
`permissive`; `controlled` and `authoritative` fail closed.

The prelude and API reference are generated from shared Pi templates. After
changing those templates, regenerate them from the repository root:

```sh
python scripts/generate-docxdriver-repl-prelude.py
```

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
