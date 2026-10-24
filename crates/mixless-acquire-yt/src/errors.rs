use crate::pacing;
use mixless_acquire::AcquireError;

pub(super) fn stops_retries(error: &AcquireError) -> bool {
    matches!(error, AcquireError::ToolUnavailable(_)) || pacing::is_rate_limit(&error.to_string())
}

pub(super) fn downloader_error(message: &str) -> AcquireError {
    let errors: Vec<_> = message
        .lines()
        .filter(|line| line.trim_start().starts_with("ERROR:"))
        .map(str::to_owned)
        .collect();
    let details = if errors.is_empty() {
        summary(&[message.to_owned()])
    } else {
        summary(&errors)
    };
    AcquireError::Msg(if details.is_empty() {
        "Audio download failed".into()
    } else {
        format!("Audio download failed: {details}")
    })
}

pub(super) fn summary(errors: &[String]) -> String {
    // Format and candidate fallbacks can report the same cause many times.
    // Keep distinct causes, collapse whitespace, and cap the persisted UI text.
    let mut unique = Vec::new();
    for error in errors {
        for part in error.split("; ") {
            let part = part.split_whitespace().collect::<Vec<_>>().join(" ");
            if !part.is_empty() && !unique.contains(&part) {
                unique.push(part);
            }
        }
    }
    let text = unique.join("; ");
    if text.chars().count() <= 420 {
        text
    } else {
        format!("{}…", text.chars().take(419).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_terminal_failures_stop_retries() {
        assert!(stops_retries(&AcquireError::ToolUnavailable(
            "yt-dlp cannot start".into()
        )));
        assert!(!stops_retries(&downloader_error(
            "ERROR: HTTP Error 403: Forbidden"
        )));
        assert!(stops_retries(&downloader_error(
            "ERROR: HTTP Error 429: Too Many Requests"
        )));
    }

    #[test]
    fn repeated_nested_failures_are_deduplicated_and_bounded() {
        let repeated = ["source unavailable\n"; 12].join("; ");
        assert_eq!(
            summary(&[repeated.clone(), repeated, "different failure".into()]),
            "source unavailable; different failure"
        );
        let message =
            downloader_error(&format!("progress\nERROR: {}\n", "错误".repeat(1000))).to_string();
        assert!(!message.contains("progress"));
        assert!(message.chars().count() < 450);
    }
}
