//! Pure-Rust streaming extractor for the Linux `Unity.tar.xz` archive.
//!
//! Pipeline: `reqwest::blocking::Response` (`Read`) → [`xz2::read::XzDecoder`]
//! → [`tar::Archive`] → filter entries → write matching headers to disk. The
//! archive itself is never materialised; we read it sequentially and discard
//! everything except `libil2cpp/**/*.h`.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::libil2cpp_header_subpath;

pub fn extract(client: &reqwest::blocking::Client, url: &str, out_dir: &Path) -> Result<usize> {
    let resp = client
        .get(url)
        .send()
        .with_context(|| format!("GET {url}"))?
        .error_for_status()?;

    let xz = xz2::read::XzDecoder::new(resp);
    let mut archive = tar::Archive::new(xz);

    let mut extracted = 0usize;
    for entry in archive.entries().context("reading tar index")? {
        let mut entry = entry.context("reading tar entry")?;
        let path = entry
            .path()
            .context("decoding tar entry path")?
            .into_owned();
        let Some(rel) = libil2cpp_header_subpath(&path) else {
            continue;
        };

        let dest = out_dir.join(&rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let mut file =
            fs::File::create(&dest).with_context(|| format!("creating {}", dest.display()))?;
        std::io::copy(&mut entry, &mut file)
            .with_context(|| format!("writing {}", dest.display()))?;
        extracted += 1;
    }
    Ok(extracted)
}
