use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use ua_desktop_core::guidance::guidance;
use ua_desktop_core::updates::{
    self, GitHubReleases, RuntimeSelection, SelectionStore, UpdateReview,
};
use ua_desktop_core::{
    CodexImportOutcome, DesktopSettings, DesktopSettingsStore, DockerPrerequisite, ProcessDocker,
    RuntimeConfiguration, RuntimeLock, RuntimeManager, RuntimePhase, RuntimeStatus,
};

pub struct DesktopController {
    settings_store: DesktopSettingsStore,
    selection_store: SelectionStore,
    data_dir: PathBuf,
    state: Mutex<ControllerState>,
}

struct ControllerState {
    settings: DesktopSettings,
    manager: RuntimeManager<ProcessDocker>,
    update_pending: bool,
    update_review: Option<UpdateReview>,
    check_generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiSubscriptionState {
    Disconnected,
    PendingImport,
    SourceUnavailable,
    Imported,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntegrationStatus {
    pub workspace_dir: String,
    pub workspace_is_external: bool,
    pub workspace_available: bool,
    pub workspace_issue: Option<String>,
    pub openai_subscription_state: OpenAiSubscriptionState,
    pub openai_subscription_detail: String,
    pub pending_codex_auth_file: Option<String>,
    pub refresh_external_skills_on_start: bool,
    pub offer_unlisted_runtimes: bool,
}

impl DesktopController {
    pub fn new(
        runtime_lock: RuntimeLock,
        data_dir: PathBuf,
        settings_path: PathBuf,
    ) -> Result<Self, String> {
        let selection_store = SelectionStore {
            path: settings_path.with_file_name("runtime-lock.json"),
        };
        // A missing host selection must not opt existing opaque UA data into a
        // newly bundled runtime: the desktop cannot tell which UA wrote it.
        if !selection_store.path.try_exists().map_err(error_text)?
            && data_dir.try_exists().map_err(error_text)?
            && std::fs::read_dir(&data_dir)
                .map_err(error_text)?
                .next()
                .transpose()
                .map_err(error_text)?
                .is_some()
        {
            return Err(format!(
                "Existing UA data has no saved desktop runtime selection, so this desktop cannot tell which runtime created it. UA data was not changed. Restore the desktop-owned runtime-lock.json, or to start fresh, move {} aside.",
                data_dir.display()
            ));
        }
        let selection = selection_store.load_or_initialize(&runtime_lock)?;
        let settings_store = DesktopSettingsStore::new(settings_path, data_dir.clone());
        let settings = settings_store.load().map_err(|error| error.to_string())?;
        let manager = build_manager(&selection.lock, &data_dir, &settings)?;
        Ok(Self {
            settings_store,
            selection_store,
            data_dir,
            state: Mutex::new(ControllerState {
                settings,
                manager,
                update_pending: selection.update_pending,
                update_review: None,
                check_generation: 0,
            }),
        })
    }

    pub fn runtime_status(&self) -> Result<RuntimeStatus, String> {
        let mut state = self.lock_state()?;
        let status = runtime_status(&state, &self.data_dir)?;
        if state.update_pending
            && matches!(status.phase, RuntimePhase::Ready | RuntimePhase::Onboarding)
        {
            self.selection_store.save(&RuntimeSelection {
                lock: state.manager.runtime_lock().clone(),
                update_pending: false,
            })?;
            state.update_pending = false;
        }
        Ok(status)
    }

    /// Read-only Docker probe for the first-run prerequisite dialog.
    ///
    /// It runs on a timer while Docker is missing, so it never starts, pulls,
    /// or writes anything and never holds the Docker serialization lock longer
    /// than a single `docker info` call.
    pub fn docker_prerequisite(&self) -> Result<DockerPrerequisite, String> {
        self.lock_state()?
            .manager
            .docker_prerequisite()
            .map_err(error_text)
    }

    pub fn start_runtime(&self) -> Result<RuntimeStatus, String> {
        let mut state = self.lock_state()?;
        require_workspace(&state.settings, &self.data_dir)?;
        self.select_newest_for_first_install(&mut state)?;
        state.manager.start().map_err(error_text)
    }

    /// A first install uses the newest stable UA on the compatibility list
    /// in this repository's `main` branch rather than the version built into
    /// the launcher, so a launcher built months ago still installs a current
    /// UA. Only a fresh computer qualifies: no container, no UA data, and no
    /// pending update. The selection only ever moves forward. When GitHub
    /// cannot be reached, the built-in version is installed and the progress
    /// text says so.
    fn select_newest_for_first_install(&self, state: &mut ControllerState) -> Result<(), String> {
        if state.update_pending || !holds_no_files(&self.data_dir)? {
            return Ok(());
        }
        if state.manager.status().map_err(error_text)?.phase != RuntimePhase::NotCreated {
            return Ok(());
        }
        let built_in = state.manager.runtime_lock().ua_version.clone();
        let list = match updates::fetch_compatibility_list(&GitHubReleases) {
            Ok(list) => list,
            Err(reason) => {
                log::warn!(
                    "first install uses the built-in UA {built_in} because the compatibility list could not be read: {reason}"
                );
                ua_desktop_core::progress::report(format!(
                    "Could not check GitHub for the newest compatible Universal Agents, so installing UA {built_in}, which came with this launcher. You can update it afterwards."
                ));
                return Ok(());
            }
        };
        let Some(lock) = updates::first_install_target(&list, state.manager.runtime_lock()) else {
            return Ok(());
        };
        log::info!(
            "first install selects UA {} from the compatibility list instead of the built-in UA {built_in}",
            lock.ua_version
        );
        let manager = build_manager(&lock, &self.data_dir, &state.settings)?;
        self.selection_store.save(&RuntimeSelection {
            lock,
            update_pending: false,
        })?;
        state.manager = manager;
        state.update_review = None;
        state.check_generation += 1;
        Ok(())
    }

    pub fn stop_runtime(&self) -> Result<RuntimeStatus, String> {
        let state = self.lock_state()?;
        state.manager.stop().map_err(error_text)?;
        runtime_status(&state, &self.data_dir)
    }

    pub fn restart_runtime(&self) -> Result<RuntimeStatus, String> {
        let state = self.lock_state()?;
        require_workspace(&state.settings, &self.data_dir)?;
        state.manager.restart().map_err(error_text)
    }

    pub fn check_runtime_updates(&self) -> Result<UpdateReview, String> {
        let (current, pending, generation, offer_unlisted) = {
            let mut state = self.lock_state()?;
            state.check_generation += 1;
            state.update_review = None;
            (
                state.manager.runtime_lock().clone(),
                state.update_pending,
                state.check_generation,
                state.settings.offer_unlisted_runtimes,
            )
        };
        // Release requests must not hold the Docker/controller mutex.
        let mut review = updates::check(&GitHubReleases, &current, pending, offer_unlisted)?;
        let mut state = self.lock_state()?;
        mark_downloaded(&state.manager, &mut review);
        if state.check_generation != generation
            || state.manager.runtime_lock() != &current
            || state.update_pending != pending
        {
            return Err("The runtime changed during the release check. Check again.".to_owned());
        }
        state.update_review = Some(review.clone());
        Ok(review)
    }

    pub fn install_runtime_update(
        &self,
        review_id: &str,
        acknowledged: bool,
    ) -> Result<RuntimeStatus, String> {
        let mut state = self.lock_state()?;
        let review = state
            .update_review
            .as_ref()
            .ok_or("Check for updates and review the release notes first.")?;
        let candidate = updates::approve(
            review,
            state.manager.runtime_lock(),
            review_id,
            acknowledged,
        )?;
        require_workspace(&state.settings, &self.data_dir)?;
        let existing = state.manager.status().map_err(error_text)?;
        if existing.phase == RuntimePhase::Error {
            return Err(existing.detail);
        }
        let manager = build_manager(&candidate, &self.data_dir, &state.settings)?;
        // Persist approval before Docker mutation. If download/start is interrupted,
        // Start and subsequent launches resume this exact user-approved digest.
        self.selection_store.save(&RuntimeSelection {
            lock: candidate,
            update_pending: true,
        })?;
        state.manager = manager;
        state.update_pending = true;
        state.update_review = None;
        state.check_generation += 1;
        log::info!(
            "applying user-approved UA runtime update to {}",
            state.manager.runtime_lock().ua_version
        );
        state.manager.start().map_err(|error| format!("The approved update is pending: {error}. Use Start to retry; persistent data was retained."))
    }

    /// Download the reviewed runtime image without restarting Universal Agents.
    ///
    /// This is the background half of an update: it fetches the exact reviewed
    /// digest so the user can finish active work and apply it later through the
    /// existing reviewed Update and restart path. It never replaces or restarts
    /// the container and never changes the selected runtime.
    pub fn download_runtime_update(&self, review_id: &str) -> Result<UpdateReview, String> {
        let mut state = self.lock_state()?;
        let candidate = {
            let review = state
                .update_review
                .as_ref()
                .ok_or("Check for updates and review the release notes first.")?;
            updates::approve_download(review, state.manager.runtime_lock(), review_id)?
        };
        state
            .manager
            .download_runtime_image(&candidate)
            .map_err(error_text)?;
        let review = state
            .update_review
            .as_mut()
            .ok_or("Check for updates again before downloading.")?;
        review.image_downloaded = true;
        for entry in &mut review.catalog {
            if entry.image == candidate.image {
                entry.downloaded = true;
            }
        }
        Ok(review.clone())
    }

    /// Make one of the newer versions from the last check the update target.
    /// Download and Switch then use the returned review id, as for the offered
    /// version. The review it replaces can no longer be installed.
    pub fn review_runtime_version(&self, version: &str) -> Result<UpdateReview, String> {
        let mut state = self.lock_state()?;
        let base = state
            .update_review
            .as_ref()
            .ok_or_else(|| guidance("Check for updates first."))?;
        let review =
            updates::review_for(base, state.manager.runtime_lock(), version).map_err(guidance)?;
        log::info!("update target changed to UA {version}");
        state.update_review = Some(review.clone());
        Ok(review)
    }

    /// The release page for a launcher or UA version the last check returned.
    pub fn release_page(&self, product: &str, version: &str) -> Result<String, String> {
        let state = self.lock_state()?;
        let review = state
            .update_review
            .as_ref()
            .ok_or_else(|| guidance("Check for updates first."))?;
        updates::release_page_url(review, product, version).map_err(guidance)
    }

    pub fn recent_logs(&self, tail: usize) -> Result<String, String> {
        self.lock_state()?.manager.logs(tail).map_err(error_text)
    }

    pub fn web_url(&self) -> Result<String, String> {
        Ok(self.lock_state()?.manager.web_url())
    }

    /// The loopback port of UA's secure broker, which serves credential forms.
    pub fn broker_port(&self) -> Result<u16, String> {
        Ok(self
            .lock_state()?
            .manager
            .runtime_lock()
            .container_ports
            .broker)
    }

    /// The folder agents see at the locked workspace mount
    /// (UA's shared team folder).
    ///
    /// This is the chosen external workspace when one is saved and the
    /// app-managed folder otherwise, so "Open folder" always reflects what
    /// the runtime is actually using.
    pub fn workspace_dir(&self) -> Result<PathBuf, String> {
        let state = self.lock_state()?;
        Ok(effective_workspace(&state, &self.data_dir))
    }

    pub fn docs_url(&self) -> Result<String, String> {
        Ok(self.lock_state()?.manager.runtime_lock().docs_url())
    }

    pub fn integration_status(&self) -> Result<IntegrationStatus, String> {
        let state = self.lock_state()?;
        Ok(integration_status(&state, &self.data_dir))
    }

    pub fn registry_access(
        &self,
        method: &str,
        username: &str,
        token: &str,
    ) -> Result<RuntimeStatus, String> {
        use ua_desktop_core::registry::{ProcessRegistry, login_gh, login_pat, verify_image};
        // Serialize authentication with lifecycle operations sharing Docker's store.
        let mut state = self.lock_state()?;
        ua_desktop_core::progress::report("Signing Docker into GHCR…");
        // Registry errors are fixed app text that never echoes command output
        // (it can contain the token), so the status window may show them.
        match method {
            "pat" => login_pat(&ProcessRegistry, username, token).map_err(guidance)?,
            "gh" => login_gh(&ProcessRegistry).map_err(guidance)?,
            "existing" => (),
            _ => return Err("Unknown image authentication method.".to_owned()),
        }
        require_workspace(&state.settings, &self.data_dir)?;
        // Verify access to the image that will actually be installed.
        self.select_newest_for_first_install(&mut state)?;
        ua_desktop_core::progress::report("Verifying access to the runtime image…");
        verify_image(&ProcessRegistry, state.manager.runtime_lock()).map_err(guidance)?;
        state.manager.start().map_err(error_text)
    }

    pub fn select_workspace(&self, path: PathBuf) -> Result<IntegrationStatus, String> {
        let mut state = self.lock_state()?;
        let settings = state
            .settings
            .clone()
            .with_external_workspace(Some(path), &self.data_dir)
            .map_err(error_text)?;
        self.apply_settings(&mut state, settings, true)?;
        Ok(integration_status(&state, &self.data_dir))
    }

    pub fn use_internal_workspace(&self) -> Result<IntegrationStatus, String> {
        let mut state = self.lock_state()?;
        let settings = state
            .settings
            .clone()
            .with_external_workspace(None, &self.data_dir)
            .map_err(error_text)?;
        self.apply_settings(&mut state, settings, true)?;
        Ok(integration_status(&state, &self.data_dir))
    }

    pub fn connect_openai_subscription(
        &self,
        auth_file: PathBuf,
    ) -> Result<IntegrationStatus, String> {
        let mut state = self.lock_state()?;
        let settings = state
            .settings
            .clone()
            .with_codex_auth_file(auth_file, &self.data_dir)
            .map_err(error_text)?;
        self.apply_settings(&mut state, settings, false)?;
        // Selection is durable intent, not a lifecycle operation. The UI retries
        // import once the runtime is ready, without coupling it to image access.
        Ok(integration_status(&state, &self.data_dir))
    }

    pub fn cancel_openai_subscription_import(&self) -> Result<IntegrationStatus, String> {
        let mut state = self.lock_state()?;
        let mut settings = state.settings.clone();
        settings.pending_codex_auth_file = None;
        self.apply_settings(&mut state, settings, false)?;
        Ok(integration_status(&state, &self.data_dir))
    }

    pub fn finish_openai_subscription(&self) -> Result<IntegrationStatus, String> {
        let mut state = self.lock_state()?;
        if !matches!(
            state.manager.status().map_err(error_text)?.phase,
            RuntimePhase::Ready | RuntimePhase::Onboarding
        ) {
            return Ok(integration_status(&state, &self.data_dir));
        }
        self.finish_codex_import_locked(&mut state)?;
        Ok(integration_status(&state, &self.data_dir))
    }

    /// Whether update checks also offer pre-releases and releases not yet on
    /// the compatibility list. A review prepared under the other choice is
    /// dropped so it cannot be installed.
    pub fn set_unlisted_runtime_offers(&self, enabled: bool) -> Result<IntegrationStatus, String> {
        let mut state = self.lock_state()?;
        let settings = state
            .settings
            .clone()
            .with_unlisted_runtime_offers(enabled, &self.data_dir)
            .map_err(error_text)?;
        state.settings = self
            .settings_store
            .save(settings)
            .map_err(|error| error.to_string())?;
        state.update_review = None;
        state.check_generation += 1;
        Ok(integration_status(&state, &self.data_dir))
    }

    pub fn set_external_skill_refresh(&self, enabled: bool) -> Result<IntegrationStatus, String> {
        let mut state = self.lock_state()?;
        let settings = state
            .settings
            .clone()
            .with_external_skill_refresh(enabled, &self.data_dir)
            .map_err(error_text)?;
        self.apply_settings(&mut state, settings, false)?;
        Ok(integration_status(&state, &self.data_dir))
    }

    fn apply_settings(
        &self,
        state: &mut ControllerState,
        settings: DesktopSettings,
        start_runtime: bool,
    ) -> Result<(), String> {
        let saved = self
            .settings_store
            .save(settings)
            .map_err(|error| error.to_string())?;
        let manager = build_manager(state.manager.runtime_lock(), &self.data_dir, &saved)?;

        state.settings = saved;
        state.manager = manager;
        if start_runtime {
            state.manager.start().map_err(error_text)?;
        }
        Ok(())
    }

    fn finish_codex_import_locked(&self, state: &mut ControllerState) -> Result<(), String> {
        let Some(auth_file) = state
            .settings
            .available_codex_auth_file(&self.data_dir)
            .map_err(error_text)?
        else {
            return Ok(());
        };
        match state
            .manager
            .import_codex_auth(&auth_file)
            .map_err(error_text)?
        {
            CodexImportOutcome::PendingRuntime => Ok(()),
            CodexImportOutcome::Imported => {
                let mut settings = state.settings.clone();
                settings.pending_codex_auth_file = None;
                settings.openai_subscription_connected = true;
                self.apply_settings(state, settings, false)
            }
        }
    }

    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, ControllerState>, String> {
        self.state
            .lock()
            .map_err(|_| "desktop controller state is unavailable".to_owned())
    }
}

/// Report which reviewed and catalog images are already downloaded. These are
/// read-only Docker calls; a missing or stopped Docker leaves the flags false
/// rather than failing the check.
fn mark_downloaded(manager: &RuntimeManager<ProcessDocker>, review: &mut UpdateReview) {
    let mut inspected: HashMap<String, bool> = HashMap::new();
    let mut present = |image: &str| {
        *inspected
            .entry(image.to_owned())
            .or_insert_with(|| match manager.image_present(image) {
                Ok(present) => present,
                Err(error) => {
                    log::debug!("could not inspect the local image store for {image}: {error}");
                    false
                }
            })
    };
    if let Some(image) = review.image.clone() {
        review.image_downloaded = present(&image);
    }
    for entry in &mut review.catalog {
        entry.downloaded = present(&entry.image);
    }
}

fn build_manager(
    runtime_lock: &RuntimeLock,
    data_dir: &Path,
    settings: &DesktopSettings,
) -> Result<RuntimeManager<ProcessDocker>, String> {
    RuntimeManager::new(
        ProcessDocker,
        runtime_lock.clone(),
        data_dir.to_owned(),
        RuntimeConfiguration {
            external_workspace: settings.external_workspace.clone(),
            refresh_external_skills_on_start: settings.refresh_external_skills_on_start,
        },
    )
    .map_err(error_text)
}

fn effective_workspace(state: &ControllerState, data_dir: &Path) -> PathBuf {
    state
        .settings
        .external_workspace
        .clone()
        .unwrap_or_else(|| state.manager.runtime_lock().default_workspace_dir(data_dir))
}

fn integration_status(state: &ControllerState, data_dir: &Path) -> IntegrationStatus {
    let workspace = effective_workspace(state, data_dir);
    let workspace_issue = workspace_issue(&state.settings, data_dir);
    let (subscription_state, subscription_detail) = if state
        .settings
        .pending_codex_auth_file
        .is_some()
    {
        match state.settings.available_codex_auth_file(data_dir) {
            Ok(Some(_)) => (
                OpenAiSubscriptionState::PendingImport,
                "The selected auth.json is waiting for a one-shot, read-only import. Once UA is available, the desktop app will import it into UA's protected credential store so you can test the connection during onboarding."
                    .to_owned(),
            ),
            Ok(None) => unreachable!("a pending path was checked above"),
            Err(error) => (
                OpenAiSubscriptionState::SourceUnavailable,
                format!(
                    "The saved import source is unavailable. Choose its replacement or cancel this pending import. {}",
                    recovery_error_text(error)
                ),
            ),
        }
    } else if state.settings.openai_subscription_connected {
        (
            OpenAiSubscriptionState::Imported,
            "Subscription credentials are stored in UA's protected /data/auth directory and can refresh without exposing the source file to agents."
                .to_owned(),
        )
    } else {
        (
            OpenAiSubscriptionState::Disconnected,
            "Choose a file-backed Codex auth.json to use your ChatGPT/Codex subscription."
                .to_owned(),
        )
    };

    IntegrationStatus {
        workspace_dir: workspace.display().to_string(),
        workspace_is_external: state.settings.external_workspace.is_some(),
        workspace_available: workspace_issue.is_none(),
        workspace_issue,
        openai_subscription_state: subscription_state,
        openai_subscription_detail: subscription_detail,
        pending_codex_auth_file: state
            .settings
            .pending_codex_auth_file
            .as_deref()
            .map(|path| path.display().to_string()),
        refresh_external_skills_on_start: state.settings.refresh_external_skills_on_start,
        offer_unlisted_runtimes: state.settings.offer_unlisted_runtimes,
    }
}

/// True when `directory` is missing or holds only empty folders. The launcher
/// itself may create the default shared folder before anything is installed
/// (Open folder), while UA writes files as soon as it first starts.
fn holds_no_files(directory: &Path) -> Result<bool, String> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(error_text(error)),
    };
    for entry in entries {
        let entry = entry.map_err(error_text)?;
        let kind = entry.file_type().map_err(error_text)?;
        if !kind.is_dir() || !holds_no_files(&entry.path())? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn runtime_status(state: &ControllerState, data_dir: &Path) -> Result<RuntimeStatus, String> {
    let mut status = state.manager.status().map_err(error_text)?;
    if !matches!(status.phase, RuntimePhase::DockerUnavailable) {
        if let Some(issue) = workspace_issue(&state.settings, data_dir) {
            status.phase = RuntimePhase::Error;
            status.detail = format!(
                "The saved agent workspace is unavailable. Choose a replacement folder or use the app workspace. {issue}"
            );
        }
    }
    Ok(status)
}

fn workspace_issue(settings: &DesktopSettings, data_dir: &Path) -> Option<String> {
    settings
        .available_workspace(data_dir)
        .err()
        .map(recovery_error_text)
}

fn require_workspace(settings: &DesktopSettings, data_dir: &Path) -> Result<(), String> {
    settings
        .available_workspace(data_dir)
        .map(|_| ())
        .map_err(|error| {
            format!(
                "The saved agent workspace must be repaired before the runtime can start: {}",
                recovery_error_text(error)
            )
        })
}

fn error_text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn recovery_error_text(error: impl std::fmt::Display) -> String {
    let text = error.to_string();
    text.strip_prefix("invalid desktop settings: ")
        .unwrap_or(&text)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn runtime_lock() -> RuntimeLock {
        ua_desktop_core::RuntimeLock::bundled().expect("repository runtime lock")
    }

    #[test]
    fn a_fresh_computer_may_hold_empty_folders_but_no_files() {
        let temp_dir = TempDir::new().unwrap();
        let data_dir = temp_dir.path().join("ua-data");
        assert!(holds_no_files(&data_dir).unwrap(), "missing folder");
        // Open folder creates the default shared folder before any install.
        fs::create_dir_all(data_dir.join("workspaces/team/team-shared")).unwrap();
        assert!(holds_no_files(&data_dir).unwrap(), "only empty folders");
        fs::write(
            data_dir.join("workspaces/team/team-shared/notes.txt"),
            "work",
        )
        .unwrap();
        assert!(
            !holds_no_files(&data_dir).unwrap(),
            "a file means UA or the user wrote data"
        );
    }

    #[test]
    fn existing_data_without_runtime_selection_cannot_adopt_bundled_upgrade() {
        let temp = TempDir::new().unwrap();
        let data = temp.path().join("ua-data");
        fs::create_dir_all(&data).unwrap();
        let opaque = data.join("opaque-state");
        fs::write(&opaque, b"existing UA data").unwrap();
        let settings = temp.path().join("settings.json");
        let result = DesktopController::new(runtime_lock(), data, settings.clone());
        assert!(
            result
                .err()
                .unwrap()
                .contains("no saved desktop runtime selection")
        );
        assert_eq!(fs::read(opaque).unwrap(), b"existing UA data");
        assert!(!settings.with_file_name("runtime-lock.json").exists());
    }

    #[test]
    fn existing_data_keeps_previous_selection_until_explicit_update() {
        let temp = TempDir::new().unwrap();
        let data = temp.path().join("ua-data");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("opaque-state"), b"existing UA data").unwrap();
        let settings = temp.path().join("settings.json");
        let previous = RuntimeLock::from_json(include_str!(
            "../../crates/desktop-core/tests/fixtures/runtime-v0.8.4.json"
        ))
        .unwrap();
        let store = SelectionStore {
            path: settings.with_file_name("runtime-lock.json"),
        };
        store.load_or_initialize(&previous).unwrap();
        let controller = DesktopController::new(runtime_lock(), data, settings).unwrap();
        assert_eq!(
            controller.lock_state().unwrap().manager.runtime_lock(),
            &previous
        );
    }

    #[test]
    fn downloading_an_update_requires_a_current_review() {
        let temp = TempDir::new().unwrap();
        let data = temp.path().join("ua-data");
        fs::create_dir_all(&data).unwrap();
        let settings = temp.path().join("desktop-settings.json");
        let controller = DesktopController::new(runtime_lock(), data, settings).unwrap();

        // Without a prior check there is no cached review, so the download must
        // refuse before touching Docker rather than pulling an unapproved
        // digest. The Docker probe itself is covered with a fake command
        // boundary in desktop-core, so this test stays daemon-free.
        let error = controller.download_runtime_update("unknown").unwrap_err();
        assert!(error.contains("Check for updates"), "{error}");
    }

    /// GitHub as the update check sees it: two listed UA versions newer than
    /// the bundled one, and launcher 99.0.0.
    struct FakeReleases;
    impl updates::ReleaseSource for FakeReleases {
        fn get(&self, endpoint: &str, accept: updates::Accept) -> Result<String, String> {
            let lock = runtime_lock();
            let repository = lock.image.split_once("@sha256:").unwrap().0.to_owned();
            let entry = |version: &str, digest: char| {
                serde_json::json!({
                    "ua_version": version,
                    "host_contract_version": lock.host_contract_version,
                    "image": if version == lock.ua_version { lock.image.clone() } else { format!("{repository}@sha256:{}", digest.to_string().repeat(64)) },
                })
            };
            Ok(match accept {
                updates::Accept::Raw => serde_json::json!({"schema_version": 1, "runtimes": [
                    entry(&lock.ua_version, '0'), entry("99.0.0", 'a'), entry("99.1.0", 'b'),
                ]})
                .to_string(),
                updates::Accept::Json if endpoint.ends_with("/releases/latest") => {
                    serde_json::json!({"tag_name": "v99.0.0"}).to_string()
                }
                updates::Accept::Json => {
                    serde_json::json!([{"tag_name": "v99.0.0"}, {"tag_name": "v99.1.0"}])
                        .to_string()
                }
                updates::Accept::Binary => return Err("no assets".to_owned()),
            })
        }
    }

    #[test]
    fn a_chosen_newer_version_replaces_the_cached_review() {
        let temp = TempDir::new().unwrap();
        let data = temp.path().join("ua-data");
        fs::create_dir_all(&data).unwrap();
        let controller =
            DesktopController::new(runtime_lock(), data, temp.path().join("settings.json"))
                .unwrap();
        let error = controller.review_runtime_version("99.0.0").unwrap_err();
        assert!(error.contains("Check for updates first"), "{error}");
        assert!(ua_desktop_core::guidance::is_guidance(&error));
        assert!(controller.release_page("launcher", "99.0.0").is_err());

        let offered = updates::check(&FakeReleases, &runtime_lock(), false, false).unwrap();
        assert_eq!(offered.latest_version, "99.1.0");
        controller.lock_state().unwrap().update_review = Some(offered.clone());
        let chosen = controller.review_runtime_version("99.0.0").unwrap();
        assert_eq!(chosen.latest_version, "99.0.0");
        let cached = controller
            .lock_state()
            .unwrap()
            .update_review
            .clone()
            .unwrap();
        assert_eq!(cached.review_id, chosen.review_id);
        assert_ne!(cached.review_id, offered.review_id);

        let error = controller
            .review_runtime_version(&runtime_lock().ua_version)
            .unwrap_err();
        assert!(error.contains("Only a newer runtime"), "{error}");
        assert!(ua_desktop_core::guidance::is_guidance(&error));
        // A refused choice keeps the chosen review.
        let cached = controller
            .lock_state()
            .unwrap()
            .update_review
            .clone()
            .unwrap();
        assert_eq!(cached.review_id, chosen.review_id);

        assert_eq!(
            controller.release_page("launcher", "99.0.0").unwrap(),
            "https://github.com/enzimo/universal-agents-desktop/releases/tag/v99.0.0"
        );
        assert_eq!(
            controller.release_page("runtime", "99.1.0").unwrap(),
            "https://github.com/enzimo/universal-agents/releases/tag/v99.1.0"
        );
        assert!(controller.release_page("runtime", "98.0.0").is_err());
    }

    #[test]
    fn selecting_subscription_only_saves_intent_without_docker() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("auth.json");
        fs::write(&source, "{}").unwrap();
        let data = temp.path().join("app/ua-data");
        fs::create_dir_all(&data).unwrap();
        let settings = temp.path().join("app/desktop-settings.json");
        let controller =
            DesktopController::new(runtime_lock(), data.clone(), settings.clone()).unwrap();
        let status = controller
            .connect_openai_subscription(source.canonicalize().unwrap())
            .unwrap();
        assert_eq!(
            status.openai_subscription_state,
            OpenAiSubscriptionState::PendingImport
        );
        assert!(
            DesktopSettingsStore::new(settings, data)
                .load()
                .unwrap()
                .pending_codex_auth_file
                .is_some()
        );
    }

    #[test]
    fn open_folder_targets_the_effective_workspace() {
        let temp = TempDir::new().unwrap();
        let data_dir = temp.path().join("ua-data");
        fs::create_dir_all(&data_dir).unwrap();
        let settings_path = temp.path().join("desktop-settings.json");
        let controller =
            DesktopController::new(runtime_lock(), data_dir.clone(), settings_path.clone())
                .unwrap();

        // Without a saved folder the effective workspace is UA's shared team
        // folder inside app data, the same path the runtime lock mounts.
        assert_eq!(
            controller.workspace_dir().unwrap(),
            data_dir.join("workspaces").join("team").join("team-shared")
        );

        // A saved folder becomes the folder the Open folder action reveals.
        // Settings are written directly so this test needs no Docker.
        let workspace = temp.path().join("project");
        fs::create_dir_all(&workspace).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let store = DesktopSettingsStore::new(settings_path, data_dir.clone());
        store
            .save(
                DesktopSettings::default()
                    .with_external_workspace(Some(workspace.clone()), &data_dir)
                    .expect("select workspace"),
            )
            .expect("save settings");
        let controller =
            DesktopController::new(runtime_lock(), data_dir, store.path().to_owned()).unwrap();

        assert_eq!(controller.workspace_dir().unwrap(), workspace);
    }

    #[test]
    fn controller_opens_with_unavailable_saved_sources() {
        let temp_dir = TempDir::new().expect("temporary directory");
        let app_dir = temp_dir.path().join("app-data");
        let data_dir = app_dir.join("ua-data");
        let workspace = temp_dir.path().join("project");
        let auth_file = temp_dir.path().join("codex/auth.json");
        fs::create_dir_all(&data_dir).expect("data directory");
        fs::create_dir_all(&workspace).expect("workspace directory");
        fs::create_dir_all(auth_file.parent().expect("auth parent")).expect("auth directory");
        fs::write(&auth_file, "{}").expect("auth file");
        let settings_path = app_dir.join("desktop-settings.json");
        let store = DesktopSettingsStore::new(settings_path.clone(), data_dir.clone());
        store
            .save(
                DesktopSettings::default()
                    .with_external_workspace(Some(workspace.clone()), &data_dir)
                    .expect("select workspace")
                    .with_codex_auth_file(auth_file.clone(), &data_dir)
                    .expect("select auth file"),
            )
            .expect("save settings");
        fs::remove_dir_all(workspace).expect("remove workspace");
        fs::remove_file(auth_file).expect("remove auth file");

        let controller = DesktopController::new(runtime_lock(), data_dir, settings_path)
            .expect("controller must remain available for recovery");
        let status = controller.integration_status().expect("integration status");

        assert!(!status.workspace_available);
        assert!(status.workspace_issue.is_some());
        assert_eq!(
            status.openai_subscription_state,
            OpenAiSubscriptionState::SourceUnavailable
        );
    }

    #[test]
    fn unavailable_auth_import_can_be_cancelled_without_docker() {
        let temp_dir = TempDir::new().expect("temporary directory");
        let app_dir = temp_dir.path().join("app-data");
        let data_dir = app_dir.join("ua-data");
        let auth_file = temp_dir.path().join("codex/auth.json");
        fs::create_dir_all(&data_dir).expect("data directory");
        fs::create_dir_all(auth_file.parent().expect("auth parent")).expect("auth directory");
        fs::write(&auth_file, "{}").expect("auth file");
        let settings_path = app_dir.join("desktop-settings.json");
        let store = DesktopSettingsStore::new(settings_path.clone(), data_dir.clone());
        store
            .save(
                DesktopSettings::default()
                    .with_codex_auth_file(auth_file.clone(), &data_dir)
                    .expect("select auth file"),
            )
            .expect("save settings");
        fs::remove_file(auth_file).expect("remove auth file");
        let controller =
            DesktopController::new(runtime_lock(), data_dir.clone(), settings_path.clone())
                .expect("controller");

        let status = controller
            .cancel_openai_subscription_import()
            .expect("cancel pending import");

        assert_eq!(
            status.openai_subscription_state,
            OpenAiSubscriptionState::Disconnected
        );
        assert!(
            DesktopSettingsStore::new(settings_path, data_dir)
                .load()
                .expect("saved settings")
                .pending_codex_auth_file
                .is_none()
        );
    }
}
