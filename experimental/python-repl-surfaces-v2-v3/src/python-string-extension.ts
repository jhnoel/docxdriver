/**
 * Version 2 experimental surface: raw projection string.
 *
 * The document is exposed as a canonical projection string; the agent edits
 * it with ordinary Python string and regex operations, and the host
 * reconciles original vs proposed into typed core operations.
 *
 *   pi --no-builtin-tools --tools python \
 *     -e ./packages/docxdriver-pi/src/python-string-extension.ts
 */
import { createPythonExtension } from './python-extension-factory.js';
import { PYTHON_API_REFERENCE, PYTHON_PRELUDE, PYTHON_TYPE_STUBS } from './python-string-prelude.js';
import { stringSurfaceDeriver } from './python-string-deriver.js';

export default createPythonExtension({
  name: 'string',
  prelude: PYTHON_PRELUDE,
  stubs: PYTHON_TYPE_STUBS,
  apiReference: PYTHON_API_REFERENCE,
  deriver: stringSurfaceDeriver(),
  toolName: 'python',
  toolDescription: 'Persistent sandboxed Python REPL that edits DOCX via raw projection strings (Version 2: raw projection string).',
  promptSnippet: 'Run stateful sandboxed Python editing DOCX projection strings',
  promptGuidelines: [
    'Do all DOCX work through the python tool; never use other file tools on .docx files.',
    'Work from docx_read(path) output only; never hand-edit the id, ord, or num attributes.',
    'Edit the projection string with str.replace or re.sub, keeping surrounding context and matching the exact markup text.',
    'Keep original from the same docx_read call you edited; preview with docx_preview(path, original=original, proposed=draft); commit only in a separate later python call with docx_commit(path, preview.key, proposed=draft).',
    'Repair a blocked preview by adjusting the draft string (or re-reading the document) and previewing again.',
  ],
});
