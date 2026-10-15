#!/usr/bin/env python3
"""Open docxdriver outputs in Microsoft Word for Mac; fail on any that Word
will not open without its "unreadable content" recovery dialog.

Word is the only real oracle for what Word accepts; the schema, integrity and
Open XML SDK checks in ooxml-conformance.py approximate it. This needs macOS
with Word installed, so it is not part of CI:

    cargo build -p docxdriver-cli
    python scripts/ooxml-conformance.py --keep-outputs target/word-smoke
    python scripts/word-smoke.py target/word-smoke

Outputs are named `<source>--<op>-<mode>-<hash>.docx` next to the unedited
`<source>--source.docx`. Each source is opened first: when Word already
rejects the source, its outputs are skipped, since the edit cannot be blamed
(the same baseline rule as the conformance gate). By default one output per
source/operation/change-mode is opened; `--all` opens every file. The
synthetic document from `create` is a source too, so `create` itself is
covered. Word is launched fresh and force-quit for each file, so the script
refuses to run while Word is already open. The first run asks to allow the
terminal to control Word.
"""
import argparse
import subprocess
import sys
import time
from pathlib import Path

APP = 'Microsoft Word'


def word_running():
    return subprocess.run(['pgrep', '-f', f'{APP}.app'], capture_output=True).returncode == 0


def quit_word():
    subprocess.run(['pkill', '-9', '-f', f'{APP}.app'], capture_output=True)
    while word_running():
        time.sleep(0.5)


def open_documents():
    try:
        result = subprocess.run(['osascript', '-e', f'tell application "{APP}" to get name of documents'],
                                capture_output=True, text=True, timeout=5)
    except subprocess.TimeoutExpired:
        return ''
    return result.stdout


def opens_cleanly(path, timeout):
    """True once Word lists the document; a recovery dialog keeps it from loading."""
    quit_word()
    # -F: do not restore windows from the previous (killed) session.
    subprocess.run(['open', '-F', '-a', APP, str(path)], check=True)
    deadline = time.monotonic() + timeout
    try:
        while time.monotonic() < deadline:
            time.sleep(1)
            if path.name in open_documents():
                return True
        return False
    finally:
        quit_word()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('inputs', nargs='+', help='.docx files or directories of them')
    parser.add_argument('--all', action='store_true', help='open every file, not one per operation/mode')
    parser.add_argument('--timeout', type=float, default=20, help='seconds to wait for each document (default: 20)')
    args = parser.parse_args()

    if sys.platform != 'darwin' or not Path(f'/Applications/{APP}.app').exists():
        print(f'word-smoke needs macOS with {APP} installed', file=sys.stderr)
        return 2
    if word_running():
        print(f'{APP} is running; save and quit it first (this script force-quits Word)', file=sys.stderr)
        return 2

    files = []
    for item in map(Path, args.inputs):
        files += sorted(item.glob('*.docx')) if item.is_dir() else [item]
    groups = {}
    for path in files:
        source, _, label = path.stem.partition('--')
        groups.setdefault(source, {})
        if label != 'source':
            key = path if args.all else '-'.join(label.split('-')[:2])
            groups[source].setdefault(key, path)

    failures, skipped, count = [], 0, 0
    for source, outputs in groups.items():
        source_path = next((path for path in files if path.stem == f'{source}--source'), None)
        if source_path is not None and not opens_cleanly(source_path.resolve(), args.timeout):
            print(f'skip {source}: Word rejects the unedited source ({len(outputs)} outputs)', flush=True)
            skipped += len(outputs)
            continue
        for path in outputs.values():
            count += 1
            ok = opens_cleanly(path.resolve(), args.timeout)
            print(f'[{count}] {"ok  " if ok else "FAIL"} {path.name}', flush=True)
            if not ok:
                failures.append(path)
    print(f'{count} opened, {skipped} skipped (source rejected by Word), {len(failures)} failed')
    if failures:
        print('did not open cleanly in Word:')
        for path in failures:
            print('  ', path)
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(main())
