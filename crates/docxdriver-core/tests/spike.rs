//! xmloxide adoption gate: semantic round-trip stability and a modest latency
//! budget for a document large enough to exercise the parser.

use std::time::Instant;
use xmloxide::tree::Document;

fn synthetic_document_xml(paragraphs: usize) -> String {
    let mut body = String::new();
    for index in 0..paragraphs {
        body.push_str(&format!(
            "<w:p><w:pPr><w:pStyle w:val=\"Body\"/></w:pPr><w:r><w:t xml:space=\"preserve\">Paragraph {index}: the quick brown fox jumps over the lazy dog &amp; friends, weighing 1 &lt; 2 kg. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.</w:t></w:r><w:ins w:id=\"{index}\" w:author=\"Agent\" w:date=\"2026-01-01T00:00:00Z\"><w:r><w:t>inserted bit</w:t></w:r></w:ins><w:del w:id=\"d{index}\" w:author=\"Agent\" w:date=\"2026-01-01T00:00:00Z\"><w:r><w:delText>gone</w:delText></w:r></w:del></w:p>"
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}</w:body></w:document>"
    )
}

#[test]
fn roundtrip_fidelity_and_timing() {
    let xml = synthetic_document_xml(4000);
    let input_len = xml.len();
    let parse_started = Instant::now();
    let doc = Document::parse_str(&xml).expect("parse");
    let parse_ms = parse_started.elapsed().as_secs_f64() * 1000.0;
    let serialize_started = Instant::now();
    let output = xmloxide::serial::serialize(&doc);
    let serialize_ms = serialize_started.elapsed().as_secs_f64() * 1000.0;
    let reparsed = Document::parse_str(&output).expect("reparse");
    let original_text = doc.text_content(doc.root_element().unwrap());
    let roundtrip_text = reparsed.text_content(reparsed.root_element().unwrap());
    assert_eq!(
        original_text, roundtrip_text,
        "text survives the round trip"
    );
    println!(
        "spike: {input_len} bytes document.xml — parse {parse_ms:.1}ms, serialize {serialize_ms:.1}ms"
    );
    assert!(
        parse_ms + serialize_ms < 2000.0,
        "debug-build round trip took {:.1}ms",
        parse_ms + serialize_ms
    );
}
