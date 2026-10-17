"""Trusted native plan host. No Python extensions execute inside the Monty sandbox."""
import asyncio
from collections import OrderedDict
from contextlib import contextmanager
from dataclasses import dataclass
import errno
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import stat
import tempfile
import threading
import time
from weakref import WeakValueDictionary

from docxdriver import canonical_plan_json, run_command, run_plan
from . import PRELUDE, REFERENCE
from . import quotes


@dataclass(frozen=True)
class Limits:
    docx_bytes: int = 16 * 1024 * 1024
    find_results: int = 1000
    plan_ops: int = 2000
    canonical_bytes: int = 512 * 1024
    edit_context_bytes: int = 8 * 1024
    review_bytes: int = 256 * 1024
    record_bytes: int = 4 * 1024 * 1024
    store_bytes: int = 8 * 1024 * 1024
    store_records: int = 64
    callback_seconds: float = 30


def compact(value):
    return json.dumps(value, ensure_ascii=False, allow_nan=False, separators=(',', ':'))


def size(value):
    return len(compact(value).encode())


def sha(data):
    return hashlib.sha256(data).hexdigest()


def truncate(text, limit):
    return text.encode()[:max(0, limit)].decode('utf-8', errors='ignore')


class SourceChanged(ValueError):
    pass


class PathChanged(ValueError):
    pass


class DocumentTooLarge(ValueError):
    pass


class PostWriteError(OSError):
    pass


class CommitKeyStore:
    def __init__(self, limits=Limits()):
        self.limits = limits
        self.records, self.expired, self.consumed = OrderedDict(), OrderedDict(), OrderedDict()

    @property
    def retained_bytes(self):
        return sum(size(record) for record in self.records.values())

    def terminal(self, collection, key):
        collection[key] = True
        while len(collection) > 256:
            collection.popitem(last=False)

    def put(self, record):
        if size(record) > min(self.limits.record_bytes, self.limits.store_bytes):
            raise ValueError('commit key record too large')
        self.records[record['id']] = record
        while len(self.records) > self.limits.store_records or self.retained_bytes > self.limits.store_bytes:
            key, _ = self.records.popitem(last=False)
            self.terminal(self.expired, key)

    def lookup(self, key):
        if key in self.expired:
            return None, 'commit key expired — review again'
        if key in self.consumed:
            return None, 'commit key already consumed — review again'
        return self.records.get(key), 'unknown commit key — review first'

    def consume(self, key):
        self.records.pop(key, None)
        self.terminal(self.consumed, key)

    def invalidate(self):
        for key in list(self.records):
            self.terminal(self.expired, key)
        self.records.clear()


_FILE_LOCKS = WeakValueDictionary()
_LOCKS_LOCK = threading.Lock()


@contextmanager
def file_queue(path):
    with _LOCKS_LOCK:
        lock = _FILE_LOCKS.get(str(path))
        if lock is None:
            lock = threading.RLock()
            _FILE_LOCKS[str(path)] = lock
    with lock:
        yield


def fsync_directory(path):
    descriptor = os.open(path, os.O_RDONLY)
    try:
        try:
            os.fsync(descriptor)
        except OSError as error:
            if error.errno not in (errno.EINVAL, errno.ENOTSUP, errno.EBADF, errno.EISDIR):
                raise
    finally:
        os.close(descriptor)


def review_edits(report, limits):
    edits, truncated = [], False
    for index, op in enumerate(report['ops']):
        context, cut = [], bool(op.get('context_truncated'))
        for item in op.get('affected', []):
            if not isinstance(item, dict):
                cut = True
                continue
            remaining = limits.edit_context_bytes - size(context)
            if size(item) > remaining:
                original = item
                item = {key: original[key] for key in ('para_id',) if key in original}
                item['context_truncated'] = True
                cut = True
                budget = max(0, remaining - size(item) - 24)
                item['markup'] = truncate(original.get('markup', ''), budget) + '…'
            if size([*context, item]) <= limits.edit_context_bytes:
                context.append(item)
            else:
                cut = True
        summary = op.get('summary', '')
        if len(summary.encode()) > 4096:
            summary = truncate(summary, 4096) + '…'
            truncated = True
        edit = {'index': op.get('index', index) + 1, 'op': op['op'], 'outcome': op['outcome'], 'summary': summary, 'context': context}
        if cut:
            edit['context_truncated'] = True
            truncated = True
        edits.append(edit)
    result = {'commit_key': 'k1:' + '0' * 32, 'edits': edits, 'truncated': truncated}
    while size(result) > limits.review_bytes:
        edit = next((item for item in reversed(edits) if item['context']), None)
        if edit:
            edit['context'] = []
        else:
            edit = next((item for item in reversed(edits) if item['summary']), None)
            if not edit:
                raise ValueError('review result exceeds aggregate limit')
            edit['summary'] = truncate(edit['summary'], len(edit['summary'].encode()) // 2)
        edit['context_truncated'] = result['truncated'] = True
    return result


class PlanHost:
    def __init__(self, cwd, store, execution, cancelled=None, limits=Limits()):
        self.root = Path(cwd).resolve(strict=True)
        self.store, self.execution, self.limits = store, execution, limits
        self.cancelled = cancelled or threading.Event()
        self.deadline = time.monotonic() + limits.callback_seconds
        self.notices = []
        self.audit = os.getenv('DOCXDRIVER_QUOTE_AUDIT_COMMENTS', '').strip().lower() in ('1', 'true', 'yes', 'on')
        self.policy = os.getenv('DOCXDRIVER_QUOTE_PROVENANCE_POLICY', 'permissive').strip().lower() if self.audit else 'permissive'
        if self.policy not in ('permissive', 'controlled', 'authoritative'):
            raise ValueError('DOCXDRIVER_QUOTE_PROVENANCE_POLICY must be permissive, controlled, or authoritative')

    def check(self):
        if self.cancelled.is_set():
            raise RuntimeError('aborted')
        if self.deadline is not None and time.monotonic() >= self.deadline:
            raise TimeoutError('host callback timed out')

    def notice(self, summary, key=None):
        summary = truncate(str(summary), 4096)
        self.notices.append(f'{key} — {summary}' if key else summary)
        self.notices[:] = self.notices[-32:]

    def path(self, value):
        if not isinstance(value, str) or not value or '\x00' in value:
            raise ValueError('path must be a non-empty string')
        lexical = Path(os.path.abspath(self.root / value))
        if not lexical.is_relative_to(self.root):
            raise ValueError('path escapes cwd')
        target = lexical.resolve()
        if not target.is_relative_to(self.root):
            raise ValueError('path escapes cwd')
        return target

    def read_bytes(self, path, limit=None):
        self.check()
        limit = self.limits.docx_bytes if limit is None else limit
        # Reject devices/FIFOs, avoid symlink swaps, and cap growth after the stat probe.
        descriptor = os.open(path, os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0) | getattr(os, 'O_NONBLOCK', 0))
        with os.fdopen(descriptor, 'rb') as file:
            info = os.fstat(file.fileno())
            if not stat.S_ISREG(info.st_mode):
                raise ValueError('DOCX source must be a regular file')
            if info.st_size > limit:
                raise DocumentTooLarge(f'document exceeds input limit ({limit} bytes)')
            chunks, total = [], 0
            while True:
                self.check()
                data = file.read(min(64 * 1024, limit - total + 1))
                if not data:
                    break
                total += len(data)
                if total > limit:
                    raise DocumentTooLarge(f'document exceeds input limit ({limit} bytes)')
                chunks.append(data)
            return b''.join(chunks)

    def assert_path(self, path, parent=None):
        actual = path.resolve(strict=True)
        if actual != path or not actual.is_relative_to(self.root):
            raise PathChanged('path changed after review — re-read and review again')
        actual_parent = path.parent.resolve(strict=True)
        if parent is not None and actual_parent != parent:
            raise PathChanged('path changed after review — re-read and review again')

    def assert_source(self, path, expected):
        self.assert_path(path)
        try:
            actual = sha(self.read_bytes(path))
        except DocumentTooLarge:
            raise SourceChanged('source changed after review — re-read and review again')
        if actual != expected:
            raise SourceChanged('source changed after review — re-read and review again')

    def atomic_write(self, path, data, expected=None):
        parent = path.parent.resolve(strict=True)
        if not parent.is_relative_to(self.root):
            raise PathChanged('path changed outside cwd before write')
        if expected is not None:
            self.assert_source(path, expected)
        descriptor, temporary = tempfile.mkstemp(prefix='.docxdriver-', suffix='.tmp', dir=parent)
        renamed = False
        try:
            with os.fdopen(descriptor, 'wb') as file:
                if expected is not None:
                    os.fchmod(file.fileno(), stat.S_IMODE(path.stat().st_mode))
                file.write(data)
                file.flush()
                os.fsync(file.fileno())
            self.check()
            if path.parent.resolve(strict=True) != parent or not parent.is_relative_to(self.root):
                raise PathChanged('path changed outside cwd before write')
            if expected is None:
                # Exclusive link never overwrites a destination created concurrently.
                os.link(temporary, path)
            else:
                self.assert_path(path, parent)
                self.assert_source(path, expected)
                self.check()
                os.replace(temporary, path)
            renamed = True
            fsync_directory(parent)
        except Exception as error:
            if renamed:
                raise PostWriteError(f'failure after atomic write: {error}') from error
            raise
        finally:
            try:
                Path(temporary).unlink(missing_ok=True)
            except OSError as error:
                if renamed:
                    raise PostWriteError(f'write completed; temporary cleanup failed: {error}') from error
                raise

    def create(self, path, html):
        if not isinstance(html, str) or len(html.encode()) > self.limits.docx_bytes:
            self.notice('create refused: html must be a string within the document input limit')
            return None
        target = self.path(path)
        with file_queue(target):
            if target.exists():
                self.notice('create refused: file already exists')
                return None
            result = run_command(None, 'create', html=html)
            if result.get('outcome') != 'completed' or not result.get('bytes'):
                self.notice(result.get('diagnostic', {}).get('message', 'create failed'))
                return None
            if len(result['bytes']) > self.limits.docx_bytes:
                self.notice('create refused: candidate exceeds input limit')
                return None
            self.check()
            try:
                self.atomic_write(target, result['bytes'])
            except PostWriteError:
                self.notice('create infrastructure failure — write completed')
                raise
            self.notice('created')

    def read(self, path, view='markup', kind=None):
        target = self.path(path)
        if isinstance(kind, dict):
            kind = kind.get('kind')
        if kind in ('comments', 'styles', 'revisions'):
            arguments = {'read_kind': kind}
        else:
            if view not in ('markup', 'final', 'original'):
                raise ValueError('view must be one of markup, final, original')
            arguments = {'view': view}
        with file_queue(target):
            result = quotes.result_of(run_command(self.read_bytes(target), 'read', **arguments))
            return result if 'read_kind' in arguments else {'markup': result['markup'], 'equations': result.get('equations', [])}

    def find(self, path, query, ignore_case=False):
        if not isinstance(query, str) or not query.strip():
            raise ValueError('query must be a non-empty string')
        target = self.path(path)
        with file_queue(target):
            result = quotes.result_of(run_command(self.read_bytes(target), 'find', query=query, ignore_case=bool(ignore_case)))['matches']
            if len(result) > self.limits.find_results:
                raise ValueError('find exceeds match limit; narrow the query')
            return result

    def require_quotes(self):
        if not self.audit:
            raise ValueError('structured quotation mode is disabled; enable DOCXDRIVER_QUOTE_AUDIT_COMMENTS')
        if self.policy != 'permissive':
            raise ValueError(f'quotation provenance policy {self.policy} is reserved but not implemented; use permissive')

    def resolve_quote(self, spec):
        self.require_quotes()
        if not isinstance(spec, dict):
            raise ValueError('quote must be an object')
        target = self.path(spec.get('source'))
        data = self.read_bytes(target)
        result = quotes.result_of(run_command(data, 'find', query=spec.get('select'), ignore_case=False))
        match = next((match for match in result['matches'] if match.get('id') == spec.get('at') and spec.get('select', '') in match['text']), None)
        if match is None:
            raise ValueError('quote span was not found exactly in the selected paragraph')
        resolved = quotes.resolve_quote(spec, match['text'], sha(data))
        resolved['provenance_tier'] = 'unregistered_local'
        return resolved, {'source': spec['source'], 'path': str(target), 'hash': sha(data), 'provenance_tier': 'unregistered_local'}

    def review(self, path, state):
        try:
            if not isinstance(state, dict) or not isinstance(state.get('plan'), dict):
                raise ValueError('state must contain a plan dictionary')
            authored = state['plan']
            if not isinstance(authored.get('ops'), list):
                raise ValueError('plan must contain an ops list')
            if len(authored['ops']) > self.limits.plan_ops:
                raise ValueError('plan exceeds operation limit')
            if size(authored) > self.limits.canonical_bytes:
                raise ValueError('plan exceeds canonical size limit')
            if self.audit and self.policy != 'permissive':
                self.require_quotes()
            target = self.path(path)
            with file_queue(target):
                data = self.read_bytes(target)
                bindings, trusted = [], []
                if self.audit:
                    state, bindings, trusted = quotes.resolve_plan(state, self.resolve_quote)
                    ids = quotes.protected_mutations(data, state['plan']['ops'])
                    if ids:
                        raise ValueError('plan attempts to mutate protected quotation audit comments: ' + ', '.join(ids))
                elif quotes.contains_structured(state):
                    self.require_quotes()
                plan = {**state['plan'], 'base': 'sha256:' + sha(data)}
                canonical = canonical_plan_json(compact(plan))
                parsed = json.loads(canonical)
                if 'error' in parsed:
                    raise ValueError(parsed['error'])
                if len(canonical.encode()) > self.limits.canonical_bytes:
                    raise ValueError('plan exceeds canonical size limit')
                preview = run_plan(data, parsed)
                if preview.get('outcome') != 'previewed' or not quotes.completed(preview.get('report'), len(parsed['ops'])):
                    raise ValueError(preview.get('diagnostic', {}).get('message', 'review rejected: core report incomplete'))
                if not re.fullmatch(r'p1:sha256:[0-9a-f]{64}', preview.get('preview_key', '')):
                    raise ValueError('review rejected: core preview key missing')
                result = review_edits(preview['report'], self.limits)
                key = 'k1:' + secrets.token_hex(16)
                record = {'id': key, 'root': str(self.root), 'path': str(target), 'display': path, 'source': sha(data),
                          'plan': canonical, 'core_key': preview['preview_key'], 'execution': self.execution,
                          'state': 'reviewed', 'bindings': bindings, 'trusted': trusted}
                if size(record) > self.limits.record_bytes:
                    raise ValueError('commit key record too large')
                self.check()
                self.store.put(record)
                result['commit_key'] = key
                self.notice(f"review ok: {len(parsed['ops'])} ops; commit_key: {key}", key)
                return result
        except ValueError as error:
            self.notice(f'review blocked: {error}')
            return None

    def commit(self, key):
        if not isinstance(key, str) or not re.fullmatch(r'k1:[0-9a-f]{32}', key):
            self.notice('malformed commit key')
            return None
        record, diagnostic = self.store.lookup(key)
        if record is None:
            self.notice(diagnostic)
            return None
        if self.execution <= record['execution']:
            self.notice('commit requires a later python execution', key)
            return None
        if record['state'] == 'committing':
            self.notice('commit already in progress', key)
            return None
        # Receipt binds the root/target of review, independently of the next call's cwd.
        self.root = Path(record['root'])
        target = Path(record['path'])
        renamed = False
        with file_queue(target):
            try:
                self.assert_source(target, record['source'])
                for binding in record['bindings']:
                    try:
                        self.assert_source(Path(binding['path']), binding['hash'])
                    except FileNotFoundError:
                        raise SourceChanged('quotation source changed after review')
                data = self.read_bytes(target)
                # The second read must still match the immutable reviewed source.
                if sha(data) != record['source']:
                    raise SourceChanged('source changed after review — re-read and review again')
                plan = json.loads(record['plan'])
                self.check()
                record['state'] = 'committing'
                self.deadline = None  # Finish the irreversible window even after its time budget.
                result = run_plan(data, plan, record['core_key'])
                if result.get('outcome') != 'committed' or not result.get('bytes'):
                    self.store.consume(key)
                    self.notice(result.get('diagnostic', {}).get('message', 'commit rejected'), key)
                    return None
                if result.get('source') != 'sha256:' + record['source'] or not quotes.completed(result.get('report'), len(plan['ops'])):
                    raise ValueError('candidate verification failed: source/report mismatch')
                candidate = result['bytes']
                warnings = 0
                if self.audit:
                    candidate, warnings = quotes.apply_audit(candidate, record['trusted'])
                if len(candidate) > self.limits.docx_bytes:
                    raise ValueError('candidate exceeds input limit')
                quotes.result_of(run_command(candidate, 'read', view='final'))
                self.check()
                self.atomic_write(target, candidate, record['source'])
                renamed = True
                # Post-write verification is non-cancellable: report the completed write truthfully.
                descriptor = os.open(target, os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0) | getattr(os, 'O_NONBLOCK', 0))
                with os.fdopen(descriptor, 'rb') as file:
                    if os.fstat(file.fileno()).st_size != len(candidate) or sha(file.read(len(candidate) + 1)) != sha(candidate):
                        raise PostWriteError('read-back mismatch after atomic write')
                self.store.consume(key)
                suffix = f'; quotation audit warnings: {warnings}' if self.audit else ''
                if self.cancelled.is_set():
                    suffix += ' — cancellation arrived during commit; write completed'
                self.notice(f"committed: {len(plan['ops'])} ops{suffix}", key)
            except (SourceChanged, PathChanged, DocumentTooLarge, ValueError) as error:
                self.store.consume(key)
                self.notice(str(error), key)
            except BaseException as error:
                if renamed or isinstance(error, PostWriteError):
                    self.store.consume(key)
                    self.notice(f'commit infrastructure failure — write completed; commit key consumed: {error}', key)
                else:
                    record['state'] = 'reviewed'
                    self.notice(f'commit infrastructure failure — commit key kept, retry: {error}', key)
                raise

    def call(self, name, args):
        self.check()
        callbacks = {'_docx_create': self.create, '_docx_read': self.read, '_docx_find': self.find,
                     '_docx_review': self.review, '_docx_commit': self.commit,
                     '_docx_help': lambda topic=None: REFERENCE, '_quote_lint': quotes.lint_quotes,
                     '_term_render': quotes.render_term}
        if name == '_quote_find':
            self.require_quotes()
            return self.find(*args)
        if name == '_quote_validate':
            return self.resolve_quote(*args)[0]
        if name == '_inline_validate':
            self.require_quotes()
            value, policy = args
            return quotes.compile_inline(value.get('parts', []), policy, lambda spec: self.resolve_quote(spec)[0])
        if name not in callbacks:
            raise ValueError('unknown docxdriver callback')
        return callbacks[name](*args)


class NativeBridge:
    """Async embedding adapter; synchronous native work runs off the server event loop."""
    def __init__(self, limits=Limits()):
        self.limits = limits
        self.sessions = OrderedDict()
        self.lock = asyncio.Lock()

    async def close(self):
        async with self.lock:
            for store in self.sessions.values():
                store.invalidate()
            self.sessions.clear()

    async def request(self, *, on_notices=None, **payload):
        async with self.lock:
            if payload['action'] == 'init':
                return {'prelude': PRELUDE, 'reference': REFERENCE}
            session = payload['session']
            if payload['action'] == 'reset':
                store = self.sessions.pop(session, None)
                if store:
                    store.invalidate()
                return {'result': None}
            if session not in self.sessions:
                if len(self.sessions) >= 128:
                    _, evicted = self.sessions.popitem(last=False)
                    evicted.invalidate()
                self.sessions[session] = CommitKeyStore(self.limits)
            self.sessions.move_to_end(session)
            cancelled = threading.Event()
            store = self.sessions[session]
            def execute():
                host = PlanHost(payload['cwd'], store, payload['execution'], cancelled, self.limits)
                try:
                    return {'result': host.call(payload['name'], payload.get('args', [])), 'notices': host.notices}
                except BaseException as error:
                    return {'error': str(error), 'notices': host.notices}
            worker = asyncio.create_task(asyncio.to_thread(execute))
            try:
                result = await asyncio.shield(worker)
            except asyncio.CancelledError:
                cancelled.set()
                # Never abandon a native callback that may still be writing.
                while not worker.done():
                    try:
                        await asyncio.shield(worker)
                    except asyncio.CancelledError:
                        cancelled.set()
                result = worker.result()
                if on_notices:
                    on_notices(result.get('notices', []))
                raise
            if on_notices:
                on_notices(result.get('notices', []))
            if 'error' in result:
                raise RuntimeError(result['error'])
            return result
