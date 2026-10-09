# docxdriver-wasm

Wasm is a serialization adapter over the typed core API. `executeRequest`
accepts a serialized `Request` and returns a typed result plus committed bytes.
`parsePlanJson` and `parsePlanToml` expose core plan parsers; TOML parsing
retains structured diagnostics and source spans.
