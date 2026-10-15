# docs

The current architecture and contract are specified in
[`superpowers/specs/2026-08-09-docxdriver-pi-toml-plans-e2e-design.md`](superpowers/specs/2026-08-09-docxdriver-pi-toml-plans-e2e-design.md).
Public surface documentation lives beside each package.

[Formatting support and roadmap](formatting-roadmap.md) distinguishes current
read, authoring and editing support and proposes milestones for closing the gaps.
The [capability matrix](../packages/docxdriver/schema/formatting-capabilities.json)
is checked against native/WASM behavior, typed fields and Python exposure.

[HTML projection parity](html-projection-parity.md) maps the prior representation
to HTML and MathML and describes the frozen-baseline conformance checks.

[HTML versus prior projection benchmark](benchmarks/html-vs-legacy-2026-10-07.md)
reports engine measurements and 42 paired live-agent trials.

[Full HTML optimization through Python](benchmarks/html-optimization-through-python-2026-10-07.md)
reports the controlled before/after comparison with the full skill documentation.
