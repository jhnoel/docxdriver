//! Print the integrity findings of `.docx` files, one `path\tpart\tmessage\tcount`
//! line each, and with `--round-trip` every XML part the engine cannot
//! rewrite faithfully:
//!
//!     cargo run --release -p docxdriver-core --example integrity -- [--round-trip] FILE...

use std::io::Read;

fn main() {
    let mut round_trip = false;
    let mut status = 0;
    for path in std::env::args().skip(1) {
        if path == "--round-trip" {
            round_trip = true;
            continue;
        }
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("{path}: {error}");
                status = 2;
                continue;
            }
        };
        let findings = docxdriver_core::integrity::check(&bytes);
        for ((part, message), count) in findings.iter() {
            println!("{path}\t{part}\t{message}\t{count}");
        }
        if !findings.is_empty() {
            status = status.max(1);
        }
        if round_trip {
            let Ok(mut archive) = zip::ZipArchive::new(std::io::Cursor::new(&bytes)) else {
                continue;
            };
            for index in 0..archive.len() {
                let Ok(mut file) = archive.by_index(index) else { continue };
                let name = file.name().to_string();
                if !(name.ends_with(".xml") || name.ends_with(".rels")) {
                    continue;
                }
                let mut data = Vec::new();
                if file.read_to_end(&mut data).is_err() {
                    continue;
                }
                if let Some(fault) = docxdriver_core::integrity::round_trip_fault(&data) {
                    println!("{path}\t{name}\tround-trip: {fault}\t1");
                    status = status.max(1);
                }
            }
        }
    }
    std::process::exit(status);
}
