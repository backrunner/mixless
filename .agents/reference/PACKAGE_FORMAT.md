# Mixless package v1

Extension: `.mixpack`. Implemented by `mixless-pack`. All multibyte header fields are unsigned little-endian. The container is custom; it is not ZIP, SQLite, or a solid archive.

## Layout

The first 256 bytes are a superblock. Bytes 0–7 contain `MIXPACK\0`; bytes 8–11 contain format version 1. Reserved header bytes are zero. There are two 96-byte commit slots at offsets 32 and 128.

| Slot offset | Bytes | Meaning |
| --- | --- | --- |
| 0 | 8 | Generation, greater than zero |
| 8 | 8 | Absolute index offset |
| 16 | 8 | Compressed index size |
| 24 | 8 | Uncompressed index size |
| 32 | 32 | BLAKE3 of uncompressed index |
| 64 | 32 | BLAKE3 of the first 64 slot bytes |

Objects consist of blocks of at most 1 MiB. Each block is keyed by BLAKE3 of its uncompressed bytes and is stored as a Zstandard frame at level 3 when compression saves space, otherwise verbatim. Files share identical blocks throughout the package, including across libraries and generations. The index records block offsets, lengths, and codecs; object records carry ordered block hashes, full-file BLAKE3, and total size.

Indexes are Zstandard-compressed UTF-8 JSON, capped at 64 MiB both stored and decoded. Record payloads are separately chunked objects, capped at 64 MiB on import. Rust `serde` field names in the v1 manifest and portable record types are the serialization contract. Paths in serialized track records are empty; filesystem destinations are generated from validated identities and audio extensions during import. Decoder window and decoded block sizes are bounded independently of advertised compressed sizes.

## Commits and recovery

The writer holds an exclusive advisory lock, appends changed blocks and a complete index, syncs data, then writes generation modulo 2 to the corresponding slot and syncs again. Readers hold a shared lock and select the highest generation whose slot, index hash, and block bounds validate. An interrupted append or torn slot leaves the preceding commit readable. Readers do not silently substitute older audio if a referenced payload is corrupt: object validation fails the import. Checksums detect accidental damage; they do not authenticate a package's author.

Unchanged updates reuse verified blocks and do not create another commit. Obsolete blocks, indexes, and interrupted tails remain in the file. Export to a new package to reclaim space. A new file gets a committed empty index before payload writes; an interrupted first export can therefore be reopened and retried. Desktop **Export…** stages a replacement beside the destination and publishes it only after completion; **Update existing…** appends in place.

## Library records

Library schema v2 adds a random 128-bit persistent library identity and a mapping from `(source identity, source track ID)` to local track IDs. Audio, track metadata, artwork, analysis and its version, waveform and its version, cue slots and cue generation, playlist order (including repetitions and empty playlists), library order, and unavailable import placeholders are included. Import queues do not resume network work on the receiving machine. Folder playlists are restored as ordinary playlists without source-machine folder watches. Model weights and app preferences are excluded.

Selected-playlist export replaces those playlists in the package and retains other playlists. Entire-library export replaces that source library's package snapshot; other source libraries are retained. Import merges into the destination library: repeat imports update that source's records, without deleting local tracks or playlists absent from the package. Same-named playlists from distinct libraries remain separate. This is portable restoration, not a bidirectional sync protocol.

All referenced objects are extracted and verified in temporary managed storage before database publication. The database restore is one transaction across every source. Only new media is removed on a failed transaction; already-published content-addressed stem caches may remain and can be cleared in Storage. Audio is persisted under `package-media` beside the destination database, so the source package can be disconnected after import. Existing media with a matching target name must also match its content hash.

Stems use the existing immutable cache, under its process/content locks. Imported manifests must match the current evidence and model signatures; all three stereo 44.1 kHz float32 WAVs must have matching duration and finite samples. Playback never needs the inference runtime or model files. Analysis and waveform payload versions retain their normal compatibility gates; use matching app versions to avoid recomputation of older analysis. Export includes caches that already exist; it does not generate missing analysis or stems. The result reports missing caches.

## Verification

`cargo test -p mixless-pack` covers multi-playlist and empty-playlist restoration, repeated entries, ID remapping, cues, waveform, analysis readiness, cache-only stem playback, source deletion, incremental deduplication and no-op writes, multiple libraries, interrupted append/torn slot recovery, malformed metadata and checksums, and cancellation. Hardware-specific playback performance and physical Intel Mac UI acceptance require separate measurement.
