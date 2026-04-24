//! Resolves which Unity editor archive to download for a given release.
//!
//! We prefer the Linux `.tar.xz` (available for 2017.4.1+) because it is
//! extractable in pure Rust with zero auxiliary tools. We fall back to the
//! macOS `.pkg` (available back to Unity 5.x) when the Linux archive is
//! missing. A `HEAD` probe decides availability rather than baking the cutoff
//! into a version comparison — Unity has been known to omit builds for
//! specific patch releases.

use anyhow::{bail, Context, Result};

use crate::releases::Release;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    LinuxTarXz,
    MacPkg,
}

/// An archive URL paired with its detected format.
#[derive(Clone, Debug)]
pub struct ArchiveSource {
    kind: SourceKind,
    url: String,
}

impl ArchiveSource {
    pub fn kind(&self) -> SourceKind {
        self.kind
    }
    pub fn url(&self) -> &str {
        &self.url
    }
}

/// Returns the first reachable archive for `release`, probing the candidates
/// in preference order via `HEAD` requests.
pub fn resolve(client: &reqwest::blocking::Client, release: &Release) -> Result<ArchiveSource> {
    let candidates = [
        ArchiveSource {
            kind: SourceKind::LinuxTarXz,
            url: format!(
                "https://download.unity3d.com/download_unity/{rev}/LinuxEditorInstaller/Unity.tar.xz",
                rev = release.revision,
            ),
        },
        ArchiveSource {
            kind: SourceKind::MacPkg,
            url: format!(
                "https://download.unity3d.com/download_unity/{rev}/MacEditorInstaller/Unity-{ver}.pkg",
                rev = release.revision,
                ver = release.version,
            ),
        },
    ];

    for c in &candidates {
        if head_ok(client, &c.url)? {
            return Ok(c.clone());
        }
    }
    bail!(
        "no extractable archive found on CDN for {} (revision {})",
        release.version,
        release.revision,
    );
}

fn head_ok(client: &reqwest::blocking::Client, url: &str) -> Result<bool> {
    let resp = client
        .head(url)
        .send()
        .with_context(|| format!("HEAD {url}"))?;
    Ok(resp.status().is_success())
}
