//! Pulls Unity IL2CPP struct headers from Unity editor archives.
//!
//! Public modules:
//! - [`version`]: the [`version::UnityVersion`] type and its parser.
//! - [`releases`]: fetches the Unity Hub releases index.
//! - [`pull`]: streams a Unity Linux editor archive and extracts `libil2cpp/` headers.

pub mod pull;
pub mod releases;
pub mod version;
