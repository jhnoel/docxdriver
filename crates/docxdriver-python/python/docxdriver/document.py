# docxdriver document API: Document over .docx bytes, Review, create.
#
#   doc = Document(Path("a.docx").read_bytes())
#   doc.markup()                       body with the ids plans address
#   doc.comments() / revisions() / styles() / assets()
#   doc.find("thirty")
#   review = doc.review(Plan(ops=[...]))
#   doc = doc.commit("K7QM-3XRP-...")  a new Document
#   Path("a.docx").write_bytes(doc.data)
#
# Commit keys prove a review was read. The host keeps each reviewed plan
# under its key. A sandbox host shows the key only to the model, beside the
# report, so committing means typing the key back in a later submission;
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
        _docx_comments,
        _docx_commit,
        _docx_create,
        _docx_find,
        _docx_markup,
        _docx_review,
        _docx_revisions,
        _docx_styles,
    )
except ImportError:
    pass


class Document:
    """One .docx as bytes. Reads never write; commit returns a new Document."""

    def __init__(self, data):
        self.data = data

    def markup(self, view="markup"):
        """The body as HTML-like markup with stable paragraph ids.
        view: "markup" (tracked changes as <ins>/<del>), "final", "original"."""
        return _docx_markup(self.data, view)

    def comments(self):
        """Comment threads: id, status, anchor, root, replies."""
        return json.loads(_docx_comments(self.data))

    def revisions(self):
        """Tracked changes: id, kind, author, text; ids feed AcceptRevision."""
        return json.loads(_docx_revisions(self.data))

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
        """Preview a Plan (or its dict) without writing. Read .report; the
        commit key is issued with it."""
        plan = json.dumps(plan.to_dict() if hasattr(plan, "to_dict") else plan)
        reviewed = json.loads(_docx_review(self.data, plan))
        return Review(reviewed["report"], reviewed.get("key"))

    def commit(self, key):
        """Apply the plan reviewed under key to these bytes; returns the
        edited Document. Keys are single-use."""
        return Document(_docx_commit(self.data, key))


class Review:
    """A previewed plan: .report lists each op's outcome and affected markup."""

    def __init__(self, report, key=None):
        self.report = report
        self.key = key


def create(paragraphs=None, html=None):
    """A new Document from plain paragraphs or simple HTML; pass exactly one."""
    if (paragraphs is None) == (html is None):
        raise ValueError("pass exactly one of paragraphs or html")
    return Document(_docx_create(paragraphs, html))
