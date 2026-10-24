//! Read candidate metadata first; download only a recording that passes DJ
//! identity/duration gates. Child processes have one bounded job deadline.
use mixless_acquire::{candidate_score, primary_artist, AcquireError, ResolveJob};
use serde_json::Value;
mod errors;
mod media;
mod metadata;
mod pacing;
mod process;
mod toolchain;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
use toolchain::locate;

/// Corresponding native sources copied into the app by the packaging tool.
pub fn audio_source_archive() -> &'static str {
    env!("MIXLESS_AUDIO_SOURCE_ARCHIVE")
}

pub struct YoutubeMusicAcquire {
    ytdlp: PathBuf,
    deno: Option<PathBuf>,
}
#[derive(Debug, Clone)]
struct Candidate {
    id: String,
    title: String,
    artist: String,
    duration_ms: u32,
    score: f32,
}
fn err(message: impl Into<String>) -> AcquireError {
    AcquireError::Msg(message.into())
}

impl YoutubeMusicAcquire {
    pub fn detect() -> Result<Self, AcquireError> {
        let provider = Self {
            ytdlp: locate("yt-dlp").ok_or_else(|| {
                AcquireError::ToolUnavailable(
                    "yt-dlp is missing. Install yt-dlp, then retry.".into(),
                )
            })?,
            deno: locate("deno"),
        };
        Ok(provider)
    }
    pub fn fetch(&self, job: &ResolveJob, dest: &Path) -> Result<PathBuf, AcquireError> {
        self.fetch_named(job, dest, mixless_acquire::DownloadFilename::RecordingId)
    }
    pub fn fetch_named(
        &self,
        job: &ResolveJob,
        dest: &Path,
        naming: mixless_acquire::DownloadFilename,
    ) -> Result<PathBuf, AcquireError> {
        if job.spotify_id.len() != 22
            || !job.spotify_id.bytes().all(|b| b.is_ascii_alphanumeric())
            || job.duration_ms == 0
        {
            return Err(err("invalid recording ID or duration"));
        }
        fs::create_dir_all(dest).map_err(|e| err(e.to_string()))?;
        let dest = dest.canonicalize().map_err(|e| err(e.to_string()))?;
        // Each TempDir owns its own cleanup. A similarly named directory
        // may belong to another active download or to the user.
        let final_path = dest.join(naming.filename(job));
        // The importer revalidates the actual decoder duration even on reuse.
        if final_path.is_file() && fs::metadata(&final_path).is_ok_and(|m| m.len() > 0) {
            return Ok(final_path);
        }
        let deadline = Instant::now() + Duration::from_secs(180);
        let search_deadline = Instant::now() + Duration::from_secs(35);
        // Search both catalogs concurrently. A good metadata match can still be
        // unplayable (removed/region restricted), so retain several alternatives.
        let (music, youtube) = std::thread::scope(|scope| {
            let music = scope.spawn(|| self.music_search(job));
            let youtube = self.youtube_search(job, search_deadline);
            (
                music
                    .join()
                    .unwrap_or_else(|_| Err(err("Music search stopped"))),
                youtube,
            )
        });
        let mut candidates = Vec::new();
        let mut errors = Vec::new();
        for result in [music, youtube] {
            match result {
                Ok(found) => candidates.extend(found),
                Err(error) => {
                    if errors::stops_retries(&error) {
                        return Err(error);
                    }
                    errors.push(error.to_string());
                }
            }
        }
        rank_candidates(&mut candidates);
        let mut attempted = std::collections::HashSet::new();
        for pass in 0..2 {
            if let Some(path) =
                try_candidates(&candidates, &mut attempted, &mut errors, |chosen| {
                    let attempt_deadline = deadline.min(Instant::now() + Duration::from_secs(45));
                    self.download(job, chosen, &dest, &final_path, attempt_deadline)
                })?
            {
                return Ok(path);
            }
            if pass == 0 && Instant::now() < deadline {
                // A simpler query recovers songs obscured by collaborator
                // credits and ranking of official/lyric uploads.
                let query = format!("{} {}", primary_artist(&job.artist), job.title);
                match self.youtube_search_query(
                    job,
                    &query,
                    deadline.min(Instant::now() + Duration::from_secs(25)),
                ) {
                    Ok(found) => candidates = found,
                    Err(error) => {
                        if errors::stops_retries(&error) {
                            return Err(error);
                        }
                        candidates.clear();
                        errors.push(error.to_string());
                    }
                }
                rank_candidates(&mut candidates);
            }
        }
        if attempted.is_empty() {
            Err(err(if errors.is_empty() {
                "No matching audio found for this artist, version and duration".into()
            } else {
                format!("Audio search failed: {}", errors::summary(&errors))
            }))
        } else {
            Err(err(format!(
                "Matching audio could not be downloaded: {}",
                errors::summary(&errors)
            )))
        }
    }

    fn download(
        &self,
        job: &ResolveJob,
        chosen: &Candidate,
        dest: &Path,
        final_path: &Path,
        deadline: Instant,
    ) -> Result<PathBuf, AcquireError> {
        let mut errors = Vec::new();
        // A recording can have an unavailable AAC rendition while its Opus or
        // HLS audio is healthy. Try those before abandoning the recording.
        for format in [
            "bestaudio[ext=m4a]/bestaudio",
            "bestaudio[ext=webm]",
            "bestaudio[protocol=m3u8_native]",
        ] {
            if Instant::now() >= deadline {
                break;
            }
            match self.download_format(job, chosen, dest, final_path, format, deadline) {
                Ok(path) => return Ok(path),
                Err(error) => {
                    if errors::stops_retries(&error) {
                        return Err(error);
                    }
                    errors.push(error.to_string());
                }
            }
        }
        Err(err(if errors.is_empty() {
            "Audio download timed out".into()
        } else {
            errors::summary(&errors)
        }))
    }

    fn download_format(
        &self,
        job: &ResolveJob,
        chosen: &Candidate,
        dest: &Path,
        final_path: &Path,
        format: &str,
        deadline: Instant,
    ) -> Result<PathBuf, AcquireError> {
        let staging = tempfile::Builder::new()
            .prefix(".mixless-")
            .tempdir_in(&dest)
            .map_err(|e| err(e.to_string()))?;
        let mut args = self.base_args();
        args.extend(
            [
                "--no-playlist",
                "--no-progress",
                "--no-warnings",
                "--socket-timeout",
                "12",
                "--retries",
                "2",
                "--fragment-retries",
                "2",
                "--concurrent-fragments",
                "1",
                "--sleep-requests",
                "1",
                "--retry-sleep",
                "http:exp=2:20",
                "-f",
                format,
                "--fixup",
                "never",
                "--downloader",
                "native",
            ]
            .map(OsString::from),
        );
        args.extend(["--print", "after_move:filepath", "-o"].map(OsString::from));
        // Keep playlist names out of yt-dlp's template/environment expansion.
        // Names such as "100%", "%(title)s" and "$HOME" stay literal folders.
        args.push("source.%(ext)s".into());
        args.push(format!("https://www.youtube.com/watch?v={}", chosen.id).into());
        let output = self.run_in(&args, deadline, Some(staging.path()))?;
        let printed = String::from_utf8_lossy(&output);
        let path = printed
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| err("downloader returned no audio path"))?;
        let path = staging
            .path()
            .join(path)
            .canonicalize()
            .map_err(|_| err("downloader did not create the reported file"))?;
        let root = staging
            .path()
            .canonicalize()
            .map_err(|e| err(e.to_string()))?;
        if !path.starts_with(&root) || !fs::metadata(&path).is_ok_and(|m| m.len() > 0) {
            return Err(err("downloader returned an invalid audio file"));
        }
        let prepared = staging.path().join("audio.m4a");
        media::prepare(&path, &prepared, deadline)?;
        metadata::write(&prepared, job)?;
        // Stable remote IDs avoid non-ASCII title collisions and prefix matches
        // accidentally loading a thumbnail or JSON sidecar as audio. Staging
        // lives inside `dest`, so rename stays on one volume and also works on
        // filesystems without hard links (exFAT/FAT32 removable drives).
        fs::rename(&prepared, &final_path)
            .or_else(|_| fs::hard_link(&prepared, &final_path))
            .map_err(|e| err(format!("publish audio: {e}")))?;
        let provenance = serde_json::json!({"spotify_id":job.spotify_id,"source_url":format!("https://www.youtube.com/watch?v={}",chosen.id),"title":chosen.title,"artist":chosen.artist,"duration_ms":chosen.duration_ms,"score":chosen.score});
        let _ = fs::write(final_path.with_extension("json"), provenance.to_string());
        Ok(final_path.to_path_buf())
    }
    fn base_args(&self) -> Vec<OsString> {
        let mut args = vec![OsString::from("--ignore-config")];
        if let Some(deno) = &self.deno {
            args.push("--js-runtimes".into());
            args.push(format!("deno:{}", deno.display()).into());
        }
        args
    }
    fn youtube_search(
        &self,
        job: &ResolveJob,
        deadline: Instant,
    ) -> Result<Vec<Candidate>, AcquireError> {
        self.youtube_search_query(
            job,
            &format!("{} {} official audio", job.artist, job.title),
            deadline,
        )
    }
    fn youtube_search_query(
        &self,
        job: &ResolveJob,
        query: &str,
        deadline: Instant,
    ) -> Result<Vec<Candidate>, AcquireError> {
        let mut args = self.base_args();
        args.extend(
            [
                "--flat-playlist",
                "--dump-single-json",
                "--skip-download",
                "--no-warnings",
            ]
            .map(OsString::from),
        );
        args.push(format!("ytsearch10:{query}").into());
        let bytes = self.run(&args, deadline)?;
        let data: Value = serde_json::from_slice(&bytes)
            .map_err(|e| err(format!("invalid search metadata: {e}")))?;
        Ok(parse_youtube(job, &data))
    }
    fn music_search(&self, job: &ResolveJob) -> Result<Vec<Candidate>, AcquireError> {
        let _permit = pacing::acquire(Instant::now() + Duration::from_secs(30)).map_err(err)?;
        let agent = mixless_net::agent(
            "https://music.youtube.com/youtubei/v1/search?prettyPrint=false",
            Duration::from_secs(12),
        )
        .map_err(err)?;
        let response=agent.post("https://music.youtube.com/youtubei/v1/search?prettyPrint=false")
            .send_json(serde_json::json!({"context":{"client":{"clientName":"WEB_REMIX","clientVersion":"1.20260819.01.00","hl":"en"}},"query":format!("{} {}",job.artist,job.title),"params":"EgWKAQIIAWoKEAkQBRAKEAMQBA%3D%3D"}))
            .map_err(|e| {
                if let ureq::Error::Status(429, response) = &e {
                    pacing::rate_limited(response.header("Retry-After").and_then(|s| s.parse().ok()).unwrap_or(60));
                }
                err(format!("YouTube Music search: {e}"))
            })?;
        let data: Value = response.into_json().map_err(|e| err(e.to_string()))?;
        let mut result = Vec::new();
        parse_music(job, &data, &mut result);
        Ok(result)
    }
    fn run(&self, args: &[OsString], deadline: Instant) -> Result<Vec<u8>, AcquireError> {
        self.run_in(args, deadline, None)
    }
    fn run_in(
        &self,
        args: &[OsString],
        deadline: Instant,
        working_dir: Option<&Path>,
    ) -> Result<Vec<u8>, AcquireError> {
        let _permit = pacing::acquire(deadline).map_err(err)?;
        let proxy = mixless_net::proxy_for(
            args.last()
                .and_then(|arg| arg.to_str())
                .filter(|url| url.starts_with("https://"))
                .unwrap_or("https://www.youtube.com/results"),
        )
        .map_err(err)?
        .unwrap_or_default();
        self.run_process(args, deadline, working_dir, &proxy)
    }

    fn run_process(
        &self,
        args: &[OsString],
        deadline: Instant,
        working_dir: Option<&Path>,
        proxy: &str,
    ) -> Result<Vec<u8>, AcquireError> {
        let mut command = Command::new(&self.ytdlp);
        // Keep yt-dlp on the same explicit proxy route as Rust metadata requests.
        command.arg("--proxy").arg(proxy).args(args);
        if let Some(dir) = working_dir {
            command.current_dir(dir);
        }
        let output = process::run(command, deadline)?;
        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr);
            if pacing::is_rate_limit(&message) {
                pacing::rate_limited(60);
            }
            return Err(errors::downloader_error(&message));
        }
        Ok(output.stdout)
    }
}
fn try_candidates(
    candidates: &[Candidate],
    attempted: &mut std::collections::HashSet<String>,
    errors: &mut Vec<String>,
    mut download: impl FnMut(&Candidate) -> Result<PathBuf, AcquireError>,
) -> Result<Option<PathBuf>, AcquireError> {
    let choices: Vec<_> = candidates
        .iter()
        .filter(|c| c.score >= 0.72 && !attempted.contains(&c.id))
        .take(3)
        .collect();
    for chosen in choices {
        attempted.insert(chosen.id.clone());
        match download(chosen) {
            Ok(path) => return Ok(Some(path)),
            Err(error) => {
                if errors::stops_retries(&error) {
                    return Err(error);
                }
                errors.push(error.to_string());
            }
        }
    }
    Ok(None)
}

fn rank_candidates(candidates: &mut Vec<Candidate>) {
    candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|c| seen.insert(c.id.clone()));
}

fn find_duration(value: &Value) -> Option<u32> {
    if let Some(text) = value["text"].as_str() {
        if let Some(ms) = duration(text.trim()) {
            return Some(ms);
        }
    }
    match value {
        Value::Object(object) => object.values().find_map(find_duration),
        Value::Array(array) => array.iter().find_map(find_duration),
        _ => None,
    }
}

fn candidate(
    job: &ResolveJob,
    id: &str,
    title: &str,
    artist: &str,
    duration_ms: u32,
) -> Option<Candidate> {
    if id.len() != 11
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return None;
    }
    Some(Candidate {
        id: id.into(),
        title: title.into(),
        artist: artist.into(),
        duration_ms,
        score: candidate_score(job, title, artist, duration_ms)?,
    })
}
fn parse_youtube(job: &ResolveJob, data: &Value) -> Vec<Candidate> {
    data["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            candidate(
                job,
                t["id"].as_str()?,
                t["title"].as_str()?,
                t["artist"]
                    .as_str()
                    .or(t["channel"].as_str())
                    .or(t["uploader"].as_str())
                    .unwrap_or(""),
                (t["duration"].as_f64()? * 1000.).round() as u32,
            )
        })
        .collect()
}
fn find_video(value: &Value) -> Option<&str> {
    if let Some(id) = value["videoId"].as_str() {
        return Some(id);
    }
    match value {
        Value::Object(o) => o.values().find_map(find_video),
        Value::Array(a) => a.iter().find_map(find_video),
        _ => None,
    }
}
fn duration(text: &str) -> Option<u32> {
    let parts: Vec<_> = text.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let mut seconds = 0u32;
    for p in parts {
        let n = p.parse::<u32>().ok()?;
        seconds = seconds.checked_mul(60)?.checked_add(n)?;
    }
    seconds.checked_mul(1000)
}
fn parse_music(job: &ResolveJob, value: &Value, result: &mut Vec<Candidate>) {
    if let Some(row) = value.get("musicResponsiveListItemRenderer") {
        let columns = row["flexColumns"].as_array();
        if let Some(columns) = columns {
            let text = |column: &Value| {
                column["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"]
                    .as_array()
                    .map(|runs| {
                        runs.iter()
                            .filter_map(|r| r["text"].as_str().map(str::to_owned))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            };
            let title = columns
                .first()
                .map(|c| text(c).join(""))
                .unwrap_or_default();
            let subtitle = columns.get(1).map(text).unwrap_or_default();
            // Collaborators can occupy several runs. Durations sometimes live
            // in fixedColumns instead of the second flex column.
            let credits = subtitle.join("");
            let artist = credits.split('•').next().unwrap_or("").trim();
            let length = subtitle
                .iter()
                .find_map(|s| duration(s.trim()))
                .or_else(|| find_duration(&row["fixedColumns"]));
            if let (Some(id), Some(ms)) = (find_video(row), length) {
                if let Some(c) = candidate(job, id, &title, artist, ms) {
                    result.push(c);
                }
            }
        }
        return;
    }
    match value {
        Value::Object(o) => {
            for v in o.values() {
                parse_music(job, v, result)
            }
        }
        Value::Array(a) => {
            for v in a {
                parse_music(job, v, result)
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_fetch_preserves_other_downloads_and_user_directories() {
        let dir = tempfile::tempdir().unwrap();
        let provider = YoutubeMusicAcquire {
            ytdlp: "unused".into(),
            deno: None,
        };
        let job = ResolveJob {
            spotify_id: "a".repeat(22),
            title: "Song".into(),
            artist: "Artist".into(),
            duration_ms: 180000,
            isrc: None,
        };
        let cached = dir.path().join(format!("{}.m4a", job.spotify_id));
        fs::write(&cached, b"cached audio").unwrap();
        for name in [".mixless-active-download", ".mixless-user-files"] {
            fs::create_dir(dir.path().join(name)).unwrap();
            fs::write(dir.path().join(name).join("keep"), b"in progress").unwrap();
        }
        assert_eq!(
            provider.fetch(&job, dir.path()).unwrap(),
            cached.canonicalize().unwrap()
        );
        for name in [".mixless-active-download", ".mixless-user-files"] {
            assert_eq!(
                fs::read(dir.path().join(name).join("keep")).unwrap(),
                b"in progress"
            );
        }
    }
    #[test]
    #[ignore = "live metadata-only YouTube Music/YouTube search"]
    fn live_metadata_search() {
        let provider = YoutubeMusicAcquire::detect().unwrap();
        let job = ResolveJob {
            spotify_id: "18Bk73XB46nJMXicmF83yv".into(),
            title: "Back Again".into(),
            artist: "KLYDIX".into(),
            duration_ms: 191333,
            isrc: None,
        };
        let music = provider.music_search(&job);
        eprintln!(
            "YouTube Music matched candidates: {:?}",
            music.as_ref().map(|c| c.len())
        );
        let youtube = provider.youtube_search(&job, Instant::now() + Duration::from_secs(45));
        eprintln!(
            "YouTube matched candidates: {:?}",
            youtube.as_ref().map(|c| c.len())
        );
        assert!(
            music.as_ref().is_ok_and(|c| !c.is_empty())
                || youtube.as_ref().is_ok_and(|c| !c.is_empty())
        );
    }
    #[test]
    fn search_does_not_select_the_first_result_or_unknown_duration() {
        let job = ResolveJob {
            spotify_id: "a".repeat(22),
            title: "Song".into(),
            artist: "Artist".into(),
            duration_ms: 180000,
            isrc: None,
        };
        let data = serde_json::json!({"entries":[{"id":"aaaaaaaaaaa","title":"Song (Live)","channel":"Artist","duration":180},{"id":"bbbbbbbbbbb","title":"Song","channel":"Artist - Topic","duration":180},{"id":"ccccccccccc","title":"Song","channel":"Artist","duration":null}]});
        let found = parse_youtube(&job, &data);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "bbbbbbbbbbb");
    }
    #[test]
    fn failed_downloads_try_another_candidate_and_expanded_search_skips_attempted_ids() {
        let candidates: Vec<_> = (0..5)
            .map(|i| Candidate {
                id: format!("{i:011}"),
                title: "Song".into(),
                artist: "Artist".into(),
                duration_ms: 180000,
                score: 1.0,
            })
            .collect();
        let mut attempted = std::collections::HashSet::new();
        let mut errors = Vec::new();
        let first = try_candidates(&candidates, &mut attempted, &mut errors, |_| {
            Err(err("source unavailable"))
        });
        assert!(first.unwrap().is_none());
        assert_eq!(errors.len(), 3);
        let mut calls = Vec::new();
        let next = try_candidates(&candidates, &mut attempted, &mut errors, |c| {
            calls.push(c.id.clone());
            if c.id.ends_with('3') {
                Err(err("download failed"))
            } else {
                Ok("audio.m4a".into())
            }
        });
        assert_eq!(next.unwrap(), Some(PathBuf::from("audio.m4a")));
        assert_eq!(calls, ["00000000003", "00000000004"]);
    }

    #[test]
    #[cfg(unix)]
    fn unavailable_aac_uses_an_alternate_format_and_cleans_staging() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-yt-dlp");
        let fixture = dir.path().join("fixture.m4a");
        media::tests::write_wav(&fixture, 44_100, 2, 0.2);
        fs::write(
            &script,
            r#"#!/bin/sh
while [ "$#" -gt 0 ]; do
    case "$1" in
        -f) shift; format="$1" ;;
        -o) shift; template="$1" ;;
    esac
    shift
done
printf '%s\n' "$format" >> "$(dirname "$0")/formats"
case "$format" in
    *m4a*) echo 'HTTP Error 403: Forbidden' >&2; exit 1 ;;
esac
path="${template%.*}.m4a"
[ "$template" = 'source.%(ext)s' ] || exit 2
# yt-dlp must only download; all conversion belongs to the Rust library.
cp "$(dirname "$0")/fixture.m4a" "$path"
printf '%s\n' "$path"
"#,
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let provider = YoutubeMusicAcquire {
            ytdlp: script,
            deno: None,
        };
        let job = &reported_jobs()[2];
        let chosen = Candidate {
            id: "aaaaaaaaaaa".into(),
            title: job.title.clone(),
            artist: job.artist.clone(),
            duration_ms: job.duration_ms,
            score: 1.0,
        };
        let dest = mixless_acquire::playlist_download_dir(dir.path(), "夜行 100% %(title)s $HOME");
        fs::create_dir_all(&dest).unwrap();
        let final_path = dest.join(format!("{}.m4a", job.spotify_id));
        let path = provider
            .download(
                job,
                &chosen,
                &dest,
                &final_path,
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        assert_eq!(path, final_path);
        use lofty::{file::TaggedFileExt, prelude::Accessor};
        let tagged = lofty::probe::Probe::open(&path).unwrap().read().unwrap();
        let tag = tagged.primary_tag().unwrap();
        assert_eq!(tag.title().as_deref(), Some(job.title.as_str()));
        assert_eq!(tag.artist().as_deref(), Some(job.artist.as_str()));
        assert!(dest.join(format!("{}.json", job.spotify_id)).is_file());
        assert_eq!(
            fs::read_to_string(dir.path().join("formats"))
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            ["bestaudio[ext=m4a]/bestaudio", "bestaudio[ext=webm]"]
        );
        assert!(fs::read_dir(&dest).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".mixless-")
        }));
    }

    fn reported_jobs() -> Vec<ResolveJob> {
        [
            ("2QWGkc7Tnz6672asv4BtLi", "Moments", "MitiS, Adara", 278400),
            (
                "6AoG52kxfptY0QBQnjuQOe",
                "Hollow",
                "Dabin, Kai Wachi, Lø Spirit",
                240000,
            ),
            (
                "2XaNNKsCnNokevsz9EkYvY",
                "Day Cycle",
                "Last Checkpoint",
                179514,
            ),
            (
                "0gsAxRNic7xofkxG93H5Ha",
                "Idle World",
                "Last Checkpoint",
                191760,
            ),
        ]
        .into_iter()
        .map(|(id, title, artist, duration_ms)| ResolveJob {
            spotify_id: id.into(),
            title: title.into(),
            artist: artist.into(),
            duration_ms,
            isrc: None,
        })
        .collect()
    }

    #[test]
    fn music_search_reads_all_credited_artists_and_fixed_duration_columns() {
        let job = &reported_jobs()[2];
        let row = serde_json::json!({"musicResponsiveListItemRenderer": {
            "playlistItemData": {"videoId": "aaaaaaaaaaa"},
            "flexColumns": [
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Day Cycle"}]}}},
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [
                    {"text": "Memory Machine"}, {"text": ", "}, {"text": "Last Checkpoint"},
                    {"text": " & "}, {"text": "Full Circle Avenue"}, {"text": " • "}, {"text": "Outer Reach"}
                ]}}}
            ],
            "fixedColumns": [{"musicResponsiveListItemFixedColumnRenderer": {"text": {"runs": [{"text": "3:00"}]}}}]
        }});
        let mut found = vec![];
        parse_music(job, &row, &mut found);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].artist,
            "Memory Machine, Last Checkpoint & Full Circle Avenue"
        );
        assert_eq!(found[0].duration_ms, 180000);
        let mut wrong = job.clone();
        wrong.artist = "Outer Reach".into();
        found.clear();
        parse_music(&wrong, &row, &mut found);
        assert!(
            found.is_empty(),
            "album names must not count as artist credits"
        );
    }

    #[test]
    #[ignore = "live searches and downloads for the four reported missing tracks; temporary files only"]
    fn live_reported_tracks_download() {
        let provider = YoutubeMusicAcquire::detect().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let jobs = reported_jobs();
        std::thread::scope(|scope| {
            for job in &jobs {
                let provider = &provider;
                let dest = dir.path();
                scope.spawn(move || {
                    let started = Instant::now();
                    let path = provider
                        .fetch(job, dest)
                        .unwrap_or_else(|e| panic!("{}: {e}", job.title));
                    let decoded = mixless_engine::decode_file(&path).unwrap();
                    let seconds = decoded.frames as f64 / decoded.sample_rate as f64;
                    assert!(mixless_acquire::duration_ok(
                        (seconds * 1000.).round() as u32,
                        job.duration_ms
                    ));
                    eprintln!(
                        "{}: downloaded and decoded {:.2}s in {:.1}s",
                        job.title,
                        seconds,
                        started.elapsed().as_secs_f64()
                    );
                });
            }
        });
    }

    #[test]
    #[ignore = "live Fire Away regression: search, download and full decode into a temporary directory"]
    fn live_fire_away_download() {
        let mut provider = YoutubeMusicAcquire::detect().unwrap();
        let job = ResolveJob {
            spotify_id: "6Mu3CIPbtoPh1kEyBMLRJz".into(),
            title: "Fire Away (feat. Slayyyter) - Frost Children Remix".into(),
            artist: "Madeon, Frost Children, Slayyyter".into(),
            duration_ms: 196800,
            isrc: None,
        };
        let dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let quote =
                |path: &Path| format!("'{}'", path.display().to_string().replace('\'', "'\\''"));
            let trap = dir.path().join("forbidden-tool-calls");
            for tool in ["ffmpeg", "ffprobe"] {
                let path = dir.path().join(tool);
                fs::write(
                    &path,
                    format!("#!/bin/sh\necho {tool} >> {}\nexit 91\n", quote(&trap)),
                )
                .unwrap();
                fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
            }
            let wrapper = dir.path().join("download-only");
            fs::write(
                &wrapper,
                format!(
                    "#!/bin/sh\nexport PATH={}:\"/usr/bin:/bin\"\nexec {} \"$@\"\n",
                    quote(dir.path()),
                    quote(&provider.ytdlp)
                ),
            )
            .unwrap();
            fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
            provider.ytdlp = wrapper;
        }
        let path = provider.fetch(&job, dir.path()).unwrap();
        assert!(
            !dir.path().join("forbidden-tool-calls").exists(),
            "download invoked external ffmpeg/ffprobe"
        );
        let decoded = mixless_engine::decode_file(&path).unwrap();
        let seconds = decoded.frames as f64 / decoded.sample_rate as f64;
        assert!(mixless_acquire::duration_ok(
            (seconds * 1000.).round() as u32,
            job.duration_ms
        ));
        eprintln!(
            "Fire Away: downloaded and decoded {seconds:.3}s; {}",
            fs::read_to_string(path.with_extension("json")).unwrap()
        );
    }

    #[test]
    fn unsafe_ids_are_rejected_before_writing() {
        let provider = YoutubeMusicAcquire {
            ytdlp: "unused".into(),
            deno: None,
        };
        let job = ResolveJob {
            spotify_id: "../../escape".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            duration_ms: 180000,
            isrc: None,
        };
        assert!(provider.fetch(&job, Path::new("/unused")).is_err());
    }
    #[test]
    #[cfg(unix)]
    fn timed_out_sidecar_is_reaped() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-yt-dlp");
        fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let provider = YoutubeMusicAcquire {
            ytdlp: script,
            deno: None,
        };
        let now = Instant::now();
        // Exercise child termination independently of network policy discovery
        // and other tests consuming the shared request budget.
        assert!(provider
            .run_process(&[], now + Duration::from_millis(100), None, "")
            .is_err());
        assert!(now.elapsed() < Duration::from_secs(2));
    }
}
