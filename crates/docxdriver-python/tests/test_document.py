"""docxdriver.Document against the native engine; run after installing the wheel."""
import re
import unittest

from docxdriver import Document, create
from docxdriver.plan import CommentAdd, Plan, ReplaceText


def first_paragraph(markup):
    return re.search(r'<p id="([0-9A-F]{8})"', markup).group(1)


class DocumentTests(unittest.TestCase):
    def test_create_read_find(self):
        doc = create(paragraphs=['Alpha', 'Beta'])
        self.assertIsInstance(doc.data, bytes)
        self.assertIn('Beta', doc.markup())
        self.assertEqual(doc.comments(), [])
        self.assertEqual(doc.revisions(), [])
        self.assertTrue(doc.styles())
        self.assertEqual(doc.assets(), [])
        self.assertEqual(len(doc.find('alpha', ignore_case=True)), 1)
        self.assertEqual(doc.find('alpha'), [])

    def test_review_then_commit(self):
        doc = create(html='<p>Rent is due in thirty days.</p>')
        at = first_paragraph(doc.markup())
        review = doc.review(Plan(author='T', ops=[
            ReplaceText(at=at, select='thirty', with_='sixty'),
            CommentAdd(at=at, text='Check'),
        ]))
        self.assertEqual(review.report['completed'], 2)

        edited = review.commit()
        self.assertIsInstance(edited, Document)
        self.assertIn('sixty days', edited.markup('final'))
        self.assertIn('thirty days', edited.markup('original'))
        self.assertEqual(len(edited.revisions()), 2)
        self.assertEqual(len(edited.comments()), 1)
        # The reviewed document is unchanged.
        self.assertEqual(doc.revisions(), [])

    def test_engine_errors_raise(self):
        doc = create(paragraphs=['Alpha'])
        with self.assertRaises(ValueError):
            doc.review({'author': 'T', 'change_mode': 'track', 'ops': [{'op': 'nope'}]})
        with self.assertRaises(ValueError):
            create()
        with self.assertRaises(ValueError):
            Document(b'not a docx').markup()


if __name__ == '__main__':
    unittest.main()
