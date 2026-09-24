//! Public playlist pages expose a counted snapshot, unlike the embed preview.
use super::*;
use base64::Engine;

pub(super) fn parse(html: &str, id: &str) -> Result<SpotifyPlaylistMeta, SpotifyError> {
    let incomplete = || {
        SpotifyError::Parse("The complete playlist is unavailable. Nothing was changed; retry when Spotify provides the full playlist.".into())
    };
    let start = html.find("id=\"initialState\"").ok_or_else(incomplete)?;
    let script = html[start..].split_once('>').ok_or_else(incomplete)?.1;
    let encoded = script.split_once("</script>").ok_or_else(incomplete)?.0;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|_| incomplete())?;
    let data: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| incomplete())?;
    let entity = &data["entities"]["items"][format!("spotify:playlist:{id}")];
    if entity["__typename"] != "Playlist" {
        return Err(incomplete());
    }
    let content = &entity["content"];
    let total = content["totalCount"].as_u64().ok_or_else(incomplete)? as usize;
    let items = content["items"].as_array().ok_or_else(incomplete)?;
    if items.len() != total || !content["pagingInfo"]["nextOffset"].is_null() {
        return Err(incomplete());
    }
    let tracks = items
        .iter()
        .map(|row| {
            let t = &row["itemV2"]["data"];
            SpotifyTrackMeta {
                id: if t["__typename"] == "Track" {
                    t["uri"]
                        .as_str()
                        .and_then(|s| s.strip_prefix("spotify:track:"))
                        .unwrap_or("")
                        .into()
                } else {
                    String::new()
                },
                title: t["name"].as_str().unwrap_or("Unavailable track").into(),
                artist: t["artists"]["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|a| a["profile"]["name"].as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                duration_ms: t["duration"]["totalMilliseconds"]
                    .as_u64()
                    .and_then(|n| n.try_into().ok())
                    .unwrap_or(0),
                isrc: None,
            }
        })
        .collect();
    Ok(SpotifyPlaylistMeta {
        id: id.into(),
        name: entity["name"].as_str().ok_or_else(incomplete)?.into(),
        owner: String::new(),
        tracks,
        total_tracks: Some(total),
        warning: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page(total: usize, items: serde_json::Value) -> String {
        let data = serde_json::json!({"entities":{"items":{"spotify:playlist:test":{
            "__typename":"Playlist", "name":"Test", "content":{"totalCount":total,"items":items}
        }}}});
        format!(
            "<script id=\"initialState\" type=\"text/plain\">{}</script>",
            base64::engine::general_purpose::STANDARD.encode(data.to_string())
        )
    }
    #[test]
    fn counted_snapshot_retains_duplicates_and_unavailable_slots() {
        let track = serde_json::json!({"itemV2":{"data":{"__typename":"Track","uri":"spotify:track:abc", "name":"Song","duration":{"totalMilliseconds":197000},"artists":{"items":[{"profile":{"name":"Madeon"}}]}}}});
        let html = page(3, serde_json::json!([track.clone(), null, track]));
        let p = parse(&html, "test").unwrap();
        assert_eq!(p.tracks.len(), 3);
        assert_eq!(p.tracks[0].artist, "Madeon");
        assert_eq!(p.tracks[0].duration_ms, 197000);
        assert!(p.tracks[1].id.is_empty());
        assert_eq!(p.tracks[0].id, p.tracks[2].id);
        assert!(parse(&html, "wrong").is_err());
        assert!(parse(&page(4, serde_json::json!([])), "test").is_err());
        assert!(parse(&page(0, serde_json::json!([])), "test")
            .unwrap()
            .tracks
            .is_empty());
    }
}
