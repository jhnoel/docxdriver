/**
 * Version 1 Python prelude (typed-plan surface).
 *
 * Three regions are produced by `scripts/generate-python-dataclasses.mjs`
 * from the checked-in core schema ($defs.editOp.oneOf, selected by the
 * declarative capability manifest): the operation dataclasses, the Monty
 * type stubs for those classes, and the concise operation reference. The
 * wrapper layer below is hand-authored: public docx_* functions call the
 * private _docx_* host callbacks (bound via externalLookup). PYTHON_TYPE_STUBS
 * is what Monty's type checker resolves user-snippet names against — the
 * checker never sees prelude definitions — so every public name must be
 * declared there.
 *
 * This is the only production prelude. The V2 raw-string and V3 model
 * preludes live in the experimental archive; their projection-parser-backed
 * `docx_structure` helper is not part of V1. `docx_read` delegates to core
 * read (full structural markup) and
 * `docx_find` returns core find match records verbatim — no rendered markup
 * is parsed or re-projected in TypeScript.
 */

/**
 * The Python prelude: typed operation dataclasses, the Plan container, and
 * docx_* wrappers over the private host callbacks.
 */
export const PYTHON_PRELUDE: string = `
from dataclasses import dataclass
import re

# BEGIN GENERATED DATACLASSES
@dataclass
class ClearFooter:
    kind: str | None = None
    section: int | None = None
    def to_dict(self):
        value = {"op": "clear_footer"}
        if self.kind is not None:
            value["kind"] = self.kind
        if self.section is not None:
            value["section"] = self.section
        return value

@dataclass
class ClearHeader:
    kind: str | None = None
    section: int | None = None
    def to_dict(self):
        value = {"op": "clear_header"}
        if self.kind is not None:
            value["kind"] = self.kind
        if self.section is not None:
            value["section"] = self.section
        return value

@dataclass
class CommentAdd:
    at: str
    text: str
    occurrence: int | None = None
    select: str | None = None
    def to_dict(self):
        value = {"op": "comment_add"}
        if self.at is not None:
            value["at"] = self.at
        if self.text is not None:
            value["text"] = self.text
        if self.occurrence is not None:
            value["occurrence"] = self.occurrence
        if self.select is not None:
            value["select"] = self.select
        return value

@dataclass
class CommentDelete:
    comment_id: str
    def to_dict(self):
        value = {"op": "comment_delete"}
        if self.comment_id is not None:
            value["comment_id"] = self.comment_id
        return value

@dataclass
class CommentReply:
    comment_id: str
    text: str
    def to_dict(self):
        value = {"op": "comment_reply"}
        if self.comment_id is not None:
            value["comment_id"] = self.comment_id
        if self.text is not None:
            value["text"] = self.text
        return value

@dataclass
class CommentSetStatus:
    comment_id: str
    status: str
    def to_dict(self):
        value = {"op": "comment_set_status"}
        if self.comment_id is not None:
            value["comment_id"] = self.comment_id
        if self.status is not None:
            value["status"] = self.status
        return value

@dataclass
class DeleteEquation:
    at: str
    equation: int | None = None
    def to_dict(self):
        value = {"op": "delete_equation"}
        if self.at is not None:
            value["at"] = self.at
        if self.equation is not None:
            value["equation"] = self.equation
        return value

@dataclass
class DeleteParagraphs:
    at: list
    def to_dict(self):
        value = {"op": "delete_paragraphs"}
        if self.at is not None:
            value["at"] = self.at
        return value

@dataclass
class FormatParagraph:
    at: str
    alignment: str | None = None
    clear: list | None = None
    indent_left: float | None = None
    indent_right: float | None = None
    line_spacing: dict | None = None
    space_after: float | None = None
    space_before: float | None = None
    style: str | None = None
    def to_dict(self):
        value = {"op": "format_paragraph"}
        if self.at is not None:
            value["at"] = self.at
        if self.alignment is not None:
            value["alignment"] = self.alignment
        if self.clear is not None:
            value["clear"] = self.clear
        if self.indent_left is not None:
            value["indent_left"] = self.indent_left
        if self.indent_right is not None:
            value["indent_right"] = self.indent_right
        if self.line_spacing is not None:
            value["line_spacing"] = self.line_spacing
        if self.space_after is not None:
            value["space_after"] = self.space_after
        if self.space_before is not None:
            value["space_before"] = self.space_before
        if self.style is not None:
            value["style"] = self.style
        return value

@dataclass
class FormatText:
    at: str
    select: str
    bold: bool | None = None
    clear: list | None = None
    color: str | None = None
    font_size: float | None = None
    italic: bool | None = None
    occurrence: int | None = None
    strike: bool | None = None
    subscript: bool | None = None
    superscript: bool | None = None
    underline: bool | None = None
    def to_dict(self):
        value = {"op": "format_text"}
        if self.at is not None:
            value["at"] = self.at
        if self.select is not None:
            value["select"] = self.select
        if self.bold is not None:
            value["bold"] = self.bold
        if self.clear is not None:
            value["clear"] = self.clear
        if self.color is not None:
            value["color"] = self.color
        if self.font_size is not None:
            value["font_size"] = self.font_size
        if self.italic is not None:
            value["italic"] = self.italic
        if self.occurrence is not None:
            value["occurrence"] = self.occurrence
        if self.strike is not None:
            value["strike"] = self.strike
        if self.subscript is not None:
            value["subscript"] = self.subscript
        if self.superscript is not None:
            value["superscript"] = self.superscript
        if self.underline is not None:
            value["underline"] = self.underline
        return value

@dataclass
class InsertParagraph:
    at: str
    position: str
    as_: str | None = None
    style: str | None = None
    with_: str | Inline | Quote | Term | None = None
    def to_dict(self):
        value = {"op": "insert_paragraph"}
        if self.at is not None:
            value["at"] = self.at
        if self.position is not None:
            value["position"] = self.position
        if self.as_ is not None:
            value["as"] = self.as_
        if self.style is not None:
            value["style"] = self.style
        if self.with_ is not None:
            value["with"] = self.with_.to_dict() if hasattr(self.with_, "to_dict") else self.with_
        return value

@dataclass
class ReplaceEquation:
    at: str
    mathml: str
    display: bool | None = None
    equation: int | None = None
    def to_dict(self):
        value = {"op": "replace_equation"}
        if self.at is not None:
            value["at"] = self.at
        if self.mathml is not None:
            value["mathml"] = self.mathml
        if self.display is not None:
            value["display"] = self.display
        if self.equation is not None:
            value["equation"] = self.equation
        return value

@dataclass
class ReplaceParagraph:
    at: str
    with_: str | Inline | Quote | Term
    def to_dict(self):
        value = {"op": "replace_paragraph"}
        if self.at is not None:
            value["at"] = self.at
        if self.with_ is not None:
            value["with"] = self.with_.to_dict() if hasattr(self.with_, "to_dict") else self.with_
        return value

@dataclass
class ReplaceText:
    at: str
    select: str
    with_: str | Inline | Quote | Term
    occurrence: int | None = None
    def to_dict(self):
        value = {"op": "replace_text"}
        if self.at is not None:
            value["at"] = self.at
        if self.select is not None:
            value["select"] = self.select
        if self.with_ is not None:
            value["with"] = self.with_.to_dict() if hasattr(self.with_, "to_dict") else self.with_
        if self.occurrence is not None:
            value["occurrence"] = self.occurrence
        return value

@dataclass
class SetEvenAndOddHeaders:
    even_and_odd: bool
    def to_dict(self):
        value = {"op": "set_even_and_odd_headers"}
        if self.even_and_odd is not None:
            value["even_and_odd"] = self.even_and_odd
        return value

@dataclass
class SetFooter:
    with_: str | Inline | Quote | Term
    kind: str | None = None
    section: int | None = None
    def to_dict(self):
        value = {"op": "set_footer"}
        if self.with_ is not None:
            value["with"] = self.with_.to_dict() if hasattr(self.with_, "to_dict") else self.with_
        if self.kind is not None:
            value["kind"] = self.kind
        if self.section is not None:
            value["section"] = self.section
        return value

@dataclass
class SetHeader:
    with_: str | Inline | Quote | Term
    kind: str | None = None
    section: int | None = None
    def to_dict(self):
        value = {"op": "set_header"}
        if self.with_ is not None:
            value["with"] = self.with_.to_dict() if hasattr(self.with_, "to_dict") else self.with_
        if self.kind is not None:
            value["kind"] = self.kind
        if self.section is not None:
            value["section"] = self.section
        return value

# END GENERATED DATACLASSES

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

@dataclass
class Plan:
    author: str = "docxdriver"
    change_mode: str = "track"
    operations: list = None
    def to_dict(self):
        ops = self.operations if self.operations is not None else []
        return {
            "author": self.author,
            "change_mode": self.change_mode,
            "ops": [op.to_dict() if hasattr(op, "to_dict") else op for op in ops],
        }

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
`;

/**
 * Stub declarations for Monty's type checker. Mirrors every public name the
 * prelude defines so type-checked user snippets can resolve them.
 */
export const PYTHON_TYPE_STUBS: string = `
class re:
    @staticmethod
    def sub(pattern: str, repl: str, string: str, count: int = 0, flags: int = 0) -> str: ...
    @staticmethod
    def findall(pattern: str, string: str, flags: int = 0) -> list: ...
    @staticmethod
    def finditer(pattern: str, string: str, flags: int = 0): ...
    @staticmethod
    def search(pattern: str, string: str, flags: int = 0): ...
    @staticmethod
    def match(pattern: str, string: str, flags: int = 0): ...
    @staticmethod
    def split(pattern: str, string: str, maxsplit: int = 0, flags: int = 0) -> list: ...

# BEGIN GENERATED TYPE STUBS
class ClearFooter:
    def __init__(self, kind: str | None = None, section: int | None = None): ...
    kind: str | None
    section: int | None
    def to_dict(self) -> dict: ...

class ClearHeader:
    def __init__(self, kind: str | None = None, section: int | None = None): ...
    kind: str | None
    section: int | None
    def to_dict(self) -> dict: ...

class CommentAdd:
    def __init__(self, at: str, text: str, occurrence: int | None = None, select: str | None = None): ...
    at: str
    text: str
    occurrence: int | None
    select: str | None
    def to_dict(self) -> dict: ...

class CommentDelete:
    def __init__(self, comment_id: str): ...
    comment_id: str
    def to_dict(self) -> dict: ...

class CommentReply:
    def __init__(self, comment_id: str, text: str): ...
    comment_id: str
    text: str
    def to_dict(self) -> dict: ...

class CommentSetStatus:
    def __init__(self, comment_id: str, status: str): ...
    comment_id: str
    status: str
    def to_dict(self) -> dict: ...

class DeleteEquation:
    def __init__(self, at: str, equation: int | None = None): ...
    at: str
    equation: int | None
    def to_dict(self) -> dict: ...

class DeleteParagraphs:
    def __init__(self, at: list): ...
    at: list
    def to_dict(self) -> dict: ...

class FormatParagraph:
    def __init__(self, at: str, alignment: str | None = None, clear: list | None = None, indent_left: float | None = None, indent_right: float | None = None, line_spacing: dict | None = None, space_after: float | None = None, space_before: float | None = None, style: str | None = None): ...
    at: str
    alignment: str | None
    clear: list | None
    indent_left: float | None
    indent_right: float | None
    line_spacing: dict | None
    space_after: float | None
    space_before: float | None
    style: str | None
    def to_dict(self) -> dict: ...

class FormatText:
    def __init__(self, at: str, select: str, bold: bool | None = None, clear: list | None = None, color: str | None = None, font_size: float | None = None, italic: bool | None = None, occurrence: int | None = None, strike: bool | None = None, subscript: bool | None = None, superscript: bool | None = None, underline: bool | None = None): ...
    at: str
    select: str
    bold: bool | None
    clear: list | None
    color: str | None
    font_size: float | None
    italic: bool | None
    occurrence: int | None
    strike: bool | None
    subscript: bool | None
    superscript: bool | None
    underline: bool | None
    def to_dict(self) -> dict: ...

class InsertParagraph:
    def __init__(self, at: str, position: str, as_: str | None = None, style: str | None = None, with_: str | Inline | Quote | Term | None = None): ...
    at: str
    position: str
    as_: str | None
    style: str | None
    with_: str | Inline | Quote | Term | None
    def to_dict(self) -> dict: ...

class ReplaceEquation:
    def __init__(self, at: str, mathml: str, display: bool | None = None, equation: int | None = None): ...
    at: str
    mathml: str
    display: bool | None
    equation: int | None
    def to_dict(self) -> dict: ...

class ReplaceParagraph:
    def __init__(self, at: str, with_: str | Inline | Quote | Term): ...
    at: str
    with_: str | Inline | Quote | Term
    def to_dict(self) -> dict: ...

class ReplaceText:
    def __init__(self, at: str, select: str, with_: str | Inline | Quote | Term, occurrence: int | None = None): ...
    at: str
    select: str
    with_: str | Inline | Quote | Term
    occurrence: int | None
    def to_dict(self) -> dict: ...

class SetEvenAndOddHeaders:
    def __init__(self, even_and_odd: bool): ...
    even_and_odd: bool
    def to_dict(self) -> dict: ...

class SetFooter:
    def __init__(self, with_: str | Inline | Quote | Term, kind: str | None = None, section: int | None = None): ...
    with_: str | Inline | Quote | Term
    kind: str | None
    section: int | None
    def to_dict(self) -> dict: ...

class SetHeader:
    def __init__(self, with_: str | Inline | Quote | Term, kind: str | None = None, section: int | None = None): ...
    with_: str | Inline | Quote | Term
    kind: str | None
    section: int | None
    def to_dict(self) -> dict: ...

# END GENERATED TYPE STUBS

class Plan:
    def __init__(self, author: str = "docxdriver", change_mode: str = "track", operations: list = []): ...
    author: str
    change_mode: str
    operations: list
    def to_dict(self) -> dict: ...

class ReviewResult:
    def __init__(self, commit_key: str, edits: list): ...
    commit_key: str
    edits: list

class ReadResult:
    def __init__(self, markup: str, equations: list | None = None, selection_space: dict | None = None, selections: dict | None = None, comments: list | None = None, assets: list | None = None, comments_error: str | None = None, styles: dict | None = None, css: dict | None = None): ...
    markup: str
    styles: dict
    css: dict
    equations: list
    selection_space: dict | None
    selections: dict | None
    comments: list
    assets: list
    comments_error: str | None

def docx_create(path: str, html: str): ...
from typing import overload, Literal
@overload
def docx_read(path: str, view: Literal["markup", "final", "original"] = "markup", kind: Literal["document"] = "document") -> ReadResult: ...
@overload
def docx_read(path: str, view: str = "markup", kind: str = "document") -> ReadResult | dict: ...
def docx_find(path: str, query: str, ignore_case: bool = False) -> list: ...
def docx_help(topic: str | None = None) -> str: ...
def docx_review(path: str, plan: Plan) -> ReviewResult | None: ...
def docx_commit(commit_key: str) -> None: ...

def _docx_create(path: str, html: str): ...
def _docx_read(path: str, view: str = "markup", kind: str | None = None) -> dict: ...
def _docx_find(path: str, query: str, ignore_case: bool = False) -> list: ...
def _docx_help(topic: str | None = None) -> str: ...
def _docx_review(path: str, state: dict) -> dict: ...
def _docx_commit(commit_key: str) -> None: ...
`;

/**
 * Concise reference doc for agents: generated per-operation signatures plus
 * the spec's V1 example (author, change_mode, ReplaceText/FormatText,
 * review, commit).
 */
export const PYTHON_API_REFERENCE: string = `
# docxdriver Python REPL API — Version 1 (Python-authored plan)

# BEGIN GENERATED OPERATION DOCS
## Operations

Selected from the core schema (\`$defs.editOp.oneOf\`) through the
capability manifest; field shapes, types, and enums are exactly the
checked-in schema’s. Every op dataclass has \`to_dict()\`, omitting
optional fields whose value is \`None\`; constructors use \`with_\`/\`as_\`
for Python keyword collisions and \`to_dict()\` maps back to the schema
field names. Operations are addressed by the paragraph \`id\` attribute
from \`docx_read\` (uppercase eight-digit hex) or a \`$name\` alias.

### ClearFooter

\`ClearFooter(kind: str | None = None, section: int | None = None)\`

Remove the explicit footer reference for a section/kind (defaults: section 1, kind default).

| Field | Type | Required |
| --- | --- | --- |
| \`kind\` | \`str | None\` | no |
| \`section\` | \`int | None\` | no |

### ClearHeader

\`ClearHeader(kind: str | None = None, section: int | None = None)\`

Remove the explicit header reference for a section/kind (defaults: section 1, kind default).

| Field | Type | Required |
| --- | --- | --- |
| \`kind\` | \`str | None\` | no |
| \`section\` | \`int | None\` | no |

### CommentAdd

\`CommentAdd(at: str, text: str, occurrence: int | None = None, select: str | None = None)\`

Anchor a comment with \`text\` to the paragraph at \`at\`; omit \`select\` to comment the whole paragraph, or pass \`select\`/\`occurrence\` (1-based) to pin one exact span.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`text\` | \`str\` | yes |
| \`occurrence\` | \`int | None\` | no |
| \`select\` | \`str | None\` | no |

### CommentDelete

\`CommentDelete(comment_id: str)\`

Delete a comment by decimal \`comment_id\`.

| Field | Type | Required |
| --- | --- | --- |
| \`comment_id\` | \`str\` | yes |

### CommentReply

\`CommentReply(comment_id: str, text: str)\`

Reply to an existing comment; \`comment_id\` is the decimal id from a prior \`comment_add\` or a comments-kind read.

| Field | Type | Required |
| --- | --- | --- |
| \`comment_id\` | \`str\` | yes |
| \`text\` | \`str\` | yes |

### CommentSetStatus

\`CommentSetStatus(comment_id: str, status: str)\`

Set a comment's status to \`open\` or \`resolved\` by decimal \`comment_id\`.

| Field | Type | Required |
| --- | --- | --- |
| \`comment_id\` | \`str\` | yes |
| \`status\` | \`str\` | yes |

### DeleteEquation

\`DeleteEquation(at: str, equation: int | None = None)\`

Delete a current equation while preserving the paragraph. Read docx_read(path).equations for at and equation (1-based). Omit equation only when the paragraph has one. Tracked mode keeps the equation recoverable until the deletion is accepted.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`equation\` | \`int | None\` | no |

### DeleteParagraphs

\`DeleteParagraphs(at: list)\`

Delete the paragraphs listed in \`at\` (tracked deletion by default).

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`list\` | yes |

### FormatParagraph

\`FormatParagraph(at: str, alignment: str | None = None, clear: list | None = None, indent_left: float | None = None, indent_right: float | None = None, line_spacing: dict | None = None, space_after: float | None = None, space_before: float | None = None, style: str | None = None)\`

Apply paragraph formatting to the paragraph at \`at\`: \`style\`, \`alignment\` (left/center/right/justify), \`indent_left\`/\`indent_right\`, \`space_before\`/\`space_after\`, \`line_spacing\` (dict with \`mode\` multiple/exact/at_least and \`value\`), or \`clear\`.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`alignment\` | \`str | None\` | no |
| \`clear\` | \`list | None\` | no |
| \`indent_left\` | \`float | None\` | no |
| \`indent_right\` | \`float | None\` | no |
| \`line_spacing\` | \`dict | None\` | no |
| \`space_after\` | \`float | None\` | no |
| \`space_before\` | \`float | None\` | no |
| \`style\` | \`str | None\` | no |

### FormatText

\`FormatText(at: str, select: str, bold: bool | None = None, clear: list | None = None, color: str | None = None, font_size: float | None = None, italic: bool | None = None, occurrence: int | None = None, strike: bool | None = None, subscript: bool | None = None, superscript: bool | None = None, underline: bool | None = None)\`

Apply character formatting to an exact occurrence of \`select\` in the paragraph at \`at\`: boolean attributes (\`bold\`, \`italic\`, \`underline\`, \`strike\`, \`superscript\`, \`subscript\`), \`color\` (#RRGGBB), \`font_size\` (points), \`occurrence\`, or \`clear\`.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`select\` | \`str\` | yes |
| \`bold\` | \`bool | None\` | no |
| \`clear\` | \`list | None\` | no |
| \`color\` | \`str | None\` | no |
| \`font_size\` | \`float | None\` | no |
| \`italic\` | \`bool | None\` | no |
| \`occurrence\` | \`int | None\` | no |
| \`strike\` | \`bool | None\` | no |
| \`subscript\` | \`bool | None\` | no |
| \`superscript\` | \`bool | None\` | no |
| \`underline\` | \`bool | None\` | no |

### InsertParagraph

\`InsertParagraph(at: str, position: str, as_: str | None = None, style: str | None = None, with_: str | Inline | Quote | Term | None = None)\`

Insert a new paragraph before or after an anchor: paragraph id/alias (\`at\`) or body-level table number (\`table:N\` from markup); \`with\` is its text, \`style\` its style, and \`as\` names the new paragraph id (a \`$name\` alias).

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`position\` | \`str\` | yes |
| \`as_\` (schema key \`as\`) | \`str | None\` | no |
| \`style\` | \`str | None\` | no |
| \`with_\` (schema key \`with\`) | \`str | Inline | Quote | Term | None\` | no |

### ReplaceEquation

\`ReplaceEquation(at: str, mathml: str, display: bool | None = None, equation: int | None = None)\`

Replace a current equation using presentation MathML. Read docx_read(path).equations for at and equation (1-based). Omit equation only when the paragraph has one. Display mode is preserved by default. Settle existing revisions before editing their equations.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`mathml\` | \`str\` | yes |
| \`display\` | \`bool | None\` | no |
| \`equation\` | \`int | None\` | no |

### ReplaceParagraph

\`ReplaceParagraph(at: str, with_: str | Inline | Quote | Term)\`

Replace the entire paragraph at \`at\` with \`with\`.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`with_\` (schema key \`with\`) | \`str | Inline | Quote | Term\` | yes |

### ReplaceText

\`ReplaceText(at: str, select: str, with_: str | Inline | Quote | Term, occurrence: int | None = None)\`

Replace an exact occurrence of \`select\` in the paragraph at \`at\` with \`with\`; \`occurrence\` picks the nth match (1-based).

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`select\` | \`str\` | yes |
| \`with_\` (schema key \`with\`) | \`str | Inline | Quote | Term\` | yes |
| \`occurrence\` | \`int | None\` | no |

### SetEvenAndOddHeaders

\`SetEvenAndOddHeaders(even_and_odd: bool)\`

Enable or disable distinct even-page headers/footers (\`even_and_odd\`); when true, \`kind="even"\` chrome applies to even pages.

| Field | Type | Required |
| --- | --- | --- |
| \`even_and_odd\` | \`bool\` | yes |

### SetFooter

\`SetFooter(with_: str | Inline | Quote | Term, kind: str | None = None, section: int | None = None)\`

Set section footer chrome from inner HTML (\`with\`); optional \`section\` (default 1) and \`kind\` (default/even/first).

| Field | Type | Required |
| --- | --- | --- |
| \`with_\` (schema key \`with\`) | \`str | Inline | Quote | Term\` | yes |
| \`kind\` | \`str | None\` | no |
| \`section\` | \`int | None\` | no |

### SetHeader

\`SetHeader(with_: str | Inline | Quote | Term, kind: str | None = None, section: int | None = None)\`

Set section header chrome from inner HTML (\`with\`); optional \`section\` (default 1) and \`kind\` (default/even/first).

| Field | Type | Required |
| --- | --- | --- |
| \`with_\` (schema key \`with\`) | \`str | Inline | Quote | Term\` | yes |
| \`kind\` | \`str | None\` | no |
| \`section\` | \`int | None\` | no |

# END GENERATED OPERATION DOCS

Every operation dataclass has \`to_dict()\`, omitting optional fields whose
value is \`None\`; \`Plan.to_dict()\` returns \`{"author": ..., "change_mode": ..., "ops": [...]}\`.

## Host functions

- \`docx_create(path: str, html: str)\`
- \`docx_read(path: str, view: str = "markup", kind: str = "document") -> ReadResult | dict\` — complete HTML
  markup and a current equations-address listing (at, equation, display, editable) by default (\`kind="document"\`); \`kind="comments"\`, \`"styles"\`, \`"revisions"\`, or \`"assets"\` returns the
  core listing object (and a second positional of \`comments\`/\`styles\`/\`revisions\` is treated as \`kind\`).
- \`docx_find(path: str, query: str, ignore_case: bool = False) -> list\` — core find match
  records verbatim (\`{index, id?, text, style?, number?}\` per matching paragraph; \`id\` is
  the canonical uppercase paragraph address)
- \`docx_help(topic: str | None = None) -> str\`
- \`docx_review(path: str, plan: Plan) -> ReviewResult | None\` — validates the complete plan and returns \`commit_key\` plus bounded per-operation \`edits\` with affected markup context; returns \`None\` when validation is blocked
- \`docx_commit(commit_key: str)\` — commits the immutable reviewed plan; the key owns the
  target file and plan
Review does not write the target. Review and commit must happen in separate later
Python calls; changing the plan requires a new review.

## Discovering paragraphs

\`docx_find\` plus \`docx_read\` are the supported discovery workflow:
find returns core match records to locate paragraphs, and reads return
structural markup. Operations stay addressed
by the markup-view paragraph \`id\`:

    matches = docx_find("contract.docx", "Payment is due within")
    for m in matches:
        print(m["id"], m["text"])
    markup = docx_read("contract.docx").markup

Reading \`view="final"\` or \`view="original"\` is allowed for analysis, but
review plans remain source-bound to the exact bytes and paragraph ids of the
current \`markup\` view.

## Computing operations with regex

\`re\` is pre-imported and \`ReadResult.markup\` is an ordinary string, so you can
inspect it and build operations in Python loops. Return the full ReadResult or
print its full markup and semantic metadata when reading; prefer complete document
context. Named DOCX style IDs appear in data-docx-style and map one-to-one to
ReadResult.styles (resolved CSS keyed by style ID). Presentation rules appear
in data-docx-css and map to ReadResult.css. Both catalogs are omitted from the
default display; inspect result.styles or result.css to read their definitions;
when authoring a fragment, use equivalent inline formatting rather than copying
an unresolved class. Image aliases map to canonical assets in ReadResult.assets.
ReadResult.selection_space describes comment selectors; ReadResult.selections
provides exact text for paragraphs where generated content or revisions make
HTML text ambiguous:


    original = docx_read("contract.docx").markup
    ops = []
    for m in re.finditer(r'<p id="([0-9A-F]{8})"[^>]*>([^<]*?)</p>', original):
        if "Payment is due within" in m.group(2):
            ops.append(ReplaceText(at=m.group(1), select=m.group(2),
                                   with_=re.sub(r'within \\d+ days', 'within 45 days', m.group(2))))
    plan = Plan(author="Pi", operations=ops)
    review = docx_review("contract.docx", plan)
    if review is not None:
        for edit in review.edits:
            print(edit["index"], edit["context"])
        # ...later python call...
        docx_commit(review.commit_key)

\`select\` must match the paragraph's current visible text exactly; on a document
with pending tracked changes, grep the \`view="final"\` projection instead.

## Example

    original = docx_read("contract.docx").markup
    plan = Plan(author="Pi", change_mode="track", operations=[
        ReplaceText(at="2673269E", select="thirty days", with_="sixty days"),
        FormatText(at="2673269E", select="sixty days", bold=True),
    ])
    review = docx_review("contract.docx", plan)
    # Inspect review.edits before committing in a later Python call.
    if review is not None:
        print(review.commit_key)
`;
