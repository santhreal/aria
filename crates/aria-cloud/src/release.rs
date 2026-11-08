//! The update check: the newest published release of this repository on
//! GitHub, compared with the running build by semantic version.
//!
//! The repository is the workspace `repository` field, so the check follows the
//! crate metadata and names no host of its own beyond the GitHub API. A build
//! whose own version has a pre-release part (`0.7.0-alpha.0`) is offered
//! pre-releases; a release build is offered releases only.

use std::time::Duration;

use semver::Version;
use serde::Deserialize;

use crate::error::CloudError;

/// The GitHub REST API root.
pub const GITHUB_API: &str = "https://api.github.com";

/// One entry of `GET /repos/{owner}/{repo}/releases`. Unknown fields are
/// ignored.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Release {
    /// The git tag, `v0.8.0` or `0.8.0`. A tag that is not a version is
    /// skipped, because the repository may tag other things.
    pub tag_name: String,
    /// The release page.
    pub html_url: String,
    /// A draft is never offered.
    #[serde(default)]
    pub draft: bool,
    /// Marked as a pre-release on GitHub.
    #[serde(default)]
    pub prerelease: bool,
}

/// The newest offered release against the running build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseStatus {
    /// A release newer than the running build is published at `url`.
    Newer { latest: Version, url: String },
    /// The newest offered release is the running build or older than it.
    UpToDate { latest: Version },
    /// No release this build would be offered is published.
    NoRelease,
}

/// The newest release in `releases` that a build at `installed` is offered,
/// compared with `installed`.
///
/// Drafts and tags that are not versions are skipped. Pre-releases, by the
/// GitHub flag or by a pre-release part in the tag, are offered only to a
/// build that is itself a pre-release. Versions compare by semver, so
/// `0.10.0` is newer than `0.9.0` and `0.7.0` is newer than `0.7.0-alpha.0`.
pub fn compare(releases: &[Release], installed: &Version) -> ReleaseStatus {
    let offers_prerelease = !installed.pre.is_empty();
    let newest = releases
        .iter()
        .filter(|release| !release.draft)
        .filter_map(|release| Some((tag_version(&release.tag_name)?, release)))
        .filter(|(version, release)| {
            offers_prerelease || (!release.prerelease && version.pre.is_empty())
        })
        .max_by(|a, b| a.0.cmp(&b.0));
    match newest {
        None => ReleaseStatus::NoRelease,
        Some((latest, release)) if latest > *installed => ReleaseStatus::Newer {
            latest,
            url: release.html_url.clone(),
        },
        Some((latest, _)) => ReleaseStatus::UpToDate { latest },
    }
}

/// The version a tag names, with an optional leading `v`.
fn tag_version(tag: &str) -> Option<Version> {
    Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()
}

/// `owner/name` of a `https://github.com/owner/name` repository URL.
fn repository_path(url: &str) -> Option<&str> {
    let path = url
        .strip_prefix("https://github.com/")?
        .trim_end_matches('/')
        .trim_end_matches(".git");
    let (owner, name) = path.split_once('/')?;
    (!owner.is_empty() && !name.is_empty() && !name.contains('/')).then_some(path)
}

/// A blocking update check against one repository's releases.
pub struct ReleaseCheck {
    url: String,
    installed: Version,
    agent: ureq::Agent,
}

/// The `ureq::Agent` is left out, as on `CloudClient`.
impl std::fmt::Debug for ReleaseCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReleaseCheck")
            .field("url", &self.url)
            .field("installed", &self.installed)
            .finish_non_exhaustive()
    }
}

impl ReleaseCheck {
    /// The check for this build: the workspace repository on GitHub, against
    /// the workspace version.
    pub fn for_this_build() -> Self {
        let repository = repository_path(env!("CARGO_PKG_REPOSITORY"))
            .expect("the workspace repository is a github.com owner/name URL");
        let installed =
            Version::parse(env!("CARGO_PKG_VERSION")).expect("cargo package versions are semver");
        Self::new(GITHUB_API, repository, installed)
    }

    /// A check of `repository` (`owner/name`) on the API at `api_base`, for a
    /// build at `installed`.
    pub fn new(api_base: &str, repository: &str, installed: Version) -> Self {
        let api_base = api_base.trim_end_matches('/');
        Self {
            url: format!("{api_base}/repos/{repository}/releases?per_page=100"),
            installed,
            agent: crate::http::HttpAgentBuilder::for_caller(crate::http::HttpCaller::ReleaseCheck)
                .for_url(api_base)
                .timeout(Duration::from_secs(30))
                .build(),
        }
    }

    /// The version the check compares against.
    pub fn installed(&self) -> &Version {
        &self.installed
    }

    /// Fetch the releases and compare. A repository with no release answers
    /// an empty list, which is [`ReleaseStatus::NoRelease`]; a missing
    /// repository or a rate limit is a [`CloudError::Status`].
    pub fn status(&self) -> Result<ReleaseStatus, CloudError> {
        let response = self
            .agent
            .get(&self.url)
            .set("Accept", "application/vnd.github+json")
            .set("User-Agent", concat!("aria/", env!("CARGO_PKG_VERSION")))
            .call()
            .map_err(|e| CloudError::from_ureq(&self.url, e))?;
        let releases: Vec<Release> = crate::client::parse_json(&self.url, response)?;
        Ok(compare(&releases, &self.installed))
    }
}

#[cfg(test)]
mod tests {
    //! WHY: the update check reported "Update available" whenever the latest
    //! version string differed from the installed one, so an older release, or
    //! the same version spelled `v0.7.0`, read as an update. These pin the
    //! semver comparison, the pre-release offer rule, and the request against a
    //! local server. Not covered: GitHub pagination past 100 releases.

    use super::*;
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    fn release(tag: &str, prerelease: bool, draft: bool) -> Release {
        Release {
            tag_name: tag.to_string(),
            html_url: format!("https://example.test/releases/{tag}"),
            draft,
            prerelease,
        }
    }

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn newer(latest: &str, tag: &str) -> ReleaseStatus {
        ReleaseStatus::Newer {
            latest: v(latest),
            url: format!("https://example.test/releases/{tag}"),
        }
    }

    fn current(latest: &str) -> ReleaseStatus {
        ReleaseStatus::UpToDate { latest: v(latest) }
    }

    #[test]
    fn compare_orders_by_semver_and_offers_prereleases_only_to_prerelease_builds() {
        let cases: Vec<(&str, Vec<Release>, ReleaseStatus)> = vec![
            ("0.7.0-alpha.0", vec![], ReleaseStatus::NoRelease),
            (
                "0.7.0-alpha.0",
                vec![release("v0.7.0", false, false)],
                newer("0.7.0", "v0.7.0"),
            ),
            (
                "0.7.0-alpha.0",
                vec![release("v0.7.0-alpha.0", true, false)],
                current("0.7.0-alpha.0"),
            ),
            (
                "0.7.0-alpha.0",
                vec![release("0.6.2", false, false)],
                current("0.6.2"),
            ),
            (
                "0.7.0-alpha.0",
                vec![release("v0.7.0-alpha.1", true, false)],
                newer("0.7.0-alpha.1", "v0.7.0-alpha.1"),
            ),
            // A string comparison picks 0.9.0 and calls 0.10.0 older.
            (
                "0.9.5",
                vec![
                    release("v0.9.0", false, false),
                    release("v0.10.0", false, false),
                ],
                newer("0.10.0", "v0.10.0"),
            ),
            (
                "0.10.0",
                vec![
                    release("v0.9.0", false, false),
                    release("v0.10.0", false, false),
                ],
                current("0.10.0"),
            ),
            // Drafts and tags that are not versions are skipped.
            (
                "0.7.0",
                vec![
                    release("v9.0.0", false, true),
                    release("nightly", false, false),
                    release("v0.6.0", false, false),
                ],
                current("0.6.0"),
            ),
            (
                "0.7.0",
                vec![release("nightly", false, false)],
                ReleaseStatus::NoRelease,
            ),
            // A release build is not offered a pre-release, by flag or by tag.
            (
                "0.7.0",
                vec![release("v0.8.0-beta.1", true, false)],
                ReleaseStatus::NoRelease,
            ),
            (
                "0.7.0",
                vec![release("v0.8.0-beta.1", false, false)],
                ReleaseStatus::NoRelease,
            ),
            (
                "0.7.0",
                vec![release("v0.8.0", true, false)],
                ReleaseStatus::NoRelease,
            ),
        ];
        for (installed, releases, expected) in cases {
            assert_eq!(
                compare(&releases, &v(installed)),
                expected,
                "installed {installed}, releases {releases:?}"
            );
        }
    }

    #[test]
    fn repository_path_accepts_only_a_github_owner_and_name() {
        assert_eq!(
            repository_path("https://github.com/owner/name"),
            Some("owner/name")
        );
        assert_eq!(
            repository_path("https://github.com/owner/name.git/"),
            Some("owner/name")
        );
        for bad in [
            "https://gitlab.com/owner/name",
            "http://github.com/owner/name",
            "https://github.com/owner",
            "https://github.com/owner/",
            "https://github.com//name",
            "https://github.com/owner/name/tree/main",
        ] {
            assert_eq!(repository_path(bad), None, "{bad}");
        }
        let check = ReleaseCheck::for_this_build();
        assert!(
            check.url.starts_with(&format!(
                "{GITHUB_API}/repos/{}/releases",
                repository_path(env!("CARGO_PKG_REPOSITORY")).unwrap()
            )),
            "{check:?}"
        );
        assert_eq!(check.installed(), &v(env!("CARGO_PKG_VERSION")));
    }

    /// Serve one canned response; the handle returns the request it read.
    fn serve(response: String) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("the client connects");
            let mut request = [0u8; 4096];
            let read = stream.read(&mut request).unwrap_or(0);
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            String::from_utf8_lossy(&request[..read]).into_owned()
        });
        (base, server)
    }

    #[test]
    fn status_reads_the_release_list_of_the_repository() {
        let body = r#"[{"tag_name":"v0.8.0","html_url":"https://example.test/releases/v0.8.0","draft":false,"prerelease":false,"assets":[]}]"#;
        let (base, server) = serve(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ));
        let check = ReleaseCheck::new(&base, "owner/name", v("0.7.0-alpha.0"));
        assert_eq!(check.status().unwrap(), newer("0.8.0", "v0.8.0"));
        let request = server.join().unwrap();
        assert!(
            request.starts_with("GET /repos/owner/name/releases?per_page=100 HTTP/1.1\r\n"),
            "{request}"
        );
        let lower = request.to_ascii_lowercase();
        assert!(lower.contains("\r\nuser-agent: aria/"), "{request}");
        assert!(
            lower.contains("\r\naccept: application/vnd.github+json\r\n"),
            "{request}"
        );
    }

    #[test]
    fn status_reports_an_empty_list_as_no_release_and_a_missing_repository_as_404() {
        let (base, server) = serve("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n[]".to_string());
        let check = ReleaseCheck::new(&base, "owner/name", v("0.7.0"));
        assert_eq!(check.status().unwrap(), ReleaseStatus::NoRelease);
        server.join().unwrap();

        let (base, server) = serve(
            "HTTP/1.1 404 Not Found\r\nContent-Length: 23\r\n\r\n{\"message\":\"Not Found\"}"
                .to_string(),
        );
        let check = ReleaseCheck::new(&base, "owner/name", v("0.7.0"));
        match check.status() {
            Err(CloudError::Status {
                status: 404,
                retryable: false,
                ..
            }) => {}
            other => panic!("expected a 404 status error, got {other:?}"),
        }
        server.join().unwrap();
    }
}
