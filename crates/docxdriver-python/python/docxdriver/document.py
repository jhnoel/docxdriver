# docxdriver document API: Document over .docx bytes, Review, create.
#
#   doc = Document(Path("a.docx").read_bytes())
#   doc.markup()                       body with the ids plans address
#   doc.equations() / comments() / revisions() / bookmarks() / styles() / assets()
#   doc.read("bookmarks")             any listing kind by name
#   doc.find("thirty")
#   review = doc.review(Plan(ops=[...]))
#   doc = doc.commit("K7QM-3XRP-...")  a new Document
#   Path("a.docx").write_bytes(doc.data)
#
# Commit keys prove a review was read. The host keeps each reviewed plan
# under its key. A sandbox host shows the key only to the model, at the end
# of the review report, so committing means typing it back in a later submission;
# there review.key is None. Natively, with no model, review.key is set.
#
# Valid CPython and within Monty's subset (no inheritance or method
# decorators); docxdriver-monty-repl embeds this file as DOCUMENT. The
# _docx_* engine functions come from docxdriver._core in CPython; a Monty
# host defines them. Engine failures raise ValueError.

import json

try:
    from docxdriver._core import (
        _docx_assets,
        _docx_bookmarks,
        _docx_comments,
        _docx_commit,
        _docx_create,
        _docx_equations,
        _docx_find,
        _docx_markup,
        _docx_review,
        _docx_revisions,
        _docx_styles,
    )
except ImportError:
    pass


class Document:
    """One .docx as bytes: Document(Path("a.docx").read_bytes()).
    Read: markup(view), equations(), comments(), revisions(), bookmarks(),
    styles(), assets(), read(kind), find(query). Edit: review(Plan(ops=[...])) previews, then
    commit(key) returns the edited Document; save its .data."""

    def __init__(self, data):
        self.data = data

    def __repr__(self):
        return (
            "<Document " + str(len(self.data)) + " bytes. Read: markup(view='markup'|'final'|'original'), "
            "equations(), comments(), revisions(), bookmarks(), styles(), assets(), read(kind), find(query). Edit: review(Plan(ops=[...])) "
            "-> Review, then commit(key) -> Document; save with Path(p).write_bytes(doc.data).>"
        )

    def markup(self, view="markup"):
        """The body as HTML-like markup with stable paragraph ids.
        view: "markup" (tracked changes as <ins>/<del>), "final", "original"."""
        return _docx_markup(self.data, view)

    def equations(self):
        """Equations: at (paragraph id) and equation (1-based), the address
        ReplaceEquation and DeleteEquation take."""
        return json.loads(_docx_equations(self.data))

    def comments(self):
        """Comment threads: id, status, anchor, root, replies."""
        return json.loads(_docx_comments(self.data))

    def revisions(self):
        """Tracked changes: id, kind, author, text; ids feed AcceptRevision."""
        return json.loads(_docx_revisions(self.data))

    def bookmarks(self):
        """Bookmarks in every story: name, id, hidden, start and end
        (paragraph ids), text (truncated; empty for a point) and part.
        Bookmark ops edit body bookmarks only."""
        return json.loads(_docx_bookmarks(self.data))

    def read(self, kind="document", view="markup"):
        """One read by kind: "document" (the markup string for view),
        "comments", "revisions", "bookmarks", "styles", "assets" or
        "equations" (lists)."""
        if kind == "document":
            return self.markup(view)
        if kind == "comments":
            return self.comments()
        if kind == "revisions":
            return self.revisions()
        if kind == "bookmarks":
            return self.bookmarks()
        if kind == "styles":
            return self.styles()
        if kind == "assets":
            return self.assets()
        if kind == "equations":
            return self.equations()
        raise ValueError(
            "unknown read kind: " + str(kind)
            + " (expected document, comments, revisions, bookmarks, styles, assets or equations)"
        )

    def styles(self):
        """Style definitions: styleId, name, type."""
        return json.loads(_docx_styles(self.data))

    def assets(self):
        """Embedded images and media."""
        return json.loads(_docx_assets(self.data))

    def find(self, query, ignore_case=False):
        """Paragraphs containing query: id, index, text."""
        return json.loads(_docx_find(self.data, query, ignore_case))

    def review(self, plan):
        """Preview a Plan (or its dict) without writing.
        In a sandbox, review.key is None: the host shows the review report
        in the tool result, ending with its commit key; type the key into
        doc.commit(key) in a later submission. Natively review.key is set."""
        plan = json.dumps(plan.to_dict() if hasattr(plan, "to_dict") else plan)
        reviewed = json.loads(_docx_review(self.data, plan))
        return Review(reviewed["report"], reviewed.get("key"))

    def commit(self, key):
        """Apply the plan reviewed under key to these bytes; returns the
        edited Document. Keys are single-use."""
        return Document(_docx_commit(self.data, key))


class Review:
    """A previewed plan: .report lists each op's outcome and affected markup.
    Commit with doc.commit(key). In a sandbox .key is None: the review
    report in the tool result ends with the key; type it back."""

    def __init__(self, report, key=None):
        self.report = report
        self.key = key

    def __repr__(self):
        report = self.report or {}
        done = str(report.get("completed", 0)) + " of " + str(len(report.get("ops") or [])) + " ops applied"
        if self.key is not None:
            return "<Review: " + done + "; commit with doc.commit(" + repr(self.key) + ")>"
        return (
            "<Review: " + done + ". The commit key is not available to code: the review "
            "report in this tool result (DOCX REVIEW) ends with it. In a later submission, "
            "type it into doc.commit(\"KEY\") on the Document you reviewed.>"
        )


def create(paragraphs=None, html=None):
    """A new Document from plain paragraphs or simple HTML; pass exactly one."""
    if (paragraphs is None) == (html is None):
        raise ValueError("pass exactly one of paragraphs or html")
    return Document(_docx_create(paragraphs, html))
