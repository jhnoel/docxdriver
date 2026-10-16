"""Python SDK and REPL host regression tests; run after installing the Maturin wheel."""
import asyncio
from dataclasses import replace
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch

from docxdriver import canonical_plan_json, execute_request, run_command, run_plan
from docxdriver.repl import PRELUDE, REFERENCE, host, quotes


class NativeHostTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name).resolve()
        self.store = host.CommitKeyStore()
        self.path = self.root / 'test.docx'
        self.original = run_command(None, 'create', html='<p>Hello world.</p>')['bytes']
        self.path.write_bytes(self.original)

    def tearDown(self):
        self.temp.cleanup()

    def context(self, execution=1, **kwargs):
        return host.PlanHost(self.root, self.store, execution, **kwargs)

    def review(self, context=None, text='Goodbye'):
        context = context or self.context()
        paragraph = context.find('test.docx', 'Hello')[0]['id']
        plan = {'plan': {'author': 'Reviewer', 'change_mode': 'track', 'ops': [
            {'op': 'replace_text', 'at': paragraph, 'select': 'Hello', 'with': text}]}}
        result = context.review('test.docx', plan)
        self.assertIsNotNone(result, context.notices)
        self.assertNotIn('p1:sha256:', json.dumps(result))
        return result['commit_key'], plan

    def test_native_abi_bytes_diagnostics_and_canonical_plan(self):
        metadata, data = execute_request(None, '{"Command":{"command":{"kind":"create","paragraphs":["ABI"]}}}')
        self.assertEqual(json.loads(metadata)['outcome'], 'completed')
        self.assertIsInstance(data, bytes)
        self.assertTrue(data.startswith(b'PK'))
        rejected, output = execute_request(None, '{}')
        self.assertEqual(json.loads(rejected)['outcome'], 'rejected')
        self.assertIsNone(output)
        self.assertIn('error', json.loads(canonical_plan_json('{}')))
        self.assertIn('async def docx_review', PRELUDE)
        self.assertIn('(await docx_read(', REFERENCE)

    def test_formatting_operations_and_section_metadata_match_native_contract(self):
        paragraph = self.context().find('test.docx', 'Hello')[0]['id']
        data = self.original
        operations = [
            {'op': 'format_paragraph', 'at': paragraph, 'indent_left': -12, 'first_line_indent': -18},
            {'op': 'format_text', 'at': paragraph, 'select': 'Hello', 'highlight': 'cyan'},
            {'op': 'set_section_layout', 'page_size': 'a4', 'orientation': 'landscape', 'columns': 2},
        ]
        for operation in operations:
            result = run_command(data, 'edit', author='Formatting regression', change_mode='track', op=operation)
            self.assertEqual(result['outcome'], 'completed', result)
            data = result['bytes']
        result = run_command(data, 'read', read_kind='document_ui', view='final')['result']
        for expected in ['text-indent:-18pt', 'margin-left:-12pt', 'data-docx-highlight="cyan"', 'data-docx-orientation="landscape"']:
            self.assertIn(expected, result['html'])
        self.assertIn('world.', result['html'])

    def test_lists_tables_and_section_handles_agree_through_python_plans(self):
        data = run_command(None, 'create', html='<p>Anchor</p><p>Item</p>')['bytes']
        inspected = run_command(data, 'read', read_kind='document_ui', view='final')['result']
        anchor, item = [p['id'] for p in inspected['paragraphs']]
        plan = {'base': 'sha256:' + hashlib.sha256(data).hexdigest(),
                'author': 'Formatting regression', 'change_mode': 'direct', 'ops': [
            {'op': 'set_list', 'at': [item], 'format': 'decimal', 'level': 0},
            {'op': 'revision_settle', 'target': 'all', 'action': 'accept'},
            {'op': 'insert_table', 'at': anchor, 'position': 'after', 'rows': 2, 'columns': 2, 'as': 'grid'},
            {'op': 'edit_table', 'at': '$grid', 'action': {
                'kind': 'insert_rows', 'row': 1, 'position': 'before', 'count': 1, 'as': 'rows'}},
            {'op': 'format_cell', 'at': '$rows/row:1/cell:1', 'format': {'fill': '#112233'}},
            {'op': 'set_section_boundary', 'at': item, 'break_type': 'nextPage'},
            {'op': 'set_section_layout', 'section_handle': 'section:' + item,
                'page_size': 'a4', 'orientation': 'landscape', 'columns': 2},
            {'op': 'set_first_page_mode', 'section': 1, 'enabled': True},
        ]}
        preview = run_plan(data, plan)
        self.assertEqual(preview['outcome'], 'previewed', preview)
        committed = run_plan(data, plan, preview_key=preview['preview_key'])
        self.assertEqual(committed['outcome'], 'committed', committed)
        result = run_command(committed['bytes'], 'read', read_kind='document_ui', view='final')['result']
        self.assertEqual(len(result['tables'][0]['rows']), 3)
        self.assertIn('#112233', result['html'])
        member = next(p for p in result['paragraphs'] if p['id'] == item)
        self.assertEqual(member['selectable_text'], 'Item')
        self.assertIn('list', member)
        self.assertEqual(result['sections'][0]['handle'], 'section:' + item)
        self.assertEqual(result['sections'][0]['page_size']['orient'], 'landscape')
        self.assertTrue(result['sections'][0]['first_page'])
        self.assertEqual(run_plan(committed['bytes'], plan)['outcome'], 'rejected')

    def test_shared_authoring_templates_stay_synchronized(self):
        root = Path(__file__).resolve().parents[3]
        spec = importlib.util.spec_from_file_location('repl_prelude_generator', root / 'scripts/generate-docxdriver-repl-prelude.py')
        generator = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(generator)
        namespace = {}
        exec(generator.generate(), namespace)
        self.assertEqual(namespace['PRELUDE'], PRELUDE)
        self.assertEqual(namespace['REFERENCE'], REFERENCE)

    def test_immutable_review_later_call_atomic_commit_and_replay(self):
        key, plan = self.review()
        self.context().commit(key)
        self.assertEqual(self.path.read_bytes(), self.original)
        plan['plan']['ops'][0]['with'] = 'Tampered'
        os.chmod(self.path, 0o640)
        context = self.context(2)
        context.commit(key)
        self.assertIn('Goodbye world.', context.read('test.docx', 'final')['markup'])
        self.assertEqual(self.path.stat().st_mode & 0o777, 0o640)
        self.assertIn('committed: 1 ops', '\n'.join(context.notices))
        context.commit(key)
        self.assertIn('consumed', context.notices[-1])
        self.assertEqual(list(self.root.glob('*.tmp')), [])

    def test_source_tamper_consumes_key_without_overwriting(self):
        key, _ = self.review()
        changed = self.original + b'external edit'
        self.path.write_bytes(changed)
        context = self.context(2)
        context.commit(key)
        self.assertEqual(self.path.read_bytes(), changed)
        self.assertIn('source changed', context.notices[-1])
        self.assertIn('consumed', self.store.lookup(key)[1])

    def test_symlink_swap_and_traversal_refused(self):
        key, _ = self.review()
        other = self.root / 'other.docx'
        other.write_bytes(self.original)
        self.path.unlink()
        self.path.symlink_to(other)
        context = self.context(2)
        context.commit(key)
        self.assertTrue(self.path.is_symlink())
        self.assertEqual(other.read_bytes(), self.original)
        self.assertIn('path changed', context.notices[-1])
        with self.assertRaises(ValueError):
            context.path('../outside.docx')
        (self.root / 'escape').symlink_to('/etc')
        with self.assertRaises(ValueError):
            context.path('escape/passwd')

    def test_prewrite_io_failure_keeps_key_and_postwrite_failure_consumes_it(self):
        key, _ = self.review()
        context = self.context(2)
        with patch.object(context, 'atomic_write', side_effect=OSError('disk full')), self.assertRaises(OSError):
            context.commit(key)
        self.assertEqual(self.path.read_bytes(), self.original)
        self.assertIsNotNone(self.store.lookup(key)[0])
        with patch.object(host, 'fsync_directory', side_effect=OSError('fsync failed')), self.assertRaises(host.PostWriteError):
            context.commit(key)
        self.assertIn('Goodbye', context.read('test.docx', 'final')['markup'])
        self.assertIn('write completed; commit key consumed', context.notices[-1])
        self.assertIn('consumed', self.store.lookup(key)[1])
        self.assertEqual(list(self.root.glob('*.tmp')), [])

    def test_limits_eviction_reset_and_exclusive_creation(self):
        limited = self.context(limits=replace(host.Limits(), plan_ops=0))
        paragraph = self.context().find('test.docx', 'Hello')[0]['id']
        with patch.object(host, 'run_plan', side_effect=AssertionError('should not call engine')):
            self.assertIsNone(limited.review('test.docx', {'plan': {'author': 'Reviewer', 'change_mode': 'track', 'ops': [{'op': 'delete_paragraphs', 'at': [paragraph]}]}}))
        limited = self.context(limits=replace(host.Limits(), docx_bytes=16))
        with self.assertRaises(host.DocumentTooLarge):
            limited.read_bytes(self.path)
        context = self.context()
        context.create('test.docx', '<p>overwrite</p>')
        self.assertEqual(self.path.read_bytes(), self.original)
        self.store = host.CommitKeyStore(replace(host.Limits(), store_records=1))
        first, _ = self.review()
        second, _ = self.review(text='Later')
        self.assertIn('expired', self.store.lookup(first)[1])
        self.store.invalidate()
        self.assertIn('expired', self.store.lookup(second)[1])

    def test_structured_quotes_audit_and_protected_comment_mutations(self):
        env = {'DOCXDRIVER_QUOTE_AUDIT_COMMENTS': '1', 'DOCXDRIVER_QUOTE_PROVENANCE_POLICY': 'permissive'}
        with patch.dict(os.environ, env):
            context = self.context()
            paragraph = context.find('test.docx', 'Hello')[0]['id']
            resolved, binding = context.resolve_quote({'source': 'test.docx', 'at': paragraph, 'select': 'Hello world.', 'changes': []})
            self.assertEqual(resolved['text'], '“Hello world.”')
            self.assertEqual(binding['hash'], hashlib.sha256(self.original).hexdigest())
            state = {'plan': {'author': 'Reviewer', 'change_mode': 'direct', 'ops': [
                {'op': 'replace_paragraph', 'at': paragraph, 'with': {'$kind': 'inline', 'parts': [
                    {'$kind': 'quote', 'source': 'test.docx', 'at': paragraph, 'select': 'Hello world.'}, ' — ', {'$kind': 'term', 'text': 'term'}]}}]}}
            review = context.review('test.docx', state)
            self.assertIsNotNone(review, context.notices)
            commit = self.context(2)
            commit.commit(review['commit_key'])
            self.assertIn('quotation audit warnings: 0', commit.notices[-1])
            raw = run_command(None, 'create', html='<p>Raw “quotation” remains.</p>')['bytes']
            audited, count = quotes.apply_audit(raw)
            self.assertEqual(count, 1)
            audited_again, count = quotes.apply_audit(audited)
            self.assertEqual(count, 1)
            self.assertEqual(len(quotes.protected(quotes.inspect_doc(audited_again)[1])), 1)
            self.path.write_bytes(audited)
            comment = quotes.protected(quotes.inspect_doc(audited)[1])[0]
            blocked = self.context().review('test.docx', {'plan': {'author': 'Reviewer', 'change_mode': 'track', 'ops': [{'op': 'comment_delete', 'comment_id': str(comment['id'])}]}})
            self.assertIsNone(blocked)

    def test_quote_source_changed_after_review_refuses_target_write(self):
        with patch.dict(os.environ, {'DOCXDRIVER_QUOTE_AUDIT_COMMENTS': '1'}):
            source = self.root / 'source.docx'
            source.write_bytes(self.original)
            context = self.context()
            paragraph = context.find('test.docx', 'Hello')[0]['id']
            state = {'plan': {'author': 'Reviewer', 'change_mode': 'track', 'ops': [{'op': 'replace_paragraph', 'at': paragraph,
                'with': {'$kind': 'quote', 'source': 'source.docx', 'at': paragraph, 'select': 'Hello world.'}}]}}
            result = context.review('test.docx', state)
            self.assertIsNotNone(result, context.notices)
            source.write_bytes(self.original + b'changed')
            context = self.context(2)
            context.commit(result['commit_key'])
            self.assertEqual(self.path.read_bytes(), self.original)
            self.assertIn('source changed', context.notices[-1])

    def test_quote_selectors_lint_and_inline_validation(self):
        text = '😀 The tenant must promptly pay the charge.'
        quote = quotes.resolve_quote({'source': 'source.docx', 'at': '12345678', 'select': 'The tenant must promptly pay the charge.',
            'changes': [{'kind': 'omit', 'select': ' promptly'}, {'kind': 'bracket', 'select': 'tenant', 'replacement': 'Tenant'}]}, text, 'hash')
        self.assertEqual(quote['text'], '“The [Tenant] must… pay the charge.”')
        self.assertEqual(quote['span']['start'], 3)
        with self.assertRaises(ValueError):
            quotes.resolve_quote({'source': 'x', 'at': 'x', 'select': 'tenant', 'changes': [
                {'kind': 'omit', 'select': 'tenant'}, {'kind': 'omit', 'select': 'ten'}]}, 'tenant', 'hash')
        self.assertFalse(quotes.lint_quotes("don't <a href=\"/x\">link</a>"))
        self.assertTrue(quotes.lint_quotes('Text &quot;quoted&quot;'))
        with self.assertRaises(ValueError):
            quotes.compile_inline(['raw "quote"'], 'error', None)
        with self.assertRaises(ValueError):
            quotes.compile_inline([{'$kind': 'quote'}, '.'], 'error', lambda _: {'text': '“End.”', 'quote_id': 'id'})

    def test_quotation_mode_and_reserved_policies_fail_closed(self):
        paragraph = self.context().find('test.docx', 'Hello')[0]['id']
        state = {'plan': {'author': 'Reviewer', 'change_mode': 'direct', 'ops': [
            {'op': 'replace_paragraph', 'at': paragraph, 'with': {'$kind': 'term', 'text': 'term'}}]}}
        with patch.dict(os.environ, {'DOCXDRIVER_QUOTE_AUDIT_COMMENTS': '0'}):
            context = self.context()
            self.assertIsNone(context.review('test.docx', state))
            self.assertIn('disabled', context.notices[-1])
        for policy in ('controlled', 'authoritative'):
            with patch.dict(os.environ, {'DOCXDRIVER_QUOTE_AUDIT_COMMENTS': '1', 'DOCXDRIVER_QUOTE_PROVENANCE_POLICY': policy}):
                context = self.context()
                self.assertIsNone(context.review('test.docx', state))
                self.assertIn('reserved but not implemented', context.notices[-1])
        self.assertEqual(self.store.records, {})

    def test_audit_refuses_unanchorable_and_ambiguous_renderings(self):
        from docxdriver import run_plan
        plan = {'base': 'sha256:' + hashlib.sha256(self.original).hexdigest(), 'author': 'Reviewer', 'change_mode': 'direct',
                'ops': [{'op': 'set_header', 'with': '<p>“Unanchorable” header.</p>'}]}
        preview = run_plan(self.original, plan)
        candidate = run_plan(self.original, plan, preview['preview_key'])['bytes']
        with self.assertRaisesRegex(ValueError, 'cannot carry a comment anchor'):
            quotes.apply_audit(candidate)
        candidate = run_command(None, 'create', html='<p>“Trusted” and “Trusted”</p>')['bytes']
        with self.assertRaisesRegex(ValueError, 'missing or ambiguous'):
            quotes.apply_audit(candidate, [{'kind': 'term', 'text': '“Trusted”'}])

    def test_review_context_and_key_record_limits_do_not_evict_valid_keys(self):
        self.path.write_bytes(run_command(None, 'create', html='<p>Hello ' + 'long text ' * 2000 + '</p>')['bytes'])
        context = self.context(limits=replace(host.Limits(), edit_context_bytes=128, review_bytes=256))
        paragraph = context.find('test.docx', 'Hello')[0]['id']
        plan = {'plan': {'author': 'Reviewer', 'change_mode': 'track', 'ops': [
            {'op': 'replace_text', 'at': paragraph, 'select': 'Hello', 'with': 'Bye'}]}}
        result = context.review('test.docx', plan)
        self.assertIsNotNone(result, context.notices)
        self.assertLessEqual(host.size(result), 256)
        self.assertTrue(result['truncated'])
        key = result['commit_key']
        context = self.context(limits=replace(host.Limits(), record_bytes=32))
        self.assertIsNone(context.review('test.docx', plan))
        self.assertIn('record too large', context.notices[-1])
        self.assertIsNotNone(self.store.lookup(key)[0])


class CancellationTests(unittest.IsolatedAsyncioTestCase):
    async def test_cancelled_native_callback_is_drained_before_lock_release(self):
        with tempfile.TemporaryDirectory() as root:
            bridge = host.NativeBridge()
            data = run_command(None, 'create', html='<p>Hello.</p>')['bytes']
            path = Path(root) / 'test.docx'
            path.write_bytes(data)
            paragraph = run_command(data, 'find', query='Hello')['result']['matches'][0]['id']
            result = await bridge.request(action='call', session='one', cwd=root, execution=1, name='_docx_review', args=[
                'test.docx', {'plan': {'author': 'Reviewer', 'change_mode': 'track', 'ops': [{'op': 'replace_text', 'at': paragraph, 'select': 'Hello', 'with': 'Bye'}]}}])
            key = result['result']['commit_key']
            entered, release = threading.Event(), threading.Event()
            original_write = host.PlanHost.atomic_write
            def delayed_write(instance, *args, **kwargs):
                entered.set()
                release.wait(5)
                return original_write(instance, *args, **kwargs)
            notices = []
            with patch.object(host.PlanHost, 'atomic_write', delayed_write):
                task = asyncio.create_task(bridge.request(action='call', session='one', cwd=root, execution=2, name='_docx_commit', args=[key], on_notices=notices.extend))
                self.assertTrue(await asyncio.to_thread(entered.wait, 5))
                task.cancel()
                await asyncio.sleep(0.01)
                self.assertFalse(task.done())
                release.set()
                with self.assertRaises(asyncio.CancelledError):
                    await task
            self.assertEqual(path.read_bytes(), data)
            self.assertIn('commit key kept', '\n'.join(notices))
            result = await bridge.request(action='call', session='one', cwd=root, execution=3, name='_docx_commit', args=[key])
            self.assertIn('committed: 1 ops', '\n'.join(result['notices']))
            await bridge.close()

    async def test_cancellation_after_rename_reports_completed_write_and_consumes_key(self):
        with tempfile.TemporaryDirectory() as root:
            bridge = host.NativeBridge()
            data = run_command(None, 'create', html='<p>Hello.</p>')['bytes']
            path = Path(root) / 'test.docx'
            path.write_bytes(data)
            paragraph = run_command(data, 'find', query='Hello')['result']['matches'][0]['id']
            result = await bridge.request(action='call', session='one', cwd=root, execution=1, name='_docx_review', args=[
                'test.docx', {'plan': {'author': 'Reviewer', 'change_mode': 'track', 'ops': [{'op': 'replace_text', 'at': paragraph, 'select': 'Hello', 'with': 'Bye'}]}}])
            key = result['result']['commit_key']
            renamed, release = threading.Event(), threading.Event()
            original_fsync = host.fsync_directory
            def delayed_fsync(*args):
                renamed.set()
                release.wait(5)
                return original_fsync(*args)
            notices = []
            with patch.object(host, 'fsync_directory', delayed_fsync):
                task = asyncio.create_task(bridge.request(action='call', session='one', cwd=root, execution=2, name='_docx_commit', args=[key], on_notices=notices.extend))
                self.assertTrue(await asyncio.to_thread(renamed.wait, 5))
                task.cancel()
                await asyncio.sleep(0.01)
                release.set()
                with self.assertRaises(asyncio.CancelledError):
                    await task
            self.assertIn('write completed', '\n'.join(notices))
            self.assertIn('Bye.', run_command(path.read_bytes(), 'read', view='final')['result']['markup'])
            result = await bridge.request(action='call', session='one', cwd=root, execution=3, name='_docx_commit', args=[key])
            self.assertIn('consumed', '\n'.join(result['notices']))
            await bridge.close()


if __name__ == '__main__':
    unittest.main()
