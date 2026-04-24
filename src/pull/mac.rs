//! Legacy extractor for Unity macOS `.pkg` archives.
//!
//! Unity's macOS editor installer is a flat `.pkg` (XAR archive) containing
//! one or more component packages. Each component has a `Payload` file that
//! is a gzip-compressed cpio archive of the actual installed filesystem.
//! We shell out to tools that ship cheaply on both macOS and Ubuntu CI:
//!
//! - `bsdtar -xf unity.pkg` (from `libarchive-tools` on Linux, built-in on
//!   macOS) handles XAR natively; we prefer this path because the legacy
//!   Debian `xar` binary is no longer packaged on recent Ubuntu releases.
//! - `xar -xf unity.pkg` is used as a fallback when `bsdtar` is missing
//!   (e.g. a minimal macOS dev environment without Xcode).
//! - Each `*.pkg/Payload` is extracted with `bsdtar` (auto-detects gzip),
//!   falling back to `gzip -dc | cpio -idm`.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use walkdir::WalkDir;

use super::libil2cpp_header_subpath;

pub fn extract(client: &reqwest::blocking::Client, url: &str, out_dir: &Path) -> Result<usize> {
    let tmp = tempfile::tempdir().context("creating temp dir")?;
    let pkg_path = tmp.path().join("unity.pkg");
    download_to_file(client, url, &pkg_path)?;

    let unpack_dir = tmp.path().join("xar");
    fs::create_dir_all(&unpack_dir)?;
    unpack_xar(&pkg_path, &unpack_dir)?;

    let payloads = find_payloads(&unpack_dir);
    if payloads.is_empty() {
        bail!("no Payload files found after unpacking {}", url);
    }

    let payload_dir = tmp.path().join("payload");
    fs::create_dir_all(&payload_dir)?;
    for payload in &payloads {
        extract_payload(payload, &payload_dir)
            .with_context(|| format!("extracting payload {}", payload.display()))?;
    }

    let mut extracted = 0usize;
    for entry in WalkDir::new(&payload_dir) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel_to_payload = entry.path().strip_prefix(&payload_dir).unwrap();
        let Some(rel) = libil2cpp_header_subpath(rel_to_payload) else {
            continue;
        };
        let dest = out_dir.join(&rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::copy(entry.path(), &dest).with_context(|| format!("copying to {}", dest.display()))?;
        extracted += 1;
    }
    Ok(extracted)
}

fn download_to_file(client: &reqwest::blocking::Client, url: &str, dest: &Path) -> Result<()> {
    tracing::info!(url, dest = %dest.display(), "downloading .pkg");
    let mut resp = client
        .get(url)
        .send()
        .with_context(|| format!("GET {url}"))?
        .error_for_status()?;
    let mut file =
        fs::File::create(dest).with_context(|| format!("creating {}", dest.display()))?;
    io::copy(&mut resp, &mut file).with_context(|| format!("writing {}", dest.display()))?;
    Ok(())
}

/// Unpack a flat-package XAR into `into`. Tries `bsdtar` first (available on
/// Ubuntu via `libarchive-tools` and on macOS by default) and falls back to
/// `xar` for hosts that only ship the legacy tool.
fn unpack_xar(pkg: &Path, into: &Path) -> Result<()> {
    if tool_exists("bsdtar") {
        let status = Command::new("bsdtar")
            .current_dir(into)
            .arg("-xf")
            .arg(pkg)
            .status()
            .context("spawning bsdtar for .pkg")?;
        if status.success() {
            return Ok(());
        }
    }
    if tool_exists("xar") {
        let status = Command::new("xar")
            .arg("-x")
            .arg("-C")
            .arg(into)
            .arg("-f")
            .arg(pkg)
            .status()
            .context("spawning xar")?;
        if status.success() {
            return Ok(());
        }
        bail!("xar failed extracting {}", pkg.display());
    }
    let _ = writeln!(
        io::stderr(),
        "need `bsdtar` (libarchive-tools) or `xar` to unpack .pkg; none found on PATH",
    );
    bail!("no XAR-capable tool on PATH");
}

fn find_payloads(root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && e.file_name() == "Payload")
        .map(|e| e.into_path())
        .collect()
}

/// Extracts a single `Payload` file into `into`. `Payload` is either plain
/// cpio, gzip-compressed cpio, or xz-compressed cpio depending on the pkg
/// version. We try `bsdtar` first (which auto-detects compression and
/// format), then fall back to `gzip -dc | cpio -idm`.
fn extract_payload(payload: &Path, into: &Path) -> Result<()> {
    if tool_exists("bsdtar") {
        let status = Command::new("bsdtar")
            .current_dir(into)
            .arg("-xf")
            .arg(payload)
            .status()
            .context("spawning bsdtar")?;
        if status.success() {
            return Ok(());
        }
    }

    let gzip = Command::new("gzip")
        .arg("-dc")
        .arg(payload)
        .stdout(Stdio::piped())
        .spawn()
        .context("spawning gzip")?;
    let gzip_out = gzip.stdout.expect("gzip stdout piped");

    let status = Command::new("cpio")
        .current_dir(into)
        .args(["-i", "-d", "-m", "--quiet"])
        .stdin(Stdio::from(gzip_out))
        .status()
        .context("spawning cpio")?;
    if !status.success() {
        bail!("cpio failed extracting {}", payload.display());
    }
    Ok(())
}

fn tool_exists(name: &str) -> bool {
    Command::new(name)
        .arg("--help")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.code().is_some())
        .unwrap_or(false)
}
