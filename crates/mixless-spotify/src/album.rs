//! Exact recording metadata for imports made from album-less embed previews.
use super::{http_error, SpotifyError};
use base64::Engine;
use std::{io::Read, time::Duration};

#[derive(Default)]
pub struct AlbumClient;

impl AlbumClient {
    pub fn fetch(&self, id: &str) -> Result<Option<String>, SpotifyError> {
        if id.len() != 22 || !id.bytes().all(|c| c.is_ascii_alphanumeric()) {
            return Err(SpotifyError::Parse("invalid track ID".into()));
        }
        let url = format!("https://open.spotify.com/track/{id}");
        let agent =
            mixless_net::agent(&url, Duration::from_secs(15)).map_err(SpotifyError::Http)?;
        let response = match agent
            .get(&url)
            .set("User-Agent", "Mozilla/5.0 Mixless/0.1")
            .call()
        {
            Ok(response) => response,
            Err(ureq::Error::Status(404, _)) => return Ok(None),
            Err(error) => return Err(http_error(error)),
        };
        const LIMIT: u64 = 4 * 1024 * 1024;
        let mut html = String::new();
        response
            .into_reader()
            .take(LIMIT + 1)
            .read_to_string(&mut html)
            .map_err(|e| SpotifyError::Http(e.to_string()))?;
        if html.len() as u64 > LIMIT {
            return Err(SpotifyError::Parse("track metadata is too large".into()));
        }
        parse(&html, id)
    }
}

fn parse(html: &str, id: &str) -> Result<Option<String>, SpotifyError> {
    let invalid = || SpotifyError::Parse("track metadata unavailable".into());
    let start = html.find("id=\"initialState\"").ok_or_else(invalid)?;
    let script = html[start..].split_once('>').ok_or_else(invalid)?.1;
    let encoded = script.split_once("</script>").ok_or_else(invalid)?.0;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|_| invalid())?;
    let state: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let uri = format!("spotify:track:{id}");
    let track = &state["entities"]["items"][&uri];
    if track["__typename"] != "Track" || track["uri"].as_str() != Some(uri.as_str()) {
        return Err(invalid());
    }
    Ok(track["albumOfTrack"]["name"]
        .as_str()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(String::from))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn album_belongs_to_the_exact_recording_not_related_releases() {
        let page = |track| {
            let state = serde_json::json!({"entities":{"items":{
                "spotify:track:exact":track,
                "spotify:track:related":{"__typename":"Track","uri":"spotify:track:related","albumOfTrack":{"name":"Wrong album"}}
            }}});
            format!(
                "<script id=\"initialState\">{}</script>",
                base64::engine::general_purpose::STANDARD.encode(state.to_string())
            )
        };
        let mut track = serde_json::json!({"__typename":"Track","uri":"spotify:track:exact","albumOfTrack":{"name":"  专辑 & Album  "}});
        assert_eq!(
            parse(&page(track.clone()), "exact").unwrap().as_deref(),
            Some("专辑 & Album")
        );
        track["albumOfTrack"] = serde_json::Value::Null;
        assert_eq!(parse(&page(track.clone()), "exact").unwrap(), None);
        track["uri"] = "spotify:track:wrong".into();
        assert!(parse(&page(track), "exact").is_err());
        assert!(parse("<html>unavailable</html>", "exact").is_err());
    }

    #[test]
    #[ignore = "live Spotify metadata"]
    fn live_track_has_album() {
        let album = AlbumClient.fetch("6Mu3CIPbtoPh1kEyBMLRJz").unwrap();
        assert!(album.is_some_and(|name| !name.trim().is_empty()));
    }
}
