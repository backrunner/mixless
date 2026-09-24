//! A 256-byte superblock with two checksummed commit slots. Payloads are
//! immutable; a commit becomes visible only after data has reached the disk.
use crate::{Error, Manifest, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

pub(crate) const BLOCK: usize = 1024 * 1024;
const HEADER: u64 = 256;
const MAGIC: &[u8; 8] = b"MIXPACK\0";
const MAX_INDEX: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Chunk {
    offset: u64,
    stored: u32,
    raw: u32,
    compressed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Blob {
    pub hash: String,
    pub size: u64,
    pub chunks: Vec<String>,
}

pub(crate) struct Container {
    file: File,
    pub manifest: Manifest,
    generation: u64,
    original: Option<[u8; 32]>,
    verified: std::collections::HashSet<String>,
    pub added: u64,
}

impl Container {
    pub fn open(path: &Path, write: bool) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(write)
            .create(write)
            .truncate(false)
            .open(path)?;
        if write {
            fs2::FileExt::try_lock_exclusive(&file)?;
        } else {
            fs2::FileExt::try_lock_shared(&file)?;
        }
        let len = file.metadata()?.len();
        let mut header = [0u8; HEADER as usize];
        let (manifest, generation, original) = if len == 0 && write {
            header[..8].copy_from_slice(MAGIC);
            header[8..12].copy_from_slice(&1u32.to_le_bytes());
            file.write_all(&header)?;
            file.sync_all()?;
            (Manifest::default(), 0, None)
        } else {
            file.read_exact(&mut header)?;
            if &header[..8] != MAGIC {
                return Err(Error::Invalid("Not a Mixless package".into()));
            }
            if u32::from_le_bytes(header[8..12].try_into().unwrap()) != 1 {
                return Err(Error::Invalid("Unsupported package version".into()));
            }
            let mut selected = None;
            let mut slots = [32usize, 128];
            slots.sort_by_key(|&slot| {
                std::cmp::Reverse(u64::from_le_bytes(
                    header[slot..slot + 8].try_into().unwrap(),
                ))
            });
            for slot in slots {
                let b = &header[slot..slot + 96];
                if blake3::hash(&b[..64]).as_bytes() != &b[64..] {
                    continue;
                }
                let word = |start| u64::from_le_bytes(b[start..start + 8].try_into().unwrap());
                let (generation, offset, stored, raw) = (word(0), word(8), word(16), word(24));
                if generation == 0
                    || offset < HEADER
                    || stored > MAX_INDEX
                    || raw > MAX_INDEX
                    || offset.checked_add(stored).is_none_or(|end| end > len)
                {
                    continue;
                }
                let result = (|| -> Result<_> {
                    file.seek(SeekFrom::Start(offset))?;
                    let mut compressed = vec![0; stored as usize];
                    file.read_exact(&mut compressed)?;
                    let data = decode(&compressed, raw as usize)?;
                    if blake3::hash(&data).as_bytes() != &b[32..64] {
                        return Err(Error::Invalid("Damaged package index".into()));
                    }
                    let manifest: Manifest = serde_json::from_slice(&data)?;
                    validate_chunks(&manifest.chunks, offset)?;
                    Ok((manifest, generation, Some(*blake3::hash(&data).as_bytes())))
                })();
                if let Ok(candidate) = result {
                    selected = Some(candidate);
                    break;
                }
            }
            selected.ok_or_else(|| Error::Invalid("Package has no complete commit".into()))?
        };
        let mut container = Self {
            file,
            manifest,
            generation,
            original,
            verified: Default::default(),
            added: 0,
        };
        if generation == 0 {
            container.commit()?;
        }
        Ok(container)
    }

    pub fn add(&mut self, mut input: impl Read, active: &impl Fn() -> bool) -> Result<Blob> {
        let mut blob = Blob {
            hash: String::new(),
            size: 0,
            chunks: Vec::new(),
        };
        let mut hasher = blake3::Hasher::new();
        let mut buffer = vec![0; BLOCK];
        loop {
            crate::check(active)?;
            let mut n = 0;
            while n < BLOCK {
                let read = input.read(&mut buffer[n..])?;
                if read == 0 {
                    break;
                }
                n += read;
            }
            if n == 0 {
                break;
            }
            let raw = &buffer[..n];
            hasher.update(raw);
            let hash = blake3::hash(raw).to_hex().to_string();
            if self.manifest.chunks.contains_key(&hash) {
                if self.verified.insert(hash.clone()) {
                    self.read_chunk(&hash)?;
                }
            } else {
                let encoded = zstd::bulk::compress(raw, 3)?;
                let compressed = encoded.len() < n;
                let data = if compressed { encoded.as_slice() } else { raw };
                let offset = self.file.seek(SeekFrom::End(0))?;
                self.file.write_all(data)?;
                self.added += data.len() as u64;
                self.manifest.chunks.insert(
                    hash.clone(),
                    Chunk {
                        offset,
                        stored: data.len() as u32,
                        raw: n as u32,
                        compressed,
                    },
                );
                self.verified.insert(hash.clone());
            }
            blob.chunks.push(hash);
            blob.size += n as u64;
        }
        blob.hash = hasher.finalize().to_hex().to_string();
        Ok(blob)
    }

    fn read_chunk(&mut self, hash: &str) -> Result<Vec<u8>> {
        let chunk = self
            .manifest
            .chunks
            .get(hash)
            .ok_or_else(|| Error::Invalid("Missing package block".into()))?;
        self.file.seek(SeekFrom::Start(chunk.offset))?;
        let mut stored = vec![0; chunk.stored as usize];
        self.file.read_exact(&mut stored)?;
        let raw = if chunk.compressed {
            decode(&stored, chunk.raw as usize)?
        } else {
            stored
        };
        if raw.len() != chunk.raw as usize || blake3::hash(&raw).to_hex().as_str() != hash {
            return Err(Error::Invalid("Package block checksum mismatch".into()));
        }
        Ok(raw)
    }

    pub fn extract(
        &mut self,
        blob: &Blob,
        mut output: impl Write,
        active: &impl Fn() -> bool,
    ) -> Result<()> {
        if !is_hash(&blob.hash) || blob.size > (1u64 << 40) {
            return Err(Error::Invalid("Invalid package object".into()));
        }
        let mut hasher = blake3::Hasher::new();
        let mut size = 0u64;
        for hash in &blob.chunks {
            crate::check(active)?;
            let data = self.read_chunk(hash)?;
            size = size
                .checked_add(data.len() as u64)
                .ok_or_else(|| Error::Invalid("Object too large".into()))?;
            if size > blob.size {
                return Err(Error::Invalid("Object length mismatch".into()));
            }
            hasher.update(&data);
            output.write_all(&data)?;
        }
        if size != blob.size || hasher.finalize().to_hex().as_str() != blob.hash {
            return Err(Error::Invalid("Object checksum mismatch".into()));
        }
        Ok(())
    }

    pub fn bytes(
        &mut self,
        blob: &Blob,
        limit: u64,
        active: &impl Fn() -> bool,
    ) -> Result<Vec<u8>> {
        if blob.size > limit {
            return Err(Error::Invalid("Package metadata exceeds size limit".into()));
        }
        let mut bytes = Vec::new();
        self.extract(blob, &mut bytes, active)?;
        Ok(bytes)
    }

    pub fn commit(&mut self) -> Result<u64> {
        let data = serde_json::to_vec(&self.manifest)?;
        let digest = *blake3::hash(&data).as_bytes();
        if Some(digest) == self.original {
            return Ok(self.generation);
        }
        if data.len() as u64 > MAX_INDEX {
            return Err(Error::Invalid("Package index exceeds 64 MiB".into()));
        }
        let encoded = zstd::bulk::compress(&data, 3)?;
        if encoded.len() as u64 > MAX_INDEX {
            return Err(Error::Invalid("Package index exceeds 64 MiB".into()));
        }
        let offset = self.file.seek(SeekFrom::End(0))?;
        self.file.write_all(&encoded)?;
        self.file.sync_all()?;
        let generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| Error::Invalid("Generation overflow".into()))?;
        let mut slot = [0u8; 96];
        for (pos, value) in [
            (0, generation),
            (8, offset),
            (16, encoded.len() as u64),
            (24, data.len() as u64),
        ] {
            slot[pos..pos + 8].copy_from_slice(&value.to_le_bytes());
        }
        slot[32..64].copy_from_slice(&digest);
        let checksum = blake3::hash(&slot[..64]);
        slot[64..].copy_from_slice(checksum.as_bytes());
        self.file
            .seek(SeekFrom::Start(32 + (generation % 2) * 96))?;
        self.file.write_all(&slot)?;
        self.file.sync_all()?;
        self.generation = generation;
        self.original = Some(digest);
        Ok(generation)
    }
}

pub(crate) fn is_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn decode(data: &[u8], expected: usize) -> Result<Vec<u8>> {
    let mut decoder = zstd::stream::read::Decoder::new(data)?;
    decoder.window_log_max(26)?;
    let mut raw = Vec::new();
    decoder.take(expected as u64 + 1).read_to_end(&mut raw)?;
    if raw.len() != expected {
        return Err(Error::Invalid("Compressed length mismatch".into()));
    }
    Ok(raw)
}
fn validate_chunks(chunks: &BTreeMap<String, Chunk>, end: u64) -> Result<()> {
    for (hash, c) in chunks {
        if !is_hash(hash)
            || c.raw == 0
            || c.raw as usize > BLOCK
            || c.stored == 0
            || c.stored as usize > BLOCK
            || (!c.compressed && c.raw != c.stored)
            || c.offset < HEADER
            || c.offset
                .checked_add(c.stored as u64)
                .is_none_or(|v| v > end)
        {
            return Err(Error::Invalid("Invalid package block bounds".into()));
        }
    }
    Ok(())
}
