//! Exact-track artwork via Spotify's public oEmbed endpoint. No login or audio.
use std::{io::Read, time::Duration};

use crate::{http_error, SpotifyError};

const MAX_METADATA: u64 = 256 * 1024;
const MAX_IMAGE: u64 = 8 * 1024 * 1024;

pub struct ArtworkClient {
    agent: ureq::Agent,
}

impl Default for ArtworkClient {
    fn default() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(15))
                .redirects(0)
                .user_agent("Mixless/0.1")
                .build(),
        }
    }
}

impl ArtworkClient {
    /// The host validates/decodes these bounded image bytes before caching them.
    pub fn fetch(&self, track_id: &str) -> Result<Option<Vec<u8>>, SpotifyError> {
        if track_id.len() != 22 || !track_id.bytes().all(|c| c.is_ascii_alphanumeric()) {
            return Err(SpotifyError::Parse("invalid artwork track ID".into()));
        }
        let response = match self
            .agent
            .get("https://open.spotify.com/oembed")
            .query("url", &format!("https://open.spotify.com/track/{track_id}"))
            .call()
        {
            Ok(response) => response,
            Err(ureq::Error::Status(404, _)) => return Ok(None),
            Err(error) => return Err(http_error(error)),
        };
        let metadata = read_bounded(response.into_reader(), MAX_METADATA)?;
        let value = serde_json::from_slice(&metadata)
            .map_err(|e| SpotifyError::Parse(format!("artwork metadata: {e}")))?;
        let Some(url) = artwork_url(&value)? else {
            return Ok(None);
        };
        let response = self.agent.get(&url).call().map_err(http_error)?;
        read_bounded(response.into_reader(), MAX_IMAGE).map(Some)
    }
}

fn artwork_url(value: &serde_json::Value) -> Result<Option<String>, SpotifyError> {
    let Some(raw) = value["thumbnail_url"].as_str().filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let url =
        url::Url::parse(raw).map_err(|_| SpotifyError::Parse("invalid artwork URL".into()))?;
    // Only fetch Spotify-hosted covers, even if metadata is malformed. The
    // image request carries no authorization header and cannot follow redirects.
    let spotify_image_host = matches!(
        url.host_str(),
        Some("i.scdn.co" | "image-cdn-ak.spotifycdn.com" | "image-cdn-fa.spotifycdn.com")
    );
    if url.scheme() != "https"
        || !spotify_image_host
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || !url.path().starts_with("/image/")
    {
        return Err(SpotifyError::Parse("untrusted artwork URL".into()));
    }
    Ok(Some(url.into()))
}

fn read_bounded(reader: impl Read, limit: u64) -> Result<Vec<u8>, SpotifyError> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| SpotifyError::Http(e.to_string()))?;
    if bytes.len() as u64 > limit {
        return Err(SpotifyError::Parse("artwork response is too large".into()));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oembed_cover_is_optional_and_scoped_to_spotify_images() {
        let url = "https://i.scdn.co/image/ab67616d0000b273example";
        assert_eq!(
            artwork_url(&serde_json::json!({"thumbnail_url":url}))
                .unwrap()
                .as_deref(),
            Some(url)
        );
        for host in ["image-cdn-ak.spotifycdn.com", "image-cdn-fa.spotifycdn.com"] {
            let url = format!("https://{host}/image/ab67616d00001e025b31b214d1920f97f1f35557");
            assert_eq!(
                artwork_url(&serde_json::json!({"thumbnail_url":url})).unwrap(),
                Some(url)
            );
        }
        for value in [
            serde_json::json!({}),
            serde_json::json!({"thumbnail_url":null}),
        ] {
            assert!(artwork_url(&value).unwrap().is_none());
        }
        for url in [
            "http://i.scdn.co/image/a",
            "https://example.com/image/a",
            "file:///etc/passwd",
            "https://i.scdn.co@localhost/image/a",
            "https://i.scdn.co:8443/image/a",
        ] {
            assert!(artwork_url(&serde_json::json!({"thumbnail_url":url})).is_err());
        }
    }

    #[test]
    fn artwork_response_size_is_bounded() {
        assert_eq!(read_bounded(&b"1234"[..], 4).unwrap(), b"1234");
        assert!(read_bounded(&b"12345"[..], 4).is_err());
        assert!(ArtworkClient::default().fetch("invalid/id").is_err());
    }
}
