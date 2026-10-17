"""docxdriver.plan builders against the native engine; run after installing the wheel."""
import hashlib
import json
import unittest

from docxdriver import canonical_plan_json, plan, run_command, run_plan


class PlanBuilderTests(unittest.TestCase):
    def test_source_ships_as_text(self):
        # Sandbox hosts preload this text into Monty.
        from importlib.resources import files
        source = files('docxdriver').joinpath('plan.py').read_text(encoding='utf-8')
        self.assertIn('class Plan:', source)

    def test_builders_emit_canonical_plans(self):
        built = plan.Plan(author='Builder', ops=[
            plan.ReplaceText(at='0F537164', select='Hello', with_='Goodbye'),
            plan.Revise(at='0F537164').replace('world', 'there'),
            plan.AcceptRevision('all'),
        ]).to_dict()
        built['base'] = 'sha256:' + '0' * 64
        canonical = json.loads(canonical_plan_json(json.dumps(built)))
        self.assertNotIn('error', canonical)
        self.assertEqual([op['op'] for op in canonical['ops']], ['replace_text', 'revise_text', 'revision_settle'])

    def test_builder_plan_previews_and_commits(self):
        data = run_command(None, 'create', html='<p>Hello world.</p>')['bytes']
        paragraph = run_command(data, 'read', read_kind='document_ui', view='final')['result']['paragraphs'][0]['id']
        built = plan.Plan(author='Builder', ops=[
            plan.ReplaceText(at=paragraph, select='Hello', with_='Goodbye')]).to_dict()
        built['base'] = 'sha256:' + hashlib.sha256(data).hexdigest()
        preview = run_plan(data, built)
        self.assertEqual(preview['outcome'], 'previewed', preview)
        committed = run_plan(data, built, preview_key=preview['preview_key'])
        self.assertEqual(committed['outcome'], 'committed', committed)


if __name__ == '__main__':
    unittest.main()
