use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::RuntimeError;

/// Version 3 mounts a selected folder at UA's shared team folder
/// (`/data/workspaces/team/team-shared`) instead of the pre-0.6 single
/// `/data/workspace`, which UA 0.6 and later treat as an old layout and refuse
/// to start with. It also drops the unused `assistant_cli` path.
pub const SUPPORTED_HOST_CONTRACT_VERSION: u32 = 3;

/// The only part of the instance root that UA lets agents reach.
const AGENT_WORKSPACES_DIR: &str = "workspaces";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ContainerPorts {
    pub web: u16,
    pub broker: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimeIdentity {
    pub host_contract_version: u32,
    pub ua_version: String,
    pub image: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimeLock {
    pub host_contract_version: u32,
    pub ua_version: String,
    pub image: String,
    #[serde(default)]
    pub upgrade_sources: Vec<RuntimeIdentity>,
    pub command: Vec<String>,
    pub data_mount: String,
    pub workspace_mount: String,
    pub codex_auth_mount: String,
    pub codex_home: String,
    pub codex_auth_store: String,
    pub instance_config: String,
    pub codex_import_command: Vec<String>,
    pub container_name: String,
    pub container_ports: ContainerPorts,
    pub web_ui_path: String,
    pub onboarding_ui_path: String,
    pub docs_path: String,
    pub health_path: String,
}

/// Keys `runtime-lock.json` must not contain: which UA version runs comes only
/// from the compatibility list, so the two files cannot disagree.
const IDENTITY_KEYS: [&str; 3] = ["ua_version", "image", "upgrade_sources"];

impl RuntimeLock {
    /// The contract built into this launcher combined with the newest
    /// compatible UA release in its built-in compatibility list. Used for a
    /// first install when GitHub cannot be reached, and by tests and tools.
    pub fn bundled() -> Result<Self, RuntimeError> {
        let profile = include_str!("../../../runtime-lock.json");
        let list =
            crate::CompatibilityList::from_json(include_str!("../../../compatible-runtimes.json"))
                .map_err(RuntimeError::InvalidRuntimeLock)?;
        let repository = Self::profile_repository(profile)?;
        let identity = list
            .newest_for(SUPPORTED_HOST_CONTRACT_VERSION, &repository, false)
            .ok_or_else(|| {
                RuntimeError::InvalidRuntimeLock(
                    "the built-in compatibility list has no stable UA release for this launcher"
                        .to_owned(),
                )
            })?;
        Self::from_profile_json(profile, identity)
    }

    /// Combine the host contract in `runtime-lock.json` (everything but the
    /// UA version and image) with one UA release from the compatibility list.
    pub fn from_profile_json(
        profile: &str,
        identity: &RuntimeIdentity,
    ) -> Result<Self, RuntimeError> {
        let mut value: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(profile)
                .map_err(|error| RuntimeError::InvalidRuntimeLock(error.to_string()))?;
        if let Some(key) = IDENTITY_KEYS.iter().find(|key| value.contains_key(**key)) {
            return Err(RuntimeError::InvalidRuntimeLock(format!(
                "runtime-lock.json must not contain '{key}'; UA versions come from compatible-runtimes.json"
            )));
        }
        let repository = Self::profile_repository(profile)?;
        if crate::compatibility::image_repository(&identity.image) != Some(repository.as_str()) {
            return Err(RuntimeError::InvalidRuntimeLock(format!(
                "UA {} is not published in the image repository {repository}",
                identity.ua_version
            )));
        }
        value.remove("image_repository");
        let contract = value
            .get("host_contract_version")
            .and_then(serde_json::Value::as_u64);
        if contract != Some(u64::from(identity.host_contract_version)) {
            return Err(RuntimeError::InvalidRuntimeLock(format!(
                "UA {} implements host contract {}, not this launcher's {:?}",
                identity.ua_version, identity.host_contract_version, contract
            )));
        }
        value.insert("ua_version".to_owned(), identity.ua_version.clone().into());
        value.insert("image".to_owned(), identity.image.clone().into());
        Self::from_json(&serde_json::Value::Object(value).to_string())
    }

    fn profile_repository(profile: &str) -> Result<String, RuntimeError> {
        let value: serde_json::Value = serde_json::from_str(profile)
            .map_err(|error| RuntimeError::InvalidRuntimeLock(error.to_string()))?;
        value
            .get("image_repository")
            .and_then(serde_json::Value::as_str)
            .filter(|repository| !repository.trim().is_empty() && !repository.contains('@'))
            .map(str::to_owned)
            .ok_or_else(|| {
                RuntimeError::InvalidRuntimeLock(
                    "runtime-lock.json must name the image_repository UA is published in"
                        .to_owned(),
                )
            })
    }

    pub fn identity(&self) -> RuntimeIdentity {
        RuntimeIdentity {
            host_contract_version: self.host_contract_version,
            ua_version: self.ua_version.clone(),
            image: self.image.clone(),
        }
    }

    pub fn from_json(contents: &str) -> Result<Self, RuntimeError> {
        let runtime_lock: Self = serde_json::from_str(contents)
            .map_err(|error| RuntimeError::InvalidRuntimeLock(error.to_string()))?;
        runtime_lock.validate()?;
        Ok(runtime_lock)
    }

    pub fn validate(&self) -> Result<(), RuntimeError> {
        if self.host_contract_version != SUPPORTED_HOST_CONTRACT_VERSION {
            return Err(RuntimeError::InvalidRuntimeLock(format!(
                "unsupported host contract version {}; expected {}",
                self.host_contract_version, SUPPORTED_HOST_CONTRACT_VERSION
            )));
        }

        validate_digest_pinned_image(&self.image)?;

        if self.ua_version.trim().is_empty() {
            return Err(RuntimeError::InvalidRuntimeLock(
                "ua_version cannot be empty".to_owned(),
            ));
        }
        for source in &self.upgrade_sources {
            if source.host_contract_version == 0 {
                return Err(RuntimeError::InvalidRuntimeLock(
                    "upgrade source host_contract_version must be positive".to_owned(),
                ));
            }
            if source.ua_version.trim().is_empty() {
                return Err(RuntimeError::InvalidRuntimeLock(
                    "upgrade source ua_version cannot be empty".to_owned(),
                ));
            }
            validate_digest_pinned_image(&source.image)?;
            if source.host_contract_version == self.host_contract_version
                && source.ua_version == self.ua_version
                && source.image == self.image
            {
                return Err(RuntimeError::InvalidRuntimeLock(
                    "the current runtime identity cannot also be an upgrade source".to_owned(),
                ));
            }
        }
        for (index, source) in self.upgrade_sources.iter().enumerate() {
            if self.upgrade_sources[..index].contains(source) {
                return Err(RuntimeError::InvalidRuntimeLock(
                    "upgrade source identities must be unique".to_owned(),
                ));
            }
        }
        if self.command != ["run"] {
            return Err(RuntimeError::InvalidRuntimeLock(
                "the macOS host contract requires command [\"run\"]".to_owned(),
            ));
        }
        if self.codex_import_command.len() != 3
            || self.codex_import_command[0] != "python"
            || self.codex_import_command[1] != "-c"
            || self.codex_import_command[2].trim().is_empty()
        {
            return Err(RuntimeError::InvalidRuntimeLock(
                "codex_import_command must contain python, -c, and the reviewed UA import invocation".to_owned(),
            ));
        }
        if self.data_mount != "/data" {
            return Err(RuntimeError::InvalidRuntimeLock(
                "the macOS host contract requires data_mount /data".to_owned(),
            ));
        }
        // Agents can only open folders below `<data>/workspaces/`. A mount
        // anywhere else in the instance root is either invisible to them or
        // collides with UA's control, state, and credential areas.
        validate_child_path(
            "workspace_mount",
            &self.workspace_mount,
            &self.agent_workspaces_mount(),
        )?;
        validate_child_path("codex_auth_store", &self.codex_auth_store, &self.data_mount)?;
        validate_child_path("instance_config", &self.instance_config, &self.data_mount)?;
        validate_absolute_path("codex_home", &self.codex_home)?;
        validate_absolute_path("codex_auth_mount", &self.codex_auth_mount)?;
        let expected_codex_auth_mount =
            format!("{}/auth.json", self.codex_home.trim_end_matches('/'));
        if self.codex_auth_mount != expected_codex_auth_mount {
            return Err(RuntimeError::InvalidRuntimeLock(format!(
                "codex_auth_mount must equal {expected_codex_auth_mount}"
            )));
        }
        if self.container_name.trim().is_empty() {
            return Err(RuntimeError::InvalidRuntimeLock(
                "container_name cannot be empty".to_owned(),
            ));
        }
        if self.container_ports.web == self.container_ports.broker {
            return Err(RuntimeError::InvalidRuntimeLock(
                "web and broker ports must differ".to_owned(),
            ));
        }
        validate_http_path("web_ui_path", &self.web_ui_path)?;
        validate_http_path("onboarding_ui_path", &self.onboarding_ui_path)?;
        validate_http_path("docs_path", &self.docs_path)?;
        validate_http_path("health_path", &self.health_path)?;
        Ok(())
    }

    fn agent_workspaces_mount(&self) -> String {
        format!(
            "{}/{AGENT_WORKSPACES_DIR}",
            self.data_mount.trim_end_matches('/')
        )
    }

    /// Host folder behind `workspace_mount` when no folder is selected.
    ///
    /// The whole data directory is mounted at `data_mount`, so the default is
    /// the same relative path inside it. The Access panel and "Open folder"
    /// use this so they always show the folder agents actually see.
    pub fn default_workspace_dir(&self, data_dir: &Path) -> PathBuf {
        let prefix = format!("{}/", self.data_mount.trim_end_matches('/'));
        self.workspace_mount
            .strip_prefix(&prefix)
            .unwrap_or_default()
            .split('/')
            .filter(|part| !part.is_empty())
            .fold(data_dir.to_path_buf(), |path, part| path.join(part))
    }

    pub fn web_url(&self) -> String {
        format!(
            "http://127.0.0.1:{}{}",
            self.container_ports.web, self.web_ui_path
        )
    }

    pub fn onboarding_url(&self) -> String {
        format!(
            "http://127.0.0.1:{}{}",
            self.container_ports.web, self.onboarding_ui_path
        )
    }

    pub fn docs_url(&self) -> String {
        format!(
            "http://127.0.0.1:{}{}",
            self.container_ports.web, self.docs_path
        )
    }

    pub fn health_url(&self) -> String {
        format!(
            "http://127.0.0.1:{}{}",
            self.container_ports.web, self.health_path
        )
    }
}

pub(crate) fn validate_digest_pinned_image(image: &str) -> Result<(), RuntimeError> {
    let Some((repository, digest)) = image.split_once("@sha256:") else {
        return Err(RuntimeError::InvalidRuntimeLock(
            "image must be pinned with @sha256:<64 hexadecimal characters>".to_owned(),
        ));
    };
    if repository.trim().is_empty()
        || digest.len() != 64
        || !digest.bytes().all(|value| value.is_ascii_hexdigit())
    {
        return Err(RuntimeError::InvalidRuntimeLock(
            "image must contain a repository and a 64-character SHA-256 digest".to_owned(),
        ));
    }
    Ok(())
}

fn validate_http_path(name: &str, path: &str) -> Result<(), RuntimeError> {
    if !path.starts_with('/') || path.starts_with("//") {
        return Err(RuntimeError::InvalidRuntimeLock(format!(
            "{name} must be an absolute single-origin path"
        )));
    }
    Ok(())
}

fn validate_absolute_path(name: &str, path: &str) -> Result<(), RuntimeError> {
    if !path.starts_with('/') || path.contains("/../") || path.ends_with("/..") {
        return Err(RuntimeError::InvalidRuntimeLock(format!(
            "{name} must be an absolute normalized container path"
        )));
    }
    Ok(())
}

fn validate_child_path(name: &str, path: &str, parent: &str) -> Result<(), RuntimeError> {
    validate_absolute_path(name, path)?;
    let prefix = format!("{}/", parent.trim_end_matches('/'));
    if !path.starts_with(&prefix) {
        return Err(RuntimeError::InvalidRuntimeLock(format!(
            "{name} must be below {parent}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn valid_lock() -> RuntimeLock {
        RuntimeLock {
            host_contract_version: SUPPORTED_HOST_CONTRACT_VERSION,
            ua_version: "1.2.3".to_owned(),
            image: format!("ghcr.io/example/ua@sha256:{DIGEST}"),
            upgrade_sources: Vec::new(),
            command: vec!["run".to_owned()],
            data_mount: "/data".to_owned(),
            workspace_mount: "/data/workspaces/team/team-shared".to_owned(),
            codex_auth_mount: "/codex-home/auth.json".to_owned(),
            codex_home: "/codex-home".to_owned(),
            codex_auth_store: "/data/auth/openai_codex_auth.json".to_owned(),
            instance_config: "/data/instance_config.yaml".to_owned(),
            codex_import_command: vec![
                "python".to_owned(),
                "-c".to_owned(),
                "reviewed import".to_owned(),
            ],
            container_name: "ua-desktop".to_owned(),
            container_ports: ContainerPorts {
                web: 18090,
                broker: 18600,
            },
            web_ui_path: "/webchat".to_owned(),
            onboarding_ui_path: "/".to_owned(),
            docs_path: "/docs/".to_owned(),
            health_path: "/health".to_owned(),
        }
    }

    #[test]
    fn accepts_digest_pinned_runtime() {
        valid_lock().validate().expect("valid runtime lock");
    }

    #[test]
    fn rejects_missing_or_malformed_import_invocation() {
        for command in [
            vec![],
            vec!["python"],
            vec!["sh", "-c", "import"],
            vec!["python", "-c", " "],
        ] {
            let mut lock = valid_lock();
            lock.codex_import_command = command.into_iter().map(str::to_owned).collect();
            assert!(
                lock.validate()
                    .unwrap_err()
                    .to_string()
                    .contains("codex_import_command")
            );
        }
    }

    #[test]
    fn repository_runtime_lock_matches_the_supported_contract() {
        RuntimeLock::bundled().expect("repository runtime lock");
    }

    #[test]
    fn reviewed_routes_separate_setup_chat_and_docs() {
        let lock = RuntimeLock::bundled().unwrap();
        assert_eq!(lock.web_url(), "http://127.0.0.1:18090/webchat");
        assert_eq!(lock.onboarding_url(), "http://127.0.0.1:18090/");
        assert_eq!(lock.docs_url(), "http://127.0.0.1:18090/docs/");
    }

    #[test]
    fn workspace_mount_must_be_inside_the_agent_workspaces() {
        for path in [
            "/data/workspace",
            "/data/control/project",
            "/data/workspaces",
            "/data/workspaces-other/project",
            "/data/workspaces/../control",
            "/mnt/project",
        ] {
            let mut lock = valid_lock();
            lock.workspace_mount = path.to_owned();
            let error = lock.validate().expect_err(path).to_string();
            assert!(error.contains("workspace_mount"), "{path}: {error}");
        }
    }

    #[test]
    fn default_workspace_is_the_locked_mount_inside_app_data() {
        let lock = RuntimeLock::bundled().unwrap();
        let data_dir = Path::new("app-data").join("ua-data");
        assert_eq!(lock.workspace_mount, "/data/workspaces/team/team-shared");
        assert_eq!(
            lock.default_workspace_dir(&data_dir),
            data_dir.join("workspaces").join("team").join("team-shared")
        );
    }

    #[test]
    fn bundled_runtime_is_the_newest_stable_entry_of_the_built_in_list() {
        let list =
            crate::CompatibilityList::from_json(include_str!("../../../compatible-runtimes.json"))
                .unwrap();
        let lock = RuntimeLock::bundled().unwrap();
        let newest = list
            .newest_for(
                SUPPORTED_HOST_CONTRACT_VERSION,
                "ghcr.io/enzimo/universal-agents",
                false,
            )
            .unwrap();
        assert_eq!(lock.identity(), *newest);
        assert!(lock.upgrade_sources.is_empty());
    }

    #[test]
    fn profile_and_list_entry_cannot_disagree() {
        let profile = include_str!("../../../runtime-lock.json");
        let identity = RuntimeLock::bundled().unwrap().identity();

        // The profile may not name a version itself.
        let mut with_version: serde_json::Value = serde_json::from_str(profile).unwrap();
        with_version["ua_version"] = "0.9.0".into();
        let error =
            RuntimeLock::from_profile_json(&with_version.to_string(), &identity).unwrap_err();
        assert!(
            error.to_string().contains("must not contain 'ua_version'"),
            "{error}"
        );

        // An entry from another image repository or contract is refused.
        let mut elsewhere = identity.clone();
        elsewhere.image = format!("ghcr.io/other/ua@sha256:{DIGEST}");
        assert!(RuntimeLock::from_profile_json(profile, &elsewhere).is_err());
        let mut other_contract = identity.clone();
        other_contract.host_contract_version += 1;
        assert!(RuntimeLock::from_profile_json(profile, &other_contract).is_err());

        let mut no_repository: serde_json::Value = serde_json::from_str(profile).unwrap();
        no_repository
            .as_object_mut()
            .unwrap()
            .remove("image_repository");
        assert!(RuntimeLock::from_profile_json(&no_repository.to_string(), &identity).is_err());
    }

    #[test]
    fn rejects_older_host_contracts() {
        let mut lock = valid_lock();
        lock.host_contract_version = 2;
        assert!(
            lock.validate()
                .unwrap_err()
                .to_string()
                .contains("unsupported host contract version 2")
        );
    }

    #[test]
    fn rejects_mutable_image_reference() {
        let mut runtime_lock = valid_lock();
        runtime_lock.image = "ghcr.io/example/ua:latest".to_owned();

        let error = runtime_lock.validate().expect_err("tag must be rejected");

        assert!(error.to_string().contains("@sha256"));
    }

    #[test]
    fn rejects_cross_origin_style_path() {
        let mut runtime_lock = valid_lock();
        runtime_lock.web_ui_path = "//example.com".to_owned();

        let error = runtime_lock
            .validate()
            .expect_err("cross-origin path must be rejected");

        assert!(error.to_string().contains("single-origin"));
    }
}
