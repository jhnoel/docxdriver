#!/usr/bin/env node
// Prepare a controlled comparison against a saved pre-compaction engine.
import { cp, mkdir, readFile, writeFile, symlink, access } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { createHash } from 'node:crypto';
const repo = resolve(import.meta.dirname, '..');
const args = process.argv.slice(2);
const engineIndex = args.indexOf('--engine');
if (engineIndex < 0 || !args[engineIndex+1]) throw new Error('--engine PATH_TO_SAVED_DOCXDRIVER_PACKAGE is required');
const source = resolve(args[engineIndex+1]);
const target = join(repo, 'target/html-head-to-head');
const engine = join(target, 'compact-before-astra');
const python = join(target, 'python-before-astra');
if (source === join(repo, 'packages/docxdriver')) throw new Error('Use a frozen engine, not the current package');
if (source !== engine) {
  await mkdir(engine, {recursive:true});
  for (const name of ['dist','wasm','package.json']) await cp(join(source,name), join(engine,name), {recursive:true});
}
await mkdir(python, {recursive:true});
for (const name of ['dist','wasm','package.json']) await cp(join(repo,'packages/docxdriver-pi',name), join(python,name), {recursive:true});
await cp(join(engine,'wasm/docxdriver_bg.wasm'),join(python,'wasm/docxdriver_bg.wasm'));
try {await access(join(python,'node_modules'));} catch {await symlink(join(repo,'packages/docxdriver-pi/node_modules'),join(python,'node_modules'),'dir');}
const path = join(python,'dist/python-plan-extension.js');
const js = await readFile(path,'utf8');
const definition = /    const documentOutput = [^;]+;/g;
if ([...js.matchAll(definition)].length !== 1) throw new Error('Expected one document output policy definition');
await writeFile(path,js.replace(definition,'    const documentOutput = false;'));
const wasmHash = createHash('sha256').update(await readFile(join(engine,'wasm/docxdriver_bg.wasm'))).digest('hex');
await writeFile(join(python,'SNAPSHOT.md'),`Controlled baseline: saved engine ${wasmHash}, common current Python host, generic display caps restored. This is not an exact historical host checkout.\n`);
console.log(`Prepared controlled Python baseline ${wasmHash}`);
