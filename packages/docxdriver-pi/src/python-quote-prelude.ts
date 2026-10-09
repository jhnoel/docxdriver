/** Python-side builders integrated into the provenance-controlled plan REPL. */
export const QUOTE_PYTHON_PRELUDE = `
from dataclasses import dataclass

@dataclass
class QuoteResult:
    text: str
    provenance: dict

@dataclass
class TermResult:
    text: str
    provenance: dict

class Term:
    def __init__(self, text: str, style: str = "double"):
        self.text = text
        self.style = style
    def to_dict(self):
        return {"$kind": "term", "text": self.text, "style": self.style}

@dataclass
class InlineResult:
    text: str
    audit: dict

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
`;

export const QUOTE_PYTHON_TYPE_STUBS = `
class QuoteResult:
    def __init__(self, text: str, provenance: dict): ...
    text: str
    provenance: dict

class TermResult:
    def __init__(self, text: str, provenance: dict): ...
    text: str
    provenance: dict

class Term:
    def __init__(self, text: str, style: str = "double"): ...
    text: str
    style: str
    def to_dict(self) -> dict: ...

class InlineResult:
    def __init__(self, text: str, audit: dict): ...
    text: str
    audit: dict

class Inline:
    def __init__(self, parts: list): ...
    parts: list
    def to_dict(self) -> dict: ...

class Quote:
    def __init__(self, source: str, at: str, select: str, occurrence: int = 1, style: str = "double"): ...
    source: str
    at: str
    select: str
    occurrence: int
    style: str
    changes: list
    def omit(self, select: str, occurrence: int = 1, marker: str = "…") -> Quote: ...
    def bracket(self, select: str, replacement: str, occurrence: int = 1) -> Quote: ...
    def to_dict(self) -> dict: ...

def quote_find(source: str, query: str, ignore_case: bool = False) -> list: ...
def quote_validate(quote: Quote) -> QuoteResult: ...
def quote_lint(text: str, severity: str = "error") -> list: ...
def term_render(term: Term) -> TermResult: ...
def inline_validate(inline: Inline, policy: str = "error") -> InlineResult: ...
def _quote_find(source: str, query: str, ignore_case: bool = False) -> list: ...
def _quote_validate(quote: dict) -> dict: ...
def _quote_lint(text: str, severity: str = "error") -> list: ...
def _term_render(term: dict) -> dict: ...
def _inline_validate(inline: dict, policy: str = "error") -> dict: ...
`;

export const QUOTE_API_REFERENCE = `
Use quote_find(source, query) to obtain the paragraph id and exact source text.
Create Quote(source, at, select, occurrence=1, style="double"), where select is one contiguous,
verbatim source span. Adapt it with .omit(exact_source_text) and
.bracket(exact_source_text, replacement). Adaptation selectors always refer to
the original span; overlaps fail. Finish with quote_validate(q). Only the host
constructs QuoteResult.text, including curly double quotation marks by default
(style="single" or style="none" are also available). Its provenance binds the source SHA-256, paragraph
SHA-256, paragraph id, offsets, original span, and every adaptation.

Raw quotation marks in ordinary authored text are lint errors. For a
non-quotational mention such as the word quotation, use
term_render(Term("quotation")); its quote marks are host-created and its
provenance is classified as kind="term" rather than a source quotation.
For authored document content, prefer Inline([plain_text, Term(...), Quote(...)]).
inline_validate() automatically lints every plain-text part before the host
renders structured nodes. The default policy is error; comment must be chosen
explicitly and returns warning spans that require protected comment bubbles.
In the plan REPL's flagged quotation mode, pass Quote, Term, or Inline directly
to an operation with_ field. docx_review resolves the node and binds its source;
copying QuoteResult.text into a plain string loses that provenance boundary.

Example:
q = Quote("source.docx", at="89ABCDEF", select="The tenant must promptly pay the charge.")
q.omit(" promptly")
q.bracket("tenant", "Tenant")
validated = quote_validate(q)
print(validated.text)
print(validated.provenance)
`;
