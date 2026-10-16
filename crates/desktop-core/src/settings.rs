use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DESKTOP_SETTINGS_SCHEMA_VERSION: u32 = 1;

/// External skill repositories approved during onboarding are refreshed before
/// agents launch unless the operator turns the setting off. Refreshing keeps
/// long-lived installations current; it is the default because a stale local
/// skill copy is a worse failure than one startup network call.
const REFRESH_EXTERNAL_SKILLS_ON_START_DEFAULT: bool = true;

fn refresh_external_skills_on_start_default() -> bool {
    REFRESH_EXTERNAL_SKILLS_ON_START_DEFAULT
}

/// Omit the preference while it holds the default value.
///
/// Earlier versions always wrote every field, so a settings file that merely
/// recorded a workspace or credential source also contained an incidental
/// `false`. Not persisting the default keeps "never chose" distinct from "chose
/// off" without a migration: absent means default, and only an explicit choice
/// is written.
fn is_default_external_skill_refresh(value: &bool) -> bool {
    *value == REFRESH_EXTERNAL_SKILLS_ON_START_DEFAULT
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DesktopSettings {
    pub schema_version: u32,
    pub external_workspace: Option<PathBuf>,
    pub pending_codex_auth_file: Option<PathBuf>,
    pub openai_subscription_connected: bool,
    #[serde(
        default = "refresh_external_skills_on_start_default",
        skip_serializing_if = "is_default_external_skill_refresh"
    )]
    pub refresh_external_skills_on_start: bool,
    /// Also offer UA pre-releases and releases not yet on the compatibility
    /// list. Off unless chosen, and only written once turned on. It changes
    /// only what an update check offers, never the running container.
    #[serde(default, skip_serializing_if = "is_false")]
    pub offer_unlisted_runtimes: bool,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            schema_version: DESKTOP_SETTINGS_SCHEMA_VERSION,
            external_workspace: None,
            pending_codex_auth_file: None,
            openai_subscription_connected: false,
            refresh_external_skills_on_start: REFRESH_EXTERNAL_SKILLS_ON_START_DEFAULT,
            offer_unlisted_runtimes: false,
        }
    }
}

impl DesktopSettings {
    pub fn normalize(mut self, data_dir: &Path) -> Result<Self, SettingsError> {
        self.validate_schema()?;

        let data_dir = canonical_or_absolute(data_dir)?;
        self.external_workspace = self
            .external_workspace
            .as_deref()
            .map(normalize_workspace)
            .transpose()?;
        self.pending_codex_auth_file = self
            .pending_codex_auth_file
            .as_deref()
            .map(normalize_codex_auth_file)
            .transpose()?;

        validate_relationships(&self, &data_dir)?;

        Ok(self)
    }

    /// Validate persisted settings without requiring removable sources to exist.
    ///
    /// Paths are canonicalized when the user selects them. Loading must preserve
    /// those paths when a folder or credential file is later moved so the UI can
    /// explain the problem and offer recovery.
    pub fn validate_for_storage(self, data_dir: &Path) -> Result<Self, SettingsError> {
        self.validate_schema()?;
        let data_dir = canonical_or_absolute(data_dir)?;
        for path in [
            self.external_workspace.as_deref(),
            self.pending_codex_auth_file.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            validate_docker_source_path(path)?;
        }
        validate_relationships(&self, &data_dir)?;
        Ok(self)
    }

    pub fn with_external_workspace(
        mut self,
        path: Option<PathBuf>,
        data_dir: &Path,
    ) -> Result<Self, SettingsError> {
        self.validate_schema()?;
        self.external_workspace = path.as_deref().map(normalize_workspace).transpose()?;
        self.validate_for_storage(data_dir)
    }

    pub fn with_codex_auth_file(
        mut self,
        path: PathBuf,
        data_dir: &Path,
    ) -> Result<Self, SettingsError> {
        self.validate_schema()?;
        self.pending_codex_auth_file = Some(normalize_codex_auth_file(&path)?);
        self.validate_for_storage(data_dir)
    }

    pub fn with_external_skill_refresh(
        mut self,
        enabled: bool,
        data_dir: &Path,
    ) -> Result<Self, SettingsError> {
        self.validate_schema()?;
        self.refresh_external_skills_on_start = enabled;
        self.validate_for_storage(data_dir)
    }

    pub fn with_unlisted_runtime_offers(
        mut self,
        enabled: bool,
        data_dir: &Path,
    ) -> Result<Self, SettingsError> {
        self.validate_schema()?;
        self.offer_unlisted_runtimes = enabled;
        self.validate_for_storage(data_dir)
    }

    pub fn available_workspace(&self, data_dir: &Path) -> Result<Option<PathBuf>, SettingsError> {
        self.validate_schema()?;
        let Some(workspace) = self.external_workspace.as_deref() else {
            return Ok(None);
        };
        let workspace = normalize_workspace(workspace)?;
        let data_dir = canonical_or_absolute(data_dir)?;
        if paths_overlap(&workspace, &data_dir) {
            return Err(workspace_overlap_error());
        }
        if Some(&workspace) != self.external_workspace.as_ref() {
            return Err(SettingsError::Invalid(format!(
                "The selected workspace now resolves to a different folder: {}. Choose it again to approve the new location.",
                workspace.display()
            )));
        }
        Ok(Some(workspace))
    }

    pub fn available_codex_auth_file(
        &self,
        data_dir: &Path,
    ) -> Result<Option<PathBuf>, SettingsError> {
        self.validate_schema()?;
        let Some(auth_file) = self.pending_codex_auth_file.as_deref() else {
            return Ok(None);
        };
        let auth_file = normalize_codex_auth_file(auth_file)?;
        let data_dir = canonical_or_absolute(data_dir)?;
        if is_same_or_child(&auth_file, &data_dir) {
            return Err(auth_inside_data_error());
        }
        if let Some(workspace) = self.external_workspace.as_deref() {
            if is_same_or_child(&auth_file, workspace) {
                return Err(auth_inside_workspace_error());
            }
        }
        if Some(&auth_file) != self.pending_codex_auth_file.as_ref() {
            return Err(SettingsError::Invalid(format!(
                "The selected Codex credential now resolves to a different file: {}. Choose it again to approve the new location.",
                auth_file.display()
            )));
        }
        Ok(Some(auth_file))
    }

    fn validate_schema(&self) -> Result<(), SettingsError> {
        if self.schema_version != DESKTOP_SETTINGS_SCHEMA_VERSION {
            return Err(SettingsError::UnsupportedSchema {
                found: self.schema_version,
                expected: DESKTOP_SETTINGS_SCHEMA_VERSION,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct DesktopSettingsStore {
    path: PathBuf,
    data_dir: PathBuf,
}

impl DesktopSettingsStore {
    pub fn new(path: PathBuf, data_dir: PathBuf) -> Self {
        Self { path, data_dir }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<DesktopSettings, SettingsError> {
        if !self.path.exists() {
            return DesktopSettings::default().validate_for_storage(&self.data_dir);
        }
        let contents = fs::read_to_string(&self.path)
            .map_err(|error| SettingsError::Io(self.path.clone(), error))?;
        let settings: DesktopSettings = serde_json::from_str(&contents)
            .map_err(|error| SettingsError::Decode(self.path.clone(), error))?;
        settings.validate_for_storage(&self.data_dir)
    }

    pub fn save(&self, settings: DesktopSettings) -> Result<DesktopSettings, SettingsError> {
        let settings = settings.validate_for_storage(&self.data_dir)?;
        let parent = self.path.parent().ok_or_else(|| {
            SettingsError::Invalid("Desktop settings path has no parent directory.".to_owned())
        })?;
        fs::create_dir_all(parent).map_err(|error| SettingsError::Io(parent.to_owned(), error))?;

        let temporary_path = self.path.with_extension("json.tmp");
        let serialized = serde_json::to_vec_pretty(&settings).map_err(SettingsError::Encode)?;
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary_path)
            .map_err(|error| SettingsError::Io(temporary_path.clone(), error))?;
        set_owner_only_permissions(&file, &temporary_path)?;
        file.write_all(&serialized)
            .and_then(|_| file.write_all(b"\n"))
            .and_then(|_| file.sync_all())
            .map_err(|error| SettingsError::Io(temporary_path.clone(), error))?;
        drop(file);
        fs::rename(&temporary_path, &self.path)
            .map_err(|error| SettingsError::Io(self.path.clone(), error))?;
        Ok(settings)
    }
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("invalid desktop settings: {0}")]
    Invalid(String),
    #[error("unsupported desktop settings schema {found}; expected {expected}")]
    UnsupportedSchema { found: u32, expected: u32 },
    #[error("failed to read desktop settings at {0}: {1}")]
    Decode(PathBuf, serde_json::Error),
    #[error("failed to encode desktop settings: {0}")]
    Encode(serde_json::Error),
    #[error("desktop settings I/O failed at {0}: {1}")]
    Io(PathBuf, std::io::Error),
}

fn normalize_workspace(path: &Path) -> Result<PathBuf, SettingsError> {
    let path = path.canonicalize().map_err(|error| {
        SettingsError::Invalid(format!(
            "The selected workspace is unavailable at {}: {error}",
            path.display()
        ))
    })?;
    if !path.is_dir() {
        return Err(SettingsError::Invalid(format!(
            "The selected workspace is not a directory: {}",
            path.display()
        )));
    }
    validate_docker_source_path(&path)?;
    Ok(path)
}

fn normalize_codex_auth_file(path: &Path) -> Result<PathBuf, SettingsError> {
    let path = path.canonicalize().map_err(|error| {
        SettingsError::Invalid(format!(
            "The selected Codex credential file is unavailable at {}: {error}",
            path.display()
        ))
    })?;
    if !path.is_file() || path.file_name().and_then(|name| name.to_str()) != Some("auth.json") {
        return Err(SettingsError::Invalid(
            "Choose the file-backed Codex credential named auth.json.".to_owned(),
        ));
    }
    validate_docker_source_path(&path)?;
    Ok(path)
}

fn validate_relationships(
    settings: &DesktopSettings,
    data_dir: &Path,
) -> Result<(), SettingsError> {
    if let Some(auth_file) = &settings.pending_codex_auth_file {
        if is_same_or_child(auth_file, data_dir) {
            return Err(auth_inside_data_error());
        }
    }

    if let Some(workspace) = &settings.external_workspace {
        if paths_overlap(workspace, data_dir) {
            return Err(workspace_overlap_error());
        }
        if let Some(auth_file) = &settings.pending_codex_auth_file {
            if is_same_or_child(auth_file, workspace) {
                return Err(auth_inside_workspace_error());
            }
        }
    }
    Ok(())
}

fn auth_inside_data_error() -> SettingsError {
    SettingsError::Invalid(
        "The Codex credential file must stay outside Universal Agents app data so agents cannot read it."
            .to_owned(),
    )
}

fn workspace_overlap_error() -> SettingsError {
    SettingsError::Invalid(
        "The selected workspace cannot contain, or be contained by, Universal Agents app data. Choose a project folder instead."
            .to_owned(),
    )
}

fn auth_inside_workspace_error() -> SettingsError {
    SettingsError::Invalid(
        "The selected workspace contains the Codex credential file. Choose a narrower project folder so credentials stay outside agent access."
            .to_owned(),
    )
}

fn canonical_or_absolute(path: &Path) -> Result<PathBuf, SettingsError> {
    if let Ok(canonical) = path.canonicalize() {
        return Ok(canonical);
    }
    if path.is_absolute() {
        return Ok(path.to_owned());
    }
    Err(SettingsError::Invalid(format!(
        "Desktop data path must be absolute: {}",
        path.display()
    )))
}

fn validate_docker_source_path(path: &Path) -> Result<(), SettingsError> {
    crate::platform::docker_source(path)
        .map(|_| ())
        .map_err(SettingsError::Invalid)
}

fn is_same_or_child(path: &Path, parent: &Path) -> bool {
    path == parent || path.starts_with(parent)
}

fn paths_overlap(first: &Path, second: &Path) -> bool {
    is_same_or_child(first, second) || is_same_or_child(second, first)
}

#[cfg(unix)]
fn set_owner_only_permissions(file: &fs::File, path: &Path) -> Result<(), SettingsError> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|error| SettingsError::Io(path.to_owned(), error))
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_file: &fs::File, _path: &Path) -> Result<(), SettingsError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn settings_round_trip_uses_owner_only_file() {
        let temp_dir = TempDir::new().expect("temporary directory");
        let data_dir = temp_dir.path().join("app-data");
        let workspace = temp_dir.path().join("project");
        fs::create_dir_all(&data_dir).expect("data directory");
        fs::create_dir_all(&workspace).expect("workspace directory");
        let store =
            DesktopSettingsStore::new(temp_dir.path().join("desktop-settings.json"), data_dir);
        let settings = DesktopSettings {
            external_workspace: Some(workspace.canonicalize().expect("canonical workspace")),
            ..DesktopSettings::default()
        };

        let saved = store.save(settings.clone()).expect("save settings");
        let loaded = store.load().expect("load settings");

        assert_eq!(saved, settings);
        assert_eq!(loaded, settings);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(store.path())
                    .expect("settings metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn existing_settings_default_external_skill_refresh_to_on() {
        // Settings written before this preference existed have no key at all.
        // They must adopt the new default instead of silently staying stale.
        let settings: DesktopSettings = serde_json::from_str(
            r#"{"schema_version":1,"external_workspace":null,"pending_codex_auth_file":null,"openai_subscription_connected":false}"#,
        )
        .expect("existing settings decode");

        assert!(settings.refresh_external_skills_on_start);
        assert!(DesktopSettings::default().refresh_external_skills_on_start);
    }

    #[test]
    fn unchecked_runtime_offers_are_off_unless_chosen_and_saved_only_when_on() {
        let temp_dir = TempDir::new().expect("temporary directory");
        let data_dir = temp_dir.path().join("app-data");
        fs::create_dir_all(&data_dir).expect("data directory");
        let path = temp_dir.path().join("desktop-settings.json");
        let store = DesktopSettingsStore::new(path.clone(), data_dir.clone());

        assert!(!DesktopSettings::default().offer_unlisted_runtimes);
        store
            .save(DesktopSettings::default())
            .expect("save defaults");
        assert!(
            !fs::read_to_string(&path)
                .unwrap()
                .contains("offer_unlisted_runtimes")
        );

        let on = DesktopSettings::default()
            .with_unlisted_runtime_offers(true, &data_dir)
            .expect("turn on");
        store.save(on).expect("save choice");
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("\"offer_unlisted_runtimes\": true")
        );
        assert!(store.load().expect("load").offer_unlisted_runtimes);
    }

    #[test]
    fn default_external_skill_refresh_is_not_persisted_as_a_choice() {
        let temp_dir = TempDir::new().expect("temporary directory");
        let data_dir = temp_dir.path().join("app-data");
        fs::create_dir_all(&data_dir).expect("data directory");
        let path = temp_dir.path().join("desktop-settings.json");
        let store = DesktopSettingsStore::new(path.clone(), data_dir.clone());

        store
            .save(DesktopSettings::default())
            .expect("save default settings");

        // The default must not appear in the file, so a later change to the
        // default cannot be masked by an incidental saved value.
        let contents = fs::read_to_string(&path).expect("read settings");
        assert!(
            !contents.contains("refresh_external_skills_on_start"),
            "{contents}"
        );
        // It still loads back as the default.
        assert!(
            store
                .load()
                .expect("load settings")
                .refresh_external_skills_on_start
        );
    }

    #[test]
    fn explicit_external_skill_refresh_choice_round_trips() {
        let temp_dir = TempDir::new().expect("temporary directory");
        let data_dir = temp_dir.path().join("app-data");
        fs::create_dir_all(&data_dir).expect("data directory");
        let store = DesktopSettingsStore::new(
            temp_dir.path().join("desktop-settings.json"),
            data_dir.clone(),
        );

        let saved = store
            .save(
                DesktopSettings::default()
                    .with_external_skill_refresh(false, &data_dir)
                    .expect("disable refresh"),
            )
            .expect("save settings");

        assert!(!saved.refresh_external_skills_on_start);
        assert!(
            !store
                .load()
                .expect("reload settings")
                .refresh_external_skills_on_start
        );
    }

    #[test]
    fn rejects_workspace_that_contains_app_data() {
        let temp_dir = TempDir::new().expect("temporary directory");
        let data_dir = temp_dir
            .path()
            .join("Library/Application Support/app/ua-data");
        fs::create_dir_all(&data_dir).expect("data directory");
        let settings = DesktopSettings {
            external_workspace: Some(temp_dir.path().to_owned()),
            ..DesktopSettings::default()
        };

        let error = settings
            .normalize(&data_dir)
            .expect_err("broad workspace must fail");

        assert!(error.to_string().contains("project folder"));
    }

    #[test]
    fn rejects_codex_auth_inside_workspace() {
        let temp_dir = TempDir::new().expect("temporary directory");
        let data_dir = temp_dir.path().join("app-data");
        let workspace = temp_dir.path().join("project");
        let auth_file = workspace.join(".codex/auth.json");
        fs::create_dir_all(&data_dir).expect("data directory");
        fs::create_dir_all(auth_file.parent().expect("auth parent")).expect("auth directory");
        fs::write(&auth_file, "{}").expect("auth file");
        let settings = DesktopSettings {
            external_workspace: Some(workspace),
            pending_codex_auth_file: Some(auth_file),
            ..DesktopSettings::default()
        };

        let error = settings
            .normalize(&data_dir)
            .expect_err("credential in workspace must fail");

        assert!(error.to_string().contains("contains the Codex credential"));
    }

    #[test]
    fn rejects_codex_auth_inside_app_data() {
        let temp_dir = TempDir::new().expect("temporary directory");
        let data_dir = temp_dir.path().join("app-data");
        let auth_file = data_dir.join("workspace/auth.json");
        fs::create_dir_all(auth_file.parent().expect("auth parent")).expect("auth directory");
        fs::write(&auth_file, "{}").expect("auth file");
        let settings = DesktopSettings {
            pending_codex_auth_file: Some(auth_file),
            ..DesktopSettings::default()
        };

        let error = settings
            .normalize(&data_dir)
            .expect_err("credential in app data must fail");

        assert!(
            error
                .to_string()
                .contains("outside Universal Agents app data")
        );
    }

    #[test]
    fn load_preserves_unavailable_sources_for_ui_recovery() {
        let temp_dir = TempDir::new().expect("temporary directory");
        let data_dir = temp_dir.path().join("app-data");
        let workspace = temp_dir.path().join("project");
        let auth_file = temp_dir.path().join("codex/auth.json");
        fs::create_dir_all(&data_dir).expect("data directory");
        fs::create_dir_all(&workspace).expect("workspace directory");
        fs::create_dir_all(auth_file.parent().expect("auth parent")).expect("auth directory");
        fs::write(&auth_file, "{}").expect("auth file");
        let store = DesktopSettingsStore::new(
            temp_dir.path().join("desktop-settings.json"),
            data_dir.clone(),
        );
        let saved = store
            .save(
                DesktopSettings::default()
                    .with_external_workspace(Some(workspace.clone()), &data_dir)
                    .expect("select workspace")
                    .with_codex_auth_file(auth_file.clone(), &data_dir)
                    .expect("select auth file"),
            )
            .expect("save settings");
        fs::remove_dir_all(&workspace).expect("remove test workspace");
        fs::remove_file(&auth_file).expect("remove test auth file");

        let loaded = store.load().expect("load unavailable paths");

        assert_eq!(loaded, saved);
        assert!(loaded.available_workspace(&data_dir).is_err());
        assert!(loaded.available_codex_auth_file(&data_dir).is_err());
    }

    #[test]
    fn workspace_can_be_repaired_while_auth_source_is_missing() {
        let temp_dir = TempDir::new().expect("temporary directory");
        let data_dir = temp_dir.path().join("app-data");
        let old_workspace = temp_dir.path().join("old-project");
        let new_workspace = temp_dir.path().join("new-project");
        let auth_file = temp_dir.path().join("codex/auth.json");
        fs::create_dir_all(&data_dir).expect("data directory");
        fs::create_dir_all(&old_workspace).expect("old workspace");
        fs::create_dir_all(&new_workspace).expect("new workspace");
        fs::create_dir_all(auth_file.parent().expect("auth parent")).expect("auth directory");
        fs::write(&auth_file, "{}").expect("auth file");
        let settings = DesktopSettings::default()
            .with_external_workspace(Some(old_workspace.clone()), &data_dir)
            .expect("select old workspace")
            .with_codex_auth_file(auth_file.clone(), &data_dir)
            .expect("select auth file");
        let saved_auth_file = settings.pending_codex_auth_file.clone();
        fs::remove_dir_all(&old_workspace).expect("remove old workspace");
        fs::remove_file(&auth_file).expect("remove auth file");

        let repaired = settings
            .with_external_workspace(Some(new_workspace.clone()), &data_dir)
            .expect("repair workspace");

        assert_eq!(
            repaired
                .available_workspace(&data_dir)
                .expect("available workspace"),
            Some(new_workspace.canonicalize().expect("canonical workspace"))
        );
        assert_eq!(repaired.pending_codex_auth_file, saved_auth_file);
        assert!(repaired.available_codex_auth_file(&data_dir).is_err());
    }
}
