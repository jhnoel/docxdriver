"""Regression tests for differential validation of already-invalid sources."""
import collections
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock

spec = importlib.util.spec_from_file_location('conformance', Path(__file__).with_name('ooxml-conformance.py'))
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)


class DifferentialErrors(unittest.TestCase):
    def test_sdk_missing_part_kind_is_independent_of_first_target(self):
        validator = gate.SdkValidator.__new__(gate.SdkValidator)
        messages = [
            "InvalidOperationException: Part: /word/footer1.xml doesn't exist in the package. Original Message:Specified part does not exist in the package.",
            "InvalidOperationException: Part: /word/glossary/document.xml doesn't exist in the package. Original Message:Specified part does not exist in the package.",
            'InvalidOperationException: unexpected package corruption',
        ]
        validator.process = Mock(stdin=io.StringIO(), stdout=io.StringIO(''.join(json.dumps({'fatal': m}) + '\n' for m in messages)))
        with tempfile.TemporaryDirectory() as directory:
            validator.scratch = Path(directory)
            first = validator.errors(b'')
            self.assertEqual(first, validator.errors(b''))
            self.assertNotEqual(first, validator.errors(b''))

    def test_partial_repair_unmasks_schema_and_sdk_errors(self):
        old_schema = ('word/document.xml', "Element 'ind': This element is not expected.")
        new_schema = ('word/document.xml', "Element 'spacing': This element is not expected.")
        old_sdk = ('word/document.xml', "sdk Sch_UnexpectedElementContentExpectingComplex: unexpected ind")
        new_sdk = ('word/document.xml', "sdk Sch_UnexpectedElementContentExpectingComplex: unexpected spacing")
        before = collections.Counter({old_schema: 4, old_sdk: 4})
        after = collections.Counter({old_schema: 3, old_sdk: 3, new_schema: 1, new_sdk: 1})
        self.assertFalse(gate.unmasked(after.keys() - before.keys(), before - after))

    def test_new_ordering_error_without_repair_is_reported(self):
        error = ('word/document.xml', "sdk Sch_UnexpectedElementContentExpectingComplex: unexpected spacing")
        self.assertEqual(gate.unmasked({error}, {}), {error})

    def test_xsd_repair_can_expose_sdk_error_while_snapshot_keeps_old_error(self):
        removed = {('word/document.xml', "Element 'ind': This element is not expected.")}
        exposed = {('word/document.xml', 'sdk Sch_UnexpectedElementContentExpectingComplex: unexpected spacing')}
        self.assertFalse(gate.unmasked(exposed, removed))

    def test_repairs_do_not_mask_other_parts_or_error_categories(self):
        removed = {('word/document.xml', "Element 'ind': This element is not expected.")}
        errors = {
            ('word/styles.xml', "Element 'spacing': This element is not expected."),
            ('word/document.xml', 'sdk Sch_AttributeValueDataTypeDetailed: invalid value'),
            ('word/document.xml', 'integrity: relationship footer targets a missing part'),
        }
        self.assertEqual(gate.unmasked(errors, removed), errors)


if __name__ == '__main__':
    unittest.main()
