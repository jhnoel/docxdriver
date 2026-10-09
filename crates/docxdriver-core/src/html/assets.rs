//! Stable asset URLs in HTML; payloads are fetched separately from document reads.
use crate::{document::DocxXml, package::Package};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

fn mime(part: &str) -> Option<&'static str> {
    match part.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "svg" => Some("image/svg+xml"),
        _ => None,
    }
}
pub fn asset_url(part: &str, bytes: &[u8]) -> String {
    let ext = part
        .rsplit('.')
        .next()
        .unwrap_or("bin")
        .to_ascii_lowercase();
    format!("assets/{}.{}", hex::encode(Sha256::digest(bytes)), ext)
}
pub fn image_sources(
    package: &Package,
    part: &str,
) -> (HashMap<String, String>, HashMap<String, String>) {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let rel = if dir.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{dir}/_rels/{file}.rels")
    };
    let mut out = HashMap::new();
    let mut parts = HashMap::new();
    if let Some(x) = package.get(&rel).and_then(|b| DocxXml::parse(b).ok()) {
        for n in x
            .doc
            .descendants(x.doc.root())
            .filter(|&n| x.is_local(n, "Relationship"))
        {
            if x.attr(n, "TargetMode") == Some("External") {
                continue;
            }
            if !x.attr(n, "Type").is_some_and(|t| t.ends_with("/image")) {
                continue;
            }
            if let (Some(id), Some(target)) = (x.attr(n, "Id"), x.attr(n, "Target")) {
                let target = crate::media::resolve_rel_target(part, target);
                parts.insert(id.to_string(), target.clone());
                if let Some(bytes) = package.get(&target).filter(|_| mime(&target).is_some()) {
                    out.insert(id.to_string(), asset_url(&target, bytes));
                }
            }
        }
    }
    (out, parts)
}
pub fn manifest(package: &Package, payloads: bool) -> Vec<Value> {
    let mut assets = std::collections::BTreeMap::new();
    for (part, bytes) in package.parts() {
        if let Some(mime) = mime(part) {
            let url = asset_url(part, bytes);
            let mut value = json!({"url":url,"mime":mime,"size":bytes.len()});
            let mut parts = assets
                .get(&url)
                .and_then(|v: &Value| v.get("parts"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            parts.push(json!(part));
            parts.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
            value["parts"] = json!(parts);
            if payloads {
                value["data_url"] = json!(format!("data:{mime};base64,{}", base64(bytes)));
            }
            assets.insert(url, value);
        }
    }
    assets.into_values().collect()
}
pub fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0] as usize;
        let b = chunk.get(1).copied().unwrap_or(0) as usize;
        let c = chunk.get(2).copied().unwrap_or(0) as usize;
        out.push(ABC[a >> 2] as char);
        out.push(ABC[((a & 3) << 4) | (b >> 4)] as char);
        out.push(if chunk.len() > 1 {
            ABC[((b & 15) << 2) | (c >> 6)] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ABC[c & 63] as char
        } else {
            '='
        });
    }
    out
}
