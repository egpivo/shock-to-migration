//! Provenance records for reproducible outputs.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvenanceRecord {
    pub engine_version: String,
    pub project_id: String,
    pub config_sha256: String,
    pub input_hashes: Vec<InputHash>,
    pub metric_definition_id: String,
    pub command: String,
    pub computed_at_unix: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_extracted_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputHash {
    pub path: String,
    pub sha256: String,
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(hasher.finalize().as_slice())
}

pub fn hash_file(path: &Path) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    Ok(hash_bytes(&bytes))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}
