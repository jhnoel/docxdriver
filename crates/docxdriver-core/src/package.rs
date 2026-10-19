//! OPC/zip container handling: a DOCX package is an ordered list of named parts.
//!
//! Part order is preserved from the source archive, and a part no operation
//! changed is copied through byte for byte — compressed data, method,
//! timestamps and extra fields as the source wrote them — so an edit cannot
//! disturb a part it did not touch. Only changed and new parts are deflated.

use std::io::{Cursor, Read, Write};
use std::sync::Arc;

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
/// parts, taken before each mutating operation — never a zip round-trip. The
/// source archive is shared, not copied.
#[derive(Clone)]
pub struct Package {
    parts: Vec<Part>,
    source: Option<Arc<Vec<u8>>>,
}

#[derive(Clone)]
struct Part {
    name: String,
    content: Vec<u8>,
    /// Index of the source archive entry this part still matches exactly.
    origin: Option<usize>,
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
            parts.push(Part {
                name: file.name().to_string(),
                content,
                origin: Some(index),
            });
        }
        Ok(Package {
            parts,
            source: Some(Arc::new(bytes.to_vec())),
        })
    }

    pub fn parts(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.parts.iter().map(|p| (p.name.as_str(), p.content.as_slice()))
    }

    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.parts
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.content.as_slice())
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
        match self.parts.iter().find(|p| p.name == name) {
            Some(part) => Ok(part.content.as_slice()),
            None => Err(PackageError::MissingPart(DOCUMENT_PART)),
        }
    }

    /// Replace or add a part. Setting a part to the bytes it already has
    /// leaves it untouched, so it is still copied through unchanged.
    pub fn set(&mut self, name: &str, content: Vec<u8>) {
        if let Some(entry) = self.parts.iter_mut().find(|p| p.name == name) {
            if entry.content != content {
                entry.content = content;
                entry.origin = None;
            }
        } else {
            self.parts.push(Part {
                name: name.to_string(),
                content,
                origin: None,
            });
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, PackageError> {
        let mut source = match &self.source {
            Some(bytes) => Some(ZipArchive::new(Cursor::new(bytes.as_slice()))?),
            None => None,
        };
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for part in &self.parts {
            if let (Some(index), Some(archive)) = (part.origin, source.as_mut()) {
                writer.raw_copy_file(archive.by_index_raw(index)?)?;
                continue;
            }
            writer.start_file(part.name.as_str(), options)?;
            writer.write_all(&part.content)?;
        }
        Ok(writer.finish()?.into_inner())
    }
}
