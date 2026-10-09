/**
 * Version 2 Python prelude (raw projection string surface).
 *
 * The document is exposed as a canonical projection string (docx_read returns
 * the rendered markup). The agent edits that string with ordinary Python
 * str/regex operations; the host reconciles original vs proposed into typed
 * core operations. The prelude is fed to a fresh Monty session with
 * skipTypeCheck (it is trusted host code); PYTHON_TYPE_STUBS is what the
 * checker resolves user-snippet names against, so every public name — and the
 * `re` module user snippets call — must be declared there. `re` is declared
 * as a stub namespace class because a stub-level `import re` is not exposed
 * to user snippets by Monty's checker; at runtime the prelude's real
 * `import re` binds the module in the session globals.
 */

/**
 * The Python prelude: a PreviewResult dataclass plus docx_* wrappers over the
 * private host callbacks. `docx_preview` requires both `original` and
 * `proposed` as keyword-only arguments and wraps the host's `{key}` dict into
 * a `PreviewResult` so `preview.key` works in Python.
 */
export const PYTHON_PRELUDE: string = `
import re

from dataclasses import dataclass

@dataclass
class PreviewResult:
    key: str

def docx_create(path: str, html: str):
    return _docx_create(path, html)

def docx_read(path: str, view: str = "markup") -> str:
    return _docx_read(path, view)

def docx_help(topic: str | None = None) -> str:
    return _docx_help(topic)

def docx_structure(path: str, view: str = "markup") -> dict:
    return _docx_structure(path, view)

def docx_find(path: str, pattern: str, view: str = "markup") -> list:
    """Locate body paragraphs whose visible text matches the regex.
    Returns [(paragraph_id, visible_text, raw_markup), ...] — bounded, not a dump."""
    data = docx_structure(path, view)
    return [(p["id"], p["text"], p["markup"]) for p in data["paragraphs"]
            if re.search(pattern, p["text"])]

def docx_preview(path: str, *, original: str, proposed: str):
    result = _docx_preview(path, {"original": original, "proposed": proposed})
    if result is None:
        return None
    return PreviewResult(key=result["key"])

def docx_commit(path: str, key: str, *, proposed: str):
    _docx_commit(path, key, {"proposed": proposed})
    return None
`;

/**
 * Stub declarations for Monty's type checker. Declares the `re` namespace
 * (static methods user snippets call), `PreviewResult`, the five public
 * functions (keyword-only `original`/`proposed` on preview and commit), and
 * the private `_docx_*` host callbacks.
 */
export const PYTHON_TYPE_STUBS: string = `
class re:
    @staticmethod
    def sub(pattern: str, repl: str, string: str, count: int = 0, flags: int = 0) -> str: ...
    @staticmethod
    def findall(pattern: str, string: str, flags: int = 0) -> list: ...
    @staticmethod
    def search(pattern: str, string: str, flags: int = 0): ...
    @staticmethod
    def match(pattern: str, string: str, flags: int = 0): ...
    @staticmethod
    def split(pattern: str, string: str, maxsplit: int = 0, flags: int = 0) -> list: ...

class PreviewResult:
    def __init__(self, key: str): ...
    key: str

def docx_create(path: str, html: str): ...
def docx_read(path: str, view: str = "markup") -> str: ...
def docx_help(topic: str | None = None) -> str: ...
def docx_structure(path: str, view: str = "markup") -> dict: ...
def docx_find(path: str, pattern: str, view: str = "markup") -> list: ...
def docx_preview(path: str, *, original: str, proposed: str): ...
def docx_commit(path: str, key: str, *, proposed: str) -> None: ...

def _docx_create(path: str, html: str): ...
def _docx_read(path: str, view: str = "markup") -> str: ...
def _docx_help(topic: str | None = None) -> str: ...
def _docx_structure(path: str, view: str = "markup") -> dict: ...
def _docx_preview(path: str, state: dict) -> dict: ...
def _docx_commit(path: str, key: str, state: dict) -> None: ...
`;

/**
 * Concise reference doc for agents: exact signatures plus the spec's V2
 * example verbatim (str.replace + re.sub, preview with original/proposed,
 * commit in a later call).
 */
export const PYTHON_API_REFERENCE: string = `
# docxdriver Python REPL API — Version 2 (raw projection string)

The document is exposed as one canonical projection string (docx_read). Edit
it with ordinary Python string and regex operations; the host reconciles the
original vs proposed projections into typed core operations. Never hand-edit
the id, ord, or num attributes.

## Host functions

- \`docx_create(path: str, html: str)\`
- \`docx_read(path: str, view: str = "markup") -> str\` — the projection string
- \`docx_help(topic: str | None = None) -> str\`
- \`docx_structure(path: str, view: str = "markup") -> dict\` — \`{"paragraphs": [{"id", "text", "markup"}...], "tables": [markup...]}\`
- \`docx_find(path: str, pattern: str, view: str = "markup") -> list\` — \`[(paragraph_id, visible_text, raw_markup), ...]\`
- \`docx_preview(path: str, *, original: str, proposed: str) -> PreviewResult\` (returns \`.key\`)
- \`docx_commit(path: str, key: str, *, proposed: str)\`

## Locating paragraphs without a full dump

\`docx_find\` is a bounded locate, not a dump: one entry per body paragraph
whose visible text matches the regex, with the decoded visible text and the
raw markup (the exact projection substring you can replace in a draft).
Table-cell paragraphs are never returned; access tables via \`docx_structure\`.

    for pid, text, raw in docx_find("contract.docx", r"thirty days"):
        draft = original.replace(raw, raw.replace("thirty days", "sixty days"))

## Example

    original = docx_read("contract.docx")

    draft = original.replace(
        "The notice period is thirty days.",
        "The notice period is sixty days.",
    )

    draft = re.sub(
        r"Payment is due within \\d+ days",
        "Payment is due within 45 days",
        draft,
    )

    preview = docx_preview(
        "contract.docx",
        original=original,
        proposed=draft,
    )
    # ...later python call...
    docx_commit(
        "contract.docx",
        preview.key,
        proposed=draft,
    )
`;
