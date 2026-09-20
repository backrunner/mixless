use std::path::{Path, PathBuf};

/// A single, portable folder component below the user's download directory.
/// Keep readable playlist names (including Unicode), without interpreting them
/// as paths. The downloader creates this folder when it needs to fetch audio.
pub fn playlist_download_dir(root: &Path, name: &str) -> PathBuf {
    let mut folder = String::new();
    // Leave room below common 255-byte component limits, even after a prefix.
    for ch in name.trim().chars() {
        let ch = if ch.is_control()
            || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        {
            '_'
        } else {
            ch
        };
        if folder.len() + ch.len_utf8() > 180 {
            break;
        }
        folder.push(ch);
    }
    let folder = folder.trim_matches(|ch: char| ch == '.' || ch.is_whitespace());
    let folder = if folder.is_empty() {
        "Spotify playlist"
    } else {
        folder
    };
    let stem = folder
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix)
                .is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
        });
    root.join(if reserved {
        format!("_{folder}")
    } else {
        folder.to_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_names_become_a_child_of_the_selected_root() {
        let root = Path::new("Downloads").join("Music");
        for name in ["My playlist", "夜行 · Drum & Bass 🎧", "2026.09"] {
            assert_eq!(playlist_download_dir(&root, name), root.join(name));
        }
    }

    #[test]
    fn unusual_names_cannot_escape_the_root_or_create_extra_components() {
        let root = Path::new("Downloads");
        for name in [
            "",
            "  ",
            ".",
            "..",
            "../../outside",
            "/tmp/music",
            "C:\\music\\set",
            "a\0b\nc",
            "<mix>: \"a|b?*",
            "CON",
            "aux.txt",
            "LPT9",
        ] {
            let path = playlist_download_dir(root, name);
            assert_eq!(path.parent(), Some(root), "{name:?}");
            let folder = path.file_name().unwrap().to_str().unwrap();
            assert!(!folder.is_empty());
            assert!(!folder.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*']));
            assert!(!folder.chars().any(char::is_control));
        }
        assert_eq!(
            playlist_download_dir(root, ".."),
            root.join("Spotify playlist")
        );
        assert_eq!(playlist_download_dir(root, "CON"), root.join("_CON"));
        assert_eq!(
            playlist_download_dir(root, "aux.txt"),
            root.join("_aux.txt")
        );
        assert_eq!(playlist_download_dir(root, "  Set...  "), root.join("Set"));
        let long = playlist_download_dir(root, &"音乐🎧".repeat(100));
        assert!(long.file_name().unwrap().to_str().unwrap().len() <= 180);
    }
}
