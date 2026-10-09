/**
 * Version 3 experimental surface: structured Python document model.
 *
 * The document is exposed as Python-native Document/Paragraph/Selection
 * objects; mutations record a change set the host replays into typed core
 * operations. Preview and commit flow through the shared host and commit
 * gate.
 *
 *   pi --no-builtin-tools --tools python \
 *     -e ./packages/docxdriver-pi/src/python-model-extension.ts
 */
import { createPythonExtension } from './python-extension-factory.js';
import { PYTHON_API_REFERENCE, PYTHON_PRELUDE, PYTHON_TYPE_STUBS } from './python-model-prelude.js';
import { modelSurfaceDeriver } from './python-model-deriver.js';

export default createPythonExtension({
  name: 'model',
  prelude: PYTHON_PRELUDE,
  stubs: PYTHON_TYPE_STUBS,
  apiReference: PYTHON_API_REFERENCE,
  deriver: modelSurfaceDeriver(),
  toolName: 'python',
  toolDescription: 'Persistent sandboxed Python REPL with a structured DOCX document model (Version 3: Python-native Document/Paragraph/Selection).',
  promptSnippet: 'Run stateful sandboxed Python mutating a structured DOCX document model',
  promptGuidelines: [
    'Do all DOCX work through the python tool; never use other file tools on .docx files.',
    'Open the document once with docx_open(path); mutate paragraphs and selections in place; reopen only after an external change.',
    'Use para.replace(old, new, occurrence=...) / para.text / para.delete() / para.style, doc.select(...).bold = True, and doc.insert_after(para, ...) / doc.insert_before(para, ...).',
    'Preview with docx_preview(doc); commit only in a separate later python call with docx_commit(doc, preview.key).',
    'Repair a stale selection by adjusting the paragraph text or reopening the document, then preview again.',
  ],
});
