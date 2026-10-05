//! Ask GitHub whether a newer release exists.
//!
//! Both Rak and oyvey are published as GitHub releases, and each has its own
//! repository (see `crate::repo_for`), so the check is per repository rather
//! than once. The GUI shows the result; it never installs anything by itself --
//! the download still goes through the normal installer so the manifest stays
//! authoritative.

use anyhow::{Context, Result};
use serde::Deserialize;

/// One repository's release state, as shown in the GUI.
#[derive(Debug, Clone)]
pub struct ReleaseRow {
    /// `owner/name`.
    pub repo: &'static str,
    /// The release tag, e.g. `v0.9.0`. Empty when the query failed.
    pub tag: String,
    /// Human-facing release page.
    pub url: String,
    /// Whether `tag` is newer than the running installer.
    pub newer: bool,
    /// Why the query failed, if it did. A failed check is shown, never hidden:
    /// silently reporting "up to date" because GitHub was unreachable would be
    /// the worst possible answer.
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
}

/// Query `repo` for its latest release. Never fails: an error is reported in the
/// row so the GUI can display it.
pub fn latest_for(repo: &'static str) -> ReleaseRow {
    let releases_page = format!("https://github.com/{}/releases", repo);
    match fetch(repo) {
        Ok((tag, url)) => ReleaseRow {
            repo,
            tag: tag.clone(),
            url,
            newer: is_newer(&tag, crate::SETUP_VERSION),
            error: None,
        },
        Err(e) => ReleaseRow {
            repo,
            tag: String::new(),
            url: releases_page,
            newer: false,
            error: Some(format!("{:#}", e)),
        },
    }
}

fn fetch(repo: &str) -> Result<(String, String)> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", repo);
    // GitHub's API answers 403 to a request with no User-Agent, which looks
    // like a network failure unless the header is set.
    let text = ureq::get(&url)
        .set(
            "User-Agent",
            concat!("rak-setup/", env!("CARGO_PKG_VERSION")),
        )
        .set("Accept", "application/vnd.github+json")
        .call()
        .with_context(|| format!("querying {}", repo))?
        .into_string()
        .with_context(|| format!("reading {} release", repo))?;
    let r: GhRelease =
        serde_json::from_str(&text).with_context(|| format!("parsing {} release", repo))?;
    Ok((r.tag_name, r.html_url))
}

/// Whether `candidate` is a strictly newer version than `current`.
///
/// Numeric component-wise, so `0.10.0` beats `0.9.0`. Comparing the strings
/// would get that backwards -- "10" sorts before "9" -- and an update check that
/// points the wrong way is worse than no update check.
///
/// A pre-release suffix is ignored, so `v1.0.0` is not reported as newer than
/// `1.0.0-rc.1`. That errs towards not nagging, which is the safe direction for
/// a released build.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        let core = v.trim().trim_start_matches('v');
        let core = core.split(['-', '+']).next().unwrap_or("");
        core.split('.')
            .map(|p| p.trim().parse::<u64>().unwrap_or(0))
            .collect()
    }
    let c = parts(candidate);
    let cur = parts(current);
    // Compare over the longer of the two, treating absent components as 0, so
    // `1.2` and `1.2.0` are equal rather than one beating the other.
    for i in 0..c.len().max(cur.len()) {
        let a = c.get(i).copied().unwrap_or(0);
        let b = cur.get(i).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{is_newer, latest_for};

    #[test]
    fn compares_numerically_not_as_text() {
        // The case a string compare gets wrong.
        assert!(is_newer("v0.10.0", "0.9.0"));
        assert!(!is_newer("0.9.0", "0.10.0"));
        assert!(is_newer("1.0.0", "0.99.99"));
    }

    #[test]
    fn equal_versions_are_not_newer() {
        assert!(!is_newer("0.9.0", "0.9.0"));
        assert!(!is_newer("v0.9.0", "0.9.0"));
    }

    #[test]
    fn missing_components_are_zero() {
        // `1.2` and `1.2.0` are the same version, not an update.
        assert!(!is_newer("1.2", "1.2.0"));
        assert!(is_newer("1.2.1", "1.2"));
    }

    #[test]
    fn a_prerelease_does_not_count_as_an_update() {
        assert!(!is_newer("1.0.0", "1.0.0-rc.1"));
        assert!(!is_newer("0.9.0", "0.9.0-beta"));
        // Two pre-releases of the same version also compare equal: this
        // function does not rank pre-releases, and does not need to.
        // GitHub's `/releases/latest` never points at a pre-release, so the
        // update check only ever sees final tags.
        assert!(!is_newer("1.0.0-rc.2", "1.0.0-rc.1"));
    }

    #[test]
    fn junk_parses_as_zero_rather_than_panicking() {
        assert!(!is_newer("not-a-version", "0.9.0"));
        assert!(is_newer("1.0.0", "not-a-version"));
    }

    /// Live lookup against the real GitHub API.
    ///
    /// Ignored by default because CI has no business depending on a third-party
    /// service. Run it with `cargo test -- --ignored live_` before a release: what
    /// breaks here is the User-Agent header and the response shape, and neither
    /// would be caught by a mock.
    #[test]
    #[ignore]
    fn live_latest_release_lookup() {
        for repo in [crate::REPO, crate::OYVEY_REPO] {
            let row = latest_for(repo);
            assert!(row.error.is_none(), "{}: {:?}", repo, row.error);
            assert!(row.tag.starts_with('v'), "{}: tag was {:?}", repo, row.tag);
            assert!(
                row.url.contains("github.com/"),
                "{}: url was {}",
                repo,
                row.url
            );
        }
    }
}
