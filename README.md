# docxdriver

DOCX editing with one source-bound typed Rust API. A `Request` is either an
individual `Command` or an atomic `Plan`; plans preview before commit and are
the only mutation route exposed by Pi.

| Surface | Role |
| --- | --- |
| `docxdriver-core` | Typed model, validation, execution, diagnostics, canonical plans |
| `docxdriver-wasm` / `docxdriver` | Typed JSON/Wasm transport and TypeScript API |
| `docxdriver-cli` | Native command and plan host with safe path I/O |
| `docxdriver-mcp` | Five-tool native MCP server over stdio or stateless HTTP; primary distribution boundary |
| `docxdriver-pi` | Five-tool plan-only Pi extension for existing Pi installations |

Document reads project DOCX into renderable HTML5 with native presentation
MathML, scoped CSS, image asset manifests and stable source identities.
Creation and inline edits accept a supported HTML5 subset. Equation operations
use `mathml`; no LaTeX is required. The TypeScript package supplies portable HTML
and browser-selection helpers. See [the HTML contract](packages/docxdriver/README.md).
The [parity matrix](docs/html-projection-parity.md) records preservation of the
prior projection and its differential verification.

Experimental: an opt-in V1 Python typed-plan REPL surface ships in
packages/docxdriver-pi (python-plan-extension.ts); V2/V3 REPL experiments are
archived at experimental/python-repl-surfaces-v2-v3 (historical research
only, excluded from builds/tests).

Builds require Rust with rustup, Node.js 20 or later, and `wasm-pack`:
`cargo install wasm-pack --version 0.14.0 --locked`. Run `npm ci` in each npm
package before building or testing. WASM and JavaScript outputs are generated
locally and ignored by Git; `npm run build` creates the runtime before compiling
TypeScript. Packaged releases include the generated runtime.

Run `cargo test --workspace`, then `npm test` in each package. The Chromium
end-to-end suite is `npm --prefix packages/docxdriver run test:e2e` (build WASM
and install Playwright Chromium first). See
[`crates/docxdriver-mcp/README.md`](crates/docxdriver-mcp/README.md) for native MCP
installation and stdio/HTTP composition.

This public repository begins with the current source snapshot. Historical
benchmark reports retain their original commit labels.
