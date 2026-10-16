import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { init, executeRequest } from '../dist/index.js';
await init(await readFile(new URL('../wasm/docxdriver_bg.wasm', import.meta.url)));

test('WASM formatting preserves text, supports signed indents and palette inheritance, and rejects stale plans', () => {
  const created = executeRequest(undefined, {Command:{command:{kind:'create',html:'<p>Before selected after</p><p>Neighbor</p>'}}});
  assert.equal(created.outcome, 'completed');
  const read = bytes => executeRequest(bytes, {Command:{command:{kind:'read',read_kind:'document_ui',view:'final'}}});
  const inspected = read(created.bytes);
  const at = inspected.result.html.match(/<p id="([0-9A-F]{8})"/)[1];
  const plan = {base: inspected.source, author:'Formatting regression', change_mode:'track', ops:[
    {op:'format_paragraph',at,indent_left:-12,first_line_indent:-18},
    {op:'format_text',at,select:'selected',highlight:'cyan'},
    {op:'set_section_layout',page_size:'a4',orientation:'landscape',columns:2},
  ]};
  const preview = executeRequest(created.bytes,{Plan:{plan}});
  assert.equal(preview.outcome,'previewed',JSON.stringify(preview));
  const committed = executeRequest(created.bytes,{Plan:{plan,preview_key:preview.preview_key}});
  assert.equal(committed.outcome,'committed');
  const markup = read(committed.bytes).result.html;
  assert.match(markup,/text-indent:-18pt/);
  assert.match(markup,/margin-left:-12pt/);
  assert.match(markup,/data-docx-highlight="cyan"/);
  assert.match(markup,/data-docx-orientation="landscape"/);
  assert.match(markup,/Neighbor/);
  assert.equal(executeRequest(committed.bytes,{Plan:{plan,preview_key:preview.preview_key}}).outcome,'rejected');
});

test('WASM direct table edits preserve inserted group aliases as positions shift', () => {
  const created = executeRequest(undefined, {Command:{command:{kind:'create',html:'<table><tr><td>A</td><td>B</td></tr><tr><td>C</td><td>D</td></tr></table><p>Neighbor</p>'}}});
  assert.equal(created.outcome,'completed');
  const read = bytes => executeRequest(bytes,{Command:{command:{kind:'read',read_kind:'document_ui',view:'final'}}});
  const inspected=read(created.bytes);
  const plan={base:inspected.source,author:'Grid regression',change_mode:'direct',ops:[
    {op:'edit_table',at:'table:1',action:{kind:'insert_rows',row:1,position:'before',count:2,as:'rows'}},
    {op:'edit_table',at:'table:1',action:{kind:'insert_columns',column:1,position:'after',count:2,as:'cols'}},
    {op:'format_row',at:'$rows/row:2',format:{repeating_header:true}},
    {op:'format_cell',at:'$rows/row:1/cell:2',format:{fill:'#FF0000'}},
    {op:'format_cell',at:'$cols/row:1/cell:2',format:{fill:'#0000FF'}},
  ]};
  const preview=executeRequest(created.bytes,{Plan:{plan}});
  assert.equal(preview.outcome,'previewed',JSON.stringify(preview));
  const committed=executeRequest(created.bytes,{Plan:{plan,preview_key:preview.preview_key}});
  assert.equal(committed.outcome,'committed',JSON.stringify(committed));
  const html=read(committed.bytes).result.html;
  assert.equal((html.match(/<tr\b/g)||[]).length,4);
  assert.equal((html.match(/<td\b/g)||[]).length,16);
  assert.match(html,/#FF0000/);
  assert.match(html,/#0000FF/);
  assert.match(html,/Neighbor/);
  assert.equal(executeRequest(committed.bytes,{Plan:{plan,preview_key:preview.preview_key}}).outcome,'rejected');
  const tracked={...plan,change_mode:'track'};
  assert.equal(executeRequest(created.bytes,{Plan:{plan:tracked}}).outcome,'rejected');
});
