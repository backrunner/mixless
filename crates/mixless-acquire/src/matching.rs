use crate::{duration_ok, ResolveJob};
use mixless_protocol::Track;
use unicode_normalization::UnicodeNormalization;

pub fn normalize(value: &str) -> String {
    let value = value.nfkc().collect::<String>().to_lowercase();
    let mut depth = 0usize;
    let mut text = String::new();
    for c in value.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            _ => text.push(if c.is_alphanumeric() { c } else { ' ' }),
        }
    }
    text.split_whitespace()
        .take_while(|word| !matches!(*word, "feat" | "ft" | "featuring"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Brackets carry either display decoration/featured credits or recording
/// identity. Keep edition text so `Song - X Remix` equals `Song (X Remix)`.
fn recording_title(value: &str) -> String {
    let mut chunks = Vec::new();
    let mut outside = String::new();
    let mut bracket = String::new();
    let mut depth = 0usize;
    for c in value.chars() {
        match c {
            '(' | '[' => {
                if depth == 0 {
                    chunks.push(normalize(&outside));
                    outside.clear();
                    bracket.clear();
                }
                depth += 1;
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                if depth == 0 && !versions(&bracket).is_empty() {
                    chunks.push(normalize(&bracket));
                }
            }
            _ if depth > 0 => bracket.push(c),
            _ => outside.push(c),
        }
    }
    chunks.push(normalize(&outside));
    chunks
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn versions(value: &str) -> Vec<&'static str> {
    let words = value
        .nfkc()
        .collect::<String>()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>();
    let padded = format!(
        " {} ",
        words.split_whitespace().collect::<Vec<_>>().join(" ")
    );
    [
        "live",
        "remix",
        "mix",
        "slowed",
        "sped",
        "nightcore",
        "cover",
        "karaoke",
        "instrumental",
        "extended",
        "clean",
        "radio edit",
    ]
    .into_iter()
    .filter(|word| padded.contains(&format!(" {word} ")))
    .collect()
}

pub fn local_match(job: &ResolveJob, track: &Track) -> bool {
    if job
        .isrc
        .as_deref()
        .zip(track.isrc.as_deref())
        .is_some_and(|(a, b)| !a.is_empty() && a.eq_ignore_ascii_case(b))
    {
        return true;
    }
    let title = recording_title(&job.title);
    let artist = normalize(&job.artist);
    !title.is_empty()
        && !artist.is_empty()
        && artist != "unknown"
        && title == recording_title(&track.title)
        && artist == normalize(&track.artist)
        && versions(&job.title) == versions(&track.title)
        && job.duration_ms > 0
        && track.duration_ms > 0
        && job.duration_ms.abs_diff(track.duration_ms) <= 2000
}

/// The identity fields `local_match` compares, normalized once per job/track.
/// `could_match` must stay a necessary condition of `local_match` so callers
/// can skip non-candidates without changing matching semantics.
pub struct MatchKey {
    title: String,
    artist: String,
    isrc: Option<String>,
}
pub fn job_key(job: &ResolveJob) -> MatchKey {
    MatchKey {
        title: recording_title(&job.title),
        artist: normalize(&job.artist),
        isrc: job
            .isrc
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(str::to_ascii_lowercase),
    }
}
pub fn track_key(track: &Track) -> MatchKey {
    MatchKey {
        title: recording_title(&track.title),
        artist: normalize(&track.artist),
        isrc: track
            .isrc
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(str::to_ascii_lowercase),
    }
}
/// Fast reject: true whenever `local_match` could return true for the pair.
/// Survivors still go through `local_match` itself.
pub fn could_match(job: &MatchKey, track: &MatchKey) -> bool {
    (job.isrc.is_some() && job.isrc == track.isrc)
        || (job.title == track.title && job.artist == track.artist)
}

/// Metadata must agree before downloading. Duration alone never proves identity.
pub fn candidate_score(
    job: &ResolveJob,
    title: &str,
    artist: &str,
    duration_ms: u32,
) -> Option<f32> {
    if !duration_ok(duration_ms, job.duration_ms)
        || duration_ms > 720_000
        || duration_ms as f64 > job.duration_ms as f64 * 1.25
        || versions(title) != versions(&job.title)
    {
        return None;
    }
    let want_title = recording_title(&job.title);
    // Spotify credits every collaborator, while Topic channels commonly credit
    // only the lead artist. Keep a lead-artist gate instead of demanding the
    // entire comma-separated credit string verbatim.
    let want_artist = normalize(primary_artist(&job.artist));
    let got_title = recording_title(title);
    let got_artist = normalize(artist.trim_end_matches(" - Topic"));
    if want_title.is_empty() || want_artist.is_empty() || want_artist == "unknown" {
        return None;
    }
    let padded = format!(" {got_title} ");
    let title_ok = got_title == want_title || padded.contains(&format!(" {want_title} "));
    let artist_ok = got_artist == want_artist
        || artist
            .replace(" x ", ",")
            .replace(" X ", ",")
            .split([',', ';', '&'])
            .any(|credit| normalize(credit.trim().trim_end_matches(" - Topic")) == want_artist)
        || got_artist
            .strip_suffix("vevo")
            .is_some_and(|a| a.trim() == want_artist)
        || padded.contains(&format!(" {want_artist} "));
    if !title_ok || !artist_ok {
        return None;
    }
    let difference = duration_ms.abs_diff(job.duration_ms) as f32 / job.duration_ms as f32;
    Some(0.45 + 0.25 + 0.30 * (1. - (difference / 0.06).min(1.)))
}

pub fn primary_artist(artist: &str) -> &str {
    artist.split([',', ';']).next().unwrap_or(artist).trim()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn job() -> ResolveJob {
        ResolveJob {
            spotify_id: "a".repeat(22),
            title: "Song".into(),
            artist: "Artist".into(),
            duration_ms: 240000,
            isrc: None,
        }
    }
    #[test]
    fn duration_and_recording_identity_are_hard_gates() {
        let j = job();
        assert!(!duration_ok(246000, 240000));
        assert!(!duration_ok(0, 240000));
        assert!(!duration_ok(240000, 0));
        assert!(
            candidate_score(&j, "Artist - Song (Official Audio)", "ArtistVEVO", 240100).unwrap()
                >= 0.72
        );
        for (title, artist, duration) in [
            ("Song (Live)", "Artist", 240000),
            ("Song", "Cover Band", 240000),
            ("Song", "Artist", 246000),
            ("Song (Slowed)", "Artist", 240000),
            ("Other Song", "Other Artist", 240000),
        ] {
            assert!(candidate_score(&j, title, artist, duration).is_none());
        }
    }
    #[test]
    fn named_remixes_match_across_bracket_and_dash_notation() {
        let mut j = job();
        j.title = "Fire Away (feat. Slayyyter) - Frost Children Remix".into();
        j.artist = "Madeon, Frost Children, Slayyyter".into();
        j.duration_ms = 196800;
        for title in [
            "Fire Away (Frost Children Remix)",
            "Madeon, Frost Children - \"Fire Away (feat. Slayyyter) [Frost Children Remix]\" {Official Visualiser}",
            "Madeon – Fire Away (ft. Slayyyter) [Frost Children Remix] Lyrics",
        ] {
            assert!(
                candidate_score(&j, title, "Madeon", 197000).is_some(),
                "{title}"
            );
        }
        for title in [
            "Fire Away",
            "Fire Away (Other Remix)",
            "Fire Away (Remix)",
            "Fire Away (Frost Children Remix) (Instrumental)",
            "Fire Away (Frost Children Remix) (Live)",
        ] {
            assert!(
                candidate_score(&j, title, "Madeon", 197000).is_none(),
                "{title}"
            );
        }
        assert!(
            candidate_score(&j, "Fire Away (Frost Children Remix)", "Cover Band", 197000).is_none()
        );
        assert!(
            candidate_score(&j, "Fire Away (Frost Children Remix)", "Madeon", 208000).is_none()
        );
        j.title = "Fire Away (Frost Children Remix)".into();
        assert!(candidate_score(&j, "Fire Away - Other Remix", "Madeon", 197000).is_none());
    }

    #[test]
    fn normalization_handles_unicode_and_preserves_edition_checks() {
        assert_eq!(normalize("Ｆｕｌｌ－Ｗｉｄｔｈ (feat. X)"), "full width");
        assert_eq!(normalize("夜曲【】"), "夜曲");
        assert_ne!(versions("Song (Live)"), versions("Song"));
    }

    #[test]
    fn collaboration_credits_match_lead_artist_without_accepting_other_recordings() {
        let mut j = job();
        j.title = "Moments".into();
        j.artist = "MitiS,\u{a0}Adara".into();
        assert!(candidate_score(&j, "Moments (feat. ADARA)", "MitiS - Topic", 240000).is_some());
        assert!(candidate_score(
            &j,
            "MitiS - Moments (Lyrics) ft. Adara",
            "Music channel",
            240000
        )
        .is_some());
        assert!(candidate_score(&j, "Moments", "Adara", 240000).is_none());
        assert!(candidate_score(&j, "Moments (Remix)", "MitiS", 240000).is_none());
        j.title = "Hollow".into();
        j.artist = "Dabin, Kai Wachi, Lø Spirit".into();
        assert!(candidate_score(&j, "Hollow", "Dabin & Kai Wachi", 240000).is_some());
        assert!(candidate_score(
            &j,
            "Dabin x Kai Wachi - Hollow (feat. Lø Spirit)",
            "Label",
            240000
        )
        .is_some());
        assert!(candidate_score(&j, "Hollow (Live)", "Dabin", 240000).is_none());
    }
}
