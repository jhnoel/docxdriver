//! Plan review scaling probe: `cargo run --release -p docxdriver-core --example plan_scaling -- PARAS OPS`.
use std::io::Write;
use std::time::Instant;

use docxdriver_core::{execute_plan, ChangeMode, EditOp, ParagraphAddress, Plan, PlanRequest, PlanResult, SourceHash};

fn package(body: &str) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    let parts = [
        ("[Content_Types].xml", "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>".to_string()),
        ("_rels/.rels", "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>".to_string()),
        ("word/document.xml", body.to_string()),
        ("word/_rels/document.xml.rels", "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".to_string()),
    ];
    for (n, c) in parts {
        w.start_file(n, o).unwrap();
        w.write_all(c.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn main() {
    let args: Vec<usize> = std::env::args().skip(1).map(|a| a.parse().unwrap()).collect();
    let (paras, ops) = (args.first().copied().unwrap_or(1500), args.get(1).copied().unwrap_or(300));
    let body: String = (0..paras)
        .map(|i| format!("<w:p w14:paraId=\"{:08X}\"><w:r><w:t>Paragraph {i} says the tenant shall pay rent within thirty days of notice.</w:t></w:r></w:p>", i + 1))
        .collect();
    let bytes = package(&format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\"><w:body>{body}</w:body></w:document>"));
    let operations = (0..ops)
        .map(|i| EditOp::ReplaceText {
            at: ParagraphAddress::id(format!("{:08X}", (i * paras / ops) + 1)),
            select: "thirty days".into(),
            with: "sixty days".into(),
            occurrence: None,
        })
        .collect();
    let plan = Plan { base: SourceHash::from_bytes(&bytes), author: "bench".into(), change_mode: ChangeMode::Track, ops: operations };
    let t = Instant::now();
    let result = execute_plan(&bytes, &PlanRequest { plan, preview_key: None });
    let ok = matches!(result, PlanResult::Previewed { .. });
    println!("paras={paras} ops={ops} previewed={ok} elapsed={:?}", t.elapsed());
}
