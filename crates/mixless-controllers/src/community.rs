//! Portable author information and explicit adapter capabilities. No local paths or device IDs.
use crate::{error, ControllerPreset, DeviceMatcher, MidiError, MidiTarget};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};

pub const ADAPTER_API_VERSION: u32 = 1;
pub const MAX_MODULE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommunityInfo {
    /// Stable publisher-chosen ID, separate from the user's local preset ID.
    pub package_id: String,
    pub package_version: String,
    pub minimum_host_version: String,
    pub author: String,
    /// SPDX expression or a LicenseRef with the full license in license_text.
    pub license: String,
    pub license_text: String,
    pub source_url: String,
    #[serde(default)]
    pub tested_on: Vec<HardwareEvidence>,
}

/// Publisher claims, never promoted to the host's hardware validation status on import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardwareEvidence {
    pub model: String,
    pub firmware: String,
    pub os: String,
    pub evidence_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterDefinition {
    pub api_version: u32,
    pub wasm_base64: String,
    pub blake3: String,
    /// Stable per-package indices, independent of the host's catalogue ordering.
    pub controls: Vec<MidiTarget>,
    #[serde(default)]
    pub reports: Vec<ReportPermission>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportAccess {
    Output,
    FeatureWrite,
    FeatureRead,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportPermission {
    pub endpoint: String,
    pub access: ReportAccess,
    pub report_id: u8,
    /// Includes the report-ID byte, including zero for unnumbered reports.
    pub length: u16,
}

impl AdapterDefinition {
    pub fn set_module(&mut self, bytes: &[u8]) {
        self.wasm_base64 = STANDARD.encode(bytes);
        self.blake3 = blake3::hash(bytes).to_hex().to_string();
    }

    pub fn module_bytes(&self) -> Result<Vec<u8>, MidiError> {
        if self.wasm_base64.len() > MAX_MODULE_BYTES.div_ceil(3) * 4 {
            return Err(error("Adapter exceeds 256 KiB"));
        }
        let bytes = STANDARD.decode(&self.wasm_base64).map_err(error)?;
        if bytes.len() > MAX_MODULE_BYTES || !bytes.starts_with(b"\0asm\x01\0\0\0") {
            return Err(error("Expected a WebAssembly 1 binary"));
        }
        if blake3::hash(&bytes).to_hex().as_str() != self.blake3 {
            return Err(error("Adapter checksum does not match"));
        }
        Ok(bytes)
    }
}

pub(crate) fn validate(p: &ControllerPreset) -> Result<(), MidiError> {
    if let Some(c) = &p.community {
        if c.package_id.is_empty()
            || c.package_id.len() > 100
            || !c
                .package_id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-_".contains(&b))
            || c.package_version.len() > 100
            || c.minimum_host_version.len() > 100
            || c.author.trim().is_empty()
            || c.author.len() > 200
            || c.license.trim().is_empty()
            || c.license.len() > 200
            || c.license_text.trim().is_empty()
            || c.license_text.len() > 64 * 1024
            || !url(&c.source_url)
            || c.tested_on.len() > 32
        {
            return Err(error(
                "Community preset needs a package ID, author, license and source URL",
            ));
        }
        semver::Version::parse(&c.package_version).map_err(error)?;
        let minimum = semver::Version::parse(&c.minimum_host_version).map_err(error)?;
        let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).map_err(error)?;
        if current < minimum {
            return Err(error(format!(
                "This preset requires mixless {} or later",
                c.minimum_host_version
            )));
        }
        if c.tested_on.iter().any(|e| {
            [&e.model, &e.firmware, &e.os]
                .into_iter()
                .any(|v| v.trim().is_empty() || v.len() > 200)
                || !url(&e.evidence_url)
        }) {
            return Err(error(
                "Hardware evidence needs a model, firmware, OS and source URL",
            ));
        }
        if p.provenance.is_empty()
            || p.provenance.iter().any(|s| {
                [&s.url, &s.revision, &s.section]
                    .into_iter()
                    .any(|v| v.trim().is_empty())
            })
        {
            return Err(error(
                "Community presets must document their independent protocol sources",
            ));
        }
    }
    if let Some(a) = &p.adapter {
        if p.community.is_none() || a.api_version != ADAPTER_API_VERSION {
            return Err(error(
                "Adapter needs community metadata and supported API version 1",
            ));
        }
        if p.endpoints.is_empty()
            || p.endpoints
                .iter()
                .any(|e| !matches!(e.matcher, DeviceMatcher::Hid { .. }))
        {
            return Err(error("Adapter API 1 requires explicit HID endpoints"));
        }
        // One decoder owns the device; mixed declarative/script controls could double-trigger holds.
        if !p.bindings.is_empty()
            || !p.feedback.is_empty()
            || !p.initialize.is_empty()
            || !p.shutdown.is_empty()
        {
            return Err(error("Adapter presets must implement their input, feedback and initialization in the adapter"));
        }
        if a.controls.len() > 256
            || a.reports.len() > 128
            || a.controls.iter().any(|t| !MidiTarget::all().contains(t))
            || a.controls
                .iter()
                .enumerate()
                .any(|(i, t)| a.controls[..i].contains(t))
        {
            return Err(error("Invalid adapter control list"));
        }
        for r in &a.reports {
            if !(1..=1024).contains(&r.length) || !p.endpoints.iter().any(|e| e.id == r.endpoint) {
                return Err(error("Invalid adapter report permission"));
            }
        }
        a.module_bytes()?;
    }
    Ok(())
}

fn url(s: &str) -> bool {
    s.starts_with("https://")
        && s.len() > 8
        && s.len() <= 2048
        && !s.chars().any(char::is_whitespace)
}

/// Covers all portable content except the installation ID and host verification label.
/// Imports get a new local ID, but retain this identity for replay and version pinning.
pub fn content_digest(p: &ControllerPreset) -> String {
    let mut portable = p.clone();
    portable.id.clear();
    portable.verification = crate::Verification::Protocol;
    blake3::hash(&serde_json::to_vec(&portable).expect("serializable preset"))
        .to_hex()
        .to_string()
}
