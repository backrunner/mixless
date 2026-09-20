//! Browse-only Spotify metadata. Public embeds are previews; authenticated API
//! reads follow pagination and retain unavailable entries. No audio bytes.
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod artwork;
pub use artwork::ArtworkClient;

#[derive(Debug, Error)]
pub enum SpotifyError {
    #[error("Use a Spotify playlist URL, spotify:playlist: URI, or 22-character playlist ID")]
    Url,
    #[error("Spotify request failed: {0}")]
    Http(String),
    #[error("Spotify rate limited; retry after {0} seconds")]
    RateLimited(u64),
    #[error("Spotify playlist unavailable: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyTrackMeta {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: u32,
    #[serde(default)]
    pub isrc: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyPlaylistMeta {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub tracks: Vec<SpotifyTrackMeta>,
    /// None means an embed does not disclose the full playlist count.
    #[serde(default)]
    pub total_tracks: Option<usize>,
    #[serde(default)]
    pub warning: Option<String>,
}

#[derive(Default)]
pub struct SpotifyClient {
    access_token: Option<String>,
}
impl SpotifyClient {
    pub fn new() -> Self {
        Self::default()
    }
    /// Token ownership/storage belongs to the host's OAuth integration.
    pub fn with_access_token(token: String) -> Self {
        Self {
            access_token: Some(token),
        }
    }
    pub fn fetch_playlist(&self, input: &str) -> Result<SpotifyPlaylistMeta, SpotifyError> {
        let id = extract_playlist_id(input).ok_or(SpotifyError::Url)?;
        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(25))
            .build();
        if let Some(token) = &self.access_token {
            return fetch_api(&id, |url| {
                // Never forward a bearer token to a host from an untrusted next URL.
                if !url.starts_with("https://api.spotify.com/v1/") {
                    return Err(SpotifyError::Parse("invalid pagination URL".into()));
                }
                let response = agent
                    .get(url)
                    .set("Authorization", &format!("Bearer {token}"))
                    .call()
                    .map_err(http_error)?;
                serde_json::from_reader(response.into_reader())
                    .map_err(|e| SpotifyError::Parse(e.to_string()))
            });
        }
        let html = agent
            .get(&format!("https://open.spotify.com/embed/playlist/{id}"))
            .set("User-Agent", "Mozilla/5.0 Mixless/0.1")
            .call()
            .map_err(http_error)?
            .into_string()
            .map_err(|e| SpotifyError::Http(e.to_string()))?;
        parse_embed(&html, &id)
    }
}
fn http_error(error: ureq::Error) -> SpotifyError {
    let message = match error {
        ureq::Error::Status(401, _) => "login expired; reconnect Spotify".into(),
        ureq::Error::Status(403 | 404, _) => {
            "playlist is private, unavailable, or not accessible to this account".into()
        }
        ureq::Error::Status(429, response) => {
            return SpotifyError::RateLimited(
                response
                    .header("Retry-After")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(60),
            );
        }
        other => other.to_string(),
    };
    SpotifyError::Http(message)
}

pub fn extract_playlist_id(input: &str) -> Option<String> {
    let input = input.trim();
    let valid = |id: &str| id.len() == 22 && id.bytes().all(|c| c.is_ascii_alphanumeric());
    if valid(input) {
        return Some(input.into());
    }
    if let Some(id) = input.strip_prefix("spotify:playlist:") {
        return valid(id).then(|| id.into());
    }
    let url = url::Url::parse(input).ok()?;
    if url.scheme() != "https" || url.host_str() != Some("open.spotify.com") {
        return None;
    }
    let mut parts: Vec<_> = url.path_segments()?.filter(|p| !p.is_empty()).collect();
    if parts.first().is_some_and(|p| p.starts_with("intl-")) {
        parts.remove(0);
    }
    if parts.first() == Some(&"embed") {
        parts.remove(0);
    }
    (parts.len() == 2 && parts[0] == "playlist" && valid(parts[1])).then(|| parts[1].into())
}

fn parse_embed(html: &str, fallback_id: &str) -> Result<SpotifyPlaylistMeta, SpotifyError> {
    let id_pos = html.find("id=\"__NEXT_DATA__\"").or_else(||html.find("id='__NEXT_DATA__'"))
        .ok_or_else(||SpotifyError::Parse("public preview not available; check playlist visibility or use authenticated access".into()))?;
    let start = html[id_pos..]
        .find('>')
        .ok_or_else(|| SpotifyError::Parse("invalid page".into()))?
        + id_pos
        + 1;
    let end = html[start..]
        .find("</script>")
        .ok_or_else(|| SpotifyError::Parse("incomplete page".into()))?
        + start;
    let value: serde_json::Value =
        serde_json::from_str(&html[start..end]).map_err(|e| SpotifyError::Parse(e.to_string()))?;
    let props = &value["props"]["pageProps"];
    if props["status"].as_u64().is_some_and(|s| s >= 400) {
        return Err(SpotifyError::Parse(format!(
            "{}; retry later or check that the playlist is public",
            props["title"].as_str().unwrap_or("public page error")
        )));
    }
    let entity = &props["state"]["data"]["entity"];
    let list = entity["trackList"]
        .as_array()
        .ok_or_else(|| SpotifyError::Parse("public preview has no track listing".into()))?;
    let mut tracks = Vec::new();
    for track in list {
        let uri = track["uri"].as_str().unwrap_or("");
        // Keep unavailable rows instead of silently shortening the playlist.
        let id = uri.strip_prefix("spotify:track:").unwrap_or("");
        tracks.push(SpotifyTrackMeta {
            id: id.into(),
            title: track["title"]
                .as_str()
                .unwrap_or("Unavailable track")
                .into(),
            artist: track["subtitle"].as_str().unwrap_or("Unknown").into(),
            duration_ms: track["duration"]
                .as_u64()
                .and_then(|v| v.try_into().ok())
                .unwrap_or(0),
            isrc: None,
        });
    }
    let total_tracks = entity["trackCount"].as_u64().map(|n| n as usize);
    let warning = match total_tracks {
        Some(total) if total > tracks.len() => Some(format!(
            "Public preview lists {} of {} tracks; the rest are only available with Spotify login.",
            tracks.len(),
            total
        )),
        _ => Some("Public Spotify preview: the full playlist count is not verified. Private playlists require login.".into()),
    };
    Ok(SpotifyPlaylistMeta {
        id: fallback_id.into(),
        name: entity["name"].as_str().unwrap_or("Spotify playlist").into(),
        owner: entity["subtitle"].as_str().unwrap_or("").into(),
        tracks,
        total_tracks,
        warning,
    })
}

fn fetch_api(
    id: &str,
    mut get: impl FnMut(&str) -> Result<serde_json::Value, SpotifyError>,
) -> Result<SpotifyPlaylistMeta, SpotifyError> {
    let meta = get(&format!("https://api.spotify.com/v1/playlists/{id}"))?;
    let mut page = if meta["tracks"].is_object() {
        meta["tracks"].clone()
    } else {
        meta["items"].clone()
    };
    if !page["items"].is_array() {
        page = get(&format!(
            "https://api.spotify.com/v1/playlists/{id}/items?limit=50"
        ))?;
    }
    let total = page["total"]
        .as_u64()
        .ok_or_else(|| SpotifyError::Parse("missing playlist count".into()))?
        as usize;
    let mut tracks = Vec::new();
    let mut seen = std::collections::HashSet::new();
    loop {
        let items = page["items"]
            .as_array()
            .ok_or_else(|| SpotifyError::Parse("missing playlist page".into()))?;
        for item in items {
            let t = if item["track"].is_object() {
                &item["track"]
            } else {
                &item["item"]
            };
            tracks.push(SpotifyTrackMeta {
                id: if t["type"].as_str() == Some("track") {
                    t["id"].as_str().unwrap_or("").into()
                } else {
                    String::new()
                },
                title: t["name"].as_str().unwrap_or("Unavailable track").into(),
                artist: t["artists"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v["name"].as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default(),
                duration_ms: t["duration_ms"]
                    .as_u64()
                    .and_then(|v| v.try_into().ok())
                    .unwrap_or(0),
                isrc: t["external_ids"]["isrc"].as_str().map(String::from),
            });
        }
        let Some(next) = page["next"].as_str() else {
            break;
        };
        if !next.starts_with("https://api.spotify.com/v1/")
            || !seen.insert(next.to_string())
            || tracks.len() > total
        {
            return Err(SpotifyError::Parse("invalid pagination".into()));
        }
        page = get(next)?;
    }
    if tracks.len() != total {
        return Err(SpotifyError::Parse(
            "playlist changed or pagination is incomplete; retry import".into(),
        ));
    }
    Ok(SpotifyPlaylistMeta {
        id: id.into(),
        name: meta["name"].as_str().unwrap_or("Spotify playlist").into(),
        owner: meta["owner"]["display_name"].as_str().unwrap_or("").into(),
        tracks,
        total_tracks: Some(total),
        warning: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn playlist_inputs_are_scoped_to_spotify() {
        for input in [
            "0bjQ85Dl4WPRHyWzsJNBA1",
            "spotify:playlist:0bjQ85Dl4WPRHyWzsJNBA1",
            "https://open.spotify.com/playlist/0bjQ85Dl4WPRHyWzsJNBA1?si=abc",
            "https://open.spotify.com/intl-zh/playlist/0bjQ85Dl4WPRHyWzsJNBA1",
        ] {
            assert!(extract_playlist_id(input).is_some());
        }
        for input in [
            "https://evil.test/playlist/0bjQ85Dl4WPRHyWzsJNBA1",
            "https://open.spotify.com/track/0bjQ85Dl4WPRHyWzsJNBA1",
            "spotify:playlist:bad",
        ] {
            assert!(extract_playlist_id(input).is_none());
        }
    }
    #[test]
    fn public_preview_reports_errors_and_unknown_completeness() {
        let page = r#"<script type="application/json" id="__NEXT_DATA__">{"props":{"pageProps":{"state":{"data":{"entity":{"name":"Example","trackList":[{"uri":"spotify:track:0bjQ85Dl4WPRHyWzsJNBA1","title":"Song","subtitle":"Artist","duration":123000}]}}}}}}</script>"#;
        let p = parse_embed(page, "id").unwrap();
        assert_eq!(p.tracks.len(), 1);
        assert!(p.warning.is_some());
        let error = r#"<script id="__NEXT_DATA__">{"props":{"pageProps":{"status":500,"title":"Page not available"}}}</script>"#;
        assert!(parse_embed(error, "id")
            .unwrap_err()
            .to_string()
            .contains("Page not available"));
    }
    #[test]
    fn api_follows_pages_and_retains_unavailable_rows() {
        let mut calls = 0;
        let p = fetch_api("id", |_| { calls+=1; Ok(if calls==1 { serde_json::json!({"name":"List","tracks":{"items":[{"track":{"type":"track","id":"a","name":"Song","duration_ms":120000}}],"total":2,"next":"https://api.spotify.com/v1/page2"}}) } else { serde_json::json!({"items":[{"track":null}],"total":2,"next":null}) }) }).unwrap();
        assert_eq!(calls, 2);
        assert_eq!(p.tracks.len(), 2);
        assert!(p.tracks[1].id.is_empty());
        assert!(p.warning.is_none());
    }
    #[test]
    #[ignore = "live Spotify service smoke test; run explicitly"]
    fn fetch_public_playlist() {
        let p = SpotifyClient::new()
            .fetch_playlist("0bjQ85Dl4WPRHyWzsJNBA1")
            .unwrap();
        assert!(!p.tracks.is_empty());
        assert!(p.warning.is_some());
    }
}
