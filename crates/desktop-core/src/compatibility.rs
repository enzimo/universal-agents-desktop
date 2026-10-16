//! The UA versions known to work with each desktop host contract.
//!
//! `compatible-runtimes.json` at the repository root is the single list of UA
//! releases a launcher may install. Each entry names one UA version, its exact
//! image digest and the host contract it implements. A launcher offers the
//! newest entry for its own contract; entries for other contracts are skipped,
//! so launchers of different contracts share one file. The launcher reads the
//! list from this repository's `main` branch when it checks for updates and
//! uses the copy built into it when GitHub cannot be reached.
//!
//! Each stable UA release opens a pull request that appends itself; the
//! repository's compatibility workflow verifies the digest against the UA
//! release and that the UA files defining the contract are unchanged before
//! merging it (see docs/COMPATIBILITY_LIST.md).

use semver::Version;
use serde::Deserialize;

use crate::RuntimeIdentity;

pub const COMPATIBILITY_LIST_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListFile {
    schema_version: u32,
    runtimes: Vec<ListEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListEntry {
    ua_version: String,
    host_contract_version: u32,
    image: String,
}

/// A validated compatibility list, in ascending version order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompatibilityList {
    runtimes: Vec<RuntimeIdentity>,
}

impl CompatibilityList {
    pub fn from_json(text: &str) -> Result<Self, String> {
        let file: ListFile = serde_json::from_str(text)
            .map_err(|error| format!("the compatibility list is not valid: {error}"))?;
        if file.schema_version != COMPATIBILITY_LIST_SCHEMA_VERSION {
            return Err(format!(
                "the compatibility list uses schema {}; this launcher reads schema {COMPATIBILITY_LIST_SCHEMA_VERSION}",
                file.schema_version
            ));
        }
        let mut previous: Option<Version> = None;
        let mut runtimes = Vec::with_capacity(file.runtimes.len());
        for entry in file.runtimes {
            let version = parse_version(&entry.ua_version).ok_or_else(|| {
                format!(
                    "the compatibility list has an invalid UA version '{}'",
                    entry.ua_version
                )
            })?;
            if previous
                .as_ref()
                .is_some_and(|previous| &version <= previous)
            {
                return Err(format!(
                    "the compatibility list must be in ascending version order without repeats; UA {} is out of place",
                    entry.ua_version
                ));
            }
            if entry.host_contract_version == 0 {
                return Err(format!(
                    "UA {} in the compatibility list has no host contract",
                    entry.ua_version
                ));
            }
            crate::runtime_lock::validate_digest_pinned_image(&entry.image).map_err(|error| {
                format!("UA {} in the compatibility list: {error}", entry.ua_version)
            })?;
            if runtimes
                .iter()
                .any(|known: &RuntimeIdentity| known.image == entry.image)
            {
                return Err(format!(
                    "UA {} in the compatibility list repeats another version's image",
                    entry.ua_version
                ));
            }
            previous = Some(version);
            runtimes.push(RuntimeIdentity {
                host_contract_version: entry.host_contract_version,
                ua_version: entry.ua_version,
                image: entry.image,
            });
        }
        Ok(Self { runtimes })
    }

    /// A list with no entries: nothing is known to be compatible.
    pub fn empty() -> Self {
        Self {
            runtimes: Vec::new(),
        }
    }

    pub fn runtimes(&self) -> &[RuntimeIdentity] {
        &self.runtimes
    }

    pub fn find(&self, ua_version: &str) -> Option<&RuntimeIdentity> {
        self.runtimes
            .iter()
            .find(|runtime| runtime.ua_version == ua_version)
    }

    /// The newest entry a launcher of `contract` may install from
    /// `repository`. Pre-release entries count only when `include_prereleases`.
    pub fn newest_for(
        &self,
        contract: u32,
        repository: &str,
        include_prereleases: bool,
    ) -> Option<&RuntimeIdentity> {
        self.runtimes
            .iter()
            .rev()
            .filter(|runtime| runtime.host_contract_version == contract)
            .filter(|runtime| image_repository(&runtime.image) == Some(repository))
            .find(|runtime| include_prereleases || !is_prerelease(&runtime.ua_version))
    }
}

/// A UA version without its optional leading `v`, as a semantic version.
/// Pre-releases (`0.9.1-beta.1`, `0.9.1-nightly.202610120300`) sort before
/// their release.
pub fn parse_version(value: &str) -> Option<Version> {
    Version::parse(value.strip_prefix('v').unwrap_or(value)).ok()
}

pub fn is_prerelease(value: &str) -> bool {
    parse_version(value).is_some_and(|version| !version.pre.is_empty())
}

/// The repository part of a digest-pinned image reference.
pub fn image_repository(image: &str) -> Option<&str> {
    image
        .split_once("@sha256:")
        .map(|(repository, _)| repository)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPOSITORY: &str = "ghcr.io/enzimo/universal-agents";

    fn entry(version: &str, contract: u32, digest: char) -> serde_json::Value {
        serde_json::json!({
            "ua_version": version,
            "host_contract_version": contract,
            "image": format!("{REPOSITORY}@sha256:{}", digest.to_string().repeat(64)),
        })
    }

    fn list(entries: Vec<serde_json::Value>) -> String {
        serde_json::json!({"schema_version": 1, "runtimes": entries}).to_string()
    }

    #[test]
    fn repository_list_is_valid_and_offers_its_newest_contract_entry() {
        let list = CompatibilityList::from_json(include_str!("../../../compatible-runtimes.json"))
            .expect("repository compatibility list");
        let newest = list
            .newest_for(crate::SUPPORTED_HOST_CONTRACT_VERSION, REPOSITORY, false)
            .expect("an entry for this launcher");
        assert_eq!(newest, list.runtimes().last().unwrap());
    }

    #[test]
    fn newest_entry_follows_contract_repository_and_prerelease_choice() {
        let list = CompatibilityList::from_json(&list(vec![
            entry("0.8.5", 3, 'a'),
            entry("0.9.0", 3, 'b'),
            entry("0.9.1-beta.1", 3, 'c'),
            entry("1.0.0", 4, 'd'),
        ]))
        .unwrap();
        assert_eq!(
            list.newest_for(3, REPOSITORY, false).unwrap().ua_version,
            "0.9.0"
        );
        assert_eq!(
            list.newest_for(3, REPOSITORY, true).unwrap().ua_version,
            "0.9.1-beta.1"
        );
        assert_eq!(
            list.newest_for(4, REPOSITORY, false).unwrap().ua_version,
            "1.0.0"
        );
        assert!(list.newest_for(3, "ghcr.io/other/ua", false).is_none());
        assert!(list.newest_for(2, REPOSITORY, true).is_none());
        assert_eq!(list.find("0.8.5").unwrap().host_contract_version, 3);
    }

    #[test]
    fn rejects_lists_that_could_mislead_an_update() {
        let cases = [
            (
                "unordered",
                list(vec![entry("0.9.0", 3, 'a'), entry("0.8.5", 3, 'b')]),
            ),
            (
                "repeated version",
                list(vec![entry("0.9.0", 3, 'a'), entry("0.9.0", 3, 'b')]),
            ),
            (
                "repeated image",
                list(vec![entry("0.8.5", 3, 'a'), entry("0.9.0", 3, 'a')]),
            ),
            ("bad version", list(vec![entry("nightly", 3, 'a')])),
            ("no contract", list(vec![entry("0.9.0", 0, 'a')])),
            (
                "mutable image",
                list(vec![
                    serde_json::json!({"ua_version":"0.9.0","host_contract_version":3,"image":"ghcr.io/enzimo/universal-agents:0.9.0"}),
                ]),
            ),
            (
                "unknown field",
                list(vec![
                    serde_json::json!({"ua_version":"0.9.0","host_contract_version":3,"image":format!("{REPOSITORY}@sha256:{}", "a".repeat(64)),"approved":true}),
                ]),
            ),
            ("schema", r#"{"schema_version":2,"runtimes":[]}"#.to_owned()),
            ("not json", "nope".to_owned()),
        ];
        for (name, text) in cases {
            assert!(CompatibilityList::from_json(&text).is_err(), "{name}");
        }
    }

    #[test]
    fn prerelease_versions_sort_before_their_release() {
        let beta = parse_version("v0.9.1-beta.1").unwrap();
        let nightly = parse_version("0.9.1-nightly.202610120300").unwrap();
        let stable = parse_version("0.9.1").unwrap();
        assert!(parse_version("0.9.0").unwrap() < beta);
        assert!(beta < nightly && nightly < stable);
        assert!(is_prerelease("0.9.1-beta.1"));
        assert!(!is_prerelease("0.9.1"));
    }
}
