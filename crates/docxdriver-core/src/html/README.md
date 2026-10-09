# HTML projection

Public create/read/edit surfaces use HTML5 and presentation MathML. `input`
parses HTML5 and lowers a supported subset into the private atom parser;
`render` and `surface` emit standard HTML with `data-docx-*` identities.
`styles` resolves Word styles and provides scoped preview CSS. `assets` exposes
content-addressed image URLs with payloads on demand. `omml_mathml` and
`mathml_omml` convert directly between equation representations.

The private dialect and LaTeX compatibility adapter are implementation details,
not public output. DOCX remains authoritative: source-bound typed operations
change selected structures and preserve unrelated ZIP parts.

See [the public contract](../../../../packages/docxdriver/README.md).
