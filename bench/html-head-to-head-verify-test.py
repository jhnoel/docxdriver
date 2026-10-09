"""Check benchmark verifier sensitivity to incorrect edits and collateral changes."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
import zipfile
import xml.etree.ElementTree as ET

spec = importlib.util.spec_from_file_location('verify', Path(__file__).with_name('html-head-to-head-verify.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'

class VerifierTests(unittest.TestCase):
    def fixture(self, directory, name, phrase, bold=True, outside='Unchanged.', media=b'image', preserve=True):
        rpr = '<w:rPr><w:b/></w:rPr>' if bold else ''
        space = ' xml:space="preserve"' if preserve else ''
        xml = f'<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>The notice period is </w:t></w:r><w:r>{rpr}<w:t{space}>{phrase}</w:t></w:r><w:r><w:t>.</w:t></w:r></w:p><w:p><w:r><w:t>{outside}</w:t></w:r></w:p></w:body></w:document>'
        path = Path(directory) / name
        with zipfile.ZipFile(path, 'w') as z:
            z.writestr('word/document.xml', xml)
            z.writestr('word/media/image.png', media)
        return str(path)

    def test_valid_edit_allows_immaterial_space_attribute(self):
        with tempfile.TemporaryDirectory() as d:
            before = self.fixture(d, 'before.docx', 'thirty days')
            after = self.fixture(d, 'after.docx', 'sixty days', preserve=False)
            self.assertEqual(module.verify('text', before, after)['outcome'], 'ok')

    def test_unchanged_document_is_not_a_successful_edit(self):
        with tempfile.TemporaryDirectory() as d:
            before = self.fixture(d, 'before.docx', 'thirty days')
            with self.assertRaisesRegex(AssertionError, 'Unexpected text'):
                module.verify('text', before, before)

    def test_missing_bold_is_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            before = self.fixture(d, 'before.docx', 'thirty days')
            after = self.fixture(d, 'after.docx', 'sixty days', bold=False)
            with self.assertRaisesRegex(AssertionError, 'formatting'):
                module.verify('text', before, after)

    def test_collateral_text_change_is_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            before = self.fixture(d, 'before.docx', 'thirty days')
            after = self.fixture(d, 'after.docx', 'sixty days', outside='Changed.')
            with self.assertRaisesRegex(AssertionError, 'Unexpected text'):
                module.verify('text', before, after)

    def test_package_part_change_is_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            before = self.fixture(d, 'before.docx', 'thirty days')
            after = self.fixture(d, 'after.docx', 'sixty days', media=b'changed')
            with self.assertRaisesRegex(AssertionError, 'Unrelated part'):
                module.verify('text', before, after)

    def test_fully_deleted_paragraph_break_is_removed_from_final_content(self):
        data = {'word/document.xml': f'<w:document xmlns:w="{W}"><w:body><w:p><w:pPr><w:rPr><w:del w:id="1"/></w:rPr></w:pPr><w:del w:id="1"><w:r><w:delText>Deleted.</w:delText></w:r></w:del></w:p><w:p><w:r><w:t>Kept.</w:t></w:r></w:p></w:body></w:document>'.encode()}
        self.assertEqual([module.text(p) for p in module.paragraphs(data)], ['Kept.'])

    def test_plain_text_allows_normal_color_and_non_latin_font_flags(self):
        p = ET.fromstring(f'<w:p xmlns:w="{W}"><w:r><w:rPr><w:bCs/><w:iCs/><w:color w:val="000000"/></w:rPr><w:t>Claim</w:t></w:r></w:p>')
        self.assertTrue(module.plain_paragraph(p, {}))

    def test_plain_text_rejects_inherited_bold(self):
        p = ET.fromstring(f'<w:p xmlns:w="{W}"><w:pPr><w:pStyle w:val="Heading"/></w:pPr><w:r><w:t>Heading</w:t></w:r></w:p>')
        styles = f'<w:styles xmlns:w="{W}"><w:style w:styleId="Heading" w:type="paragraph"><w:rPr><w:b/></w:rPr></w:style></w:styles>'.encode()
        self.assertFalse(module.plain_paragraph(p, {'word/styles.xml': styles}))

if __name__ == '__main__':
    unittest.main()
