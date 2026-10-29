"""docxdriver.Document against the native engine; run after installing the wheel."""
import json
import re
import unittest

from docxdriver import Document, create
from docxdriver.plan import BookmarkAdd, BookmarkDelete, BookmarkMove, BookmarkRename, CommentAdd, Plan, ReplaceText


def first_paragraph(markup):
    return re.search(r'<p id="([0-9A-F]{8})"', markup).group(1)


class DocumentTests(unittest.TestCase):
    def test_create_read_find(self):
        doc = create(paragraphs=['Alpha', 'Beta'])
        self.assertIsInstance(doc.data, bytes)
        self.assertIn('Beta', doc.markup())
        self.assertEqual(doc.comments(), [])
        self.assertEqual(doc.equations(), [])
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

        edited = doc.commit(review.key)
        self.assertIsInstance(edited, Document)
        self.assertIn('sixty days', edited.markup('final'))
        self.assertIn('thirty days', edited.markup('original'))
        self.assertEqual(len(edited.revisions()), 2)
        self.assertEqual(len(edited.comments()), 1)
        # The reviewed document is unchanged.
        self.assertEqual(doc.revisions(), [])

        # Keys are single-use.
        with self.assertRaises(ValueError):
            doc.commit(review.key)

    def test_bookmarks_add_read_rename_move_delete(self):
        doc = create(paragraphs=['Pay within thirty days.', 'Second clause.'])
        first, second = re.findall(r'<p id="([0-9A-F]{8})"', doc.markup())[:2]
        self.assertEqual(doc.bookmarks(), [])
        review = doc.review(Plan(author='T', ops=[
            BookmarkAdd(at=first, name='Term', select='thirty days'),
            BookmarkAdd(at=second, name='Second'),
        ]))
        doc = doc.commit(review.key)
        marks = {mark['name']: mark for mark in doc.read(kind='bookmarks')}
        self.assertEqual(marks['Term']['text'], 'thirty days')
        self.assertEqual(marks['Term']['start'], first)
        self.assertFalse(marks['Term']['hidden'])
        self.assertEqual(marks['Second']['text'], 'Second clause.')
        self.assertEqual(doc.read(kind='bookmarks'), doc.bookmarks())
        review = doc.review(Plan(author='T', ops=[
            BookmarkRename(name='Term', new_name='Payment'),
            BookmarkMove(name='Payment', at=second, select='clause'),
            BookmarkDelete(name='Second'),
        ]))
        doc = doc.commit(review.key)
        self.assertEqual([(m['name'], m['text']) for m in doc.bookmarks()], [('Payment', 'clause')])
        with self.assertRaisesRegex(ValueError, 'close matches: "Payment"'):
            doc.review(Plan(author='T', ops=[BookmarkDelete(name='Paymnt')]))
        with self.assertRaisesRegex(ValueError, 'hidden or reserved'):
            doc.review(Plan(author='T', ops=[BookmarkAdd(at=first, name='_Mine')]))
        with self.assertRaisesRegex(ValueError, 'unknown read kind'):
            doc.read(kind='marks')

    def test_commit_needs_a_reviewed_key(self):
        doc = create(paragraphs=['Alpha'])
        with self.assertRaises(ValueError):
            doc.commit('p1:sha256:' + '0' * 64)

    def test_host_primitives_are_stateless(self):
        # What a sandbox host builds on: it keeps plan and key, issues its own.
        from docxdriver import commit_plan, review_plan
        doc = create(html='<p>Rent is due in thirty days.</p>')
        at = first_paragraph(doc.markup())
        plan = json.dumps(Plan(change_mode='direct', ops=[ReplaceText(at=at, select='thirty', with_='sixty')]).to_dict())
        reviewed = json.loads(review_plan(doc.data, plan))
        edited = Document(commit_plan(doc.data, plan, reviewed['key']))
        self.assertIn('sixty days', edited.markup())

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
