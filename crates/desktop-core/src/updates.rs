//! Release discovery and runtime selection. Never reads UA-owned data.
//!
//! The launcher installs UA versions from the compatibility list
//! (`crate::compatibility`). An update check reads UA's GitHub releases for
//! release notes and the list from this repository's `main` branch, and
//! offers the newest listed version for this launcher's host contract. When
//! the person turns on "offer pre-release and unchecked versions", it also
//! offers newer GitHub releases that are not on the list yet (betas,
//! nightlies, or stable releases still being checked), reading their exact
//! image digest from the release's `container-image.txt`. Those are marked as
//! unchecked, and the same review and acknowledgment apply.
//!
//! Each check also lists the recent UA versions for this launcher (the
//! catalog) so the person can pick any newer one, and reads this repository's
//! latest release so the launcher can say when a newer launcher exists.
use crate::compatibility::{CompatibilityList, image_repository, is_prerelease, parse_version};
use crate::{RuntimeIdentity, RuntimeLock};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    process::Stdio,
    sync::mpsc,
    time::{Duration, Instant},
};

const UA_REPO: &str = "enzimo/universal-agents";
/// This repository, as a literal so the endpoints below can be built from it.
macro_rules! desktop_repo {
    () => {
        "enzimo/universal-agents-desktop"
    };
}
const DESKTOP_REPO: &str = desktop_repo!();
const COMPATIBILITY_ENDPOINT: &str = concat!(
    "repos/",
    desktop_repo!(),
    "/contents/compatible-runtimes.json?ref=main"
);
/// GitHub's latest release excludes drafts and pre-releases.
const DESKTOP_LATEST_ENDPOINT: &str = concat!("repos/", desktop_repo!(), "/releases/latest");
/// How many UA versions the catalog lists, newest first.
const CATALOG_SIZE: usize = 5;
/// The release asset in which UA's image workflow records the exact image.
const CONTAINER_IMAGE_ASSET: &str = "container-image.txt";
const IMMUTABLE_IMAGE_LINE: &str = "Immutable image:";
/// Unlisted releases, newest first, inspected for a published image before
/// the check settles for an older one; bounds GitHub calls per check.
const MAX_UNLISTED_LOOKUPS: usize = 5;
const MAX_RESPONSE: u64 = 4 * 1024 * 1024;

/// What a GitHub API request asks for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Accept {
    /// API metadata as JSON.
    Json,
    /// A repository file's contents.
    Raw,
    /// A release asset's bytes.
    Binary,
}

impl Accept {
    fn header(self) -> &'static str {
        match self {
            Self::Json => "Accept: application/vnd.github+json",
            Self::Raw => "Accept: application/vnd.github.raw+json",
            Self::Binary => "Accept: application/octet-stream",
        }
    }
}

pub trait ReleaseSource {
    fn get(&self, endpoint: &str, accept: Accept) -> Result<String, String>;
}

pub struct GitHubReleases;
impl ReleaseSource for GitHubReleases {
    fn get(&self, endpoint: &str, accept: Accept) -> Result<String, String> {
        let deadline = Instant::now() + Duration::from_secs(25);
        let mut child = crate::platform::command("gh")
            .args([
                "api",
                "--hostname",
                "github.com",
                "--method",
                "GET",
                "--header",
                accept.header(),
                endpoint,
            ])
            .env("PATH", crate::registry::host_command_path())
            .env("GH_PROMPT_DISABLED", "1")
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|_| "Install GitHub CLI and sign in to check UA releases.".to_owned())?;
        let stdout = child.stdout.take().unwrap();
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = stdout
                .take(MAX_RESPONSE + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes);
            let _ = send.send(result);
        });
        let bytes = match receive.recv_timeout(Duration::from_secs(25)) {
            Ok(Ok(bytes)) if bytes.len() as u64 <= MAX_RESPONSE => bytes,
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(
                    "Release check timed out or exceeded its response limit. Try again.".to_owned(),
                );
            }
        };
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("Release check did not finish. Try again.".to_owned());
                }
            }
        };
        if !status.success() {
            return Err("Could not read GitHub releases. Check your connection and GitHub CLI account's access to the UA and desktop repositories.".to_owned());
        }
        String::from_utf8(bytes)
            .map_err(|_| "GitHub returned an invalid release response.".to_owned())
    }
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    body: Option<String>,
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubAsset {
    id: u64,
    name: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReleaseNotes {
    pub version: String,
    pub url: String,
    pub notes: String,
    pub breaking_changes: bool,
    pub prerelease: bool,
    /// When GitHub published the release (RFC 3339), if it says.
    pub published_at: Option<String>,
}

/// How a catalog version relates to the running one.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionRelation {
    Newer,
    Running,
    Older,
}

/// One of the recent UA versions shown under "Other versions".
///
/// Only newer versions can be chosen. Older ones are listed for reference:
/// UA has no guard against an older version opening data a newer one changed.
#[derive(Clone, Debug, Serialize)]
pub struct RuntimeCatalogEntry {
    pub version: String,
    pub published_at: Option<String>,
    /// On the compatibility list for this launcher. `false` means unchecked.
    pub listed: bool,
    pub prerelease: bool,
    pub relation: VersionRelation,
    /// The version a plain check offers.
    pub recommended: bool,
    pub notes_url: Option<String>,
    pub image: String,
    /// Whether the image is already in the local Docker image store. Filled
    /// in by the controller, like `UpdateReview::image_downloaded`.
    #[serde(default)]
    pub downloaded: bool,
}

/// The newest launcher release, for the launcher lane of the Updates drawer.
/// A failed check only affects that lane.
#[derive(Clone, Debug, Default, Serialize)]
pub struct DesktopRelease {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub newer: bool,
    pub published_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct UpdateReview {
    pub desktop_version: String,
    pub current_version: String,
    pub latest_version: String,
    pub update_pending: bool,
    pub releases: Vec<ReleaseNotes>,
    pub breaking_changes: bool,
    pub blocked_reason: Option<String>,
    pub review_id: Option<String>,
    pub image: Option<String>,
    /// Whether the offered version is on the compatibility list. `false`
    /// means it was offered only because the person asked for pre-release
    /// and unchecked versions.
    pub listed: bool,
    /// Whether the offered version is a pre-release (beta, rc, nightly).
    pub prerelease: bool,
    /// A newer UA release that is not on the compatibility list yet, while a
    /// listed version is offered (only when unchecked versions are not offered).
    pub newer_unlisted: Option<String>,
    /// Whether the reviewed image is already in the local Docker image store.
    ///
    /// Filled in by the controller after discovery (it needs Docker), so the
    /// launcher can say "downloaded" or offer a download without pulling on a
    /// plain check. Discovery itself never touches Docker.
    #[serde(default)]
    pub image_downloaded: bool,
    /// Recent UA versions for this launcher, newest first, always including
    /// the running one. Empty while an update is pending or when the
    /// compatibility list cannot be read.
    pub catalog: Vec<RuntimeCatalogEntry>,
    pub desktop: DesktopRelease,
    #[serde(skip)]
    pub source_lock: RuntimeLock,
    #[serde(skip)]
    pub candidate: Option<RuntimeLock>,
    /// Notes of every release newer than the running one, so a review for
    /// another catalog version needs no second GitHub request.
    #[serde(skip)]
    newer_notes: Vec<ReleaseNotes>,
}

/// A non-draft GitHub release of UA.
#[derive(Clone)]
struct Release {
    version: Version,
    notes: ReleaseNotes,
    image_asset: Option<u64>,
}

/// The Universal Agents project page on GitHub, opened from the launcher's mark.
pub fn ua_repository_url() -> String {
    format!("https://github.com/{UA_REPO}")
}

/// Read the compatibility list from this repository's `main` branch.
pub fn fetch_compatibility_list(source: &impl ReleaseSource) -> Result<CompatibilityList, String> {
    let text = source.get(COMPATIBILITY_ENDPOINT, Accept::Raw).map_err(|error| {
        log::warn!("could not read the compatibility list: {error}");
        "Could not read the compatibility list from GitHub. Check your connection and GitHub CLI access, then try again.".to_owned()
    })?;
    CompatibilityList::from_json(&text).map_err(|error| {
        log::error!("the compatibility list on GitHub is invalid: {error}");
        "The compatibility list on GitHub is invalid, so no update was prepared.".to_owned()
    })
}

/// Network discovery never changes the selected runtime or touches Docker.
pub fn check(
    source: &impl ReleaseSource,
    current: &RuntimeLock,
    pending: bool,
    offer_unlisted: bool,
) -> Result<UpdateReview, String> {
    let current_version =
        parse_version(&current.ua_version).ok_or("The selected runtime has an invalid version.")?;
    let history = release_history(source, offer_unlisted)?;
    let newer: Vec<Release> = history
        .iter()
        .filter(|release| release.version > current_version)
        .cloned()
        .collect();
    let mut review = UpdateReview {
        desktop_version: env!("CARGO_PKG_VERSION").to_owned(),
        current_version: current.ua_version.clone(),
        latest_version: newer
            .first()
            .map(|release| release.notes.version.clone())
            .unwrap_or_else(|| current.ua_version.clone()),
        update_pending: pending,
        breaking_changes: newer.iter().any(|release| release.notes.breaking_changes),
        releases: newer.iter().map(|release| release.notes.clone()).collect(),
        blocked_reason: None,
        review_id: None,
        image: None,
        listed: false,
        prerelease: false,
        newer_unlisted: None,
        image_downloaded: false,
        catalog: Vec::new(),
        desktop: check_desktop(source, env!("CARGO_PKG_VERSION")),
        source_lock: current.clone(),
        candidate: None,
        newer_notes: newer.iter().map(|release| release.notes.clone()).collect(),
    };
    if pending {
        review.blocked_reason =
            Some("An approved update is pending. Use Start to finish or retry it.".to_owned());
        return Ok(review);
    }
    let list = match fetch_compatibility_list(source) {
        Ok(list) => list,
        // The person already accepts versions nobody has checked, so an
        // unreadable list only means nothing counts as listed.
        Err(reason) if offer_unlisted => {
            log::warn!(
                "offering only unchecked UA versions because the compatibility list is unavailable: {reason}"
            );
            CompatibilityList::empty()
        }
        // Up to date: nothing to prepare, and no catalog without the list.
        Err(_) if newer.is_empty() => return Ok(review),
        Err(reason) => {
            review.blocked_reason = Some(reason);
            return Ok(review);
        }
    };
    let repository = image_repository(&current.image).unwrap_or_default();
    let listed = list
        .newest_for(current.host_contract_version, repository, offer_unlisted)
        .filter(|entry| parse_version(&entry.ua_version).is_some_and(|v| v > current_version))
        .cloned();
    let unlisted = if offer_unlisted {
        newer_unlisted_images(source, current, &newer, &list, listed.as_ref())
    } else {
        Vec::new()
    };
    let target = match (unlisted.first(), &listed) {
        (Some(identity), _) => Some((identity.clone(), false)),
        (None, Some(identity)) => Some((identity.clone(), true)),
        (None, None) => None,
    };
    review.catalog = build_catalog(CatalogInputs {
        history: &history,
        list: &list,
        current,
        current_version: &current_version,
        unlisted: &unlisted,
        include_prereleases: offer_unlisted,
        recommended: target
            .as_ref()
            .map(|(identity, _)| identity.ua_version.as_str()),
    });
    let Some((identity, is_listed)) = target else {
        if !newer.is_empty() {
            review.blocked_reason = Some(not_installable_reason(
                &list,
                current,
                &current_version,
                &newer,
                offer_unlisted,
            ));
        }
        return Ok(review);
    };
    if let Err(reason) = prepare(&mut review, current, &identity, is_listed) {
        review.blocked_reason = Some(reason);
        return Ok(review);
    }
    if !offer_unlisted {
        let target_version = parse_version(&identity.ua_version);
        review.newer_unlisted = newer
            .first()
            .filter(|release| Some(&release.version) > target_version.as_ref())
            .map(|release| release.notes.version.clone());
    }
    Ok(review)
}

/// A review for `version`, one of the newer versions in `base`'s catalog,
/// built from the last check without another GitHub request. It has the
/// same notes range, upgrade source and review-id rules as the offered one.
pub fn review_for(
    base: &UpdateReview,
    current: &RuntimeLock,
    version: &str,
) -> Result<UpdateReview, String> {
    if &base.source_lock != current {
        return Err("The update review is stale. Check for updates again.".to_owned());
    }
    if base.update_pending {
        return Err(
            "An approved update is pending. Use Start to finish or retry it first.".to_owned(),
        );
    }
    let entry = base
        .catalog
        .iter()
        .find(|entry| entry.version == version)
        .ok_or_else(|| {
            format!("UA {version} is not one of the versions offered. Check for updates again.")
        })?;
    if entry.relation != VersionRelation::Newer {
        return Err("Only a newer runtime can be installed.".to_owned());
    }
    let identity = RuntimeIdentity {
        host_contract_version: current.host_contract_version,
        ua_version: entry.version.clone(),
        image: entry.image.clone(),
    };
    let mut review = base.clone();
    review.blocked_reason = None;
    prepare(&mut review, current, &identity, entry.listed)?;
    review.image_downloaded = entry.downloaded;
    Ok(review)
}

/// Point `review` at `identity`: keep the notes of every release up to it,
/// build the candidate that replaces only the running selection, and bind
/// the review id to both locks and the notes.
fn prepare(
    review: &mut UpdateReview,
    current: &RuntimeLock,
    identity: &RuntimeIdentity,
    listed: bool,
) -> Result<(), String> {
    let target_version =
        parse_version(&identity.ua_version).ok_or("The offered runtime has an invalid version.")?;
    review.releases = review
        .newer_notes
        .iter()
        .filter(|release| parse_version(&release.version).is_some_and(|v| v <= target_version))
        .cloned()
        .collect();
    review.breaking_changes = review
        .releases
        .iter()
        .any(|release| release.breaking_changes);
    review.latest_version = identity.ua_version.clone();
    review.listed = listed;
    review.prerelease = is_prerelease(&identity.ua_version);
    review.review_id = None;
    review.image = None;
    review.candidate = None;
    review.image_downloaded = false;
    let mut candidate = current.clone();
    candidate.ua_version = identity.ua_version.clone();
    candidate.image = identity.image.clone();
    // Only the exact running selection may be replaced by this update.
    candidate.upgrade_sources = vec![current.identity()];
    validate_update(current, &candidate)?;
    let mut digest = Sha256::new();
    digest.update(serde_json::to_vec(current).map_err(|e| e.to_string())?);
    digest.update(serde_json::to_vec(&candidate).map_err(|e| e.to_string())?);
    digest.update(serde_json::to_vec(&review.releases).map_err(|e| e.to_string())?);
    review.review_id = Some(format!("{:x}", digest.finalize()));
    review.image = Some(candidate.image.clone());
    review.candidate = Some(candidate);
    Ok(())
}

/// Every non-draft UA release on GitHub, newest first. Pre-releases are
/// included only when unchecked versions are offered.
fn release_history(
    source: &impl ReleaseSource,
    include_prereleases: bool,
) -> Result<Vec<Release>, String> {
    let mut history = Vec::new();
    let mut complete = false;
    // Fetch all pages, rather than assuming GitHub creation order is version order.
    for page in 1..=10 {
        let endpoint = format!("repos/{UA_REPO}/releases?per_page=100&page={page}");
        let releases: Vec<GitHubRelease> =
            serde_json::from_str(&source.get(&endpoint, Accept::Json)?)
                .map_err(|_| "GitHub returned invalid release metadata.")?;
        let last_page = releases.len() < 100;
        for release in releases {
            if release.draft {
                continue;
            }
            let Some(version) = parse_version(&release.tag_name) else {
                continue;
            };
            let prerelease = release.prerelease || !version.pre.is_empty();
            if prerelease && !include_prereleases {
                continue;
            }
            let notes = release.body.unwrap_or_default();
            let breaking_changes = notes.lines().any(|line| {
                let line = line.trim();
                line.starts_with('#') && line.to_lowercase().contains("breaking")
            });
            history.push(Release {
                image_asset: release
                    .assets
                    .iter()
                    .find(|asset| asset.name == CONTAINER_IMAGE_ASSET)
                    .map(|asset| asset.id),
                notes: ReleaseNotes {
                    version: version.to_string(),
                    url: format!(
                        "https://github.com/{UA_REPO}/releases/tag/{}",
                        release.tag_name
                    ),
                    notes,
                    breaking_changes,
                    prerelease,
                    published_at: release.published_at,
                },
                version,
            });
        }
        if last_page {
            complete = true;
            break;
        }
    }
    if !complete {
        return Err(
            "Release history is too large to review completely. No update was prepared.".to_owned(),
        );
    }
    history.sort_by(|a, b| b.version.cmp(&a.version));
    history.dedup_by(|a, b| a.version == b.version);
    Ok(history)
}

/// Releases that are not on the list and are newer than the listed
/// candidate, newest first, each with its exact image read from
/// `container-image.txt`. They are assumed to implement this launcher's
/// contract; nothing has checked that, which the review says. At most
/// `MAX_UNLISTED_LOOKUPS` releases are inspected.
fn newer_unlisted_images(
    source: &impl ReleaseSource,
    current: &RuntimeLock,
    newer: &[Release],
    list: &CompatibilityList,
    listed: Option<&RuntimeIdentity>,
) -> Vec<RuntimeIdentity> {
    let listed_version = listed.and_then(|entry| parse_version(&entry.ua_version));
    let Some(repository) = image_repository(&current.image) else {
        return Vec::new();
    };
    newer
        .iter()
        .filter(|release| {
            listed_version
                .as_ref()
                .is_none_or(|listed| &release.version > listed)
        })
        .filter(|release| {
            !list
                .runtimes()
                .iter()
                .any(|entry| parse_version(&entry.ua_version).as_ref() == Some(&release.version))
        })
        .take(MAX_UNLISTED_LOOKUPS)
        .filter_map(|release| {
            let Some(asset) = release.image_asset else {
                log::info!(
                    "UA {} has no {CONTAINER_IMAGE_ASSET} yet; skipping it",
                    release.notes.version
                );
                return None;
            };
            let endpoint = format!("repos/{UA_REPO}/releases/assets/{asset}");
            let text = match source.get(&endpoint, Accept::Binary) {
                Ok(text) => text,
                Err(error) => {
                    log::warn!(
                        "could not read {CONTAINER_IMAGE_ASSET} for UA {}: {error}",
                        release.notes.version
                    );
                    return None;
                }
            };
            let image = published_image(&text, repository);
            if image.is_none() {
                log::warn!(
                    "UA {} has no usable image in {CONTAINER_IMAGE_ASSET}; skipping it",
                    release.notes.version
                );
            }
            image.map(|image| RuntimeIdentity {
                host_contract_version: current.host_contract_version,
                ua_version: release.notes.version.clone(),
                image,
            })
        })
        .collect()
}

/// The digest-pinned image a UA release's `container-image.txt` names, if it
/// is in the expected repository.
fn published_image(container_image_txt: &str, repository: &str) -> Option<String> {
    let image = container_image_txt
        .lines()
        .find_map(|line| line.trim().strip_prefix(IMMUTABLE_IMAGE_LINE))?
        .trim();
    crate::runtime_lock::validate_digest_pinned_image(image).ok()?;
    (image_repository(image) == Some(repository)).then(|| image.to_owned())
}

struct CatalogInputs<'a> {
    history: &'a [Release],
    list: &'a CompatibilityList,
    current: &'a RuntimeLock,
    current_version: &'a Version,
    unlisted: &'a [RuntimeIdentity],
    include_prereleases: bool,
    recommended: Option<&'a str>,
}

/// The recent UA versions this launcher can run: the compatibility-list
/// entries for its contract and image repository, the unchecked releases
/// found when they are offered, and the running version. Newest first, cut
/// to `CATALOG_SIZE`, always keeping the running version.
fn build_catalog(inputs: CatalogInputs<'_>) -> Vec<RuntimeCatalogEntry> {
    let CatalogInputs {
        history,
        list,
        current,
        current_version,
        unlisted,
        include_prereleases,
        recommended,
    } = inputs;
    let repository = image_repository(&current.image);
    let mut identities: Vec<(Version, RuntimeIdentity, bool)> = list
        .runtimes()
        .iter()
        .filter(|entry| entry.host_contract_version == current.host_contract_version)
        .filter(|entry| image_repository(&entry.image) == repository)
        .filter(|entry| include_prereleases || !is_prerelease(&entry.ua_version))
        .map(|entry| (entry.clone(), true))
        .chain(unlisted.iter().map(|entry| (entry.clone(), false)))
        .chain(std::iter::once((
            current.identity(),
            list.find(&current.ua_version).is_some_and(|entry| {
                entry.host_contract_version == current.host_contract_version
                    && entry.image == current.image
            }),
        )))
        .filter_map(|(identity, listed)| {
            parse_version(&identity.ua_version).map(|version| (version, identity, listed))
        })
        .collect();
    // Stable sort: for a repeated version the list entry, added first, wins.
    identities.sort_by(|a, b| b.0.cmp(&a.0));
    identities.dedup_by(|a, b| a.0 == b.0);
    let running = identities
        .iter()
        .position(|(version, ..)| version == current_version);
    if let Some(running) = running.filter(|&index| index >= CATALOG_SIZE) {
        let kept = identities.remove(running);
        identities.truncate(CATALOG_SIZE - 1);
        identities.push(kept);
    }
    identities.truncate(CATALOG_SIZE);
    identities
        .into_iter()
        .map(|(version, identity, listed)| {
            let release = history.iter().find(|release| release.version == version);
            RuntimeCatalogEntry {
                relation: match version.cmp(current_version) {
                    std::cmp::Ordering::Greater => VersionRelation::Newer,
                    std::cmp::Ordering::Equal => VersionRelation::Running,
                    std::cmp::Ordering::Less => VersionRelation::Older,
                },
                recommended: recommended == Some(identity.ua_version.as_str()),
                published_at: release.and_then(|release| release.notes.published_at.clone()),
                notes_url: release.map(|release| release.notes.url.clone()),
                prerelease: !version.pre.is_empty(),
                version: identity.ua_version,
                image: identity.image,
                listed,
                downloaded: false,
            }
        })
        .collect()
}

#[derive(Deserialize)]
struct LatestDesktopRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    published_at: Option<String>,
}

/// Compare this launcher with this repository's latest release. Failures are
/// reported in the result, so they never block the UA check.
pub fn check_desktop(source: &impl ReleaseSource, current: &str) -> DesktopRelease {
    let mut release = DesktopRelease {
        current_version: current.to_owned(),
        ..DesktopRelease::default()
    };
    let latest = source
        .get(DESKTOP_LATEST_ENDPOINT, Accept::Json)
        .and_then(|text| {
            serde_json::from_str::<LatestDesktopRelease>(&text)
                .map_err(|error| format!("invalid release metadata: {error}"))
        });
    let latest = match latest {
        Ok(latest) if !latest.draft && !latest.prerelease => latest,
        Ok(latest) => {
            log::warn!(
                "the latest launcher release {} is a draft or pre-release; ignoring it",
                latest.tag_name
            );
            return release;
        }
        Err(error) => {
            log::warn!("could not read the latest launcher release: {error}");
            release.error = Some(
                "Could not check GitHub for a newer launcher. Check your connection and GitHub CLI access, then choose Check now.".to_owned(),
            );
            return release;
        }
    };
    let (Some(latest_version), Some(current_version)) =
        (parse_version(&latest.tag_name), parse_version(current))
    else {
        log::warn!(
            "cannot compare launcher versions: latest tag {} and current {current}",
            latest.tag_name
        );
        release.error = Some("The newest launcher release has an unreadable version.".to_owned());
        return release;
    };
    release.newer = latest_version > current_version;
    release.latest_version = Some(latest_version.to_string());
    release.published_at = latest.published_at;
    release
}

/// The web page a release link in the Updates drawer may open: the latest
/// launcher release, or the notes of a UA release the last check returned.
/// The renderer names only a product and version, never a URL.
pub fn release_page_url(
    review: &UpdateReview,
    product: &str,
    version: &str,
) -> Result<String, String> {
    match product {
        "launcher" if review.desktop.latest_version.as_deref() == Some(version) => Ok(format!(
            "https://github.com/{DESKTOP_REPO}/releases/tag/v{version}"
        )),
        "runtime" => review
            .catalog
            .iter()
            .filter(|entry| entry.version == version)
            .find_map(|entry| entry.notes_url.clone())
            .or_else(|| {
                review
                    .newer_notes
                    .iter()
                    .find(|notes| notes.version == version)
                    .map(|notes| notes.url.clone())
            })
            .ok_or_else(|| {
                format!("No release page is known for UA {version}. Check for updates again.")
            }),
        "launcher" => Err(format!(
            "Launcher {version} is not the newest launcher release. Check for updates again."
        )),
        _ => Err("Unknown release page.".to_owned()),
    }
}

/// Why none of the newer releases can be installed.
fn not_installable_reason(
    list: &CompatibilityList,
    current: &RuntimeLock,
    current_version: &Version,
    newer: &[Release],
    offer_unlisted: bool,
) -> String {
    let needs_newer_desktop = list.runtimes().iter().any(|entry| {
        entry.host_contract_version > current.host_contract_version
            && parse_version(&entry.ua_version).is_some_and(|v| v > *current_version)
    });
    if needs_newer_desktop {
        return "The newer UA release needs a newer desktop app. Install the latest Universal Agents desktop app first.".to_owned();
    }
    let latest = newer
        .first()
        .map(|release| release.notes.version.as_str())
        .unwrap_or_default();
    if offer_unlisted {
        format!(
            "UA {latest} has no published image yet, so it cannot be installed. Try again later."
        )
    } else {
        format!(
            "UA {latest} is not on this launcher's compatibility list yet. It becomes available here once it has been checked with this launcher. To try it sooner, turn on “Offer pre-release and unchecked versions”."
        )
    }
}

/// A runtime-only update may change identity and reviewed upgrade sources, but
/// cannot change executable code, mounts, ports, repository, or desktop contract.
pub fn validate_profile(bundled: &RuntimeLock, candidate: &RuntimeLock) -> Result<(), String> {
    candidate.validate().map_err(|e| e.to_string())?;
    if image_repository(&candidate.image) != image_repository(&bundled.image) {
        return Err(
            "The runtime image repository changed; install a reviewed desktop update first."
                .to_owned(),
        );
    }
    let mut profile = candidate.clone();
    profile.ua_version = bundled.ua_version.clone();
    profile.image = bundled.image.clone();
    profile.upgrade_sources = bundled.upgrade_sources.clone();
    if &profile != bundled {
        return Err(
            "This runtime changes the desktop host contract. Update the desktop app first."
                .to_owned(),
        );
    }
    Ok(())
}

/// An update must keep this launcher's contract, move to a newer version,
/// and name the exact running selection as the only runtime it replaces.
pub fn validate_update(current: &RuntimeLock, candidate: &RuntimeLock) -> Result<(), String> {
    validate_profile(current, candidate)?;
    let current_version =
        parse_version(&current.ua_version).ok_or("Invalid current runtime version.")?;
    let next_version =
        parse_version(&candidate.ua_version).ok_or("Invalid candidate runtime version.")?;
    if next_version <= current_version {
        return Err("Only a newer runtime can be installed.".to_owned());
    }
    if !candidate.upgrade_sources.contains(&current.identity()) {
        return Err("The update does not replace your exact runtime image.".to_owned());
    }
    Ok(())
}

/// The newest stable listed version to install on a first install, if it is
/// newer than `selected` (the version built into this launcher).
pub fn first_install_target(
    list: &CompatibilityList,
    selected: &RuntimeLock,
) -> Option<RuntimeLock> {
    let repository = image_repository(&selected.image)?;
    let entry = list.newest_for(selected.host_contract_version, repository, false)?;
    let newer = parse_version(&entry.ua_version)? > parse_version(&selected.ua_version)?;
    if !newer {
        return None;
    }
    let mut lock = selected.clone();
    lock.ua_version = entry.ua_version.clone();
    lock.image = entry.image.clone();
    lock.upgrade_sources = Vec::new();
    lock.validate().ok()?;
    Some(lock)
}

pub fn approve(
    review: &UpdateReview,
    current: &RuntimeLock,
    id: &str,
    acknowledged: bool,
) -> Result<RuntimeLock, String> {
    if !acknowledged {
        return Err("Review and acknowledge the release notes before updating.".to_owned());
    }
    if &review.source_lock != current || review.review_id.as_deref() != Some(id) {
        return Err("The update review is stale. Check for updates again.".to_owned());
    }
    let candidate = review
        .candidate
        .clone()
        .ok_or("No compatible update is prepared.")?;
    validate_update(current, &candidate)?;
    Ok(candidate)
}

/// Validate a review for *downloading* without applying it.
///
/// Downloading is reversible and changes neither the selected runtime, the
/// container, nor UA data, so it does not require the release-notes
/// acknowledgment that Update and restart requires. It still requires a
/// current review bound to the exact reviewed digest, so the image that is
/// fetched is always the one the review named.
pub fn approve_download(
    review: &UpdateReview,
    current: &RuntimeLock,
    id: &str,
) -> Result<RuntimeLock, String> {
    if &review.source_lock != current || review.review_id.as_deref() != Some(id) {
        return Err("The update review is stale. Check for updates again.".to_owned());
    }
    let candidate = review
        .candidate
        .clone()
        .ok_or("No compatible update is prepared.")?;
    validate_update(current, &candidate)?;
    Ok(candidate)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeSelection {
    #[serde(flatten)]
    pub lock: RuntimeLock,
    #[serde(default)]
    pub update_pending: bool,
}

pub struct SelectionStore {
    pub path: PathBuf,
}
impl SelectionStore {
    pub fn load_or_initialize(&self, bundled: &RuntimeLock) -> Result<RuntimeSelection, String> {
        match fs::read(&self.path) {
            Ok(bytes) => {
                let selection: RuntimeSelection = serde_json::from_slice(&bytes).map_err(
                    |_| "Saved runtime-lock.json is invalid; the runtime was left untouched.",
                )?;
                // An older desktop's selection uses a container layout this
                // desktop cannot run, and there is no reviewed migration.
                if selection.lock.host_contract_version != bundled.host_contract_version {
                    log::error!(
                        "saved runtime selection uses another host contract: path={} saved_contract={} supported_contract={} saved_ua_version={}",
                        self.path.display(),
                        selection.lock.host_contract_version,
                        bundled.host_contract_version,
                        selection.lock.ua_version
                    );
                    // The contract numbers stay in the log line above for diagnosis;
                    // the user-facing text leaves out that internal term.
                    return Err(format!(
                        "This computer has data from an older Universal Agents desktop app (UA {}) that this version can't use. Nothing was changed. To start fresh, remove the old universal-agents-desktop Docker container if there is one, move the desktop app-data folder ({}) aside, and open the app again.",
                        selection.lock.ua_version,
                        self.path
                            .parent()
                            .map(|parent| parent.display().to_string())
                            .unwrap_or_default()
                    ));
                }
                validate_profile(bundled, &selection.lock)?;
                Ok(selection)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let selection = RuntimeSelection {
                    lock: bundled.clone(),
                    update_pending: false,
                };
                self.save(&selection)?;
                Ok(selection)
            }
            Err(error) => Err(format!("Cannot read selected runtime lock: {error}")),
        }
    }

    pub fn save(&self, selection: &RuntimeSelection) -> Result<(), String> {
        selection.lock.validate().map_err(|e| e.to_string())?;
        let parent = self
            .path
            .parent()
            .ok_or("Runtime selection needs a parent directory.")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = self.path.with_extension("json.tmp");
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        // A crashed write leaves only the old selected lock authoritative.
        if temporary.exists() {
            fs::remove_file(&temporary).map_err(|e| e.to_string())?;
        }
        let result = (|| {
            let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
            file.write_all(&serde_json::to_vec_pretty(selection).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            drop(file);
            fs::rename(&temporary, &self.path).map_err(|e| e.to_string())?;
            // Rename is the commit point: a directory sync failure must not
            // leave the controller using the old selection after commit.
            #[cfg(unix)]
            if let Err(error) = fs::File::open(parent).and_then(|dir| dir.sync_all()) {
                log::warn!("Runtime selection committed but directory sync failed: {error}");
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::HashMap;

    const REPOSITORY: &str = "ghcr.io/enzimo/universal-agents";

    fn image(digest: char) -> String {
        format!("{REPOSITORY}@sha256:{}", digest.to_string().repeat(64))
    }
    fn current() -> RuntimeLock {
        // Keep release-discovery fixtures independent of the bundled release.
        RuntimeLock::from_json(include_str!("../tests/fixtures/runtime-v0.8.4.json")).unwrap()
    }
    fn listed(version: &str, contract: u32, digest: char) -> serde_json::Value {
        json!({"ua_version": version, "host_contract_version": contract, "image": image(digest)})
    }
    fn list(entries: Vec<serde_json::Value>) -> String {
        json!({"schema_version": 1, "runtimes": entries}).to_string()
    }
    /// The update the default fixtures offer: UA 0.9.0 from the list.
    fn next() -> RuntimeLock {
        let mut new = current();
        new.ua_version = "0.9.0".into();
        new.image = image('a');
        new.upgrade_sources = vec![current().identity()];
        new
    }
    struct Source {
        pages: Vec<String>,
        list: Result<String, String>,
        assets: HashMap<u64, String>,
        desktop: Result<String, String>,
        calls: RefCell<Vec<String>>,
    }
    impl ReleaseSource for Source {
        fn get(&self, endpoint: &str, accept: Accept) -> Result<String, String> {
            self.calls.borrow_mut().push(endpoint.into());
            match accept {
                Accept::Raw => {
                    assert_eq!(endpoint, COMPATIBILITY_ENDPOINT);
                    self.list.clone()
                }
                Accept::Binary => {
                    let id: u64 = endpoint.rsplit('/').next().unwrap().parse().unwrap();
                    self.assets.get(&id).cloned().ok_or("no such asset".into())
                }
                Accept::Json if endpoint == DESKTOP_LATEST_ENDPOINT => self.desktop.clone(),
                Accept::Json => {
                    let page: usize = endpoint.rsplit('=').next().unwrap().parse().unwrap();
                    self.pages
                        .get(page - 1)
                        .cloned()
                        .ok_or("unexpected page".into())
                }
            }
        }
    }
    fn container_image_txt(digest: char) -> String {
        format!(
            "Release: v0.9.1\nImage: {REPOSITORY}:0.9.1\nImmutable image: {}\n",
            image(digest)
        )
    }
    fn desktop_release(tag: &str, prerelease: bool) -> String {
        json!({"tag_name": tag, "prerelease": prerelease, "published_at": "2026-10-13T08:00:00Z"})
            .to_string()
    }
    fn versions(review: &UpdateReview) -> Vec<&str> {
        review
            .catalog
            .iter()
            .map(|entry| entry.version.as_str())
            .collect()
    }
    fn source() -> Source {
        Source {
            pages: vec![
                json!([
                    {"tag_name":"0.8.5", "body":"### Breaking Changes\nMigrate first."},
                    {"tag_name":"v0.9.0", "body":"<script>unsafe()</script>"},
                    {"tag_name":"v0.9.1-beta.1", "prerelease":true, "body":"beta",
                     "assets":[{"id":7, "name":"container-image.txt"}]},
                    {"tag_name":"9.0.0", "draft":true},
                    {"tag_name":"nonsense"}
                ])
                .to_string(),
            ],
            list: Ok(list(vec![
                listed("0.8.4", 3, 'e'),
                listed("0.8.5", 3, 'b'),
                listed("0.9.0", 3, 'a'),
            ])),
            assets: HashMap::from([(7, container_image_txt('c'))]),
            desktop: Ok(desktop_release("v0.0.1", false)),
            calls: RefCell::new(vec![]),
        }
    }

    #[test]
    fn offers_the_newest_listed_version_with_all_skipped_release_notes() {
        let review = check(&source(), &current(), false, false).unwrap();
        assert_eq!(review.latest_version, "0.9.0");
        assert!(review.listed && !review.prerelease);
        // Pre-releases stay out unless they are asked for.
        assert_eq!(review.releases.len(), 2);
        assert!(review.releases.iter().all(|release| !release.prerelease));
        assert!(review.breaking_changes);
        assert!(!review.releases[0].breaking_changes);
        assert!(review.releases[1].breaking_changes);
        let candidate = approve(
            &review,
            &current(),
            review.review_id.as_ref().unwrap(),
            true,
        )
        .unwrap();
        assert_eq!(candidate, next());
        // Only the exact running selection may be replaced.
        assert_eq!(candidate.upgrade_sources, vec![current().identity()]);
        let wire = serde_json::to_value(&review).unwrap();
        assert!(wire.get("candidate").is_none());
        assert!(wire.get("source_lock").is_none());
    }

    #[test]
    fn a_newer_release_not_on_the_list_is_named_but_not_offered() {
        let mut source = source();
        source.pages = vec![json!([{"tag_name":"v0.9.0"}, {"tag_name":"v0.9.1"}]).to_string()];
        let review = check(&source, &current(), false, false).unwrap();
        assert_eq!(review.latest_version, "0.9.0");
        assert_eq!(review.newer_unlisted.as_deref(), Some("0.9.1"));
        assert!(review.review_id.is_some());

        // With nothing newer on the list, the reason says why and how to try it.
        source.list = Ok(list(vec![listed("0.8.4", 3, 'e')]));
        let review = check(&source, &current(), false, false).unwrap();
        assert!(review.review_id.is_none());
        let reason = review.blocked_reason.unwrap();
        assert!(
            reason.contains("UA 0.9.1 is not on this launcher's compatibility list yet"),
            "{reason}"
        );
        assert!(
            reason.contains("Offer pre-release and unchecked versions"),
            "{reason}"
        );
    }

    #[test]
    fn a_release_for_a_newer_contract_asks_for_a_newer_desktop() {
        let mut source = source();
        source.pages = vec![json!([{"tag_name":"v1.0.0"}]).to_string()];
        source.list = Ok(list(vec![listed("0.8.4", 3, 'e'), listed("1.0.0", 4, 'f')]));
        let review = check(&source, &current(), false, false).unwrap();
        assert!(review.review_id.is_none());
        assert!(
            review
                .blocked_reason
                .unwrap()
                .contains("needs a newer desktop app")
        );
    }

    #[test]
    fn an_unreadable_or_invalid_list_prepares_nothing() {
        let mut source = source();
        source.list = Err("offline".into());
        let reason = check(&source, &current(), false, false)
            .unwrap()
            .blocked_reason
            .unwrap();
        assert!(
            reason.contains("Could not read the compatibility list"),
            "{reason}"
        );
        source.list = Ok("{\"schema_version\":1,\"runtimes\":[{\"ua_version\":\"0.9.0\"}]}".into());
        let reason = check(&source, &current(), false, false)
            .unwrap()
            .blocked_reason
            .unwrap();
        assert!(
            reason.contains("compatibility list on GitHub is invalid"),
            "{reason}"
        );

        // Someone who accepts unchecked versions is still offered one,
        // marked unchecked, when the list cannot be read.
        source.list = Err("not found".into());
        let review = check(&source, &current(), false, true).unwrap();
        assert_eq!(review.latest_version, "0.9.1-beta.1");
        assert!(!review.listed);
        assert!(review.review_id.is_some());
    }

    #[test]
    fn offering_unchecked_versions_installs_the_newest_published_prerelease() {
        let source = source();
        let review = check(&source, &current(), false, true).unwrap();
        assert_eq!(review.latest_version, "0.9.1-beta.1");
        assert!(!review.listed && review.prerelease);
        assert_eq!(review.image.as_deref(), Some(image('c').as_str()));
        assert_eq!(
            review.releases.len(),
            3,
            "the beta's notes are part of the review"
        );
        assert!(review.newer_unlisted.is_none());
        let candidate = approve(
            &review,
            &current(),
            review.review_id.as_ref().unwrap(),
            true,
        )
        .unwrap();
        assert_eq!(candidate.ua_version, "0.9.1-beta.1");
        assert_eq!(
            candidate.host_contract_version,
            current().host_contract_version
        );
    }

    #[test]
    fn unchecked_releases_without_a_usable_image_fall_back_to_older_ones() {
        let mut source = source();
        source.pages = vec![
            json!([
                {"tag_name":"v0.9.0"},
                {"tag_name":"v0.9.2-nightly.202610120300", "prerelease":true},
                {"tag_name":"v0.9.3-nightly.202610130300", "prerelease":true,
                 "assets":[{"id":8, "name":"container-image.txt"}]},
                {"tag_name":"v0.9.1", "assets":[{"id":7, "name":"container-image.txt"}]}
            ])
            .to_string(),
        ];
        // 0.9.3's image is in another repository; 0.9.2 has no image yet.
        source.assets.insert(
            8,
            format!(
                "Immutable image: ghcr.io/other/ua@sha256:{}\n",
                "d".repeat(64)
            ),
        );
        let review = check(&source, &current(), false, true).unwrap();
        assert_eq!(review.latest_version, "0.9.1");
        assert!(!review.listed && !review.prerelease);

        // Nothing usable and nothing listed: the reason says to try later.
        source.assets.clear();
        source.list = Ok(list(vec![listed("0.8.4", 3, 'e')]));
        let reason = check(&source, &current(), false, true)
            .unwrap()
            .blocked_reason
            .unwrap();
        assert!(reason.contains("no published image yet"), "{reason}");
    }

    #[test]
    fn a_listed_version_newer_than_every_unchecked_one_wins() {
        let mut source = source();
        source.list = Ok(list(vec![
            listed("0.8.4", 3, 'e'),
            listed("0.9.0", 3, 'a'),
            listed("0.9.1", 3, 'f'),
        ]));
        source.pages = vec![
            json!([
                {"tag_name":"v0.9.1-beta.1", "prerelease":true, "assets":[{"id":7, "name":"container-image.txt"}]},
                {"tag_name":"v0.9.1"}
            ])
            .to_string(),
        ];
        let review = check(&source, &current(), false, true).unwrap();
        assert_eq!(review.latest_version, "0.9.1");
        assert!(review.listed);
        assert!(
            !source
                .calls
                .borrow()
                .iter()
                .any(|call| call.contains("/assets/"))
        );
    }

    #[test]
    fn downloading_requires_a_current_review_but_not_acknowledgment() {
        let review = check(&source(), &current(), false, false).unwrap();
        let id = review.review_id.as_ref().unwrap();
        // Downloading never applies the update, so no acknowledgment is needed.
        assert_eq!(approve_download(&review, &current(), id).unwrap(), next());
        // A stale or unknown review still cannot fetch another digest.
        assert!(approve_download(&review, &current(), "other").is_err());
        assert!(approve_download(&review, &next(), id).is_err());
        let mut changed = current();
        changed.image = image('9');
        assert!(approve_download(&review, &changed, id).is_err());
        // A review without a prepared candidate cannot be downloaded.
        let mut up_to_date = source();
        up_to_date.pages = vec!["[]".into()];
        let blocked = check(&up_to_date, &current(), false, false).unwrap();
        assert!(approve_download(&blocked, &current(), "anything").is_err());
    }

    #[test]
    fn missing_acknowledgment_and_stale_reviews_cannot_install() {
        let review = check(&source(), &current(), false, false).unwrap();
        let id = review.review_id.as_ref().unwrap();
        assert!(approve(&review, &current(), id, false).is_err());
        assert!(approve(&review, &current(), "other", true).is_err());
        assert!(approve(&review, &next(), id, true).is_err());
    }

    #[test]
    fn updates_never_change_the_contract_go_backwards_or_replace_another_runtime() {
        let mut older = next();
        older.ua_version = "0.8.0".into();
        assert!(validate_update(&current(), &older).is_err());
        let mut changed = next();
        changed.container_ports.web += 1;
        assert!(validate_update(&current(), &changed).is_err());
        let mut elsewhere = next();
        elsewhere.image = elsewhere.image.replace("enzimo/", "other/");
        assert!(validate_update(&current(), &elsewhere).is_err());
        let mut unrelated = next();
        unrelated.upgrade_sources.clear();
        assert!(validate_update(&current(), &unrelated).is_err());
        assert!(validate_update(&current(), &next()).is_ok());
    }

    #[test]
    fn pending_selection_never_prepares_another_update() {
        let source = source();
        let review = check(&source, &current(), true, false).unwrap();
        assert!(review.update_pending);
        assert!(review.review_id.is_none());
        assert!(review.catalog.is_empty());
        // Only the release list and the launcher release are read.
        assert_eq!(source.calls.borrow().len(), 2);
        assert!(
            !source
                .calls
                .borrow()
                .contains(&COMPATIBILITY_ENDPOINT.to_owned())
        );
        assert!(review_for(&review, &current(), "0.9.0").is_err());
    }

    #[test]
    fn release_order_is_semantic_and_pagination_does_not_skip_older_creation_dates() {
        let mut source = source();
        source.pages = vec![
            serde_json::to_string(&vec![json!({"tag_name":"0.1.0"}); 100]).unwrap(),
            json!([{"tag_name":"0.9.0"}, {"tag_name":"0.10.0"}]).to_string(),
        ];
        source.list = Ok(list(vec![
            listed("0.9.0", 3, 'a'),
            listed("0.10.0", 3, 'b'),
        ]));
        let review = check(&source, &current(), false, false).unwrap();
        assert_eq!(review.latest_version, "0.10.0");
        assert_eq!(review.releases.len(), 2);
        // Two release pages, the launcher release and the list.
        assert_eq!(source.calls.borrow().len(), 4);
    }

    #[test]
    fn up_to_date_check_lists_recent_versions_but_does_not_need_the_list() {
        let mut source = source();
        source.pages = vec![json!([{"tag_name":"v0.8.4"}]).to_string()];
        source.list = Ok(list(vec![listed("0.8.3", 3, 'd'), listed("0.8.4", 3, 'e')]));
        let review = check(&source, &current(), false, false).unwrap();
        assert_eq!(review.latest_version, current().ua_version);
        assert!(review.releases.is_empty());
        assert!(review.review_id.is_none() && review.blocked_reason.is_none());
        assert_eq!(versions(&review), ["0.8.4", "0.8.3"]);
        assert_eq!(review.catalog[0].relation, VersionRelation::Running);
        assert_eq!(review.catalog[1].relation, VersionRelation::Older);
        // An unreadable list leaves nothing to prepare, so it is not an error.
        source.list = Err("offline".into());
        let review = check(&source, &current(), false, false).unwrap();
        assert!(review.blocked_reason.is_none());
        assert!(review.catalog.is_empty());
    }

    #[test]
    fn the_catalog_lists_five_versions_newest_first_and_keeps_the_running_one() {
        let mut source = source();
        let names = [
            "0.8.2", "0.8.3", "0.8.4", "0.8.5", "0.9.0", "0.9.1", "0.9.2", "0.9.3",
        ];
        source.pages = vec![
            serde_json::to_string(
                &names
                    .iter()
                    .map(|name| json!({"tag_name": format!("v{name}"), "published_at": format!("2026-10-01T00:00:00Z{name}")}))
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        ];
        let digests = ['1', '2', 'e', '4', '5', '6', '7', '8'];
        source.list = Ok(list(
            names
                .iter()
                .zip(digests)
                .map(|(name, digest)| listed(name, 3, digest))
                .chain([listed("1.0.0", 4, '9')])
                .collect(),
        ));
        let review = check(&source, &current(), false, false).unwrap();
        // The running version replaces the fifth row; another contract is skipped.
        assert_eq!(
            versions(&review),
            ["0.9.3", "0.9.2", "0.9.1", "0.9.0", "0.8.4"]
        );
        assert!(review.catalog[0].recommended);
        assert!(review.catalog[1..].iter().all(|entry| !entry.recommended));
        assert!(
            review.catalog[..4]
                .iter()
                .all(|entry| entry.relation == VersionRelation::Newer)
        );
        assert_eq!(review.catalog[4].relation, VersionRelation::Running);
        assert!(
            review
                .catalog
                .iter()
                .all(|entry| entry.listed && !entry.prerelease)
        );
        assert_eq!(
            review.catalog[0].notes_url.as_deref(),
            Some("https://github.com/enzimo/universal-agents/releases/tag/v0.9.3")
        );
        assert_eq!(
            review.catalog[0].published_at.as_deref(),
            Some("2026-10-01T00:00:00Z0.9.3")
        );
        assert_eq!(review.catalog[0].image, image('8'));

        // With few newer versions, older ones fill the rest for reference.
        source.list = Ok(list(
            names[..4]
                .iter()
                .zip(digests)
                .map(|(name, digest)| listed(name, 3, digest))
                .collect(),
        ));
        let review = check(&source, &current(), false, false).unwrap();
        assert_eq!(versions(&review), ["0.8.5", "0.8.4", "0.8.3", "0.8.2"]);
        let relations: Vec<_> = review.catalog.iter().map(|entry| entry.relation).collect();
        assert_eq!(
            relations,
            [
                VersionRelation::Newer,
                VersionRelation::Running,
                VersionRelation::Older,
                VersionRelation::Older
            ]
        );
    }

    #[test]
    fn unchecked_and_prerelease_versions_are_listed_only_when_offered() {
        let mut source = source();
        source.list = Ok(list(vec![
            listed("0.8.4", 3, 'e'),
            listed("0.8.5", 3, 'b'),
            listed("0.9.0", 3, 'a'),
            listed("0.9.1-rc.1", 3, 'f'),
        ]));
        let review = check(&source, &current(), false, false).unwrap();
        assert_eq!(versions(&review), ["0.9.0", "0.8.5", "0.8.4"]);

        source.pages = vec![
            json!([
                {"tag_name":"0.8.5"}, {"tag_name":"v0.9.0"},
                {"tag_name":"v0.9.1-rc.1", "prerelease":true},
                {"tag_name":"v0.9.2-beta.1", "prerelease":true,
                 "assets":[{"id":7, "name":"container-image.txt"}]}
            ])
            .to_string(),
        ];
        let review = check(&source, &current(), false, true).unwrap();
        assert_eq!(
            versions(&review),
            ["0.9.2-beta.1", "0.9.1-rc.1", "0.9.0", "0.8.5", "0.8.4"]
        );
        let beta = &review.catalog[0];
        assert!(!beta.listed && beta.prerelease && beta.recommended);
        assert_eq!(beta.image, image('c'));
        let rc = &review.catalog[1];
        assert!(rc.listed && rc.prerelease && !rc.recommended);
    }

    #[test]
    fn choosing_a_newer_catalog_version_reviews_only_the_notes_up_to_it() {
        let source = source();
        let base = check(&source, &current(), false, false).unwrap();
        let calls = source.calls.borrow().len();
        let chosen = review_for(&base, &current(), "0.8.5").unwrap();
        // Built from the last check: no further GitHub requests.
        assert_eq!(source.calls.borrow().len(), calls);
        assert_eq!(chosen.latest_version, "0.8.5");
        assert_eq!(chosen.releases.len(), 1);
        assert_eq!(chosen.releases[0].version, "0.8.5");
        assert!(chosen.breaking_changes && chosen.listed && chosen.blocked_reason.is_none());
        assert_eq!(chosen.image.as_deref(), Some(image('b').as_str()));
        assert_ne!(chosen.review_id, base.review_id);
        assert_eq!(versions(&chosen), versions(&base));
        let candidate = approve(
            &chosen,
            &current(),
            chosen.review_id.as_ref().unwrap(),
            true,
        )
        .unwrap();
        assert_eq!(candidate.ua_version, "0.8.5");
        assert_eq!(candidate.upgrade_sources, vec![current().identity()]);
        // The offered review's id does not install the chosen version.
        assert!(approve(&chosen, &current(), base.review_id.as_ref().unwrap(), true).is_err());
        // Choosing the offered version again gives the same review.
        let again = review_for(&chosen, &current(), "0.9.0").unwrap();
        assert_eq!(again.review_id, base.review_id);
        assert_eq!(again.releases.len(), base.releases.len());
    }

    #[test]
    fn only_newer_catalog_versions_can_be_chosen() {
        let base = check(&source(), &current(), false, false).unwrap();
        let error = review_for(&base, &current(), "0.8.4").unwrap_err();
        assert!(error.contains("Only a newer runtime"), "{error}");
        let mut older = source();
        older.list = Ok(list(vec![
            listed("0.8.3", 3, 'd'),
            listed("0.8.4", 3, 'e'),
            listed("0.9.0", 3, 'a'),
        ]));
        let base = check(&older, &current(), false, false).unwrap();
        let error = review_for(&base, &current(), "0.8.3").unwrap_err();
        assert!(error.contains("Only a newer runtime"), "{error}");
        let error = review_for(&base, &current(), "0.9.1-beta.1").unwrap_err();
        assert!(error.contains("not one of the versions offered"), "{error}");
        // A review made for another running selection is stale.
        assert!(
            review_for(&base, &next(), "0.9.0")
                .unwrap_err()
                .contains("stale")
        );
    }

    #[test]
    fn the_launcher_release_is_compared_without_affecting_the_ua_check() {
        let check_tag = |tag: &str, prerelease: bool| {
            let mut source = source();
            source.desktop = Ok(desktop_release(tag, prerelease));
            check_desktop(&source, "0.5.0")
        };
        let newer = check_tag("v0.5.5", false);
        assert!(newer.newer && newer.error.is_none());
        assert_eq!(newer.latest_version.as_deref(), Some("0.5.5"));
        assert_eq!(newer.published_at.as_deref(), Some("2026-10-13T08:00:00Z"));
        assert!(!check_tag("v0.5.0", false).newer);
        assert!(!check_tag("0.4.9", false).newer);
        let prerelease = check_tag("v0.6.0-beta.1", true);
        assert!(
            !prerelease.newer && prerelease.latest_version.is_none() && prerelease.error.is_none()
        );
        let unreadable = check_tag("latest", false);
        assert!(!unreadable.newer);
        assert!(unreadable.error.unwrap().contains("unreadable version"));

        // A failed launcher check is reported in its own lane only.
        let mut source = source();
        source.desktop = Err("offline".into());
        let review = check(&source, &current(), false, false).unwrap();
        assert!(review.review_id.is_some());
        assert!(!review.desktop.newer);
        assert!(review.desktop.error.unwrap().contains("newer launcher"));
    }

    #[test]
    fn release_pages_are_limited_to_versions_the_last_check_returned() {
        let mut source = source();
        source.desktop = Ok(desktop_release("v9.9.9", false));
        let review = check(&source, &current(), false, false).unwrap();
        assert_eq!(
            release_page_url(&review, "launcher", "9.9.9").unwrap(),
            "https://github.com/enzimo/universal-agents-desktop/releases/tag/v9.9.9"
        );
        assert!(release_page_url(&review, "launcher", "9.9.8").is_err());
        assert_eq!(
            release_page_url(&review, "runtime", "0.8.5").unwrap(),
            "https://github.com/enzimo/universal-agents/releases/tag/0.8.5"
        );
        assert!(release_page_url(&review, "runtime", "0.1.0").is_err());
        assert!(release_page_url(&review, "https://example.com", "9.9.9").is_err());
    }

    #[test]
    fn first_install_moves_only_forward_to_a_stable_listed_version() {
        let compatibility = CompatibilityList::from_json(&list(vec![
            listed("0.8.4", 3, 'e'),
            listed("0.9.0", 3, 'a'),
            listed("0.9.1-beta.1", 3, 'c'),
        ]))
        .unwrap();
        let target = first_install_target(&compatibility, &current()).unwrap();
        assert_eq!(target.identity(), next().identity());
        assert!(target.upgrade_sources.is_empty());
        assert!(first_install_target(&compatibility, &next()).is_none());
    }

    #[test]
    fn published_image_reads_only_the_exact_digest_in_the_same_repository() {
        assert_eq!(
            published_image(&container_image_txt('c'), REPOSITORY),
            Some(image('c'))
        );
        assert!(
            published_image("Image: ghcr.io/enzimo/universal-agents:0.9.1\n", REPOSITORY).is_none()
        );
        assert!(published_image(&container_image_txt('c'), "ghcr.io/other/ua").is_none());
        assert!(
            published_image(
                "Immutable image: ghcr.io/enzimo/universal-agents@sha256:short",
                REPOSITORY
            )
            .is_none()
        );
    }

    #[test]
    fn selection_survives_desktop_relaunch_and_failed_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let store = SelectionStore {
            path: dir.path().join("runtime-lock.json"),
        };
        assert_eq!(
            store.load_or_initialize(&current()).unwrap().lock,
            current()
        );
        store
            .save(&RuntimeSelection {
                lock: next(),
                update_pending: true,
            })
            .unwrap();
        let restored = store.load_or_initialize(&current()).unwrap();
        assert_eq!(restored.lock, next());
        assert!(restored.update_pending);
        // A failed replacement must retain the selected version on disk.
        fs::create_dir(store.path.with_extension("json.tmp")).unwrap();
        assert!(
            store
                .save(&RuntimeSelection {
                    lock: current(),
                    update_pending: false
                })
                .is_err()
        );
        assert_eq!(store.load_or_initialize(&current()).unwrap().lock, next());
        fs::write(&store.path, "corrupt").unwrap();
        assert!(store.load_or_initialize(&current()).is_err());
        assert_eq!(fs::read_to_string(&store.path).unwrap(), "corrupt");
    }

    #[test]
    fn older_desktop_selection_is_refused_with_plain_guidance() {
        let dir = tempfile::tempdir().unwrap();
        let store = SelectionStore {
            path: dir.path().join("runtime-lock.json"),
        };
        // Desktop 0.4.x saved host contract 2 selections like this one.
        let saved = include_str!("../tests/fixtures/runtime-v0.4.2.json");
        fs::write(&store.path, saved).unwrap();
        let error = store.load_or_initialize(&current()).unwrap_err();
        assert!(
            error.contains("older Universal Agents desktop app (UA 0.4.2)"),
            "{error}"
        );
        assert!(!error.contains("host contract"), "{error}");
        assert!(error.contains("Nothing was changed"), "{error}");
        assert_eq!(fs::read_to_string(&store.path).unwrap(), saved);
    }
}
