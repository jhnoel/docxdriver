/**
 * Version 3 Python prelude (structured document model surface).
 *
 * The host returns only plain data (dicts/lists/strings) via the private
 * `_docx_open` callback; the prelude builds Python-native Document/Paragraph/
 * Selection objects. Mutations record a change set (text_changes,
 * fmt_changes, style_changes, insertions, deleted flags) that the host
 * deriver replays into typed core operations at preview. Stale selections
 * raise ValueError at mutation time (naming the paragraph) and are
 * re-validated by the deriver replay.
 *
 * Monty 0.0.21 runtime constraints (probe-verified): no `@property`/method
 * decorators, no `__getattr__`/`__setattr__` interception, no descriptor
 * protocol, no `yield`. The public attributes therefore cannot hook
 * assignment. Instead `text` / `style` / the selection format attributes are
 * PLAIN attributes whose divergence from the model's running state is
 * detected lazily by `_sync()` at serialize time: an assigned `text` becomes
 * a whole-paragraph replacement entry, an assigned `style` a style change,
 * and assigned selection flags land in the paragraph's `fmt_changes`.
 * `doc.select`/`doc.regex` register every selection so serialize can sync
 * their flags even when the agent never keeps a reference.
 *
 * The prelude is fed with skipTypeCheck (trusted host code); PYTHON_TYPE_STUBS
 * is what the checker resolves user-snippet names against, so every public
 * name the agent uses (Document, Paragraph, Selection, docx_*) must be
 * declared there.
 */

/**
 * The Python prelude: Document/Paragraph/Selection classes plus docx_*
 * wrappers over the private host callbacks. `docx_preview` wraps the host's
 * `{key}` dict into a PreviewResult so `preview.key` works in Python.
 */
export const PYTHON_PRELUDE: string = `
import re as _re

from dataclasses import dataclass

@dataclass
class PreviewResult:
    key: str

class Selection:
    """A targeted span inside one paragraph; attribute writes record format
    changes. Format attributes are plain booleans (None until assigned);
    Document._sync_all flushes the ASSIGNED values (True and False) into the
    paragraph's fmt_changes, replacing the entry on every sync so later
    format intent survives (an agent assigning bold=True then bold=False ends
    with no bold)."""
    _KEYS = ("bold", "italic", "underline", "strike", "superscript", "subscript")
    def __init__(self, paragraph, expected_text, occurrence):
        self._paragraph = paragraph
        self.expected_text = expected_text
        self.occurrence = occurrence      # 1-based within the paragraph
        for key in Selection._KEYS:
            setattr(self, key, None)
        self._fmt_entry = None
    def _sync(self):
        fmt = {}
        for key in Selection._KEYS:
            value = getattr(self, key)
            if value is not None:
                fmt[key] = bool(value)
        if self._fmt_entry is None:
            if fmt:
                self._fmt_entry = {"expected": self.expected_text, "occurrence": self.occurrence, "fmt": fmt}
                self._paragraph.fmt_changes.append(self._fmt_entry)
        else:
            self._fmt_entry["fmt"] = fmt

class Paragraph:
    """A paragraph with a running text model. The text and style attributes are
    plain and may be assigned; _sync() converts a divergence from the running
    state into a recorded change at the next mutation or serialize."""
    def __init__(self, doc, data):
        self._doc = doc
        self.id = data["id"]
        self._original_markup = data["markup"]
        self._original_text = data["text"]
        self._text = data["text"]
        self.text = data["text"]
        self._style = data.get("style")
        self.style = data.get("style")
        self._deleted = False
        self.text_changes = []   # {"expected": str, "new": str, "occurrence": int|None}
        self.fmt_changes = []    # {"expected": str, "occurrence": int|None, "fmt": dict}
        self.style_changes = []  # [str]
    def _sync(self):
        if self.text != self._text:
            self.text_changes = [{"expected": self._text, "new": self.text, "occurrence": None}]
            self.fmt_changes = []
            self._text = self.text
        if self.style != self._style:
            self.style_changes = [self.style]
            self._style = self.style
    def replace(self, old, new, occurrence=None):
        if self._deleted: raise ValueError(f"paragraph {self.id} is deleted")
        if not isinstance(old, str) or not isinstance(new, str):
            raise ValueError(f"replace: old and new must be strings in paragraph {self.id}")
        self._sync()
        if occurrence is None:
            count = self._text.count(old)
            if count == 0: raise ValueError(f"replace: {old!r} not found in paragraph {self.id}")
            if count > 1: raise ValueError(f"replace: {old!r} occurs {count} times in paragraph {self.id} — pass occurrence=1..{count}")
            occurrence = 1
        elif not (isinstance(occurrence, int) and occurrence >= 1):
            raise ValueError(f"replace: occurrence must be a positive integer in paragraph {self.id}")
        spans = [i for i in range(len(self._text)) if self._text.startswith(old, i)]
        if len(spans) < occurrence:
            raise ValueError(f"replace: occurrence {occurrence} out of range in paragraph {self.id}")
        self.text_changes.append({"expected": old, "new": new, "occurrence": occurrence})
        self._text = self._text[:spans[occurrence - 1]] + new + self._text[spans[occurrence - 1] + len(old):]
        self.text = self._text
    def delete(self):
        if self._deleted: raise ValueError(f"paragraph {self.id} already deleted")
        self._deleted = True
        self.text_changes = []
        self.fmt_changes = []
        self.style_changes = []

class Document:
    def __init__(self, data):
        self.path = data["path"]
        self.source_hash = data["source_hash"]
        self.paragraphs = [Paragraph(self, p) for p in data["paragraphs"]]
        self.insertions = []  # {"anchor": str, "position": "after"|"before", "text": str, "style": str|None}
        self._selections = []
    def _find(self, text, occurrence):
        found = []
        for para in self.paragraphs:
            if para._deleted: continue
            count = para._text.count(text)
            for i in range(1, count + 1):
                found.append((para, i))
        if occurrence is None:
            if len(found) == 0: raise ValueError(f"select: {text!r} not found in any paragraph")
            if len(found) > 1: raise ValueError(f"select: {text!r} is ambiguous — pass occurrence=1..{len(found)}")
            return found[0]
        if not (isinstance(occurrence, int) and occurrence >= 1):
            raise ValueError("select: occurrence must be a positive integer")
        if occurrence > len(found):
            raise ValueError(f"select: occurrence {occurrence} out of range — {text!r} occurs {len(found)} time(s)")
        return found[occurrence - 1]
    def select(self, text, occurrence=None):
        para, occ = self._find(text, occurrence)
        selection = Selection(para, text, occ)
        self._selections.append(selection)
        return selection
    def regex(self, pattern):
        matches = []
        for para in self.paragraphs:
            if para._deleted: continue
            for m in _re.finditer(pattern, para._text):
                selection = Selection(para, m.group(0), 1 + para._text[:m.start()].count(m.group(0)))
                matches.append(selection)
        self._selections.extend(matches)
        return matches
    def insert_after(self, para, text, style=None):
        self.insertions.append({"anchor": para.id, "position": "after", "text": text, "style": style})
    def insert_before(self, para, text, style=None):
        self.insertions.append({"anchor": para.id, "position": "before", "text": text, "style": style})
    def _sync_all(self):
        for selection in self._selections:
            selection._sync()
        for para in self.paragraphs:
            para._sync()
    def serialize(self):
        self._sync_all()
        return {
            "path": self.path, "source_hash": self.source_hash,
            "paragraphs": [{
                "id": p.id, "original_markup": p._original_markup, "original_text": p._original_text,
                "text": p._text, "style": p._style, "deleted": p._deleted,
                "text_changes": p.text_changes, "fmt_changes": p.fmt_changes,
                "style_changes": p.style_changes,
            } for p in self.paragraphs],
            "insertions": self.insertions,
        }

def docx_create(path: str, html: str):
    return _docx_create(path, html)

def docx_read(path: str, view: str = "markup") -> str:
    return _docx_read(path, view)

def docx_help(topic: str | None = None) -> str:
    return _docx_help(topic)

def docx_open(path: str):
    return Document(_docx_open(path))

def docx_preview(doc: Document):
    result = _docx_preview(doc.path, doc.serialize())
    if result is None:
        return None
    return PreviewResult(key=result["key"])

def docx_commit(doc: Document, key: str):
    _docx_commit(doc.path, key, doc.serialize())
    return None
`;

/**
 * Stub declarations for Monty's type checker. Mirrors every public name the
 * prelude defines so type-checked user snippets can resolve them. Plain
 * attributes stand in for the runtime's plain-attribute model (the checker
 * validates against the stub; runtime behavior comes from the prelude class).
 * Stub order avoids forward references (Document before Paragraph before
 * Selection).
 */
export const PYTHON_TYPE_STUBS: string = `
class PreviewResult:
    def __init__(self, key: str): ...
    key: str

class Document:
    def __init__(self, data: dict): ...
    path: str
    source_hash: str
    paragraphs: list
    insertions: list
    def select(self, text: str, occurrence: int | None = None) -> Selection: ...
    def regex(self, pattern: str) -> list: ...
    def insert_after(self, para: Paragraph, text: str, style: str | None = None): ...
    def insert_before(self, para: Paragraph, text: str, style: str | None = None): ...
    def serialize(self) -> dict: ...

class Paragraph:
    def __init__(self, doc: Document, data: dict): ...
    id: str
    text: str
    style: str | None
    text_changes: list
    fmt_changes: list
    style_changes: list
    def replace(self, old: str, new: str, occurrence: int | None = None): ...
    def delete(self): ...

class Selection:
    def __init__(self, paragraph: Paragraph, expected_text: str, occurrence: int): ...
    expected_text: str
    occurrence: int
    bold: bool
    italic: bool
    underline: bool
    strike: bool
    superscript: bool
    subscript: bool

def docx_create(path: str, html: str): ...
def docx_read(path: str, view: str = "markup") -> str: ...
def docx_help(topic: str | None = None) -> str: ...
def docx_open(path: str) -> Document: ...
def docx_preview(doc: Document): ...
def docx_commit(doc: Document, key: str) -> None: ...

def _docx_create(path: str, html: str): ...
def _docx_read(path: str, view: str = "markup") -> str: ...
def _docx_help(topic: str | None = None) -> str: ...
def _docx_open(path: str) -> dict: ...
def _docx_preview(path: str, state: dict) -> dict: ...
def _docx_commit(path: str, key: str, state: dict) -> None: ...
`;

/**
 * Concise reference doc for agents: exact signatures plus the spec's V3
 * example (traversal, selection formatting, preview, commit).
 */
export const PYTHON_API_REFERENCE: string = `
# docxdriver Python REPL API — Version 3 (structured document model)

The document is exposed as Python-native objects: open it once with docx_open,
mutate paragraphs and selections, preview, and commit in a later call. The
host turns the recorded change set into typed core operations; stale
selections raise ValueError naming the paragraph.

## Document

- \`docx_open(path: str) -> Document\`
- \`docx_preview(doc: Document) -> PreviewResult\` (returns \`.key\`)
- \`docx_commit(doc: Document, key: str)\`
- \`docx_create(path: str, html: str)\`, \`docx_read(path: str, view: str = "markup") -> str\`,
  \`docx_help(topic: str | None = None) -> str\`

## Model

- \`doc.paragraphs\` — \`Paragraph\` list; \`doc.select(text, occurrence=None) -> Selection\`;
  \`doc.regex(pattern)\` — a list of \`Selection\`s, one per match;
  \`doc.insert_after(para, text, style=None)\` / \`doc.insert_before(para, text, style=None)\`.
- \`Paragraph\`: \`.id\`, \`.text\` (get/set — assigning replaces the whole
  paragraph), \`.style\` (get/set), \`.replace(old, new, occurrence=None)\`,
  \`.delete()\`.
- \`Selection\`: \`.bold\`, \`.italic\`, \`.underline\`, \`.strike\`,
  \`.superscript\`, \`.subscript\` — assigning True records a format change.

## Example

    doc = docx_open("contract.docx")

    for para in doc.paragraphs:
        if "thirty days" in para.text:
            para.replace("thirty days", "sixty days")

    doc.select("sixty days").bold = True

    preview = docx_preview(doc)
    # ...later python call...
    docx_commit(doc, preview.key)
`;
