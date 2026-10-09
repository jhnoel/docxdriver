import type { ExtensionAPI } from '@earendil-works/pi-coding-agent';
import { registerDocxTools } from './tools.js';

/**
 * Pi coding-agent extension: path-based DOCX tools over the docxdriver WASM engine.
 *
 * Load with:
 *   pi -e ./packages/docxdriver-pi/src/index.ts
 * or install as a pi package (package.json "pi.extensions").
 */
export default async function (pi: ExtensionAPI): Promise<void> {
  await registerDocxTools(pi);
}
