//! Downloads a Unity editor archive for a given version and extracts every
//! `libil2cpp/**/*.h` file into `headers/<version>/`.
//!
//! Source resolution order (cheapest/cleanest first):
//! 1. Linux `.tar.xz` — pure-Rust streaming extraction. Exists for 2017.4+.
//! 2. macOS `.pkg` — shells out to `xar` + `cpio` (via `bsdtar` if
//!    available). Exists for Unity 5.0 through current, so this is the
//!    fallback for pre-2017.4 legacy versions.
//!
//! If neither archive is present on the CDN the pull is reported as
//! unsupported and the caller skips the version.

mod linux;
mod mac;
mod source;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::releases::Release;

pub use source::{ArchiveSource, SourceKind};

/// Header match predicate shared by every extractor: a path is kept iff it
/// contains the segment `il2cpp/libil2cpp/` and ends in `.h`. Returns the
/// sub-path relative to the `libil2cpp/` root.
pub(crate) fn libil2cpp_header_subpath(path: &Path) -> Option<PathBuf> {
    const PREFIX: &str = "il2cpp/libil2cpp/";
    let s = path.to_str()?;
    if !s.ends_with(".h") {
        return None;
    }
    let idx = s.find(PREFIX)?;
    let rel = &s[idx + PREFIX.len()..];
    if rel.is_empty() {
        return None;
    }
    Some(PathBuf::from(rel))
}

/// Download the Unity editor archive for `release` and extract every
/// matching header into `headers_root/<version>/`.
pub fn pull_release(
    client: &reqwest::blocking::Client,
    release: &Release,
    headers_root: &Path,
) -> Result<PathBuf> {
    let out_dir = headers_root.join(release.version.to_string());
    fs::create_dir_all(&out_dir).with_context(|| format!("creating {}", out_dir.display()))?;

    let source = source::resolve(client, release)?;
    tracing::info!(
        version = %release.version,
        kind = ?source.kind(),
        url = source.url(),
        "extracting",
    );

    let extracted = match source.kind() {
        SourceKind::LinuxTarXz => linux::extract(client, source.url(), &out_dir)?,
        SourceKind::MacPkg => mac::extract(client, source.url(), &out_dir)?,
    };

    if extracted == 0 {
        anyhow::bail!(
            "no libil2cpp/ headers found in archive for {} ({}) — archive layout may have changed",
            release.version,
            source.url(),
        );
    }
    tracing::info!(version = %release.version, files = extracted, "extracted");
    Ok(out_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_linux_editor_archive_layout() {
        let p = Path::new("Editor/Data/il2cpp/libil2cpp/il2cpp-class-internals.h");
        assert_eq!(
            libil2cpp_header_subpath(p).unwrap(),
            PathBuf::from("il2cpp-class-internals.h"),
        );
    }

    #[test]
    fn matches_mac_app_bundle_layout() {
        let p = Path::new("Applications/Unity/Unity.app/Contents/il2cpp/libil2cpp/vm/Object.h");
        assert_eq!(
            libil2cpp_header_subpath(p).unwrap(),
            PathBuf::from("vm/Object.h"),
        );
    }

    #[test]
    fn ignores_cpp_and_other_dirs() {
        assert!(
            libil2cpp_header_subpath(Path::new("Editor/Data/il2cpp/libil2cpp/vm/Object.cpp"))
                .is_none()
        );
        assert!(
            libil2cpp_header_subpath(Path::new("Editor/Data/Mono/include/mono/Object.h")).is_none()
        );
    }
}
