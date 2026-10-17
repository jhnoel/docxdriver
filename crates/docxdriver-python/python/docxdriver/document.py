# docxdriver document API: Document over .docx bytes, Review, create.
#
#   doc = Document(Path("a.docx").read_bytes())
#   doc.markup()                       body with the ids plans address
#   doc.comments() / revisions() / styles() / assets()
#   doc.find("thirty")
#   review = doc.review(Plan(ops=[...]))
#   doc = review.commit()              a new Document
#   Path("a.docx").write_bytes(doc.data)
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
    """One .docx as bytes. Every method reads these bytes; none writes."""

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
        """Preview a Plan (or its dict) without writing. Inspect .report,
        then .commit()."""
        plan = json.dumps(plan.to_dict() if hasattr(plan, "to_dict") else plan)
        reviewed = json.loads(_docx_review(self.data, plan))
        return Review(self, plan, reviewed["key"], reviewed["report"])


class Review:
    """A previewed plan. .report lists each op's outcome and affected markup."""

    def __init__(self, document, plan, key, report):
        self.document = document
        self.plan = plan
        self.key = key
        self.report = report

    def commit(self):
        """Apply the reviewed plan; returns the edited Document."""
        return Document(_docx_commit(self.document.data, self.plan, self.key))


def create(paragraphs=None, html=None):
    """A new Document from plain paragraphs or simple HTML; pass exactly one."""
    if (paragraphs is None) == (html is None):
        raise ValueError("pass exactly one of paragraphs or html")
    return Document(_docx_create(paragraphs, html))
