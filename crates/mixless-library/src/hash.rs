use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::LibraryError;

/// blake3(len || mtime_secs || first_64k || last_4k) — no inode.
pub fn content_hash(path: &Path) -> Result<String, LibraryError> {
    let mut file = File::open(path)?;
    let meta = file.metadata()?;
    let len = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut hasher = blake3::Hasher::new();
    hasher.update(&len.to_le_bytes());
    hasher.update(&mtime.to_le_bytes());

    let mut head = vec![0u8; 64 * 1024];
    let n = file.read(&mut head)?;
    hasher.update(&head[..n]);

    if len > 4 * 1024 {
        let back = 4 * 1024u64;
        file.seek(SeekFrom::End(-(back as i64).min(len as i64)))?;
        let mut tail = vec![0u8; 4 * 1024];
        let n = file.read(&mut tail)?;
        hasher.update(&tail[..n]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}
