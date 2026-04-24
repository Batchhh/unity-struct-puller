//! CLI front-end for the unity-struct-puller crate.
//!
//! Subcommands:
//! - `list-remote`: versions Unity currently advertises in its releases index.
//! - `list-local`:  versions already pulled into the `headers/` directory.
//! - `missing`:     remote versions that are not yet present locally.
//! - `pull <ver>`:  pull headers for one version.
//! - `sync`:        pull every missing version (with an optional `--limit`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};

use unity_struct_puller::{pull, releases, version::UnityVersion};

#[derive(Parser)]
#[command(about, version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,

    /// Directory where per-version header trees are written.
    #[arg(long, default_value = "headers", env = "HEADERS_DIR", global = true)]
    headers_dir: PathBuf,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print every version Unity currently advertises.
    ListRemote,
    /// Print every version already pulled into `headers/`.
    ListLocal,
    /// Print versions advertised remotely but not yet pulled.
    Missing,
    /// Pull headers for one specific version.
    Pull { version: UnityVersion },
    /// Pull headers for every missing version.
    Sync {
        /// Stop after this many successful pulls in one run.
        #[arg(long)]
        limit: Option<usize>,
        /// Process only every Mth missing version, offset by N-1 (1-indexed).
        /// Pass as `N/M`, e.g. `--shard 3/20` for the third of twenty disjoint
        /// slices. Lets a matrix of CI jobs cover the full missing list in
        /// parallel with zero overlap.
        #[arg(long, value_parser = parse_shard)]
        shard: Option<(u32, u32)>,
    },
}

fn parse_shard(s: &str) -> Result<(u32, u32), String> {
    let (n, m) = s
        .split_once('/')
        .ok_or_else(|| format!("shard must be N/M, got {s:?}"))?;
    let n: u32 = n.parse().map_err(|e| format!("bad shard numerator: {e}"))?;
    let m: u32 = m
        .parse()
        .map_err(|e| format!("bad shard denominator: {e}"))?;
    if m == 0 || n == 0 || n > m {
        return Err(format!("invalid shard {n}/{m}: require 1 <= N <= M"));
    }
    Ok((n, m))
}

fn main() -> Result<()> {
    init_tracing()?;
    let cli = Cli::parse();
    let client = build_client()?;
    std::fs::create_dir_all(&cli.headers_dir)?;

    match cli.cmd {
        Cmd::ListRemote => {
            for r in releases::fetch_releases(&client)? {
                println!("{}", r.version);
            }
        }
        Cmd::ListLocal => {
            for v in local_versions(&cli.headers_dir)? {
                println!("{v}");
            }
        }
        Cmd::Missing => {
            for v in missing_versions(&client, &cli.headers_dir)? {
                println!("{v}");
            }
        }
        Cmd::Pull { version } => {
            let release = releases::fetch_releases(&client)?
                .into_iter()
                .find(|r| r.version == version)
                .ok_or_else(|| anyhow!("version {version} not in release index"))?;
            pull::pull_release(&client, &release, &cli.headers_dir)?;
        }
        Cmd::Sync { limit, shard } => run_sync(&client, &cli.headers_dir, limit, shard)?,
    }
    Ok(())
}

fn run_sync(
    client: &reqwest::blocking::Client,
    headers_dir: &Path,
    limit: Option<usize>,
    shard: Option<(u32, u32)>,
) -> Result<()> {
    let remote = releases::fetch_releases(client)?;
    let local: BTreeSet<_> = local_versions(headers_dir)?.into_iter().collect();

    let missing: Vec<_> = remote
        .into_iter()
        .filter(|r| !local.contains(&r.version))
        .collect();

    let targets: Vec<_> = match shard {
        Some((n, m)) => missing
            .into_iter()
            .enumerate()
            .filter(|(i, _)| (*i as u32) % m == n - 1)
            .map(|(_, r)| r)
            .collect(),
        None => missing,
    };
    tracing::info!(targets = targets.len(), ?shard, "sync start");

    let mut pulled = 0usize;
    let mut failed = 0usize;
    for release in targets {
        match pull::pull_release(client, &release, headers_dir) {
            Ok(_) => {
                pulled += 1;
                if limit.is_some_and(|l| pulled >= l) {
                    break;
                }
            }
            Err(e) => {
                failed += 1;
                tracing::error!(version = %release.version, error = ?e, "pull failed");
            }
        }
    }
    tracing::info!(pulled, failed, "sync done");
    println!("pulled {pulled} new version(s), {failed} failed");
    Ok(())
}

fn local_versions(headers_dir: &Path) -> Result<Vec<UnityVersion>> {
    let mut out = Vec::new();
    if !headers_dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(headers_dir)
        .with_context(|| format!("reading {}", headers_dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if let Ok(v) = name_str.parse::<UnityVersion>() {
            out.push(v);
        }
    }
    out.sort();
    Ok(out)
}

fn missing_versions(
    client: &reqwest::blocking::Client,
    headers_dir: &Path,
) -> Result<Vec<UnityVersion>> {
    let remote = releases::fetch_releases(client)?;
    let local: BTreeSet<_> = local_versions(headers_dir)?.into_iter().collect();
    Ok(remote
        .into_iter()
        .map(|r| r.version)
        .filter(|v| !local.contains(v))
        .collect())
}

fn build_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60 * 60))
        .user_agent(concat!("unity-struct-puller/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(Into::into)
}

fn init_tracing() -> Result<()> {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("unity_struct_puller=info,info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
    Ok(())
}
