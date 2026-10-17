# docxdriver plan vocabulary: typed edit operations, Revise,
# Accept/RejectRevision and the Plan container. Plan(...).to_dict() is the
# docxdriver-core plan JSON without its source-bound "base".
#
# Valid CPython and within Monty's subset (dataclasses, no inheritance);
# docxdriver-monty-repl embeds this file as PLAN. The GENERATED region comes
# from the core schema; regenerate with `python scripts/generate-python-plan.py`.

from dataclasses import dataclass

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
    with_: str | None = None
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
            value["with"] = self.with_
        return value

@dataclass
class InsertTable:
    at: str
    columns: int
    position: str
    rows: int
    as_: str | None = None
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
    with_: str
    def to_dict(self):
        value = {"op": "replace_paragraph"}
        if self.at is not None:
            value["at"] = self.at
        if self.with_ is not None:
            value["with"] = self.with_
        return value

@dataclass
class ReplaceText:
    at: str
    select: str
    with_: str
    occurrence: int | None = None
    def to_dict(self):
        value = {"op": "replace_text"}
        if self.at is not None:
            value["at"] = self.at
        if self.select is not None:
            value["select"] = self.select
        if self.with_ is not None:
            value["with"] = self.with_
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
    with_: str
    kind: str | None = None
    section: int | None = None
    def to_dict(self):
        value = {"op": "set_footer"}
        if self.with_ is not None:
            value["with"] = self.with_
        if self.kind is not None:
            value["kind"] = self.kind
        if self.section is not None:
            value["section"] = self.section
        return value

@dataclass
class SetHeader:
    with_: str
    kind: str | None = None
    section: int | None = None
    def to_dict(self):
        value = {"op": "set_header"}
        if self.with_ is not None:
            value["with"] = self.with_
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
    def replace(self, select: str, with_: str, occurrence: int | None = None, all: bool = False):
        edit = {"select": select, "with": with_}
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
class Plan:
    author: str = "docxdriver"
    change_mode: str = "track"
    ops: list = None
    def to_dict(self):
        ops = self.ops if self.ops is not None else []
        return {
            "author": self.author,
            "change_mode": self.change_mode,
            "ops": [op.to_dict() if hasattr(op, "to_dict") else op for op in ops],
        }
