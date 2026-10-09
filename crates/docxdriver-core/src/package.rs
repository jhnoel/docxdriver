//! OPC/zip container handling: a DOCX package is an ordered list of named parts.
//!
//! Part order is preserved from the source archive so an untouched package
//! round-trips structurally. (Byte-level copy-through of unchanged compressed
//! entries is a planned optimization; v1 re-deflates every part on write.)

use std::io::{Cursor, Read, Write};

use thiserror::Error;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

#[derive(Debug, Error)]
pub enum PackageError {
    #[error("not a DOCX package: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("package I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("package has no {0}")]
    MissingPart(&'static str),
}

/// Clone is the ops-list rollback snapshot: a plain memcpy of the decompressed
/// parts, taken before each mutating operation — never a zip round-trip.
#[derive(Clone)]
pub struct Package {
    parts: Vec<(String, Vec<u8>)>,
}

pub const DOCUMENT_PART: &str = "word/document.xml";

const PACKAGE_RELS_PART: &str = "_rels/.rels";
const OFFICE_DOCUMENT_REL: &str = "/officeDocument";

impl Package {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PackageError> {
        let mut archive = ZipArchive::new(Cursor::new(bytes))?;
        let mut parts = Vec::with_capacity(archive.len());
        for index in 0..archive.len() {
            let mut file = archive.by_index(index)?;
            if file.is_dir() {
                continue;
            }
            let mut content = Vec::with_capacity(file.size() as usize);
            file.read_to_end(&mut content)?;
            parts.push((file.name().to_string(), content));
        }
        Ok(Package { parts })
    }

    pub fn parts(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.parts.iter().map(|(n, b)| (n.as_str(), b.as_slice()))
    }

    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.parts
            .iter()
            .find(|(part, _)| part == name)
            .map(|(_, content)| content.as_slice())
    }

    /// The main document part's name. OPC discovers it through the package
    /// rels (the officeDocument relationship) — `word/document.xml` is only
    /// the overwhelming convention, and real packages deviate. Falls back to
    /// the conventional name when the rels are absent or unhelpful.
    pub fn document_part_name(&self) -> String {
        let resolved = self.get(PACKAGE_RELS_PART).and_then(|bytes| {
            let doc = xmloxide::Document::parse_bytes(bytes).ok()?;
            let root = doc.root();
            doc.descendants(root).find_map(|node| {
                if !doc.is_element(node)
                    || doc.node_name(node)?.rsplit(':').next()? != "Relationship"
                {
                    return None;
                }
                let rel_type = doc.attribute(node, "Type")?;
                if !rel_type.ends_with(OFFICE_DOCUMENT_REL) {
                    return None;
                }
                Some(
                    doc.attribute(node, "Target")?
                        .trim_start_matches('/')
                        .to_string(),
                )
            })
        });
        match resolved {
            Some(name) if self.get(&name).is_some() => name,
            _ => DOCUMENT_PART.to_string(),
        }
    }

    pub fn document_xml(&self) -> Result<&[u8], PackageError> {
        let name = self.document_part_name();
        match self.parts.iter().find(|(part, _)| *part == name) {
            Some((_, content)) => Ok(content.as_slice()),
            None => Err(PackageError::MissingPart(DOCUMENT_PART)),
        }
    }

    pub fn set(&mut self, name: &str, content: Vec<u8>) {
        if let Some(entry) = self.parts.iter_mut().find(|(part, _)| part == name) {
            entry.1 = content;
        } else {
            self.parts.push((name.to_string(), content));
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, PackageError> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, content) in &self.parts {
            writer.start_file(name.as_str(), options)?;
            writer.write_all(content)?;
        }
        Ok(writer.finish()?.into_inner())
    }
}
