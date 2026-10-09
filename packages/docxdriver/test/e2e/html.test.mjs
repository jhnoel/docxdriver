import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { chromium } from 'playwright';
import { init, executeRequest, readHtml, renderHtmlDocument, resolveHtmlSelection, mountHtmlProjection } from '../../dist/index.js';

const repo = fileURLToPath(new URL('../../../../', import.meta.url));
const artifacts = resolve(repo, 'target/html-e2e');

function modelCssRules(model, scope = '.docxdriver') {
  return Object.entries(model.css ?? {})
    .map(([key, declarations]) => `${scope} [data-docx-css="${key}"]{${declarations}}`)
    .join('\n');
}

test('native DOCX → WASM HTML → Chromium selection → source-bound edit → DOCX', { timeout: 120_000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const fixture = spawnSync('cargo', ['test', '-q', '-p', 'docxdriver-core', '--test', 'html_e2e', 'e2e_artifact_fixture'], {
    cwd: repo, env: { ...process.env, DOCXDRIVER_HTML_E2E_OUT: artifacts }, encoding: 'utf8', timeout: 60_000,
  });
  assert.equal(fixture.status, 0, fixture.stderr + fixture.stdout);
  const input = new Uint8Array(await readFile(resolve(artifacts, 'source.docx')));
  await init(await readFile(new URL('../../wasm/docxdriver_bg.wasm', import.meta.url)));
  const projection = readHtml(input, 'final');
  const native = JSON.parse(await readFile(resolve(artifacts, 'native.json'), 'utf8'));
  assert.deepEqual(projection, native, 'native and WASM must emit identical projections');
  assert.equal(projection.equations?.[0]?.latex, undefined);
  const portable = renderHtmlDocument(input, 'final', 'DOCX HTML end-to-end proof');
  await writeFile(resolve(artifacts, 'preview.html'), portable);

  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 1000, height: 1000 } });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.setContent(portable, { waitUntil: 'load' });
    const rendering = await page.evaluate(() => {
      const math = document.querySelector('math');
      const image = document.querySelector('img');
      const note = document.querySelector('[data-docx-note] a');
      const styled = [...document.querySelectorAll('p')].find(p => p.textContent === 'Styled final paragraph');
      return {
        mathNamespace: math?.namespaceURI,
        mathWidth: math?.getBoundingClientRect().width,
        fractionHeight: document.querySelector('mfrac')?.getBoundingClientRect().height,
        nestedLists: document.querySelectorAll('ol > li > ul').length,
        rowspan: document.querySelector('td[rowspan]')?.rowSpan,
        imageLoaded: image?.complete && image.naturalWidth > 0,
        imageWidth: image?.getBoundingClientRect().width,
        imageHeight: image?.getBoundingClientRect().height,
        noteExists: !!document.querySelector(note?.getAttribute('href') ?? '#missing'),
        noteText: note?.textContent,
        paragraphAlignment: styled && getComputedStyle(styled).textAlign,
        customElements: document.querySelectorAll('equation, image, field, format, comment-start, comment-end, footnote, endnote').length,
        ids: [...document.querySelectorAll('[id]')].map(n => n.id),
      };
    });
    assert.equal(rendering.mathNamespace, 'http://www.w3.org/1998/Math/MathML');
    assert.ok(rendering.mathWidth > 0 && rendering.fractionHeight > 20, JSON.stringify(rendering));
    assert.equal(rendering.nestedLists, 1);
    assert.equal(rendering.rowspan, 2);
    assert.equal(rendering.imageLoaded, true);
    assert.equal(rendering.imageWidth, 32);
    assert.equal(rendering.imageHeight, 24);
    assert.equal(rendering.noteExists, true);
    assert.equal(rendering.noteText, '1');
    assert.equal(rendering.paragraphAlignment, 'right');
    assert.equal(rendering.customElements, 0);
    assert.equal(new Set(rendering.ids).size, rendering.ids.length);
    await page.screenshot({ path: resolve(artifacts, 'rendered.png'), fullPage: true });

    // Exercise the exported browser helper against actual DOM Ranges, including
    // supplementary Unicode characters before the selected repeated phrase.
    await page.evaluate(`window.resolveHtmlSelection = ${resolveHtmlSelection.toString()}`);
    const selection = await page.evaluate(projection => {
      const p = [...document.querySelectorAll('p[id]')].find(p => p.textContent.includes('Repeat 😀'));
      const walker = document.createTreeWalker(p, NodeFilter.SHOW_TEXT);
      let text;
      while ((text = walker.nextNode())) if (text.textContent.includes('Repeat 😀')) break;
      const offset = text.textContent.lastIndexOf('target');
      const range = document.createRange(); range.setStart(text, offset); range.setEnd(text, offset + 6);
      return window.resolveHtmlSelection(document.querySelector('main'), range, projection);
    }, projection);
    assert.equal(selection.select, 'target');
    assert.equal(selection.occurrence, 2);
    assert.equal(selection.start, [...'Repeat 😀 target, then '].length);
    const plan = {
      base: projection.source, author: 'Browser e2e', change_mode: 'track',
      ops: [
        { op: 'replace_text', at: selection.at, select: selection.select, occurrence: selection.occurrence,
          with: '<span style="color:#0055AA;font-size:14pt"><strong>revised</strong></span>' },
        { op: 'replace_equation', at: projection.equations[0].at,
          mathml: '<math><msub><mi>A</mi><mn>1</mn></msub></math>' },
      ],
    };
    const preview = executeRequest(input, { Plan: { plan } });
    assert.equal(preview.outcome, 'previewed', JSON.stringify(preview));
    assert.ok(preview.report.ops.every(op => op.affected?.[0]?.markup.includes('<p')));
    const committed = executeRequest(input, { Plan: { plan, preview_key: preview.preview_key } });
    assert.equal(committed.outcome, 'committed', JSON.stringify(committed));
    await writeFile(resolve(artifacts, 'edited.docx'), committed.bytes);
    const final = readHtml(committed.bytes, 'final');
    assert.equal(final.paragraphs.find(p => p.id === selection.at).text, 'Repeat 😀 target, then revised.');
    assert.match(final.equations[0].mathml, /<msub>/);
    assert.match(readHtml(committed.bytes, 'original').html, /<mfrac>/);
    assert.equal(readHtml(committed.bytes, 'original').paragraphs.find(p => p.id === selection.at).text, 'Repeat 😀 target, then target.');
    await page.setContent(renderHtmlDocument(committed.bytes, 'markup'), { waitUntil: 'load' });
    assert.equal(await page.locator('ins').count() > 0, true);
    assert.equal(await page.locator('del').count() > 0, true);
    assert.equal(await page.locator('img').evaluate(n => n.complete && n.naturalWidth > 0), true);
    // Notes and math are excluded from source text, while browser content
    // remains fully visible. Resolve a whole paragraph spanning a MathML node.
    const mapped = await page.evaluate(projection => {
      const p = [...document.querySelectorAll('p[id]')].find(p => p.textContent.startsWith('Equation '));
      const range = document.createRange(); range.selectNodeContents(p);
      return window.resolveHtmlSelection(document.querySelector('main'), range, projection);
    }, readHtml(committed.bytes, 'markup'));
    assert.equal(mapped.select, 'Equation  after math.');
    const stale = executeRequest(committed.bytes, { Plan: { plan, preview_key: preview.preview_key } });
    assert.equal(stale.outcome, 'rejected');
    assert.deepEqual(errors, []);
    await writeFile(resolve(artifacts, 'verification.json'), JSON.stringify({ rendering, selection, mapped, nativeEqualsWasm: true, staleRejected: true }, null, 2));
  } finally { await browser.close(); }
});

// Read and render an existing Word document rather than only generated fixtures.
test('existing DOCX renders in Chromium in all revision views', { timeout: 120_000 }, async () => {
  const bytes = new Uint8Array(await readFile(resolve(repo, 'test-docs/ctnf-18690238-data-stream.docx')));
  await init(await readFile(new URL('../../wasm/docxdriver_bg.wasm', import.meta.url)));
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 1000, height: 1000 } });
    const summary = [];
    for (const view of ['markup', 'final', 'original']) {
      const projection = readHtml(bytes, view);
      assert.ok(projection.paragraphs.length > 10);
      const rendered = renderHtmlDocument(bytes, view, `Existing document: ${view}`);
      await page.setContent(rendered, { waitUntil: 'load' });
      const state = await page.evaluate(() => ({
        paragraphs: document.querySelectorAll('p[id], h1[id], h2[id], h3[id]').length,
        text: document.querySelector('main').textContent,
        brokenImages: [...document.images].filter(n => !n.complete || n.naturalWidth === 0).length,
      }));
      assert.ok(state.paragraphs > 10 && state.text.length > 100);
      assert.equal(state.brokenImages, 0);
      summary.push({ view, paragraphs: state.paragraphs, textLength: state.text.length });
      if (view === 'final') {
        await writeFile(resolve(artifacts, 'existing-document.html'), rendered);
        await page.screenshot({ path: resolve(artifacts, 'existing-document.png'), fullPage: true });
      }
    }
    await writeFile(resolve(artifacts, 'existing-document-verification.json'), JSON.stringify(summary, null, 2));
  } finally { await browser.close(); }
});

test('review regressions render normal-weight headings and preserve integral limit geometry', { timeout: 120_000 }, async () => {
  await init(await readFile(new URL('../../wasm/docxdriver_bg.wasm', import.meta.url)));
  const authored = `<!-- outside -->
    <h1><span style="font-weight:normal;font-style:normal;text-decoration:none;vertical-align:baseline">Normal heading</span></h1>
    <p>A<!-- hidden -->B</p>
    <p><math><mstyle mathvariant="bold"><mi>x</mi><mo>+</mo><mn>2</mn></mstyle></math></p>
    <p><math><msubsup><mo mathvariant="bold">∫</mo><mn>0</mn><mn>1</mn></msubsup><mi>x</mi></math></p>
    <p><math><munderover><mo>∫</mo><mn>0</mn><mn>1</mn></munderover><mi>x</mi></math></p>`;
  const created = executeRequest(undefined, { Command: { command: { kind: 'create', html: authored } } });
  assert.equal(created.outcome, 'completed', JSON.stringify(created));
  const projection = readHtml(created.bytes, 'final');
  assert.equal(projection.paragraphs[1].text, 'AB');
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage();
    await page.setContent(renderHtmlDocument(created.bytes), { waitUntil: 'load' });
    const state = await page.evaluate(() => {
      const heading = getComputedStyle(document.querySelector('h1 span'));
      const limitOffset = tag => {
        const node = document.querySelector(tag);
        const base = node.firstElementChild.getBoundingClientRect();
        const upper = node.lastElementChild.getBoundingClientRect();
        return (upper.left + upper.right - base.left - base.right) / 2;
      };
      return {
        headingWeight: heading.fontWeight, headingStyle: heading.fontStyle,
        headingDecoration: heading.textDecorationLine, headingBaseline: heading.verticalAlign,
        commentsVisible: /hidden|outside/.test(document.querySelector('main').textContent),
        boldVariants: document.querySelectorAll('[mathvariant="bold"]').length,
        sideLimitOffset: limitOffset('msubsup'), underLimitOffset: limitOffset('munderover'),
      };
    });
    assert.equal(state.headingWeight, '400');
    assert.equal(state.headingStyle, 'normal');
    assert.equal(state.headingDecoration, 'none');
    assert.equal(state.headingBaseline, 'baseline');
    assert.equal(state.commentsVisible, false);
    assert.equal(state.boldVariants, 4);
    assert.ok(state.sideLimitOffset > 2, JSON.stringify(state));
    assert.ok(Math.abs(state.underLimitOffset) < 1, JSON.stringify(state));
    await writeFile(resolve(artifacts, 'review-regression-verification.json'), JSON.stringify(state, null, 2));
  } finally { await browser.close(); }
});

test('prior projection feature inventory survives actual browser parsing in every view', { timeout: 120_000 }, async () => {
  await init(await readFile(new URL('../../wasm/docxdriver_bg.wasm', import.meta.url)));
  const bytes = new Uint8Array(await readFile(resolve(repo, 'crates/docxdriver-core/tests/fixtures/projection-parity/parity.docx')));
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 1000, height: 1000 } });
    const report = [];
    for (const view of ['markup', 'final', 'original']) {
      const projection = readHtml(bytes, view);
      await page.setContent(renderHtmlDocument(bytes, view, `Projection parity: ${view}`), { waitUntil: 'load' });
      const inventory = await page.evaluate(() => {
        const all = selector => [...document.querySelectorAll(selector)];
        const opaque = document.querySelector('[data-docx-image-unavailable]');
        return {
          sections: all('section[data-docx-section]').length,
          headers: all('header').length, footers: all('footer').length,
          math: all('math').length, matrices: all('mtable').length,
          groupBrace: all('math mo').some(n => n.textContent === '⏞'),
          deletedMathText: all('math').some(n => n.textContent === 'Old math'),
          opaqueWidth: opaque?.getAttribute('data-docx-image-width'),
          opaqueHeight: opaque?.getAttribute('data-docx-image-height'),
          opaqueAlt: opaque?.getAttribute('data-docx-image-alt'),
          link: document.querySelector('[data-docx-href]')?.getAttribute('data-docx-href'),
          safeActiveLink: document.querySelector('[data-docx-href]')?.getAttribute('href'),
          imagesLoaded: all('img').every(n => n.complete && n.naturalWidth > 0),
          joinedField: all('[data-docx-field="REF joined"]').map(n => n.textContent).join(''),
          notesResolved: all('[data-docx-note] a').every(n => !!document.querySelector(n.getAttribute('href'))),
          comments: all('[data-docx-comment-start], [data-docx-comment-end]').length,
          pendingFormats: all('[data-docx-format-change]').map(n => n.getAttribute('data-docx-revision')).sort(),
        };
      });
      assert.equal(inventory.sections, 2);
      assert.equal(inventory.headers, 6); assert.equal(inventory.footers, 6);
      assert.equal(inventory.math, 28); assert.equal(inventory.matrices, 2);
      assert.equal(inventory.groupBrace, true); assert.equal(inventory.deletedMathText, true);
      assert.equal(inventory.opaqueWidth, '48'); assert.equal(inventory.opaqueHeight, '36');
      assert.equal(inventory.opaqueAlt, 'Opaque picture');
      assert.equal(inventory.link, 'custom-scheme:opaque-target'); assert.equal(inventory.safeActiveLink, '#');
      assert.equal(inventory.imagesLoaded, true); assert.equal(inventory.notesResolved, true);
      assert.equal(inventory.joinedField, view === 'final' ? 'Joined firstJoined second' : 'Joined first');
      assert.equal(inventory.comments, view === 'markup' ? 2 : 0);
      if (view === 'markup') assert.deepEqual(inventory.pendingFormats, ['31', '32']);
      if (view === 'final') {
        await page.evaluate(`window.resolveHtmlSelection = ${resolveHtmlSelection.toString()}`);
        const mapped = await page.evaluate(projection => {
          const span = [...document.querySelectorAll('[data-docx-paragraph]')].find(n => n.textContent === 'Joined second');
          const range = document.createRange(); range.selectNodeContents(span);
          return window.resolveHtmlSelection(document.querySelector('main'), range, projection);
        }, projection);
        assert.equal(mapped.select, 'Joined second');
        assert.equal(projection.paragraphs.find(p => p.id === mapped.at).selectable_text, 'Joined second');
      }
      report.push({ view, ...inventory });
      if (view === 'markup') {
        await writeFile(resolve(artifacts, 'parity.html'), renderHtmlDocument(bytes, view, 'Projection parity'));
        await page.screenshot({ path: resolve(artifacts, 'parity.png'), fullPage: true });
      }
    }
    await writeFile(resolve(artifacts, 'parity-verification.json'), JSON.stringify(report, null, 2));
  } finally { await browser.close(); }
});

test('model CSS maps preserve browser formatting and inline asset associations', async () => {
  await init(await readFile(new URL('../../wasm/docxdriver_bg.wasm', import.meta.url)));
  const created = executeRequest(undefined, {Command:{command:{kind:'create',html:Array.from({length:40}, (_,i)=>`<p style="text-align:right;font-size:14pt;color:#123456;margin-top:10pt;margin-bottom:20pt;margin-left:4pt;text-indent:2pt">Paragraph ${i}: <span style="color:#987654;font-family:Arial">😀 context</span></p>`).join('')}}});
  assert.ok(created.bytes);
  const browser = await chromium.launch({headless:true});
  try {
    const page = await browser.newPage();
    for (const bytes of [created.bytes, new Uint8Array(await readFile(resolve(repo, 'crates/docxdriver-core/tests/fixtures/projection-parity/parity.docx')))]) {
      const ui = readHtml(bytes, 'markup');
      const model = executeRequest(bytes, {Command:{command:{kind:'read',view:'markup'}}}).result;
      if (bytes === created.bytes) {
        assert.ok(Object.keys(model.css).length > 0);
        assert.match(model.markup, /data-docx-css=/);
        assert.doesNotMatch(model.markup, /<style/);
      }
      const inspect = () => ({
        paragraphs:[...document.querySelectorAll('p[id]')].map(p=>({id:p.id,text:p.textContent,align:getComputedStyle(p).textAlign,size:getComputedStyle(p).fontSize,color:getComputedStyle(p).color,marginTop:getComputedStyle(p).marginTop,marginBottom:getComputedStyle(p).marginBottom,marginLeft:getComputedStyle(p).marginLeft,textIndent:getComputedStyle(p).textIndent,fontWeight:getComputedStyle(p).fontWeight,fontStyle:getComputedStyle(p).fontStyle,lineHeight:getComputedStyle(p).lineHeight,runs:[...p.querySelectorAll('span')].map(s=>({text:s.textContent,color:getComputedStyle(s).color,font:getComputedStyle(s).fontFamily}))})),
        images:[...document.querySelectorAll('img')].map(img=>({alt:img.alt,width:img.width,height:img.height})),
        mathNamespaces:[...document.querySelectorAll('math')].map(math=>math.namespaceURI),
      });
      const css = `${ui.css}\n${modelCssRules(model)}`;
      await page.setContent(`<style>${css}</style><main class="docxdriver">${ui.html}</main>`);
      const expected = await page.evaluate(inspect);
      await page.setContent(`<style>${css}</style><main class="docxdriver">${model.markup}</main>`);
      assert.deepEqual(await page.evaluate(inspect), expected);
      for (const asset of model.assets ?? []) {
        assert.ok(ui.assets.some(a=>a.url===asset.asset_url && JSON.stringify(a.parts)===JSON.stringify(asset.parts)));
      }
      const sources = await page.locator('img').evaluateAll(images=>images.map(img=>img.getAttribute('src')));
      assert.ok(sources.every(src=>model.assets.some(a=>a.url===src)));
      const imageParts = await page.locator('img').evaluateAll(images=>images.map(img=>({src:img.getAttribute('src'),part:img.getAttribute('data-docx-image-part')})));
      assert.ok(imageParts.every(image=>model.assets.some(a=>a.url===image.src && a.parts.includes(image.part))));
    }
  } finally {await browser.close();}
});


test('document-local CSS maps stay isolated when fragments share a page', async () => {
  await init(await readFile(new URL('../../wasm/docxdriver_bg.wasm', import.meta.url)));
  const readModel = color => {
    const created = executeRequest(undefined, {Command:{command:{kind:'create',html:Array.from({length:30}, (_,i)=>`<p style="color:${color};font-size:14pt;margin-top:10pt;margin-bottom:20pt">Paragraph ${i}</p>`).join('')}}});
    assert.ok(created.bytes);
    return executeRequest(created.bytes, {Command:{command:{kind:'read',view:'final'}}}).result;
  };
  const mountModel = (model, scope) => {
    const rules = Object.entries(model.css).map(([key, declarations]) =>
      `[data-docx-scope="${scope}"] [data-docx-css="${key}"]{${declarations}}`).join('');
    return `<div class="docxdriver" data-docx-scope="${scope}"><style>${rules}</style>${model.markup}</div>`;
  };
  const redModel = readModel('#ff0000'), blueModel = readModel('#0000ff');
  assert.match(redModel.markup, /data-docx-css=/);
  assert.notEqual(redModel.css.dx1, blueModel.css.dx1);
  assert.equal(JSON.stringify(readModel('#ff0000')), JSON.stringify(redModel));
  const red = mountModel(redModel, 'red'), blue = mountModel(blueModel, 'blue');
  const browser = await chromium.launch({headless:true});
  try {
    const page = await browser.newPage();
    await page.setContent(`<main>${red}${blue}${red}</main>`);
    const colors = await page.locator('main > div').evaluateAll(docs => docs.map(doc => [...doc.querySelectorAll('p')].map(p => getComputedStyle(p.querySelector('span') ?? p).color)));
    assert.deepEqual(colors, [Array(30).fill('rgb(255, 0, 0)'), Array(30).fill('rgb(0, 0, 255)'), Array(30).fill('rgb(255, 0, 0)')]);
  } finally {await browser.close();}
});


test('content controls retain complete source context in WASM and browser reads', async () => {
  const fixture = spawnSync('cargo', ['test', '-q', '-p', 'docxdriver-core', '--test', 'html_e2e', 'content_controls_read_edit_and_table_anchors_keep_full_context'], {
    cwd:repo, env:{...process.env,DOCXDRIVER_HTML_E2E_OUT:artifacts}, encoding:'utf8', timeout:60_000,
  });
  assert.equal(fixture.status,0,fixture.stderr+fixture.stdout);
  await init(await readFile(new URL('../../wasm/docxdriver_bg.wasm',import.meta.url)));
  const input=new Uint8Array(await readFile(resolve(artifacts,'controls.docx')));
  const browser=await chromium.launch({headless:true});
  try {
    const page=await browser.newPage();
    for (const view of ['markup','final','original']) {
      const model=executeRequest(input,{Command:{command:{kind:'read',view}}}).result;
      const ui=readHtml(input,view);
      for (const html of [model.markup,ui.html]) {
        await page.setContent(html);
        assert.deepEqual(await page.locator('p').allTextContents(),['Before control.','Critical control context.','Controlled cell.','Nested table context.','After control.']);
        assert.equal(await page.locator('section').count(),2);
        assert.equal(await page.locator('table').count(),2);
      }
    }
    const projection=readHtml(input,'final');
    await page.setContent(`<main>${projection.html}</main>`);
    await page.evaluate(`window.resolveHtmlSelection = ${resolveHtmlSelection.toString()}`);
    const selected=await page.evaluate(projection=>{
      const p=document.getElementById('22222222'); const range=document.createRange();range.selectNodeContents(p);
      return window.resolveHtmlSelection(document.querySelector('main'),range,projection);
    },projection);
    assert.equal(selected.at,'22222222');assert.equal(selected.select,'Critical control context.');
  } finally {await browser.close();}
});

test('MathML accent writes retain explicit upper and lower semantics in Chromium', async () => {
  await init(await readFile(new URL('../../wasm/docxdriver_bg.wasm',import.meta.url)));
  const html='<p><math><munder accentunder="true"><mi>x</mi><mo stretchy="true">¯</mo></munder></math></p><p><math><mover accent="false"><mi>x</mi><mo>ˆ</mo></mover></math></p><p><math><munder accentunder="true"><mi>x</mi><mo stretchy="true">⏟</mo></munder></math></p>';
  const created=executeRequest(undefined,{Command:{command:{kind:'create',html}}});
  assert.equal(created.outcome,'completed');
  const browser=await chromium.launch({headless:true});
  try {
    const page=await browser.newPage();
    await page.setContent(renderHtmlDocument(created.bytes,'final'));
    assert.equal(await page.locator('math').nth(0).locator('munder').getAttribute('accentunder'),'true');
    assert.equal(await page.locator('math').nth(0).locator('mo').getAttribute('stretchy'),'true');
    assert.equal(await page.locator('math').nth(1).locator('mover').getAttribute('accent'),'false');
    assert.equal(await page.locator('math').nth(2).locator('munder').getAttribute('accentunder'),'true');
    assert.equal(await page.locator('math').nth(2).locator('mo').textContent(),'⏟');
    assert.ok(await page.locator('math').evaluateAll(nodes=>nodes.every(n=>n.namespaceURI==='http://www.w3.org/1998/Math/MathML' && n.getBoundingClientRect().height>0)));
  } finally {await browser.close();}
});

test('multiple documents and repeated copies keep note targets and source selections isolated', async () => {
  await init(await readFile(new URL('../../wasm/docxdriver_bg.wasm',import.meta.url)));
  const input=new Uint8Array(await readFile(resolve(repo,'crates/docxdriver-core/tests/fixtures/projection-parity/parity.docx')));
  const first=readHtml(input,'final');
  const assets=executeRequest(input,{Command:{command:{kind:'read',read_kind:'assets'}}}).result.assets;
  const at=first.paragraphs.find(p=>p.selectable_text.includes('Unicode')).id;
  const plan={base:first.source,author:'Test',change_mode:'direct',ops:[{op:'replace_text',at,select:'Unicode',with:'Changed'}]};
  const preview=executeRequest(input,{Plan:{plan}});assert.equal(preview.outcome,'previewed');
  const committed=executeRequest(input,{Plan:{plan,preview_key:preview.preview_key}});assert.equal(committed.outcome,'committed');
  const second=readHtml(committed.bytes,'final');
  const browser=await chromium.launch({headless:true});
  try {
    const page=await browser.newPage();
    // Deterministic core fragment URLs differ between source documents.
    await page.setContent(`<main id="a">${first.html}</main><main id="b">${second.html}</main>`);
    assert.equal(await page.evaluate(()=>[...document.querySelectorAll('main')].every(root=>[...root.querySelectorAll('[data-docx-note] a')].every(link=>root.contains(document.getElementById(link.hash.slice(1)))))),true);
    // The mounting helper also supports repeated copies, and keeps model IDs.
    await page.setContent('<main id="a"></main><main id="b"></main><main id="c"></main>');
    await page.evaluate(`window.mountCounters = new WeakMap(); window.mountHtmlProjection = ${mountHtmlProjection.toString()}; window.resolveHtmlSelection = ${resolveHtmlSelection.toString()}`);
    await page.evaluate(({first,second,assets})=>{
      window.mountHtmlProjection(document.getElementById('a'),first,assets);
      window.mountHtmlProjection(document.getElementById('b'),second,assets);
      window.mountHtmlProjection(document.getElementById('c'),first,assets);
    },{first,second,assets});
    assert.equal(await page.locator('img').evaluateAll(images=>images.every(img=>img.complete && img.naturalWidth>0)),true);
    const state=await page.evaluate(({first,at})=>{
      const roots=[...document.querySelectorAll('main')];
      const ids=[...document.querySelectorAll('[id]')].map(n=>n.id);
      const linksLocal=roots.every(root=>[...root.querySelectorAll('[data-docx-note] a')].every(link=>root.contains(document.getElementById(link.hash.slice(1)))));
      const root=document.getElementById('c'); const p=root.querySelector(`[data-docx-paragraph="${at}"]`);
      const range=document.createRange();range.selectNodeContents(p);
      return {linksLocal,idsUnique:new Set(ids).size===ids.length,selected:window.resolveHtmlSelection(root,range,first)};
    },{first,at});
    assert.equal(state.linksLocal,true);assert.equal(state.idsUnique,true);assert.equal(state.selected.at,at);assert.match(state.selected.select,/Unicode/);
    const selectedPreview=executeRequest(input,{Plan:{plan:{base:first.source,author:'Test',change_mode:'direct',ops:[{op:'format_text',at:state.selected.at,select:'Unicode',bold:true}]}}});
    assert.equal(selectedPreview.outcome,'previewed');
    const link=page.locator('#c [data-docx-note] a').first();const target=await link.getAttribute('href');await link.click();assert.equal(await page.evaluate(()=>location.hash),target);
  } finally {await browser.close();}
});
