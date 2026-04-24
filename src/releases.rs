//! Enumerates Unity editor releases via the public GraphQL endpoint.
//!
//! The older REST index at `services.api.unity.com/unity/editor/release/v1`
//! only exposes ~1100 recent versions back to `2017.4.6f1`. The GraphQL
//! endpoint at `services.unity.com/graphql` exposes the full historical list
//! (including Unity 5.x and pre-2017.4 versions that never had a Linux
//! editor). For each release it returns the version string plus the short
//! revision hash we need to construct download URLs on `download.unity3d.com`.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;

use crate::version::UnityVersion;

const GRAPHQL_URL: &str = "https://services.unity.com/graphql";
const PAGE_SIZE: u32 = 250;

/// Historical Unity major-version prefixes. `getUnityReleaseMajorVersions`
/// only advertises currently-supported streams, so we query the full set
/// ourselves. We also merge in whatever the API returns to future-proof
/// against a new major (e.g. `7000`) landing.
const HISTORICAL_MAJORS: &[&str] = &[
    "5", "2017", "2018", "2019", "2020", "2021", "2022", "2023", "6000",
];

/// A single Unity editor release. The revision hash is the 12-char prefix
/// Unity uses in archive download URLs (e.g. `e80cc3114ac1`).
#[derive(Clone, Debug)]
pub struct Release {
    pub version: UnityVersion,
    pub revision: String,
}

#[derive(Deserialize)]
struct Gql<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct MajorsResp {
    #[serde(rename = "getUnityReleaseMajorVersions")]
    majors: Vec<MajorVersion>,
}

#[derive(Deserialize)]
struct MajorVersion {
    version: String,
}

#[derive(Deserialize)]
struct ReleasesResp {
    #[serde(rename = "getUnityReleases")]
    releases: ReleasesPage,
}

#[derive(Deserialize)]
struct ReleasesPage {
    edges: Vec<Edge>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

#[derive(Deserialize)]
struct Edge {
    node: Node,
}

#[derive(Deserialize)]
struct Node {
    version: UnityVersion,
    #[serde(rename = "shortRevision")]
    short_revision: String,
}

#[derive(Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
}

/// Fetches every release across every historical major, deduplicated and
/// sorted ascending by version. Makes one GraphQL request per major plus
/// additional pagination requests as needed.
pub fn fetch_releases(client: &reqwest::blocking::Client) -> Result<Vec<Release>> {
    let majors = collect_majors(client)?;
    let mut out: Vec<Release> = Vec::new();
    for major in majors {
        out.extend(fetch_major(client, &major)?);
    }
    out.sort_by(|a, b| a.version.cmp(&b.version));
    out.dedup_by(|a, b| a.version == b.version);
    Ok(out)
}

fn collect_majors(client: &reqwest::blocking::Client) -> Result<Vec<String>> {
    let mut set: std::collections::BTreeSet<String> =
        HISTORICAL_MAJORS.iter().map(|s| (*s).to_string()).collect();

    let query = "query { getUnityReleaseMajorVersions { version } }";
    let body = json!({ "query": query });
    let resp: Gql<MajorsResp> = graphql_request(client, &body)?;
    let data = resp
        .data
        .context("GraphQL returned no data for major versions")?;
    for m in data.majors {
        if let Some(top) = m.version.split('.').next() {
            set.insert(top.to_string());
        }
    }
    Ok(set.into_iter().collect())
}

fn fetch_major(client: &reqwest::blocking::Client, major: &str) -> Result<Vec<Release>> {
    let query = r#"
        query($limit: Int!, $skip: Int!, $version: String!) {
          getUnityReleases(limit: $limit, skip: $skip, version: $version) {
            edges { node { version shortRevision } }
            pageInfo { hasNextPage }
          }
        }
    "#;

    let mut out: Vec<Release> = Vec::new();
    let mut skip = 0u32;
    loop {
        let body = json!({
            "query": query,
            "variables": { "limit": PAGE_SIZE, "skip": skip, "version": major },
        });
        let resp: Gql<ReleasesResp> = graphql_request(client, &body)?;
        let page = resp
            .data
            .with_context(|| format!("GraphQL returned no data for major {major}"))?
            .releases;

        let got = page.edges.len();
        for edge in page.edges {
            out.push(Release {
                version: edge.node.version,
                revision: edge.node.short_revision,
            });
        }
        if !page.page_info.has_next_page || got == 0 {
            break;
        }
        skip += got as u32;
    }
    Ok(out)
}

fn graphql_request<T: for<'de> Deserialize<'de>>(
    client: &reqwest::blocking::Client,
    body: &serde_json::Value,
) -> Result<Gql<T>> {
    let resp = client
        .post(GRAPHQL_URL)
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .context("posting to Unity GraphQL endpoint")?
        .error_for_status()?;
    let parsed: Gql<T> = resp.json().context("decoding GraphQL response")?;
    if let Some(errs) = &parsed.errors {
        anyhow::bail!("GraphQL errors: {errs}");
    }
    Ok(parsed)
}
