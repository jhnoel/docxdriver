# Sandbox wrappers over the private _docx_*/_quote_*/_term_*/_inline_*
# callbacks a host provides, the result values they return, and the
# Quote/Term/Inline nodes the host resolves at review. Runs after
# docxdriver's plan.py (uses Plan).

import re
from dataclasses import dataclass

class Term:
    def __init__(self, text: str, style: str = "double"):
        self.text = text
        self.style = style
    def to_dict(self):
        return {"$kind": "term", "text": self.text, "style": self.style}

class Inline:
    def __init__(self, parts: list):
        self.parts = parts
    def to_dict(self):
        return {"$kind": "inline", "parts": [part.to_dict() if hasattr(part, "to_dict") else part for part in self.parts]}

class Quote:
    def __init__(self, source: str, at: str, select: str, occurrence: int = 1, style: str = "double"):
        self.source = source
        self.at = at
        self.select = select
        self.occurrence = occurrence
        self.style = style
        self.changes = []

    def omit(self, select: str, occurrence: int = 1, marker: str = "…"):
        self.changes.append({"kind": "omit", "select": select, "occurrence": occurrence, "marker": marker})
        return self

    def bracket(self, select: str, replacement: str, occurrence: int = 1):
        self.changes.append({"kind": "bracket", "select": select, "replacement": replacement, "occurrence": occurrence})
        return self

    def to_dict(self):
        return {"$kind": "quote", "source": self.source, "at": self.at, "select": self.select, "occurrence": self.occurrence, "style": self.style, "changes": self.changes}

@dataclass
class QuoteResult:
    text: str
    provenance: dict

@dataclass
class TermResult:
    text: str
    provenance: dict

@dataclass
class InlineResult:
    text: str
    audit: dict

@dataclass
class ReviewResult:
    commit_key: str
    edits: list

@dataclass
class ReadResult:
    markup: str
    equations: list = None
    selection_space: dict = None
    selections: dict = None
    comments: list = None
    assets: list = None
    comments_error: str = None
    styles: dict = None
    css: dict = None
    def __repr__(self):
        return "ReadResult(markup=" + repr(self.markup) + ", styles=" + str(len(self.styles or {})) + " definitions, css=" + str(len(self.css or {})) + " rules; inspect .styles and .css, equations=" + repr(self.equations) + ", selection_space=" + repr(self.selection_space) + ", selections=" + repr(self.selections) + ", comments=" + repr(self.comments) + ", assets=" + repr(self.assets) + ", comments_error=" + repr(self.comments_error) + ")"

def docx_create(path: str, html: str):
    return _docx_create(path, html)

def docx_read(path: str, view: str = "markup", kind: str = "document"):
    _listing_kinds = ("comments", "styles", "revisions", "assets")
    if kind == "document" and view in _listing_kinds:
        kind = view
    if kind in _listing_kinds:
        return _docx_read(path, view, kind)
    if kind != "document":
        raise ValueError('kind must be one of document, comments, styles, revisions, assets')
    result = _docx_read(path, view)
    return ReadResult(markup=result["markup"], styles=result.get("styles", {}), css=result.get("css", {}), equations=result.get("equations", []), selection_space=result.get("selection_space"), selections=result.get("selections"), comments=result.get("comments", []), assets=result.get("assets", []), comments_error=result.get("comments_error"))

def docx_find(path: str, query: str, ignore_case: bool = False) -> list:
    return _docx_find(path, query, ignore_case)

def docx_help(topic: str | None = None) -> str:
    return _docx_help(topic)

def _node(value):
    return value.to_dict() if hasattr(value, "to_dict") else value

def docx_review(path: str, plan: Plan) -> ReviewResult | None:
    # Quote/Term/Inline nodes in `with` become {"$kind": ...} dicts the host resolves.
    state = plan.to_dict()
    for op in state["ops"]:
        if "with" in op:
            op["with"] = _node(op["with"])
        for edit in op.get("edits", []):
            edit["with"] = _node(edit["with"])
    result = _docx_review(path, {"plan": state})
    if result is None:
        return None
    return ReviewResult(commit_key=result["commit_key"], edits=result["edits"])

def docx_commit(commit_key: str):
    _docx_commit(commit_key)
    return None

def quote_find(source: str, query: str, ignore_case: bool = False) -> list:
    return _quote_find(source, query, ignore_case)

def quote_validate(quote: Quote) -> QuoteResult:
    result = _quote_validate(quote.to_dict())
    return QuoteResult(text=result["text"], provenance=result)

def quote_lint(text: str, severity: str = "error") -> list:
    return _quote_lint(text, severity)

def term_render(term: Term) -> TermResult:
    result = _term_render(term.to_dict())
    return TermResult(text=result["text"], provenance=result)

def inline_validate(inline: Inline, policy: str = "error") -> InlineResult:
    result = _inline_validate(inline.to_dict(), policy)
    return InlineResult(text=result["text"], audit=result["audit"])
