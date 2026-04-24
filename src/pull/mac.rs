//! Legacy extractor for Unity macOS `.pkg` archives.
//!
//! Unity's macOS editor installer is a flat `.pkg` (XAR archive) containing
//! one or more component packages. Each component has a `Payload` file that
//! is a gzip-compressed cpio archive of the actual installed filesystem.
//! Parsing XAR + gzipped cpio in pure Rust is plausible but fiddly, so we
//! shell out to tools that ship on both macOS and the Ubuntu CI image:
//!
//! - `xar -xf unity.pkg -C .`  unpacks the outer flat package.
//! - `bsdtar` or `cpio` reads each `*.pkg/Payload` as a cpio archive.
//!
//! We prefer `bsdtar` when present because it handles gzip-compressed cpio
//! with a single command; otherwise we fall back to `gzip -dc | cpio`.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use walkdir::WalkDir;

use super::libil2cpp_header_subpath;

pub fn extract(client: &reqwest::blocking::Client, url: &str, out_dir: &Path) -> Result<usize> {
    ensure_tool("xar")?;

    let tmp = tempfile::tempdir().context("creating temp dir")?;
    let pkg_path = tmp.path().join("unity.pkg");
    download_to_file(client, url, &pkg_path)?;

    let unpack_dir = tmp.path().join("xar");
    fs::create_dir_all(&unpack_dir)?;
    run(
        Command::new("xar")
            .arg("-x")
            .arg("-C")
            .arg(&unpack_dir)
            .arg("-f")
            .arg(&pkg_path),
        "xar",
    )?;

    let payloads = find_payloads(&unpack_dir);
    if payloads.is_empty() {
        bail!("no Payload files found after xar extraction of {}", url);
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

fn run(cmd: &mut Command, name: &str) -> Result<()> {
    let status = cmd.status().with_context(|| format!("spawning {name}"))?;
    if !status.success() {
        bail!("{name} exited with {status}");
    }
    Ok(())
}

fn ensure_tool(name: &str) -> Result<()> {
    if tool_exists(name) {
        Ok(())
    } else {
        let _ = writeln!(
            io::stderr(),
            "required tool `{name}` not found on PATH; install it (e.g. `sudo apt install xar cpio libarchive-tools`).",
        );
        bail!("missing required tool: {name}");
    }
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
