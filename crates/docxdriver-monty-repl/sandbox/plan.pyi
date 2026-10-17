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
    def __init__(self, at: str, position: str, as_: str | None = None, style: str | None = None, with_: str | None = None): ...
    at: str
    position: str
    as_: str | None
    style: str | None
    with_: str | None
    def to_dict(self) -> dict: ...

class InsertTable:
    def __init__(self, at: str, columns: int, position: str, rows: int, as_: str | None = None): ...
    at: str
    columns: int
    position: str
    rows: int
    as_: str | None
    def to_dict(self) -> dict: ...

class ReplaceEquation:
    def __init__(self, at: str, mathml: str, display: bool | None = None, equation: int | None = None): ...
    at: str
    mathml: str
    display: bool | None
    equation: int | None
    def to_dict(self) -> dict: ...

class ReplaceParagraph:
    def __init__(self, at: str, with_: str): ...
    at: str
    with_: str
    def to_dict(self) -> dict: ...

class ReplaceText:
    def __init__(self, at: str, select: str, with_: str, occurrence: int | None = None): ...
    at: str
    select: str
    with_: str
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
    def __init__(self, with_: str, kind: str | None = None, section: int | None = None): ...
    with_: str
    kind: str | None
    section: int | None
    def to_dict(self) -> dict: ...

class SetHeader:
    def __init__(self, with_: str, kind: str | None = None, section: int | None = None): ...
    with_: str
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
    def replace(self, select: str, with_: str, occurrence: int | None = None, all: bool = False) -> Revise: ...
    def to_dict(self) -> dict: ...

def AcceptRevision(target: str) -> RevisionSettle: ...
def RejectRevision(target: str) -> RevisionSettle: ...

class Plan:
    def __init__(self, author: str = "docxdriver", change_mode: str = "track", ops: list = []): ...
    author: str
    change_mode: str
    ops: list
    def to_dict(self) -> dict: ...

