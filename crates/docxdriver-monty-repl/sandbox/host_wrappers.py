# Sandbox wrappers over the private _docx_*/_quote_*/_term_*/_inline_*
# callbacks a host provides, and the result values they return. Runs
# after plan.py (uses Plan).

import re
from dataclasses import dataclass

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

def docx_review(path: str, plan: Plan) -> ReviewResult | None:
    result = _docx_review(path, {"plan": plan.to_dict()})
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
