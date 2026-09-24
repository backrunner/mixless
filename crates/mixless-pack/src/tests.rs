use super::*;
use mixless_protocol::{Cue, CueKind, StemAnalysis, StemFrame, TempoMap, TrackAnalysis, Waveform};
use std::io::{Seek, SeekFrom};

fn processor(root: &Path) -> Processor {
    Processor::new(root.join("absent-models"), root.join("cache"), false)
}
fn track(lib: &Library, root: &Path, name: &str, seed: f32) -> TrackId {
    let path = root.join(format!("{name}.wav"));
    let mut wav = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 2,
            sample_rate: 44100,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for i in 0..8820 {
        wav.write_sample(((i as f32 * 0.01).sin() * seed).clamp(-1., 1.))
            .unwrap();
    }
    wav.finalize().unwrap();
    let id = lib.import_file(&path).unwrap();
    let hash = lib.get_track(id).unwrap().content_hash;
    let analysis = TrackAnalysis {
        track_id: id,
        duration_sec: 0.1,
        sample_rate: 44100,
        tempo: TempoMap {
            global_bpm: 120.,
            meter_num: 4,
            meter_den: 4,
            ..Default::default()
        },
        key: Some("C".into()),
        camelot: Some("8B".into()),
        key_confidence: 0.9,
        sections: vec![],
        phrase_boundaries: vec![],
        mix_regions: vec![],
        bars: vec![],
        moments: vec![],
        stems: None,
        waveform_path: None,
        partial: false,
    };
    lib.finish_analysis(&analysis, &hash, mixless_analyze::ANALYSIS_VERSION)
        .unwrap();
    lib.replace_auto_cues(
        id,
        &[Cue {
            index: 1,
            frame: 2205,
            kind: CueKind::Out,
            user_set: false,
        }],
    )
    .unwrap();
    lib.set_cue(id, 0, 441, CueKind::In, true).unwrap();
    let w = Waveform {
        columns: 2,
        duration_sec: 0.1,
        peak: vec![1, 2],
        peak_pos: vec![1, 2],
        peak_neg: vec![1, 2],
        rms: vec![1, 2],
        low: vec![1, 2],
        low_mid: vec![1, 2],
        mid: vec![1, 2],
        high: vec![1, 2],
        detail_pos: vec![256, 512],
        detail_neg: vec![256, 512],
        detail_rms: vec![256, 512],
    };
    lib.save_waveform(id, &hash, &w).unwrap();
    id
}
fn stems(lib: &Library, processor: &Processor, id: TrackId) {
    let hash = lib.get_track(id).unwrap().content_hash;
    let dir = processor.cache_path(&hash);
    fs::create_dir_all(&dir).unwrap();
    let analysis = StemAnalysis {
        version: mixless_stems::VERSION,
        separator_sha256: mixless_protocol::STEM_SEPARATOR.sha256.into(),
        notes_sha256: mixless_protocol::STEM_NOTES.sha256.into(),
        duration_sec: 0.1,
        residual_rms: 0.,
        frames: vec![StemFrame {
            start_sec: 0.,
            end_sec: 0.1,
            rms: [0.1; 3],
            band_db: [[0.; 3]; 3],
            onset: [0.; 3],
            drum_low_onset: 0.,
            vocal_activity: 0.,
            note_chroma: [0.; 12],
        }],
        notes: vec![],
    };
    fs::write(
        dir.join("analysis.json"),
        serde_json::to_vec(&analysis).unwrap(),
    )
    .unwrap();
    for name in &mixless_stems::PORTABLE_FILES[1..] {
        let mut wav = hound::WavWriter::create(
            dir.join(name),
            hound::WavSpec {
                channels: 2,
                sample_rate: 44100,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        for _ in 0..8820 {
            wav.write_sample(0.05f32).unwrap();
        }
        wav.finalize().unwrap();
    }
    let mut base = lib
        .load_analysis(id, mixless_analyze::ANALYSIS_VERSION)
        .unwrap()
        .unwrap();
    base.stems = Some(analysis);
    lib.finish_analysis(&base, &hash, mixless_analyze::ANALYSIS_VERSION)
        .unwrap();
}
fn export_all(lib: &Library, p: &Processor, path: &Path) -> Report {
    export(lib, p, path, Selection::Library, |_| {}, || true).unwrap()
}
fn import_all(lib: &Library, p: &Processor, path: &Path) -> Report {
    import(lib, p, path, |_| {}, || true).unwrap()
}

#[test]
fn roundtrip_multiple_playlists_order_cues_analysis_waveforms_and_stems_without_models() {
    let root = tempfile::tempdir().unwrap();
    let lib = Library::open(&root.path().join("library.db")).unwrap();
    let p = processor(root.path());
    let a = track(&lib, root.path(), "a", 0.2);
    let b = track(&lib, root.path(), "b", 0.3);
    stems(&lib, &p, a);
    lib.replace_playlist("One", &[b, a, a]).unwrap();
    lib.replace_playlist("Two", &[a]).unwrap();
    lib.replace_playlist("Empty", &[]).unwrap();
    let before: Vec<_> = lib.list_tracks().unwrap().iter().map(|t| t.id).collect();
    lib.reorder_tracks(None, &before, &[b, a]).unwrap();
    let path = root.path().join("test.mixpack");
    let exported = export_all(&lib, &p, &path);
    assert_eq!(exported.stems, 1);
    fs::remove_file(root.path().join("a.wav")).unwrap();
    fs::remove_file(root.path().join("b.wav")).unwrap();
    let dest = tempfile::tempdir().unwrap();
    let restored = Library::open(&dest.path().join("library.db")).unwrap();
    let playback = processor(dest.path());
    // Force ID remapping and confirm an unrelated local row survives.
    let local = track(&restored, dest.path(), "local", 0.4);
    let report = import_all(&restored, &playback, &path);
    assert_eq!((report.tracks, report.playlists, report.stems), (2, 3, 1));
    assert!(restored.get_track(local).is_ok());
    let playlists = restored.list_playlists().unwrap();
    let one = playlists.iter().find(|p| p.name == "One").unwrap();
    let rows = restored.playlist_tracks(PlaylistId(one.id)).unwrap();
    assert_eq!(
        rows.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(),
        ["b", "a", "a"]
    );
    let a = &rows[1];
    assert_ne!(a.id, TrackId(1));
    assert!(restored
        .analysis_ready(a.id, mixless_analyze::ANALYSIS_VERSION)
        .unwrap());
    assert_eq!(
        restored
            .load_analysis(a.id, mixless_analyze::ANALYSIS_VERSION)
            .unwrap()
            .unwrap()
            .track_id,
        a.id
    );
    assert_eq!(restored.cues(a.id).unwrap()[0].frame, 441);
    assert!(restored.automatic_cues_current(a.id).unwrap());
    assert_eq!(
        restored.load_waveform(a.id).unwrap().unwrap().detail_pos,
        [256, 512]
    );
    assert!(playback
        .playback(&a.content_hash, 4410, 44100)
        .unwrap()
        .is_some());
    // Cached analysis returns before attempting the deliberately failing loader.
    assert!(playback
        .analyze(
            &a.content_hash,
            0.1,
            || panic!("must not run inference"),
            &mut |_| {},
            &|| true
        )
        .is_ok());
    let ids = report.imported;
    let repeat = import_all(&restored, &playback, &path);
    assert_eq!(repeat.imported, ids);
    assert_eq!(restored.list_tracks().unwrap().len(), 3);
    assert_eq!(restored.list_playlists().unwrap().len(), 3);
    for id in &repeat.imported {
        assert!(restored.add_to_folder_playlist(*id).unwrap().is_none());
    }
    drop(restored);
    let restarted = Library::open(&dest.path().join("library.db")).unwrap();
    assert!(!restarted
        .list_playlists()
        .unwrap()
        .iter()
        .any(|p| p.name == "package-media"));
    assert!(restarted
        .analysis_ready(a.id, mixless_analyze::ANALYSIS_VERSION)
        .unwrap());
}

#[test]
fn late_database_failure_rolls_back_every_source_and_new_media() {
    let root = tempfile::tempdir().unwrap();
    let lib = Library::open(&root.path().join("library.db")).unwrap();
    let p = processor(root.path());
    let a = track(&lib, root.path(), "a", 0.2);
    lib.replace_playlist("Set", &[a]).unwrap();
    let path = root.path().join("test.mixpack");
    export_all(&lib, &p, &path);
    let another = tempfile::tempdir().unwrap();
    let second = Library::open(&another.path().join("library.db")).unwrap();
    let b = track(&second, another.path(), "b", 0.3);
    second.replace_playlist("Other", &[b]).unwrap();
    export_all(&second, &processor(another.path()), &path);
    let mut pack = Container::open(&path, true).unwrap();
    let playlist = pack
        .manifest
        .sources
        .values_mut()
        .next_back()
        .unwrap()
        .playlists
        .values_mut()
        .next()
        .unwrap();
    let placeholder = mixless_library::ImportItem {
        position: 7,
        external_id: "missing".into(),
        title: "Unavailable".into(),
        artist: "".into(),
        duration_ms: 0,
        status: "missing".into(),
        track_id: None,
        error: None,
    };
    // Duplicate import positions pass object checks but fail the SQL primary key
    // after media and tracks have been staged and the transaction has started.
    playlist.imports = vec![placeholder.clone(), placeholder];
    pack.commit().unwrap();
    drop(pack);
    let dest = tempfile::tempdir().unwrap();
    let restored = Library::open(&dest.path().join("library.db")).unwrap();
    assert!(import(&restored, &processor(dest.path()), &path, |_| {}, || true).is_err());
    assert!(restored.list_tracks().unwrap().is_empty());
    assert!(restored.list_playlists().unwrap().is_empty());
    assert_eq!(
        fs::read_dir(restored.package_media_dir()).unwrap().count(),
        1
    );
}

#[test]
fn pending_audio_and_unavailable_playlist_rows_roundtrip_without_network_work() {
    let root = tempfile::tempdir().unwrap();
    let lib = Library::open(&root.path().join("library.db")).unwrap();
    let path = root.path().join("pending.wav");
    fs::write(&path, b"unprepared fixture").unwrap();
    let id = lib.register_local_files(&[path]).unwrap()[0];
    let pl = lib
        .external_playlist("spotify", "source", "Remote")
        .unwrap();
    let item = mixless_library::ImportItem {
        position: 0,
        external_id: "local".into(),
        title: "Song".into(),
        artist: "Artist".into(),
        duration_ms: 0,
        status: "local".into(),
        track_id: Some(id),
        error: None,
    };
    let mut pending = item.clone();
    pending.position = 1;
    pending.external_id = "pending".into();
    pending.status = "queued".into();
    pending.track_id = None;
    lib.save_import_items(pl, &[item, pending]).unwrap();
    let path = root.path().join("test.mixpack");
    let report = export_all(&lib, &processor(root.path()), &path);
    assert_eq!(report.without_analysis, 1);
    let dest = tempfile::tempdir().unwrap();
    let restored = Library::open(&dest.path().join("library.db")).unwrap();
    import_all(&restored, &processor(dest.path()), &path);
    let pl = restored
        .list_playlists()
        .unwrap()
        .into_iter()
        .find(|p| p.name == "Remote")
        .unwrap();
    let (tracks, items) = restored.playlist_content(PlaylistId(pl.id)).unwrap();
    assert_eq!(tracks.len(), 1);
    assert!(!tracks[0].analyzed);
    assert_eq!(items.len(), 2);
    assert_eq!(items[1].status, "missing");
    assert!(!items[1].is_pending());
}

#[test]
fn writers_and_readers_cannot_observe_a_concurrent_commit() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("test.mixpack");
    let writer = Container::open(&path, true).unwrap();
    assert!(Container::open(&path, true).is_err());
    assert!(Container::open(&path, false).is_err());
    drop(writer);
    let reader = Container::open(&path, false).unwrap();
    assert!(Container::open(&path, false).is_ok());
    assert!(Container::open(&path, true).is_err());
    drop(reader);
    assert!(Container::open(&path, true).is_ok());
}

#[test]
fn incremental_updates_reuse_payloads_noops_and_same_named_playlists_stay_distinct() {
    let root = tempfile::tempdir().unwrap();
    let lib = Library::open(&root.path().join("library.db")).unwrap();
    let p = processor(root.path());
    let a = track(&lib, root.path(), "a", 0.2);
    let first = lib.replace_playlist("First", &[a]).unwrap();
    let second = lib.replace_playlist("Second", &[a, a]).unwrap();
    let path = root.path().join("test.mixpack");
    let one = export(
        &lib,
        &p,
        &path,
        Selection::Playlists(vec![first]),
        |_| {},
        || true,
    )
    .unwrap();
    let len = fs::metadata(&path).unwrap().len();
    let noop = export(
        &lib,
        &p,
        &path,
        Selection::Playlists(vec![first]),
        |_| {},
        || true,
    )
    .unwrap();
    assert_eq!(noop.generation, one.generation);
    assert_eq!(noop.added_bytes, 0);
    assert_eq!(len, fs::metadata(&path).unwrap().len());
    let two = export(
        &lib,
        &p,
        &path,
        Selection::Playlists(vec![second]),
        |_| {},
        || true,
    )
    .unwrap();
    assert_eq!(two.added_bytes, 0);
    lib.set_cue(a, 0, 800, CueKind::In, true).unwrap();
    let cue_update = export(
        &lib,
        &p,
        &path,
        Selection::Playlists(vec![first]),
        |_| {},
        || true,
    )
    .unwrap();
    assert!(cue_update.added_bytes > 0 && cue_update.added_bytes < 100_000);

    let dest = tempfile::tempdir().unwrap();
    let restored = Library::open(&dest.path().join("library.db")).unwrap();
    let q = processor(dest.path());
    assert_eq!(import_all(&restored, &q, &path).playlists, 2);
    lib.replace_playlist("First", &[]).unwrap();
    export(
        &lib,
        &p,
        &path,
        Selection::Playlists(vec![first]),
        |_| {},
        || true,
    )
    .unwrap();
    import_all(&restored, &q, &path);
    let first = restored
        .list_playlists()
        .unwrap()
        .into_iter()
        .find(|p| p.name == "First")
        .unwrap();
    assert_eq!(first.tracks, 0);
    // A second library may use the same IDs and names without aliasing them.
    let another = tempfile::tempdir().unwrap();
    let other = Library::open(&another.path().join("library.db")).unwrap();
    let b = track(&other, another.path(), "b", 0.3);
    other.replace_playlist("Second", &[b]).unwrap();
    export_all(&other, &processor(another.path()), &path);
    assert_eq!(import_all(&restored, &q, &path).playlists, 3);
    assert_eq!(
        restored
            .list_playlists()
            .unwrap()
            .iter()
            .filter(|p| p.name == "Second")
            .count(),
        2
    );
}

#[test]
fn torn_commit_and_interrupted_append_preserve_previous_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("test.mixpack");
    let mut pack = Container::open(&path, true).unwrap();
    let first = pack.add(&b"first"[..], &|| true).unwrap();
    let committed = pack.commit().unwrap();
    drop(pack);
    let before = fs::metadata(&path).unwrap().len();
    let mut pack = Container::open(&path, true).unwrap();
    pack.add(&vec![3; container::BLOCK + 13][..], &|| true)
        .unwrap();
    drop(pack);
    let mut pack = Container::open(&path, true).unwrap();
    assert_eq!(pack.bytes(&first, 100, &|| true).unwrap(), b"first");
    assert_eq!(pack.commit().unwrap(), committed);
    pack.add(&b"second"[..], &|| true).unwrap();
    let generation = pack.commit().unwrap();
    drop(pack);
    // Tear the newest slot after data publication; the older slot remains valid.
    let mut file = File::options().write(true).open(&path).unwrap();
    file.seek(SeekFrom::Start(32 + (generation % 2) * 96 + 10))
        .unwrap();
    file.write_all(&[255; 16]).unwrap();
    drop(file);
    let mut recovered = Container::open(&path, false).unwrap();
    assert_eq!(recovered.bytes(&first, 100, &|| true).unwrap(), b"first");
    drop(recovered);
    // A truncated latest index also falls back to the prior committed slot.
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(before)
        .unwrap();
    assert!(Container::open(&path, false).is_ok());
}

#[test]
fn corrupt_audio_and_unsafe_metadata_do_not_publish_database_or_media() {
    let root = tempfile::tempdir().unwrap();
    let lib = Library::open(&root.path().join("library.db")).unwrap();
    let p = processor(root.path());
    let a = track(&lib, root.path(), "a", 0.2);
    lib.replace_playlist("Set", &[a]).unwrap();
    let path = root.path().join("test.mixpack");
    export_all(&lib, &p, &path);
    let mut pack = Container::open(&path, true).unwrap();
    pack.manifest
        .sources
        .values_mut()
        .next()
        .unwrap()
        .tracks
        .values_mut()
        .next()
        .unwrap()
        .extension = "../../escape".into();
    pack.commit().unwrap();
    drop(pack);
    let dest = tempfile::tempdir().unwrap();
    let restored = Library::open(&dest.path().join("library.db")).unwrap();
    let q = processor(dest.path());
    assert!(import(&restored, &q, &path, |_| {}, || true).is_err());
    assert!(restored.list_tracks().unwrap().is_empty());
    export_all(&lib, &p, &path);
    let pack = Container::open(&path, false).unwrap();
    let audio = pack
        .manifest
        .sources
        .values()
        .next()
        .unwrap()
        .tracks
        .values()
        .next()
        .unwrap()
        .audio
        .clone();
    let descriptor = serde_json::to_value(&pack.manifest.chunks[&audio.chunks[0]]).unwrap();
    let offset = descriptor["offset"].as_u64().unwrap();
    drop(pack);
    let mut file = File::options().write(true).open(&path).unwrap();
    file.seek(SeekFrom::Start(offset)).unwrap();
    file.write_all(&[255; 8]).unwrap();
    drop(file);
    assert!(import(&restored, &q, &path, |_| {}, || true).is_err());
    assert!(restored.list_tracks().unwrap().is_empty());
    assert!(!dest.path().join("escape").exists());
    assert!(audio.size > 0);
    assert_eq!(
        fs::read_dir(restored.package_media_dir()).unwrap().count(),
        1
    ); // only lock
}

#[test]
fn cancellation_keeps_existing_package_and_library_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let lib = Library::open(&root.path().join("library.db")).unwrap();
    let p = processor(root.path());
    track(&lib, root.path(), "a", 0.2);
    let path = root.path().join("test.mixpack");
    export_all(&lib, &p, &path);
    let before = fs::read(&path).unwrap();
    assert!(export(&lib, &p, &path, Selection::Library, |_| {}, || false).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    let dest = tempfile::tempdir().unwrap();
    let restored = Library::open(&dest.path().join("library.db")).unwrap();
    assert!(import(&restored, &processor(dest.path()), &path, |_| {}, || false).is_err());
    assert!(restored.list_tracks().unwrap().is_empty());
}

#[test]
fn non_finite_stems_cannot_replace_a_valid_package_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let lib = Library::open(&root.path().join("library.db")).unwrap();
    let p = processor(root.path());
    let id = track(&lib, root.path(), "a", 0.2);
    stems(&lib, &p, id);
    let path = root.path().join("test.mixpack");
    export_all(&lib, &p, &path);
    let hash = lib.get_track(id).unwrap().content_hash;
    let mut wav = hound::WavWriter::create(
        p.cache_path(&hash).join("vocals.wav"),
        hound::WavSpec {
            channels: 2,
            sample_rate: 44100,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for _ in 0..8820 {
        wav.write_sample(f32::NAN).unwrap();
    }
    wav.finalize().unwrap();
    assert!(export(&lib, &p, &path, Selection::Library, |_| {}, || true).is_err());
    let dest = tempfile::tempdir().unwrap();
    let restored = Library::open(&dest.path().join("library.db")).unwrap();
    let playback = processor(dest.path());
    let report = import_all(&restored, &playback, &path);
    assert_eq!(report.stems, 1);
    assert!(playback.playback(&hash, 4410, 44100).unwrap().is_some());
}
