//! Read candidate metadata first; download only a recording that passes DJ
//! identity/duration gates. Child processes have one bounded job deadline.
use mixless_acquire::{candidate_score, primary_artist, AcquireError, ResolveJob};
use serde_json::Value;
use std::{
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub struct YoutubeMusicAcquire {
    ytdlp: PathBuf,
    ffmpeg: PathBuf,
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

fn locate(name: &str) -> Option<PathBuf> {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join(name)));
    beside
        .filter(|p| p.is_file())
        .or_else(|| which::which(name).ok())
        .or_else(|| {
            ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"]
                .iter()
                .map(|dir| Path::new(dir).join(name))
                .find(|p| p.is_file())
        })
}
impl YoutubeMusicAcquire {
    pub fn detect() -> Result<Self, AcquireError> {
        Ok(Self {
            ytdlp: locate("yt-dlp")
                .ok_or_else(|| err("yt-dlp is missing. Install yt-dlp and ffmpeg, then retry."))?,
            ffmpeg: locate("ffmpeg")
                .ok_or_else(|| err("ffmpeg is missing. Install ffmpeg, then retry."))?,
            deno: locate("deno"),
        })
    }
    pub fn fetch(&self, job: &ResolveJob, dest: &Path) -> Result<PathBuf, AcquireError> {
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
        let final_path = dest.join(format!("{}.m4a", job.spotify_id));
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
                Err(error) => errors.push(error.to_string()),
            }
        }
        rank_candidates(&mut candidates);
        let mut attempted = std::collections::HashSet::new();
        for pass in 0..2 {
            if let Some(path) = try_candidates(&candidates, &mut attempted, &mut errors, |chosen| {
                let attempt_deadline = deadline.min(Instant::now() + Duration::from_secs(45));
                self.download(job, chosen, &dest, &final_path, attempt_deadline)
            }) {
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
                format!("Audio search failed: {}", errors.join("; "))
            }))
        } else {
            Err(err(format!(
                "Matching audio could not be downloaded: {}",
                errors.join("; ")
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
                Err(error) => errors.push(error.to_string()),
            }
        }
        Err(err(if errors.is_empty() {
            "Audio download timed out".into()
        } else {
            errors.join("; ")
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
                "4",
                "-f",
                format,
                "-x",
                "--audio-format",
                "m4a",
                "--ffmpeg-location",
            ]
            .map(OsString::from),
        );
        args.push(self.ffmpeg.clone().into_os_string());
        args.extend(["--print", "after_move:filepath", "-o"].map(OsString::from));
        // Keep playlist names out of yt-dlp's template/environment expansion.
        // Names such as "100%", "%(title)s" and "$HOME" stay literal folders.
        args.push("audio.%(ext)s".into());
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
        if !path.starts_with(&root)
            || path.extension().and_then(|s| s.to_str()) != Some("m4a")
            || !fs::metadata(&path).is_ok_and(|m| m.len() > 0)
        {
            return Err(err("downloader returned an invalid audio file"));
        }
        // Stable remote IDs avoid non-ASCII title collisions and prefix matches
        // accidentally loading a thumbnail or JSON sidecar as audio. Staging
        // lives inside `dest`, so rename stays on one volume and also works on
        // filesystems without hard links (exFAT/FAT32 removable drives).
        fs::rename(&path, &final_path)
            .or_else(|_| fs::hard_link(&path, &final_path))
            .map_err(|e| err(format!("publish audio: {e}")))?;
        let provenance = serde_json::json!({"spotify_id":job.spotify_id,"source_url":format!("https://www.youtube.com/watch?v={}",chosen.id),"title":chosen.title,"artist":chosen.artist,"duration_ms":chosen.duration_ms,"score":chosen.score});
        let _ = fs::write(
            dest.join(format!("{}.json", job.spotify_id)),
            provenance.to_string(),
        );
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
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(12))
            .build();
        let response=agent.post("https://music.youtube.com/youtubei/v1/search?prettyPrint=false")
            .send_json(serde_json::json!({"context":{"client":{"clientName":"WEB_REMIX","clientVersion":"1.20260819.01.00","hl":"en"}},"query":format!("{} {}",job.artist,job.title),"params":"EgWKAQIIAWoKEAkQBRAKEAMQBA%3D%3D"}))
            .map_err(|e|err(format!("YouTube Music search: {e}")))?;
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
        if Instant::now() >= deadline {
            return Err(err("Audio search/download timed out"));
        }
        let mut stdout = tempfile::tempfile().map_err(|e| err(e.to_string()))?;
        let mut stderr = tempfile::tempfile().map_err(|e| err(e.to_string()))?;
        let mut command = Command::new(&self.ytdlp);
        if let Some(dir) = working_dir {
            command.current_dir(dir);
        }
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(stdout.try_clone().map_err(|e| err(e.to_string()))?)
            .stderr(stderr.try_clone().map_err(|e| err(e.to_string()))?);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command
            .spawn()
            .map_err(|e| err(format!("start downloader: {e}")))?;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(error) => {
                    terminate(&mut child);
                    return Err(err(error.to_string()));
                }
            }
            if Instant::now() >= deadline
                || stdout.metadata().is_ok_and(|m| m.len() > 8 * 1024 * 1024)
                || stderr.metadata().is_ok_and(|m| m.len() > 8 * 1024 * 1024)
            {
                terminate(&mut child);
                return Err(err(
                    "audio search/download timed out or returned excessive output; retry later",
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        use std::io::{Seek, SeekFrom};
        stdout
            .seek(SeekFrom::Start(0))
            .map_err(|e| err(e.to_string()))?;
        stderr
            .seek(SeekFrom::Start(0))
            .map_err(|e| err(e.to_string()))?;
        if !status.success() {
            let mut message = String::new();
            let _ = stderr.take(8192).read_to_string(&mut message);
            return Err(err(format!(
                "downloader failed: {}",
                message.chars().take(1200).collect::<String>()
            )));
        }
        let mut bytes = Vec::new();
        stdout
            .take(8 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .map_err(|e| err(e.to_string()))?;
        Ok(bytes)
    }
}
fn try_candidates(
    candidates: &[Candidate],
    attempted: &mut std::collections::HashSet<String>,
    errors: &mut Vec<String>,
    mut download: impl FnMut(&Candidate) -> Result<PathBuf, AcquireError>,
) -> Option<PathBuf> {
    let choices: Vec<_> = candidates
        .iter()
        .filter(|c| c.score >= 0.72 && !attempted.contains(&c.id))
        .take(3)
        .collect();
    for chosen in choices {
        attempted.insert(chosen.id.clone());
        match download(chosen) {
            Ok(path) => return Some(path),
            Err(error) => errors.push(error.to_string()),
        }
    }
    None
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

fn terminate(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
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
            ffmpeg: "unused".into(),
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
        assert!(first.is_none());
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
        assert_eq!(next, Some(PathBuf::from("audio.m4a")));
        assert_eq!(calls, ["00000000003", "00000000004"]);
    }

    #[test]
    #[cfg(unix)]
    fn unavailable_aac_uses_an_alternate_format_and_cleans_staging() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-yt-dlp");
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
[ "$template" = 'audio.%(ext)s' ] || exit 2
printf 'fixture audio' > "$path"
printf '%s\n' "$path"
"#,
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let provider = YoutubeMusicAcquire {
            ytdlp: script,
            ffmpeg: "unused".into(),
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
        assert_eq!(fs::read(&path).unwrap(), b"fixture audio");
        assert!(dest.join(format!("{}.json", job.spotify_id)).is_file());
        assert_eq!(
            fs::read_to_string(dir.path().join("formats"))
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            ["bestaudio[ext=m4a]/bestaudio", "bestaudio[ext=webm]"]
        );
        assert!(fs::read_dir(&dest).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".mixless-")));
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
                    let probe = Command::new(provider.ffmpeg.with_file_name("ffprobe"))
                        .args([
                            "-v",
                            "error",
                            "-show_entries",
                            "format=duration",
                            "-of",
                            "default=nw=1:nk=1",
                        ])
                        .arg(&path)
                        .output()
                        .unwrap();
                    assert!(probe.status.success());
                    let seconds: f64 = String::from_utf8_lossy(&probe.stdout)
                        .trim()
                        .parse()
                        .unwrap();
                    assert!(
                        mixless_acquire::duration_ok(
                            (seconds * 1000.).round() as u32,
                            job.duration_ms
                        ),
                        "{}: {seconds}s",
                        job.title
                    );
                    let decode = Command::new(&provider.ffmpeg)
                        .args(["-v", "error", "-nostdin", "-i"])
                        .arg(&path)
                        .args(["-f", "null", "-"])
                        .output()
                        .unwrap();
                    assert!(
                        decode.status.success(),
                        "{}: {}",
                        job.title,
                        String::from_utf8_lossy(&decode.stderr)
                    );
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
    fn unsafe_ids_are_rejected_before_writing() {
        let provider = YoutubeMusicAcquire {
            ytdlp: "unused".into(),
            ffmpeg: "unused".into(),
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
            ffmpeg: "unused".into(),
            deno: None,
        };
        let now = Instant::now();
        assert!(provider.run(&[], now + Duration::from_millis(100)).is_err());
        assert!(now.elapsed() < Duration::from_secs(2));
    }
}
