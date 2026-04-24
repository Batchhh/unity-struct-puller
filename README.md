# unity-struct-puller

Pulls Unity IL2CPP struct headers from every published Unity editor release and commits them
under `headers/<version>/`.

Release enumeration uses Unity's public GraphQL endpoint
(`services.unity.com/graphql`) and covers every version from **Unity 5.0.0f4**
through current (~1500 releases). For Unity 2017.4+ the Linux `.tar.xz`
editor archive is streamed through `xz2` + `tar` in pure Rust; for older
versions the macOS `.pkg` is downloaded and unpacked via `bsdtar` (or `xar`),
with each component `Payload` extracted via `bsdtar` or `gzip -dc | cpio`.

## Usage

```bash
cargo build --release

./target/release/unity-struct-puller list-remote    # all versions Unity advertises
./target/release/unity-struct-puller list-local     # versions already pulled
./target/release/unity-struct-puller missing        # remote minus local
./target/release/unity-struct-puller pull 2022.3.10f1
./target/release/unity-struct-puller sync --limit 3 # pull missing, oldest first
```

Local prerequisites for the legacy `.pkg` path: `cpio`, `libarchive-tools`
(provides `bsdtar`). macOS ships both by default.

## CI

`.github/workflows/sync.yml` runs the `sync` command on a schedule and
commits any new headers. Trigger manually with:

```bash
gh workflow run 'Sync Unity headers' -f limit=10
```

The `test` job runs `cargo test`, `cargo clippy -D warnings`, and
`cargo fmt --check` on every push.
