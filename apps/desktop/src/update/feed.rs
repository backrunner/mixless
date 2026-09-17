//! Channel selection and semantic-version policy, independent of installation.
use super::network::get_json;
use semver::Version;
use serde::Deserialize;
use std::{collections::HashMap, env};

#[derive(Deserialize)]
pub(super) struct Manifest {
    pub channel: String,
    pub version: String,
    pub platforms: HashMap<String, Platform>,
}

#[derive(Deserialize)]
pub(super) struct Platform {
    pub url: String,
    pub sha256: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn version_for_channel(version: &str, channel: &str) -> Result<Version, String> {
    let version = Version::parse(version).map_err(|e| format!("Invalid version: {e}"))?;
    let valid = match channel {
        "stable" => version.pre.is_empty(),
        "beta" => version.pre.as_str() == "beta" || version.pre.as_str().starts_with("beta."),
        _ => false,
    };
    if !valid {
        return Err(format!(
            "Version {version} does not belong to the {channel} channel"
        ));
    }
    Ok(version)
}

impl Manifest {
    pub(super) fn is_newer(&self, channel: &str, current: &str) -> Result<bool, String> {
        if self.channel != channel {
            return Err(format!("Update feed channel mismatch: {}", self.channel));
        }
        let remote = version_for_channel(&self.version, channel)?;
        let current = version_for_channel(current, channel)?;
        // Build metadata is not part of SemVer precedence.
        Ok(remote.cmp_precedence(&current).is_gt())
    }

    pub(super) fn platform(&self) -> Result<&Platform, String> {
        let platform = self
            .platforms
            .get("darwin-universal")
            .ok_or("Manifest has no universal macOS build")?;
        if platform.sha256.len() != 64 || !platform.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Manifest has an invalid SHA-256 digest".into());
        }
        Ok(platform)
    }
}

pub(super) fn manifest(channel: &str) -> Result<Manifest, String> {
    if let Some(url) = env::var_os("MIXLESS_UPDATE_MANIFEST_URL") {
        return get_json(&url.to_string_lossy());
    }
    let repo = env!("CARGO_PKG_REPOSITORY");
    let name = format!("mixless-{channel}-latest.json");
    if channel == "stable" {
        return get_json(&format!("{repo}/releases/latest/download/{name}"));
    }
    let path = repo
        .strip_prefix("https://github.com/")
        .ok_or_else(|| format!("Unsupported repository URL: {repo}"))?;
    let mut best: Option<(Version, String)> = None;
    // GitHub orders releases by creation, not semantic version. Older tags
    // published later and pages full of stable releases must not hide a beta.
    for page in 1.. {
        let releases: Vec<Release> = get_json(&format!(
            "https://api.github.com/repos/{path}/releases?per_page=100&page={page}"
        ))?;
        if let Some((version, url)) = beta_manifest(&releases, &name) {
            if best
                .as_ref()
                .is_none_or(|(v, _)| version.cmp_precedence(v).is_gt())
            {
                best = Some((version, url.to_owned()));
            }
        }
        if releases.len() < 100 {
            break;
        }
    }
    let (version, url) = best.ok_or("No beta release manifest found")?;
    let manifest: Manifest = get_json(&url)?;
    if manifest.version != version.to_string() {
        return Err("Beta manifest version does not match its release tag".into());
    }
    Ok(manifest)
}

fn beta_manifest<'a>(releases: &'a [Release], name: &str) -> Option<(Version, &'a str)> {
    releases
        .iter()
        .filter(|r| r.prerelease && !r.draft)
        .filter_map(|r| {
            let version = version_for_channel(r.tag_name.strip_prefix('v')?, "beta").ok()?;
            let url = &r
                .assets
                .iter()
                .find(|a| a.name == name)?
                .browser_download_url;
            Some((version, url.as_str()))
        })
        .max_by(|a, b| a.0.cmp_precedence(&b.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(version: &str, channel: &str) -> Manifest {
        serde_json::from_value(serde_json::json!({
            "version": version, "channel": channel, "platforms": {
                "darwin-universal": {"url": "https://example.test/Mixless.dmg", "sha256": "0".repeat(64)}
            }
        })).unwrap()
    }

    #[test]
    fn versions_never_downgrade_or_cross_channels() {
        assert!(
            manifest("1.0.0-beta.10", "beta")
                .is_newer("beta", "1.0.0-beta.9")
                .unwrap()
        );
        for version in ["1.0.0-beta.8", "1.0.0-beta.9"] {
            assert!(
                !manifest(version, "beta")
                    .is_newer("beta", "1.0.0-beta.9")
                    .unwrap()
            );
        }
        assert!(
            !manifest("1.0.0+new", "stable")
                .is_newer("stable", "1.0.0+old")
                .unwrap()
        );
        assert!(
            manifest("1.1.0", "stable")
                .is_newer("stable", "1.0.0")
                .unwrap()
        );
        for (version, feed, current_channel) in [
            ("1.1.0-beta.1", "stable", "stable"),
            ("1.1.0", "beta", "beta"),
            ("1.1.0", "stable", "beta"),
            ("bad", "stable", "stable"),
        ] {
            assert!(
                manifest(version, feed)
                    .is_newer(current_channel, "1.0.0")
                    .is_err()
            );
        }
        assert!(manifest("1.1.0", "stable").platform().is_ok());
        let mut invalid = manifest("1.1.0", "stable");
        invalid
            .platforms
            .get_mut("darwin-universal")
            .unwrap()
            .sha256 = "abc123".into();
        assert!(invalid.platform().is_err());
    }

    #[test]
    fn beta_feed_uses_version_order_and_skips_drafts_or_partial_uploads() {
        let releases: Vec<Release> = serde_json::from_value(serde_json::json!([
            {"tag_name":"v1.0.0-beta.1","draft":false,"prerelease":true,"assets":[{"name":"feed","browser_download_url":"old"}]},
            {"tag_name":"v1.0.0-beta.10","draft":false,"prerelease":true,"assets":[{"name":"feed","browser_download_url":"new"}]},
            {"tag_name":"v2.0.0-beta.1","draft":true,"prerelease":true,"assets":[{"name":"feed","browser_download_url":"draft"}]},
            {"tag_name":"v3.0.0-beta.1","draft":false,"prerelease":true,"assets":[]},
            {"tag_name":"v9.0.0","draft":false,"prerelease":false,"assets":[{"name":"feed","browser_download_url":"stable"}]}
        ])).unwrap();
        assert_eq!(beta_manifest(&releases, "feed").unwrap().1, "new");
        assert!(beta_manifest(&releases, "missing").is_none());
    }
}
