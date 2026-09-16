//! Read candidate metadata first; download only a recording that passes DJ
//! identity/duration gates. Child processes have one bounded job deadline.
use mixless_acquire::{candidate_score, AcquireError, ResolveJob};
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
        // Reap staging dirs left behind by a force-quit mid-download. The
        // destination may be a user-visible folder, so litter matters.
        for entry in fs::read_dir(&dest).into_iter().flatten().flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(".mixless-")
                && entry.file_type().is_ok_and(|t| t.is_dir())
            {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
        let final_path = dest.join(format!("{}.m4a", job.spotify_id));
        // The importer revalidates the actual decoder duration even on reuse.
        if final_path.is_file() && fs::metadata(&final_path).is_ok_and(|m| m.len() > 0) {
            return Ok(final_path);
        }
        let deadline = Instant::now() + Duration::from_secs(90);
        let mut candidates = self.music_search(job).unwrap_or_default();
        if candidates.iter().all(|c| c.score < 0.72) {
            candidates.extend(self.youtube_search(job, deadline)?);
        }
        candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
        let chosen=candidates.into_iter().find(|c|c.score>=0.72).ok_or_else(||err("no candidate matches artist, recording version and duration; link a local file or retry"))?;
        let staging = tempfile::Builder::new()
            .prefix(".mixless-")
            .tempdir_in(&dest)
            .map_err(|e| err(e.to_string()))?;
        let template = staging.path().join("audio.%(ext)s");
        let mut args = self.base_args();
        args.extend(
            [
                "--no-playlist",
                "--no-progress",
                "--no-warnings",
                "-f",
                "bestaudio[ext=m4a]/bestaudio",
                "-x",
                "--audio-format",
                "m4a",
                "--ffmpeg-location",
            ]
            .map(OsString::from),
        );
        args.push(self.ffmpeg.clone().into_os_string());
        args.extend(["--print", "after_move:filepath", "-o"].map(OsString::from));
        args.push(template.into_os_string());
        args.push(format!("https://www.youtube.com/watch?v={}", chosen.id).into());
        let output = self.run(&args, deadline)?;
        let printed = String::from_utf8_lossy(&output);
        let path = printed
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| err("downloader returned no audio path"))?;
        let path = path
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
        Ok(final_path)
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
        args.push(format!("ytsearch5:{} {}", job.artist, job.title).into());
        let bytes = self.run(&args, deadline)?;
        let data: Value = serde_json::from_slice(&bytes)
            .map_err(|e| err(format!("invalid search metadata: {e}")))?;
        Ok(parse_youtube(job, &data))
    }
    fn music_search(&self, job: &ResolveJob) -> Result<Vec<Candidate>, AcquireError> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(15))
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
        let mut stdout = tempfile::tempfile().map_err(|e| err(e.to_string()))?;
        let mut stderr = tempfile::tempfile().map_err(|e| err(e.to_string()))?;
        let mut command = Command::new(&self.ytdlp);
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
            let artist = subtitle.first().map(String::as_str).unwrap_or("");
            let length = subtitle.iter().find_map(|s| duration(s));
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
