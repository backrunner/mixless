use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::LibraryError;

/// Full-file BLAKE3. Paths, timestamps and inode are not content identity.
/// The prefix also invalidates caches created with the old head/tail fingerprint.
pub fn content_hash(path: &Path) -> Result<String, LibraryError> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("blake3-{}", hasher.finalize().to_hex()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, SeekFrom, Write};
    #[test]
    fn detects_middle_edits_with_identical_size_and_timestamp() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&vec![0; 200_000]).unwrap();
        let modified = file.as_file().metadata().unwrap().modified().unwrap();
        let before = content_hash(file.path()).unwrap();
        file.seek(SeekFrom::Start(100_000)).unwrap();
        file.write_all(&[1]).unwrap();
        file.as_file().set_modified(modified).unwrap();
        assert_ne!(before, content_hash(file.path()).unwrap());
    }
    #[test]
    fn identical_bytes_share_identity_across_paths_and_timestamps() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::write(&a, b"same audio content").unwrap();
        std::fs::copy(&a, &b).unwrap();
        File::options()
            .write(true)
            .open(&b)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH)
            .unwrap();
        assert_eq!(content_hash(&a).unwrap(), content_hash(&b).unwrap());
    }
}
