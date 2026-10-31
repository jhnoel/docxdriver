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
class BookmarkAdd:
    at: str
    name: str
    occurrence: int | None = None
    select: str | None = None
    def to_dict(self):
        value = {"op": "bookmark_add"}
        if self.at is not None:
            value["at"] = self.at
        if self.name is not None:
            value["name"] = self.name
        if self.occurrence is not None:
            value["occurrence"] = self.occurrence
        if self.select is not None:
            value["select"] = self.select
        return value

@dataclass
class BookmarkDelete:
    name: str
    def to_dict(self):
        value = {"op": "bookmark_delete"}
        if self.name is not None:
            value["name"] = self.name
        return value

@dataclass
class BookmarkMove:
    at: str
    name: str
    occurrence: int | None = None
    select: str | None = None
    def to_dict(self):
        value = {"op": "bookmark_move"}
        if self.at is not None:
            value["at"] = self.at
        if self.name is not None:
            value["name"] = self.name
        if self.occurrence is not None:
            value["occurrence"] = self.occurrence
        if self.select is not None:
            value["select"] = self.select
        return value

@dataclass
class BookmarkRename:
    name: str
    new_name: str
    def to_dict(self):
        value = {"op": "bookmark_rename"}
        if self.name is not None:
            value["name"] = self.name
        if self.new_name is not None:
            value["new_name"] = self.new_name
        return value

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
class ConvertRevisionsToFormatting:
    target: str | dict
    at: list | None = None
    def to_dict(self):
        value = {"op": "convert_revisions_to_formatting"}
        if self.target is not None:
            value["target"] = self.target
        if self.at is not None:
            value["at"] = self.at
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
class EditTable:
    action: dict
    at: str
    def to_dict(self):
        value = {"op": "edit_table"}
        if self.action is not None:
            value["action"] = self.action
        if self.at is not None:
            value["at"] = self.at
        return value

@dataclass
class FormatCell:
    at: str
    format: dict
    def to_dict(self):
        value = {"op": "format_cell"}
        if self.at is not None:
            value["at"] = self.at
        if self.format is not None:
            value["format"] = self.format
        return value

@dataclass
class FormatParagraph:
    at: str
    alignment: str | None = None
    clear: list | None = None
    first_line_indent: float | None = None
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
        if self.first_line_indent is not None:
            value["first_line_indent"] = self.first_line_indent
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
class FormatRow:
    at: str
    format: dict
    def to_dict(self):
        value = {"op": "format_row"}
        if self.at is not None:
            value["at"] = self.at
        if self.format is not None:
            value["format"] = self.format
        return value

@dataclass
class FormatTable:
    at: str
    format: dict
    def to_dict(self):
        value = {"op": "format_table"}
        if self.at is not None:
            value["at"] = self.at
        if self.format is not None:
            value["format"] = self.format
        return value

@dataclass
class FormatText:
    at: str
    select: str
    bold: bool | None = None
    clear: list | None = None
    color: str | None = None
    font_size: float | None = None
    highlight: str | None = None
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
        if self.highlight is not None:
            value["highlight"] = self.highlight
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
    with_: str | Inline | Quote | Term | list[str | Inline | Quote | Term] | None = None
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
            value["with"] = [v.to_dict() if hasattr(v, "to_dict") else v for v in self.with_] if isinstance(self.with_, list) else self.with_.to_dict() if hasattr(self.with_, "to_dict") else self.with_
        return value

@dataclass
class InsertTable:
    at: str
    columns: int
    position: str
    rows: int
    as_: str | None = None
    cells: list | None = None
    def to_dict(self):
        value = {"op": "insert_table"}
        if self.at is not None:
            value["at"] = self.at
        if self.columns is not None:
            value["columns"] = self.columns
        if self.position is not None:
            value["position"] = self.position
        if self.rows is not None:
            value["rows"] = self.rows
        if self.as_ is not None:
            value["as"] = self.as_
        if self.cells is not None:
            value["cells"] = self.cells
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
class ReviseText:
    edits: list
    at: str | None = None
    occurrence: int | None = None
    within: str | None = None
    def to_dict(self):
        value = {"op": "revise_text"}
        if self.edits is not None:
            value["edits"] = self.edits
        if self.at is not None:
            value["at"] = self.at
        if self.occurrence is not None:
            value["occurrence"] = self.occurrence
        if self.within is not None:
            value["within"] = self.within
        return value

@dataclass
class RevisionSettle:
    action: str
    target: str
    def to_dict(self):
        value = {"op": "revision_settle"}
        if self.action is not None:
            value["action"] = self.action
        if self.target is not None:
            value["target"] = self.target
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
class SetFirstPageMode:
    enabled: bool
    section: int | None = None
    def to_dict(self):
        value = {"op": "set_first_page_mode"}
        if self.enabled is not None:
            value["enabled"] = self.enabled
        if self.section is not None:
            value["section"] = self.section
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

@dataclass
class SetList:
    at: list
    format: str | None = None
    level: int | None = None
    levels: list | None = None
    num_id: int | None = None
    remove: bool | None = None
    start: int | None = None
    def to_dict(self):
        value = {"op": "set_list"}
        if self.at is not None:
            value["at"] = self.at
        if self.format is not None:
            value["format"] = self.format
        if self.level is not None:
            value["level"] = self.level
        if self.levels is not None:
            value["levels"] = self.levels
        if self.num_id is not None:
            value["num_id"] = self.num_id
        if self.remove is not None:
            value["remove"] = self.remove
        if self.start is not None:
            value["start"] = self.start
        return value

@dataclass
class SetPageMargins:
    bottom: float | None = None
    footer: float | None = None
    gutter: float | None = None
    header: float | None = None
    left: float | None = None
    right: float | None = None
    section: int | None = None
    top: float | None = None
    def to_dict(self):
        value = {"op": "set_page_margins"}
        if self.bottom is not None:
            value["bottom"] = self.bottom
        if self.footer is not None:
            value["footer"] = self.footer
        if self.gutter is not None:
            value["gutter"] = self.gutter
        if self.header is not None:
            value["header"] = self.header
        if self.left is not None:
            value["left"] = self.left
        if self.right is not None:
            value["right"] = self.right
        if self.section is not None:
            value["section"] = self.section
        if self.top is not None:
            value["top"] = self.top
        return value

@dataclass
class SetSectionBoundary:
    at: str
    break_type: str | None = None
    remove: bool | None = None
    def to_dict(self):
        value = {"op": "set_section_boundary"}
        if self.at is not None:
            value["at"] = self.at
        if self.break_type is not None:
            value["break_type"] = self.break_type
        if self.remove is not None:
            value["remove"] = self.remove
        return value

@dataclass
class SetSectionLayout:
    break_type: str | None = None
    column_separator: bool | None = None
    column_spacing: float | None = None
    column_widths: list | None = None
    columns: int | None = None
    height: float | None = None
    orientation: str | None = None
    page_number_continue: bool | None = None
    page_number_format: str | None = None
    page_number_start: int | None = None
    page_size: str | None = None
    section: int | None = None
    section_handle: str | None = None
    width: float | None = None
    def to_dict(self):
        value = {"op": "set_section_layout"}
        if self.break_type is not None:
            value["break_type"] = self.break_type
        if self.column_separator is not None:
            value["column_separator"] = self.column_separator
        if self.column_spacing is not None:
            value["column_spacing"] = self.column_spacing
        if self.column_widths is not None:
            value["column_widths"] = self.column_widths
        if self.columns is not None:
            value["columns"] = self.columns
        if self.height is not None:
            value["height"] = self.height
        if self.orientation is not None:
            value["orientation"] = self.orientation
        if self.page_number_continue is not None:
            value["page_number_continue"] = self.page_number_continue
        if self.page_number_format is not None:
            value["page_number_format"] = self.page_number_format
        if self.page_number_start is not None:
            value["page_number_start"] = self.page_number_start
        if self.page_size is not None:
            value["page_size"] = self.page_size
        if self.section is not None:
            value["section"] = self.section
        if self.section_handle is not None:
            value["section_handle"] = self.section_handle
        if self.width is not None:
            value["width"] = self.width
        return value

# END GENERATED DATACLASSES

@dataclass
class Revise:
    within: str | None = None
    at: str | None = None
    occurrence: int | None = None
    edits: list = None
    def replace(self, select: str, with_, occurrence: int | None = None, all: bool = False):
        edit = {"select": select, "with": with_.to_dict() if hasattr(with_, "to_dict") else with_}
        if occurrence is not None:
            edit["occurrence"] = occurrence
        if all:
            edit["all"] = True
        self.edits = (self.edits or []) + [edit]
        return self
    def to_dict(self):
        return ReviseText(edits=self.edits or [], at=self.at, occurrence=self.occurrence, within=self.within).to_dict()

def AcceptRevision(target: str) -> RevisionSettle:
    return RevisionSettle(action="accept", target=str(target))

def RejectRevision(target: str) -> RevisionSettle:
    return RevisionSettle(action="reject", target=str(target))

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
    default_style: str = None
    def __repr__(self):
        return "ReadResult(markup=" + repr(self.markup) + ", styles=" + str(len(self.styles or {})) + " definitions; inspect .styles, default_style=" + repr(self.default_style) + ", equations=" + repr(self.equations) + ", selection_space=" + repr(self.selection_space) + ", selections=" + repr(self.selections) + ", comments=" + repr(self.comments) + ", assets=" + repr(self.assets) + ", comments_error=" + repr(self.comments_error) + ")"

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
    _listing_kinds = ("comments", "styles", "revisions", "bookmarks", "assets")
    if kind == "document" and view in _listing_kinds:
        kind = view
    if kind in _listing_kinds:
        return _docx_read(path, view, kind)
    if kind != "document":
        raise ValueError('kind must be one of document, comments, styles, revisions, bookmarks, assets')
    result = _docx_read(path, view)
    return ReadResult(markup=result["markup"], styles=result.get("styles", {}), default_style=result.get("default_style"), equations=result.get("equations", []), selection_space=result.get("selection_space"), selections=result.get("selections"), comments=result.get("comments", []), assets=result.get("assets", []), comments_error=result.get("comments_error"))

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
class BookmarkAdd:
    def __init__(self, at: str, name: str, occurrence: int | None = None, select: str | None = None): ...
    at: str
    name: str
    occurrence: int | None
    select: str | None
    def to_dict(self) -> dict: ...

class BookmarkDelete:
    def __init__(self, name: str): ...
    name: str
    def to_dict(self) -> dict: ...

class BookmarkMove:
    def __init__(self, at: str, name: str, occurrence: int | None = None, select: str | None = None): ...
    at: str
    name: str
    occurrence: int | None
    select: str | None
    def to_dict(self) -> dict: ...

class BookmarkRename:
    def __init__(self, name: str, new_name: str): ...
    name: str
    new_name: str
    def to_dict(self) -> dict: ...

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

class ConvertRevisionsToFormatting:
    def __init__(self, target: str | dict, at: list | None = None): ...
    target: str | dict
    at: list | None
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

class EditTable:
    def __init__(self, action: dict, at: str): ...
    action: dict
    at: str
    def to_dict(self) -> dict: ...

class FormatCell:
    def __init__(self, at: str, format: dict): ...
    at: str
    format: dict
    def to_dict(self) -> dict: ...

class FormatParagraph:
    def __init__(self, at: str, alignment: str | None = None, clear: list | None = None, first_line_indent: float | None = None, indent_left: float | None = None, indent_right: float | None = None, line_spacing: dict | None = None, space_after: float | None = None, space_before: float | None = None, style: str | None = None): ...
    at: str
    alignment: str | None
    clear: list | None
    first_line_indent: float | None
    indent_left: float | None
    indent_right: float | None
    line_spacing: dict | None
    space_after: float | None
    space_before: float | None
    style: str | None
    def to_dict(self) -> dict: ...

class FormatRow:
    def __init__(self, at: str, format: dict): ...
    at: str
    format: dict
    def to_dict(self) -> dict: ...

class FormatTable:
    def __init__(self, at: str, format: dict): ...
    at: str
    format: dict
    def to_dict(self) -> dict: ...

class FormatText:
    def __init__(self, at: str, select: str, bold: bool | None = None, clear: list | None = None, color: str | None = None, font_size: float | None = None, highlight: str | None = None, italic: bool | None = None, occurrence: int | None = None, strike: bool | None = None, subscript: bool | None = None, superscript: bool | None = None, underline: bool | None = None): ...
    at: str
    select: str
    bold: bool | None
    clear: list | None
    color: str | None
    font_size: float | None
    highlight: str | None
    italic: bool | None
    occurrence: int | None
    strike: bool | None
    subscript: bool | None
    superscript: bool | None
    underline: bool | None
    def to_dict(self) -> dict: ...

class InsertParagraph:
    def __init__(self, at: str, position: str, as_: str | None = None, style: str | None = None, with_: str | Inline | Quote | Term | list[str | Inline | Quote | Term] | None = None): ...
    at: str
    position: str
    as_: str | None
    style: str | None
    with_: str | Inline | Quote | Term | list[str | Inline | Quote | Term] | None
    def to_dict(self) -> dict: ...

class InsertTable:
    def __init__(self, at: str, columns: int, position: str, rows: int, as_: str | None = None, cells: list | None = None): ...
    at: str
    columns: int
    position: str
    rows: int
    as_: str | None
    cells: list | None
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

class ReviseText:
    def __init__(self, edits: list, at: str | None = None, occurrence: int | None = None, within: str | None = None): ...
    edits: list
    at: str | None
    occurrence: int | None
    within: str | None
    def to_dict(self) -> dict: ...

class RevisionSettle:
    def __init__(self, action: str, target: str): ...
    action: str
    target: str
    def to_dict(self) -> dict: ...

class SetEvenAndOddHeaders:
    def __init__(self, even_and_odd: bool): ...
    even_and_odd: bool
    def to_dict(self) -> dict: ...

class SetFirstPageMode:
    def __init__(self, enabled: bool, section: int | None = None): ...
    enabled: bool
    section: int | None
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

class SetList:
    def __init__(self, at: list, format: str | None = None, level: int | None = None, levels: list | None = None, num_id: int | None = None, remove: bool | None = None, start: int | None = None): ...
    at: list
    format: str | None
    level: int | None
    levels: list | None
    num_id: int | None
    remove: bool | None
    start: int | None
    def to_dict(self) -> dict: ...

class SetPageMargins:
    def __init__(self, bottom: float | None = None, footer: float | None = None, gutter: float | None = None, header: float | None = None, left: float | None = None, right: float | None = None, section: int | None = None, top: float | None = None): ...
    bottom: float | None
    footer: float | None
    gutter: float | None
    header: float | None
    left: float | None
    right: float | None
    section: int | None
    top: float | None
    def to_dict(self) -> dict: ...

class SetSectionBoundary:
    def __init__(self, at: str, break_type: str | None = None, remove: bool | None = None): ...
    at: str
    break_type: str | None
    remove: bool | None
    def to_dict(self) -> dict: ...

class SetSectionLayout:
    def __init__(self, break_type: str | None = None, column_separator: bool | None = None, column_spacing: float | None = None, column_widths: list | None = None, columns: int | None = None, height: float | None = None, orientation: str | None = None, page_number_continue: bool | None = None, page_number_format: str | None = None, page_number_start: int | None = None, page_size: str | None = None, section: int | None = None, section_handle: str | None = None, width: float | None = None): ...
    break_type: str | None
    column_separator: bool | None
    column_spacing: float | None
    column_widths: list | None
    columns: int | None
    height: float | None
    orientation: str | None
    page_number_continue: bool | None
    page_number_format: str | None
    page_number_start: int | None
    page_size: str | None
    section: int | None
    section_handle: str | None
    width: float | None
    def to_dict(self) -> dict: ...

# END GENERATED TYPE STUBS

class Revise:
    def __init__(self, within: str | None = None, at: str | None = None, occurrence: int | None = None, edits: list | None = None): ...
    within: str | None
    at: str | None
    occurrence: int | None
    edits: list | None
    def replace(self, select: str, with_: str | Inline | Quote | Term, occurrence: int | None = None, all: bool = False) -> Revise: ...
    def to_dict(self) -> dict: ...

def AcceptRevision(target: str) -> RevisionSettle: ...
def RejectRevision(target: str) -> RevisionSettle: ...

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
    def __init__(self, markup: str, equations: list | None = None, selection_space: dict | None = None, selections: dict | None = None, comments: list | None = None, assets: list | None = None, comments_error: str | None = None, styles: dict | None = None, default_style: str | None = None): ...
    markup: str
    styles: dict
    default_style: str | None
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

### BookmarkAdd

\`BookmarkAdd(at: str, name: str, occurrence: int | None = None, select: str | None = None)\`

Bookmark the paragraph at \`at\` with \`name\`: omit \`select\` to wrap the whole paragraph, or pass \`select\`/\`occurrence\` (1-based) to wrap one exact span. Names start with a letter, then letters, digits or underscores, at most 40 characters, unique ignoring case; a leading \`_\` is reserved for hidden bookmarks. Applies directly in both change modes (Word does not track bookmarks). Body paragraphs only.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`name\` | \`str\` | yes |
| \`occurrence\` | \`int | None\` | no |
| \`select\` | \`str | None\` | no |

### BookmarkDelete

\`BookmarkDelete(name: str)\`

Remove the body bookmark \`name\`: its start and end markers only; the text is untouched.

| Field | Type | Required |
| --- | --- | --- |
| \`name\` | \`str\` | yes |

### BookmarkMove

\`BookmarkMove(at: str, name: str, occurrence: int | None = None, select: str | None = None)\`

Re-anchor the body bookmark \`name\` to the paragraph at \`at\` (whole paragraph, or \`select\`/\`occurrence\`), keeping its name and id.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`name\` | \`str\` | yes |
| \`occurrence\` | \`int | None\` | no |
| \`select\` | \`str | None\` | no |

### BookmarkRename

\`BookmarkRename(name: str, new_name: str)\`

Rename the body bookmark \`name\` to \`new_name\` (same rules as bookmark_add); hidden bookmarks may be renamed from but not to.

| Field | Type | Required |
| --- | --- | --- |
| \`name\` | \`str\` | yes |
| \`new_name\` | \`str\` | yes |

### ClearFooter

\`ClearFooter(kind: str | None = None, section: int | None = None)\`

Remove the explicit footer reference for a section/kind (defaults: section 1, kind default). Tracked as clear_header.

| Field | Type | Required |
| --- | --- | --- |
| \`kind\` | \`str | None\` | no |
| \`section\` | \`int | None\` | no |

### ClearHeader

\`ClearHeader(kind: str | None = None, section: int | None = None)\`

Remove the explicit header reference for a section/kind (defaults: section 1, kind default). Track mode deletes the content of the header as a tracked deletion instead (section 1 only; the empty header stays referenced).

| Field | Type | Required |
| --- | --- | --- |
| \`kind\` | \`str | None\` | no |
| \`section\` | \`int | None\` | no |

### CommentAdd

\`CommentAdd(at: str, text: str, occurrence: int | None = None, select: str | None = None)\`

Anchor a comment with \`text\` to the paragraph at \`at\`; omit \`select\` to comment the whole paragraph, or pass \`select\`/\`occurrence\` (1-based) to pin one exact span. Applies directly in both change modes (Word does not track comments).

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

### ConvertRevisionsToFormatting

\`ConvertRevisionsToFormatting(target: str | dict, at: list | None = None)\`

Convert tracked changes to amendment formatting (37 CFR 1.121): inserted text stays underlined, deleted text comes back struck through, and no revision is left. \`target\` is \`all\`, \`{"id": "<revision id>"}\` or \`{"author": "<name>"}\`; optional \`at\` limits it to revisions in those paragraphs. Inserted or deleted paragraphs and table rows stay (text underlined or struck); text deleted inside a converted insertion goes; formatting, table and section property changes are accepted; existing underline and strike are kept. A replaced word gets one plain space between its struck and underlined text. Make ordinary tracked edits first, then convert. Writes no revision in either change mode; other revisions of other authors convert only when targeted.

| Field | Type | Required |
| --- | --- | --- |
| \`target\` | \`str | dict\` | yes |
| \`at\` | \`list | None\` | no |

### DeleteEquation

\`DeleteEquation(at: str, equation: int | None = None)\`

Delete a current equation while preserving the paragraph. Read docx_read(path).equations for at and equation (1-based). Omit equation only when the paragraph has one. Tracked mode keeps the equation recoverable until the deletion is accepted.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`equation\` | \`int | None\` | no |

### DeleteParagraphs

\`DeleteParagraphs(at: list)\`

Delete the paragraphs listed in \`at\`. Track mode: a tracked deletion; accepting it leaves the id, style and list membership of the following paragraph intact. Direct mode: removed with no revision.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`list\` | yes |

### EditTable

\`EditTable(action: dict, at: str)\`

Structural table edits using a source-bound table handle. action.kind is insert_rows/delete_rows/insert_columns/delete_columns/merge_cells/split_cell/delete; coordinates and counts are one-based. Insert/delete grid operations and merge require an unmerged rectangular grid. Split keeps content in the first resulting cell; merge concatenates content in row-major order. Insert actions optionally accept as for a group alias: inserted rows use $name/row:N[/cell:N]; inserted columns use $name/row:N/cell:N within the inserted strip. Track mode records insert_rows/delete_rows/delete as tracked row insertions/deletions and refuses insert_columns/delete_columns/merge_cells/split_cell (Word does not track them); direct mode applies every action with no revision. Rejects tables containing unresolved revisions of other authors, bookmarks, comments, fields or content controls. Delete the table instead of its final row or column.

| Field | Type | Required |
| --- | --- | --- |
| \`action\` | \`dict\` | yes |
| \`at\` | \`str\` | yes |

### FormatCell

\`FormatCell(at: str, format: dict)\`

Format a source-bound table:N/row:N/cell:N handle using width, borders, fill (#RRGGBB), padding, vertical_alignment (top/center/bottom), and clear. Cell indices are physical; a vertically merged logical cell is addressed through its restart cell and applies to all backing continuation cells. Continuation targets and legacy hMerge targets are rejected. Tracked plans preserve prior cell property bags. Text, merge structure and nested content remain unchanged.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`format\` | \`dict\` | yes |

### FormatParagraph

\`FormatParagraph(at: str, alignment: str | None = None, clear: list | None = None, first_line_indent: float | None = None, indent_left: float | None = None, indent_right: float | None = None, line_spacing: dict | None = None, space_after: float | None = None, space_before: float | None = None, style: str | None = None)\`

Apply paragraph formatting to the paragraph at \`at\`: \`style\`, \`alignment\` (left/center/right/justify), signed \`indent_left\`/\`indent_right\`, signed \`first_line_indent\` (negative for hanging, zero for an explicit zero offset), \`space_before\`/\`space_after\`, \`line_spacing\` (dict with \`mode\` multiple/exact/at_least and \`value\`), or \`clear\`. Omitted properties are untouched; clear removes direct settings and inherits styles/defaults, winning over set. Track mode records the change (rPrChange/pPrChange); direct mode applies it with no revision.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`alignment\` | \`str | None\` | no |
| \`clear\` | \`list | None\` | no |
| \`first_line_indent\` | \`float | None\` | no |
| \`indent_left\` | \`float | None\` | no |
| \`indent_right\` | \`float | None\` | no |
| \`line_spacing\` | \`dict | None\` | no |
| \`space_after\` | \`float | None\` | no |
| \`space_before\` | \`float | None\` | no |
| \`style\` | \`str | None\` | no |

### FormatRow

\`FormatRow(at: str, format: dict)\`

Format a source-bound table:N/row:N handle. format accepts height in points, height_rule (auto/at_least/exact), repeating_header, allow_split and clear. False explicitly disables header repetition or splitting; clear inherits. Tracked plans preserve the complete prior row property bag for rejection.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`format\` | \`dict\` | yes |

### FormatTable

\`FormatTable(at: str, format: dict)\`

Format the table at a source-bound table:N handle or inserted $alias. format accepts width (unit auto, points/value, percent/value), column_widths (positive points, exactly matching the grid), layout (fixed/autofit), alignment, borders, cell_spacing, padding and an existing table style ID. Omitted values preserve source properties; clear removes supported direct settings and wins over set. Grid dimensions cannot be cleared. Tracked plans snapshot table and grid properties; settle a pending property change before editing that bag again.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`format\` | \`dict\` | yes |

### FormatText

\`FormatText(at: str, select: str, bold: bool | None = None, clear: list | None = None, color: str | None = None, font_size: float | None = None, highlight: str | None = None, italic: bool | None = None, occurrence: int | None = None, strike: bool | None = None, subscript: bool | None = None, superscript: bool | None = None, underline: bool | None = None)\`

Apply character formatting to an exact occurrence of \`select\` in the paragraph at \`at\`: boolean attributes (\`bold\`, \`italic\`, \`underline\`, \`strike\`, \`superscript\`, \`subscript\`), \`color\` (#RRGGBB), \`highlight\` (Word palette including none), \`font_size\` (points), \`occurrence\`, or \`clear\`. Omitted properties preserve source OOXML; false explicitly overrides inherited formatting (baseline for superscript/subscript). Clear removes the direct property and inherits styles/defaults, winning over set; superscript/subscript share one property. Track mode records the change (rPrChange/pPrChange); direct mode applies it with no revision.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`select\` | \`str\` | yes |
| \`bold\` | \`bool | None\` | no |
| \`clear\` | \`list | None\` | no |
| \`color\` | \`str | None\` | no |
| \`font_size\` | \`float | None\` | no |
| \`highlight\` | \`str | None\` | no |
| \`italic\` | \`bool | None\` | no |
| \`occurrence\` | \`int | None\` | no |
| \`strike\` | \`bool | None\` | no |
| \`subscript\` | \`bool | None\` | no |
| \`superscript\` | \`bool | None\` | no |
| \`underline\` | \`bool | None\` | no |

### InsertParagraph

\`InsertParagraph(at: str, position: str, as_: str | None = None, style: str | None = None, with_: str | Inline | Quote | Term | list[str | Inline | Quote | Term] | None = None)\`

Insert a new paragraph before or after an anchor: paragraph id/alias (\`at\`) or body-level table number (\`table:N\` from markup); \`with\` is its text, or a list of texts inserted as consecutive paragraphs in list order; \`style\` is the style of every new paragraph (omitted, each new paragraph takes the paragraph properties of the anchor as the Enter key in Word does, so it continues a list; after a heading it takes the next style the heading style names), and \`as\` names the new paragraph id (a \`$name\` alias), or with a list names the whole group \`$name\` and item N \`$name_N\`; a group expands to all its paragraphs in \`set_list\`/\`delete_paragraphs\`, anchors an insert after its last or before its first paragraph, and is rejected where one paragraph is expected. Several inserts at one anchor and position also keep plan order.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`position\` | \`str\` | yes |
| \`as_\` (schema key \`as\`) | \`str | None\` | no |
| \`style\` | \`str | None\` | no |
| \`with_\` (schema key \`with\`) | \`str | Inline | Quote | Term | list[str | Inline | Quote | Term] | None\` | no |

### InsertTable

\`InsertTable(at: str, columns: int, position: str, rows: int, as_: str | None = None, cells: list | None = None)\`

Insert a rectangular table before/after paragraph at, with rows and columns in 1..100, and optionally fill it in the same plan. cells is a list of rows, each a list of cell strings in insert_paragraph's inline content (<b>, <i>, <a href>, ...); missing trailing rows and cells stay empty, extra rows or cells are an error. Track mode inserts it as a tracked table (inserted rows and content; refused as the last block of a table cell). Optional as creates a plan-local table alias for later formatting and structural edits, and binds each cell's paragraph as $<as>_rNcM (row N, column M, from 1) for later ops in the same plan. Every cell paragraph id is reported in allocated_ids, row by row.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`columns\` | \`int\` | yes |
| \`position\` | \`str\` | yes |
| \`rows\` | \`int\` | yes |
| \`as_\` (schema key \`as\`) | \`str | None\` | no |
| \`cells\` | \`list | None\` | no |

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

### ReviseText

\`ReviseText(edits: list, at: str | None = None, occurrence: int | None = None, within: str | None = None)\`

Several exact replacements inside one bounded scope; prefer the chainable \`Revise(within, at=None, occurrence=None).replace(select, with_, occurrence=None, all=False)\` builder. Scope: \`within\` (a phrase unique in paragraph \`at\`, or in the whole document when \`at\` is omitted; \`occurrence\` picks a repeated one), else the paragraph \`at\`, else the whole document. Each edit must match exactly once in the scope unless it sets \`occurrence\` or \`all\`. All edits resolve against the scope as it stands when the operation starts, must not overlap, and become separate minimal tracked changes.

| Field | Type | Required |
| --- | --- | --- |
| \`edits\` | \`list\` | yes |
| \`at\` | \`str | None\` | no |
| \`occurrence\` | \`int | None\` | no |
| \`within\` | \`str | None\` | no |

### RevisionSettle

\`RevisionSettle(action: str, target: str)\`

Accept or reject tracked changes: \`target\` is a decimal revision id from \`docx_read(path, kind="revisions")\` or \`all\`; \`action\` is \`accept\` or \`reject\`. Prefer the \`AcceptRevision(target)\` / \`RejectRevision(target)\` shorthands. Settling \`all\` also settles tracked changes made by earlier operations in the same plan, and later selectors address the settled text.

| Field | Type | Required |
| --- | --- | --- |
| \`action\` | \`str\` | yes |
| \`target\` | \`str\` | yes |

### SetEvenAndOddHeaders

\`SetEvenAndOddHeaders(even_and_odd: bool)\`

Enable or disable distinct even-page headers/footers (\`even_and_odd\`); when true, \`kind="even"\` chrome applies to even pages. Direct mode only: Word cannot track this document setting, so track mode refuses it.

| Field | Type | Required |
| --- | --- | --- |
| \`even_and_odd\` | \`bool\` | yes |

### SetFirstPageMode

\`SetFirstPageMode(enabled: bool, section: int | None = None)\`

Enable/disable first-page stories in a section (default 1) without removing stored stories. Tracked mode records a section-property snapshot; settle pending section changes before editing again.

| Field | Type | Required |
| --- | --- | --- |
| \`enabled\` | \`bool\` | yes |
| \`section\` | \`int | None\` | no |

### SetFooter

\`SetFooter(with_: str | Inline | Quote | Term, kind: str | None = None, section: int | None = None)\`

Set section footer chrome from inner HTML (\`with\`); optional \`section\` (default 1) and \`kind\` (default/even/first). Tracked as set_header.

| Field | Type | Required |
| --- | --- | --- |
| \`with_\` (schema key \`with\`) | \`str | Inline | Quote | Term\` | yes |
| \`kind\` | \`str | None\` | no |
| \`section\` | \`int | None\` | no |

### SetHeader

\`SetHeader(with_: str | Inline | Quote | Term, kind: str | None = None, section: int | None = None)\`

Set section header chrome from inner HTML (\`with\`); optional \`section\` (default 1) and \`kind\` (default/even/first). Track mode: old content becomes a tracked deletion and the new a tracked insertion (a new header part stays, empty, after reject-all); kind=first tracks titlePg; kind=even is refused while even/odd headers are off.

| Field | Type | Required |
| --- | --- | --- |
| \`with_\` (schema key \`with\`) | \`str | Inline | Quote | Term\` | yes |
| \`kind\` | \`str | None\` | no |
| \`section\` | \`int | None\` | no |

### SetList

\`SetList(at: list, format: str | None = None, level: int | None = None, levels: list | None = None, num_id: int | None = None, remove: bool | None = None, start: int | None = None)\`

Assign explicitly selected paragraphs to a new list (format defaults to decimal), continue an existing num_id, or remove membership with remove=True. level is 0..8; IDs/starts are 1..2147483647. start creates an isolated restarted instance, and on new lists applies only to the selected level (explicit per-level start wins). Optional levels defines 1..9 per-level format/label/start/restart_after, signed indent_left/indent_right/first_line_indent and marker_bold/marker_italic/marker_font/marker_color for a new list. Label placeholders reference this or ancestor levels (%1..%9). Existing definitions are never restyled. Track mode records membership changes (pPrChange), direct mode applies them with no revision; settle pending paragraph changes first. Rejecting a tracked list change removes the definitions it added once nothing refers to them. Typed membership changes on lists containing explicit continuation paragraphs are rejected, including numbered parents; HTML multi-paragraph creation and reconstruction are supported.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`list\` | yes |
| \`format\` | \`str | None\` | no |
| \`level\` | \`int | None\` | no |
| \`levels\` | \`list | None\` | no |
| \`num_id\` | \`int | None\` | no |
| \`remove\` | \`bool | None\` | no |
| \`start\` | \`int | None\` | no |

### SetPageMargins

\`SetPageMargins(bottom: float | None = None, footer: float | None = None, gutter: float | None = None, header: float | None = None, left: float | None = None, right: float | None = None, section: int | None = None, top: float | None = None)\`

Set selected nonnegative page margins in points for a section (default 1): top/right/bottom/left/header/footer/gutter. Omitted values are preserved. Track mode records the old margins in a section-property change; direct mode applies them with no revision.

| Field | Type | Required |
| --- | --- | --- |
| \`bottom\` | \`float | None\` | no |
| \`footer\` | \`float | None\` | no |
| \`gutter\` | \`float | None\` | no |
| \`header\` | \`float | None\` | no |
| \`left\` | \`float | None\` | no |
| \`right\` | \`float | None\` | no |
| \`section\` | \`int | None\` | no |
| \`top\` | \`float | None\` | no |

### SetSectionBoundary

\`SetSectionBoundary(at: str, break_type: str | None = None, remove: bool | None = None)\`

Insert a section boundary after body paragraph at; remove=True merges its section into the following section using the following layout while preserving effective following story chains. break_type selects nextPage/continuous/oddPage/evenPage/nextColumn. Direct mode only; tracked structural boundaries and pending property revisions are rejected. The final body section is preserved. Inventory handles section:<paragraph ID> remain stable; section:$alias references an inserted paragraph boundary.

| Field | Type | Required |
| --- | --- | --- |
| \`at\` | \`str\` | yes |
| \`break_type\` | \`str | None\` | no |
| \`remove\` | \`bool | None\` | no |

### SetSectionLayout

\`SetSectionLayout(break_type: str | None = None, column_separator: bool | None = None, column_spacing: float | None = None, column_widths: list | None = None, columns: int | None = None, height: float | None = None, orientation: str | None = None, page_number_continue: bool | None = None, page_number_format: str | None = None, page_number_start: int | None = None, page_size: str | None = None, section: int | None = None, section_handle: str | None = None, width: float | None = None)\`

Selectively set section layout (section defaults to 1): page_size letter/legal/a4/a3/a5 or paired width/height in points; orientation portrait/landscape normalizes dimensions. Columns 1..45 with column_spacing, column_widths and column_separator; break_type nextPage/continuous/oddPage/evenPage/nextColumn; page_number_format, page_number_start or page_number_continue=true. Omission preserves properties; tracking snapshots section properties while preserving story relationships. Pending property changes must be settled first.

| Field | Type | Required |
| --- | --- | --- |
| \`break_type\` | \`str | None\` | no |
| \`column_separator\` | \`bool | None\` | no |
| \`column_spacing\` | \`float | None\` | no |
| \`column_widths\` | \`list | None\` | no |
| \`columns\` | \`int | None\` | no |
| \`height\` | \`float | None\` | no |
| \`orientation\` | \`str | None\` | no |
| \`page_number_continue\` | \`bool | None\` | no |
| \`page_number_format\` | \`str | None\` | no |
| \`page_number_start\` | \`int | None\` | no |
| \`page_size\` | \`str | None\` | no |
| \`section\` | \`int | None\` | no |
| \`section_handle\` | \`str | None\` | no |
| \`width\` | \`float | None\` | no |

# END GENERATED OPERATION DOCS

## Scoped revisions

\`Revise(within=None, at=None, occurrence=None)\` builds one \`ReviseText\`
operation; chain \`.replace(select, with_, occurrence=None, all=False)\` for each
change. The scope bounds every \`select\`, so short selectors stay unambiguous
in long documents and each change becomes a minimal tracked change:

    # unique phrase, no paragraph id needed:
    Revise("the tenant shall pay rent within thirty days").replace("thirty", "sixty").replace("rent", "the rent")
    Revise(at="2673269E").replace("Tenant", "Lessee", all=True)  # whole paragraph
    Revise("Section 4(b)", at="2673269E", occurrence=2).replace("4(b)", "4(c)")
    Revise().replace("Acme Corp.", "Acme, Inc.", all=True)       # whole document

\`within\` must occur exactly once in its paragraph (\`at\`) or document unless
\`occurrence\` picks one. Each \`select\` must occur exactly once in the scope
unless it passes \`occurrence\` or \`all=True\`. All selectors resolve against the
scope as it stands when the operation starts, so later replacements never see
earlier ones; overlapping selections are rejected.

\`AcceptRevision(target)\` and \`RejectRevision(target)\` build \`RevisionSettle\`
operations; \`target\` is a revision id from \`docx_read(path, kind="revisions")\`
or \`"all"\`. Put settle operations first: \`"all"\` also settles tracked changes
made earlier in the same plan.

Every operation dataclass has \`to_dict()\`, omitting optional fields whose
value is \`None\`; \`Plan.to_dict()\` returns \`{"author": ..., "change_mode": ..., "ops": [...]}\`.

Tracked plain-text replacements automatically compute word-level redlines in
ReplaceText, ReviseText, and ReplaceParagraph. Unchanged words retain their
formatting; adjacent changed words form one deletion followed by one insertion.
Rich replacements retain whole-selection tracking to apply their formatting.

## Host functions

- \`docx_create(path: str, html: str)\`
- \`docx_read(path: str, view: str = "markup", kind: str = "document") -> ReadResult | dict\` — complete HTML
  markup and a current equations-address listing (at, equation, display, editable) by default (\`kind="document"\`); \`kind="comments"\`, \`"styles"\`, \`"revisions"\`, \`"bookmarks"\`, or \`"assets"\` returns the
  core listing object (and a second positional of \`comments\`/\`styles\`/\`revisions\`/\`bookmarks\` is treated as \`kind\`).
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
ReadResult.styles (resolved CSS keyed by style ID); a paragraph without
data-docx-style uses ReadResult.default_style. Direct formatting is inline
style; a paragraph's style lists only what differs from its named style
(prop:unset removes a style declaration). The style catalog is omitted from the
default display; inspect result.styles to read its definitions. Markup copied
from a read is valid with HTML, and in replace_paragraph bare text keeps the
formatting all the paragraph's runs share. Image aliases map to canonical assets in ReadResult.assets.
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
