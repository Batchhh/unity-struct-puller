//! Parses and formats Unity version strings (e.g. `2022.3.10f1`, `6000.0.25f1`).

use std::fmt;
use std::str::FromStr;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

/// A parsed Unity version of the form `MAJOR.MINOR.PATCH{channel}{build}`.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UnityVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub channel: Channel,
    pub build: u32,
}

/// Release channel letter between the patch number and build counter.
///
/// Declaration order is also the [`Ord`] order, which makes
/// `Alpha < Beta < China < Patch < Final < Xlts` — deliberately chosen so
/// a stable `f` release sorts after its preceding `a`/`b`/`p` builds.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum Channel {
    Alpha,
    Beta,
    China,
    Patch,
    Final,
    Xlts,
}

impl Channel {
    fn letter(self) -> char {
        match self {
            Channel::Alpha => 'a',
            Channel::Beta => 'b',
            Channel::China => 'c',
            Channel::Patch => 'p',
            Channel::Final => 'f',
            Channel::Xlts => 'x',
        }
    }

    fn from_letter(c: char) -> Option<Self> {
        Some(match c {
            'a' => Channel::Alpha,
            'b' => Channel::Beta,
            'c' => Channel::China,
            'p' => Channel::Patch,
            'f' => Channel::Final,
            'x' => Channel::Xlts,
            _ => return None,
        })
    }
}

impl FromStr for UnityVersion {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let mut parts = s.splitn(3, '.');
        let major = parts
            .next()
            .context("missing major")?
            .parse::<u32>()
            .with_context(|| format!("parsing major in {s:?}"))?;
        let minor = parts
            .next()
            .context("missing minor")?
            .parse::<u32>()
            .with_context(|| format!("parsing minor in {s:?}"))?;
        let rest = parts.next().context("missing patch+channel+build")?;

        let chan_pos = rest
            .find(|c: char| !c.is_ascii_digit())
            .with_context(|| format!("missing channel letter in {s:?}"))?;
        let patch = rest[..chan_pos]
            .parse::<u32>()
            .with_context(|| format!("parsing patch in {s:?}"))?;
        let chan_char = rest[chan_pos..].chars().next().unwrap();
        let channel = Channel::from_letter(chan_char)
            .ok_or_else(|| anyhow!("unknown channel {chan_char:?} in {s:?}"))?;
        let build = rest[chan_pos + chan_char.len_utf8()..]
            .parse::<u32>()
            .with_context(|| format!("parsing build in {s:?}"))?;

        Ok(UnityVersion {
            major,
            minor,
            patch,
            channel,
            build,
        })
    }
}

impl fmt::Display for UnityVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}.{}{}{}",
            self.major,
            self.minor,
            self.patch,
            self.channel.letter(),
            self.build,
        )
    }
}

impl fmt::Debug for UnityVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Serialize for UnityVersion {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for UnityVersion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modern_version() {
        let v: UnityVersion = "2022.3.10f1".parse().unwrap();
        assert_eq!(v.major, 2022);
        assert_eq!(v.minor, 3);
        assert_eq!(v.patch, 10);
        assert_eq!(v.channel, Channel::Final);
        assert_eq!(v.build, 1);
    }

    #[test]
    fn roundtrips_through_display() {
        for raw in [
            "2022.3.10f1",
            "6000.0.25f1",
            "5.6.7p4",
            "2018.4.36a3",
            "2022.2.0b14",
        ] {
            let v: UnityVersion = raw.parse().unwrap();
            assert_eq!(v.to_string(), raw);
        }
    }

    #[test]
    fn orders_by_components() {
        let a: UnityVersion = "2022.3.10f1".parse().unwrap();
        let b: UnityVersion = "2022.3.10f2".parse().unwrap();
        let c: UnityVersion = "2022.3.11f1".parse().unwrap();
        let d: UnityVersion = "6000.0.0b1".parse().unwrap();
        assert!(a < b);
        assert!(b < c);
        assert!(c < d);
    }

    #[test]
    fn final_sorts_after_beta_within_same_patch() {
        let beta: UnityVersion = "2022.3.10b5".parse().unwrap();
        let final_: UnityVersion = "2022.3.10f1".parse().unwrap();
        assert!(beta < final_);
    }
}
