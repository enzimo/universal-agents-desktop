use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use log::{info, warn};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::RuntimeLock;

const MANAGED_LABEL: &str = "io.universal-agents.desktop.managed";
const HOST_CONTRACT_LABEL: &str = "io.universal-agents.desktop.host-contract";
const RUNTIME_VERSION_LABEL: &str = "io.universal-agents.desktop.runtime-version";
const CONFIGURATION_LABEL: &str = "io.universal-agents.desktop.configuration";
const REPLACEMENT_BACKUP_SUFFIX: &str = "-replacement-backup";
const HEALTH_TIMEOUT: Duration = Duration::from_millis(800);
const MAX_HEALTH_BYTES: u64 = 64 * 1024;
/// Recent log lines searched for UA's own explanation of a refused start.
const STARTUP_FAILURE_LOG_TAIL: usize = 200;
const MAX_STARTUP_REASON_CHARS: usize = 600;
/// Phrases UA's launchers log, once, right before exiting with a refusal
/// (`docker/launch_headless.py` and `docker/run_bundle.py`). The text that
/// follows each phrase is UA's operator-facing reason.
const STARTUP_REFUSAL_MARKERS: [&str; 2] = [" launch refused. ", " launch aborted because "];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn success(stdout: impl Into<String>) -> Self {
        Self {
            code: 0,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }
}

pub trait CommandRunner: Send + Sync + 'static {
    fn run(&self, args: &[String]) -> Result<CommandOutput, RuntimeError>;

    fn pull(&self, image: &str) -> Result<CommandOutput, RuntimeError> {
        self.run(&["pull".to_owned(), image.to_owned()])
    }

    /// Guidance when Docker's settings name a credential helper for
    /// `registry` that is not installed, so every pull would fail.
    fn missing_credential_helper(&self, _registry: &str) -> Option<String> {
        None
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessDocker;

impl CommandRunner for ProcessDocker {
    fn missing_credential_helper(&self, registry: &str) -> Option<String> {
        crate::registry::missing_credential_helper(registry)
    }

    fn run(&self, args: &[String]) -> Result<CommandOutput, RuntimeError> {
        let output = crate::platform::command("docker")
            .args(args)
            .output()
            .map_err(|error| RuntimeError::DockerCommandUnavailable(error.to_string()))?;
        Ok(CommandOutput {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }

    fn pull(&self, image: &str) -> Result<CommandOutput, RuntimeError> {
        let mut child = crate::platform::command("docker")
            .args(["pull", image])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| RuntimeError::DockerCommandUnavailable(error.to_string()))?;
        let stdout = child.stdout.take().expect("docker pull stdout is piped");
        let stderr = child.stderr.take().expect("docker pull stderr is piped");
        let (send, receive) = std::sync::mpsc::channel();
        stream_pull_output(stdout, false, send.clone());
        stream_pull_output(stderr, true, send);

        let mut stdout_bytes = Vec::new();
        let mut stderr_bytes = Vec::new();
        let mut stdout_line = Vec::new();
        let mut stderr_line = Vec::new();
        let mut progress = PullProgress::default();
        let mut finished_streams = 0;
        while finished_streams < 2 {
            match receive.recv() {
                Ok(PullOutput::Chunk { stderr, bytes }) => {
                    append_bounded(
                        if stderr {
                            &mut stderr_bytes
                        } else {
                            &mut stdout_bytes
                        },
                        &bytes,
                    );
                    observe_pull_chunk(
                        if stderr {
                            &mut stderr_line
                        } else {
                            &mut stdout_line
                        },
                        &bytes,
                        &mut progress,
                    );
                }
                Ok(PullOutput::Finished) => finished_streams += 1,
                Err(_) => break,
            }
        }
        observe_pull_line(&stdout_line, &mut progress);
        observe_pull_line(&stderr_line, &mut progress);
        let status = child
            .wait()
            .map_err(|error| RuntimeError::DockerCommandUnavailable(error.to_string()))?;
        Ok(CommandOutput {
            code: status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&stdout_bytes).trim().to_owned(),
            stderr: String::from_utf8_lossy(&stderr_bytes).trim().to_owned(),
        })
    }
}

const MAX_CAPTURED_COMMAND_BYTES: usize = 4 * 1024 * 1024;

enum PullOutput {
    Chunk { stderr: bool, bytes: Vec<u8> },
    Finished,
}

fn stream_pull_output(
    mut stream: impl Read + Send + 'static,
    stderr: bool,
    send: std::sync::mpsc::Sender<PullOutput>,
) {
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(size) => {
                    if send
                        .send(PullOutput::Chunk {
                            stderr,
                            bytes: buffer[..size].to_vec(),
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }
        let _ = send.send(PullOutput::Finished);
    });
}

fn append_bounded(output: &mut Vec<u8>, bytes: &[u8]) {
    if bytes.len() >= MAX_CAPTURED_COMMAND_BYTES {
        output.clear();
        output.extend_from_slice(&bytes[bytes.len() - MAX_CAPTURED_COMMAND_BYTES..]);
        return;
    }
    let excess = output
        .len()
        .saturating_add(bytes.len())
        .saturating_sub(MAX_CAPTURED_COMMAND_BYTES);
    if excess > 0 {
        output.drain(..excess);
    }
    output.extend_from_slice(bytes);
}

#[derive(Default)]
struct PullProgress {
    layers: HashMap<String, (u64, u64)>,
    last_reported: Option<(u64, u64)>,
}

fn observe_pull_chunk(pending: &mut Vec<u8>, bytes: &[u8], progress: &mut PullProgress) {
    for byte in bytes {
        if matches!(byte, b'\r' | b'\n') {
            observe_pull_line(pending, progress);
            pending.clear();
        } else {
            pending.push(*byte);
        }
    }
}

fn observe_pull_line(bytes: &[u8], progress: &mut PullProgress) {
    let line = String::from_utf8_lossy(bytes);
    let Some((layer, detail)) = line.split_once(':') else {
        return;
    };
    let layer = layer.trim();
    if layer.is_empty() || layer.bytes().any(|byte| !byte.is_ascii_hexdigit()) {
        return;
    }
    if detail.contains("Downloading") {
        if let Some((current, total)) = detail
            .split_whitespace()
            .find_map(|part| part.split_once('/'))
            .and_then(|(current, total)| {
                Some((parse_docker_bytes(current)?, parse_docker_bytes(total)?))
            })
        {
            progress.layers.insert(layer.to_owned(), (current, total));
        }
    } else if detail.contains("Pull complete") || detail.contains("Download complete") {
        if let Some((current, total)) = progress.layers.get_mut(layer) {
            *current = *total;
        }
    }
    let totals = progress.layers.values().fold((0_u64, 0_u64), |sum, layer| {
        (sum.0.saturating_add(layer.0), sum.1.saturating_add(layer.1))
    });
    if totals.1 > 0 && progress.last_reported != Some(totals) {
        crate::progress::report_download(totals.0.min(totals.1), totals.1);
        progress.last_reported = Some(totals);
    }
}

fn parse_docker_bytes(value: &str) -> Option<u64> {
    let split = value.find(|character: char| !character.is_ascii_digit() && character != '.')?;
    let number: f64 = value[..split].parse().ok()?;
    let multiplier = match value[split..].trim().to_ascii_lowercase().as_str() {
        "b" => 1_f64,
        "kb" => 1_000_f64,
        "mb" => 1_000_000_f64,
        "gb" => 1_000_000_000_f64,
        _ => return None,
    };
    let bytes = number * multiplier;
    (bytes.is_finite() && bytes >= 0.0).then_some(bytes.round() as u64)
}

fn checked_pull(output: CommandOutput) -> Result<CommandOutput, RuntimeError> {
    if output.code == 0 {
        Ok(output)
    } else {
        Err(RuntimeError::DockerCommandFailed {
            command: "pull".to_owned(),
            detail: command_detail(&output),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePhase {
    DockerUnavailable,
    NotCreated,
    Stopped,
    Starting,
    /// UA exited with an error while starting and Docker keeps restarting it.
    /// Unlike `Error`, the container exists and is running under Docker's
    /// restart policy, so Stop and Restart stay available.
    StartupFailed,
    Onboarding,
    Ready,
    Error,
}

/// Read-only answer to "can this host reach a working Docker?".
///
/// The launcher polls this while Docker is missing, so it must never start,
/// pull, or mutate anything. `installed` separates "install Docker" from
/// "start Docker", which are different user actions in the prerequisite dialog.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DockerPrerequisite {
    pub available: bool,
    pub installed: bool,
    pub detail: String,
    pub setup_hint: String,
    pub install_url: String,
    pub setup_guide_url: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RuntimeStatus {
    pub phase: RuntimePhase,
    pub detail: String,
    pub ua_version: String,
    pub image: String,
    pub web_url: String,
    pub docs_url: String,
    pub data_dir: String,
    pub workspace_dir: String,
    pub workspace_is_external: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RuntimeConfiguration {
    pub external_workspace: Option<PathBuf>,
    pub refresh_external_skills_on_start: bool,
}

impl RuntimeConfiguration {
    fn fingerprint(&self) -> String {
        let workspace = self
            .external_workspace
            .as_deref()
            .map(Path::to_string_lossy)
            .unwrap_or_default();
        let mut digest = Sha256::new();
        // v3 adds an explicit runtime-owned skill refresh override. Replacing
        // older containers once is required so an unchecked switch also wins
        // over a true value saved in UA's opaque configuration.
        digest.update(b"universal-agents-desktop-configuration-v3\0");
        digest.update(workspace.as_bytes());
        digest.update(if self.refresh_external_skills_on_start {
            b"\0refresh-external-skills=true".as_slice()
        } else {
            b"\0refresh-external-skills=false".as_slice()
        });
        format!("{:x}", digest.finalize())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexImportOutcome {
    PendingRuntime,
    Imported,
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("invalid runtime lock: {0}")]
    InvalidRuntimeLock(String),
    #[error("Docker command is unavailable: {0}")]
    DockerCommandUnavailable(String),
    #[error("Docker command failed: docker {command}: {detail}")]
    DockerCommandFailed { command: String, detail: String },
    #[error(
        "{}",
        crate::guidance::guidance(format!(
            "Docker already has a container named {name} that this app did not create, so the app leaves it alone. Rename or remove that container in Docker, then choose Start."
        ))
    )]
    UnmanagedContainer { name: String },
    /// A desktop-managed container from a setup this app has no record of,
    /// for example after the app-data folder was moved aside while the old
    /// container stayed in Docker.
    #[error(
        "{}",
        crate::guidance::guidance(format!(
            "Docker still has a Universal Agents container from another setup ({found}), but this app is set up for UA {expected}. The app does not change a container it has no record of. If you meant to start fresh, remove that container with \"docker rm -f {name}\" and choose Start. Removing the container does not delete any folder on this computer."
        ))
    )]
    UnrecordedContainer {
        name: String,
        found: String,
        expected: String,
    },
    #[error("managed container does not match the locked runtime: {0}")]
    RuntimeMismatch(String),
    #[error("persistent data path cannot be used: {0}")]
    InvalidDataPath(String),
    #[error(
        "{}",
        crate::guidance::guidance(format!(
            "Port {port} on this computer is already used by another program, and Universal Agents needs it. Quit that program, then retry."
        ))
    )]
    PortInUse { port: u16 },
    /// Recovery steps written entirely by this app (see `crate::guidance`).
    #[error("{0}")]
    Guidance(String),
    #[error("Docker returned an invalid container inspection response: {0}")]
    InvalidInspection(String),
    #[error("container replacement failed: {cause}; rollback: {rollback}")]
    RuntimeReplacementFailed { cause: String, rollback: String },
    #[error("OpenAI subscription import is unavailable: {0}")]
    CodexImport(String),
}

pub struct RuntimeManager<R: CommandRunner> {
    runner: R,
    runtime_lock: RuntimeLock,
    data_dir: PathBuf,
    configuration: RuntimeConfiguration,
    /// Restart count of the last startup failure written to the log, so a
    /// status poll every few seconds logs each failed attempt only once.
    logged_startup_failure: Mutex<Option<u32>>,
    /// The last container problem written to the log, for the same reason.
    logged_status_error: Mutex<Option<String>>,
}

impl<R: CommandRunner> RuntimeManager<R> {
    pub fn new(
        runner: R,
        runtime_lock: RuntimeLock,
        data_dir: PathBuf,
        configuration: RuntimeConfiguration,
    ) -> Result<Self, RuntimeError> {
        runtime_lock.validate()?;
        validate_data_path(&data_dir)?;
        validate_runtime_configuration(&configuration)?;
        Ok(Self {
            runner,
            runtime_lock,
            data_dir,
            configuration,
            logged_startup_failure: Mutex::new(None),
            logged_status_error: Mutex::new(None),
        })
    }

    pub fn runtime_lock(&self) -> &RuntimeLock {
        &self.runtime_lock
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn configuration(&self) -> &RuntimeConfiguration {
        &self.configuration
    }

    pub fn web_url(&self) -> String {
        if matches!(probe_health(&self.runtime_lock), HealthState::Onboarding) {
            self.runtime_lock.onboarding_url()
        } else {
            self.runtime_lock.web_url()
        }
    }

    /// Probe Docker without changing anything.
    ///
    /// Safe to call on a timer: it runs only `docker info` and never pulls an
    /// image, creates a container, or writes to UA's data.
    pub fn docker_prerequisite(&self) -> Result<DockerPrerequisite, RuntimeError> {
        let host = crate::platform::Host::current();
        let mut prerequisite = DockerPrerequisite {
            available: false,
            installed: false,
            detail: String::new(),
            setup_hint: host.docker_setup_hint().to_owned(),
            install_url: host.docker_install_url().to_owned(),
            setup_guide_url: crate::platform::DOCKER_SETUP_GUIDE_URL.to_owned(),
        };
        let args = [
            "info".to_owned(),
            "--format".to_owned(),
            "{{.ServerVersion}}".to_owned(),
        ];
        match self.runner.run(&args) {
            Ok(output) if output.code == 0 => {
                prerequisite.available = true;
                prerequisite.installed = true;
                prerequisite.detail = "Docker is installed and running.".to_owned();
            }
            Ok(output) => {
                // The CLI answered but the daemon did not: Docker is present
                // and simply needs to be started.
                prerequisite.installed = true;
                prerequisite.detail = format!(
                    "Docker is installed but its daemon is not reachable: {}. {}",
                    command_detail(&output),
                    host.docker_setup_hint()
                );
            }
            Err(RuntimeError::DockerCommandUnavailable(detail)) => {
                prerequisite.detail = format!(
                    "Docker CLI is required and was not found: {detail}. {}",
                    host.docker_setup_hint()
                );
            }
            Err(error) => return Err(error),
        }
        Ok(prerequisite)
    }

    /// Whether the exact runtime image is already in the local image store.
    ///
    /// Read-only, so an update check can report "downloaded" without pulling.
    pub fn image_present(&self, image: &str) -> Result<bool, RuntimeError> {
        let args = [
            "image".to_owned(),
            "inspect".to_owned(),
            image.to_owned(),
            "--format".to_owned(),
            "{{.Id}}".to_owned(),
        ];
        Ok(self.runner.run(&args)?.code == 0)
    }

    /// Download a reviewed runtime image without touching the running container.
    ///
    /// This is the "download now, restart later" half of a runtime update. The
    /// candidate is re-validated against the current runtime profile here so a
    /// caller cannot pull an image that changes the host contract, and no
    /// container is created, replaced, or restarted.
    pub fn download_runtime_image(&self, candidate: &RuntimeLock) -> Result<(), RuntimeError> {
        candidate.validate()?;
        crate::updates::validate_profile(&self.runtime_lock, candidate)
            .map_err(RuntimeError::RuntimeMismatch)?;
        self.require_docker()?;
        if self.image_present(&candidate.image)? {
            crate::progress::report(format!(
                "Universal Agents {} is already downloaded.",
                candidate.ua_version
            ));
            return Ok(());
        }
        self.pull_image(&candidate.image)
    }

    pub fn status(&self) -> Result<RuntimeStatus, RuntimeError> {
        if let Some(detail) = self.docker_unavailable_detail()? {
            return Ok(self.status_value(RuntimePhase::DockerUnavailable, detail));
        }

        match self.inspect()? {
            ContainerPresence::Missing => Ok(self.status_value(
                RuntimePhase::NotCreated,
                "Universal Agents has not been installed on this computer yet.",
            )),
            ContainerPresence::Present(container) => {
                match self.classify_container(&container) {
                    Ok(ContainerReconciliation::Current) => {}
                    Ok(ContainerReconciliation::Replaceable(detail)) => {
                        let phase = if container.state.running {
                            RuntimePhase::Starting
                        } else {
                            RuntimePhase::Stopped
                        };
                        return Ok(self.status_value(
                            phase,
                            format!("A runtime change is ready. Start or restart Universal Agents to apply it: {detail}"),
                        ));
                    }
                    Err(error) => {
                        let text = error.to_string();
                        self.log_status_error_once(&text);
                        // App-written guidance says what to do; other errors
                        // may carry raw Docker output and stay in diagnostics.
                        let detail = if crate::guidance::is_guidance(&text) {
                            crate::guidance::plain(&text)
                        } else {
                            "Universal Agents needs attention. Download diagnostics for details."
                                .to_owned()
                        };
                        return Ok(self.status_value(RuntimePhase::Error, detail));
                    }
                }
                if !container.state.running {
                    return Ok(
                        self.status_value(RuntimePhase::Stopped, "Universal Agents is stopped.")
                    );
                }
                // Docker keeps a restarting container "running" and holds the
                // last exit code until the next start. A zero code is UA's own
                // restart after first-time setup, not a failure.
                if container.state.restarting && container.state.exit_code != 0 {
                    return Ok(self.status_value(
                        RuntimePhase::StartupFailed,
                        self.startup_failure_detail(&container),
                    ));
                }

                let phase = match probe_health(&self.runtime_lock) {
                    HealthState::Onboarding => RuntimePhase::Onboarding,
                    HealthState::Ready => RuntimePhase::Ready,
                    HealthState::Unavailable(detail) => {
                        return Ok(self.status_value(RuntimePhase::Starting, detail));
                    }
                };
                let detail = match phase {
                    RuntimePhase::Onboarding => {
                        "Universal Agents is ready for first-time setup.".to_owned()
                    }
                    RuntimePhase::Ready => "Universal Agents is ready.".to_owned(),
                    _ => unreachable!("health probe returns a ready phase"),
                };
                Ok(self.status_value(phase, detail))
            }
        }
    }

    pub fn start(&self) -> Result<RuntimeStatus, RuntimeError> {
        validate_runtime_sources(&self.configuration)?;
        self.require_docker()?;
        match self.inspect()? {
            ContainerPresence::Missing => self.create_and_start()?,
            ContainerPresence::Present(container) => match self.classify_container(&container)? {
                ContainerReconciliation::Current => {
                    if !container.state.running {
                        info!(
                            "starting managed Universal Agents container {}",
                            self.runtime_lock.container_name
                        );
                        self.run_checked(&[
                            "start".to_owned(),
                            self.runtime_lock.container_name.clone(),
                        ])?;
                    }
                }
                ContainerReconciliation::Replaceable(detail) => {
                    info!("refreshing desktop-managed Universal Agents container: {detail}");
                    self.replace_container(&container)?;
                }
            },
        }
        self.status()
    }

    pub fn stop(&self) -> Result<RuntimeStatus, RuntimeError> {
        self.require_docker()?;
        match self.inspect()? {
            ContainerPresence::Missing => return self.status(),
            ContainerPresence::Present(container) => {
                self.validate_replacement_candidate(&container)?;
                if container.state.running {
                    info!(
                        "stopping managed Universal Agents container {}",
                        self.runtime_lock.container_name
                    );
                    self.run_checked(&[
                        "stop".to_owned(),
                        "--time".to_owned(),
                        "30".to_owned(),
                        self.runtime_lock.container_name.clone(),
                    ])?;
                }
            }
        }
        self.status()
    }

    pub fn restart(&self) -> Result<RuntimeStatus, RuntimeError> {
        validate_runtime_sources(&self.configuration)?;
        self.require_docker()?;
        match self.inspect()? {
            ContainerPresence::Missing => self.create_and_start()?,
            ContainerPresence::Present(container) => match self.classify_container(&container)? {
                ContainerReconciliation::Current => {
                    info!(
                        "restarting managed Universal Agents container {}",
                        self.runtime_lock.container_name
                    );
                    self.run_checked(&[
                        "restart".to_owned(),
                        "--time".to_owned(),
                        "30".to_owned(),
                        self.runtime_lock.container_name.clone(),
                    ])?;
                }
                ContainerReconciliation::Replaceable(detail) => {
                    info!(
                        "refreshing desktop-managed Universal Agents container instead of restarting: {detail}"
                    );
                    self.replace_container(&container)?;
                }
            },
        }
        self.status()
    }

    pub fn logs(&self, requested_tail: usize) -> Result<String, RuntimeError> {
        self.require_docker()?;
        let ContainerPresence::Present(container) = self.inspect()? else {
            return Ok("Universal Agents has not created a container yet.".to_owned());
        };
        self.validate_replacement_candidate(&container)?;
        let tail = requested_tail.clamp(20, 1_000);
        let output = self.run_checked(&[
            "logs".to_owned(),
            "--tail".to_owned(),
            tail.to_string(),
            self.runtime_lock.container_name.clone(),
        ])?;
        if output.stdout.is_empty() {
            Ok(output.stderr)
        } else if output.stderr.is_empty() {
            Ok(output.stdout)
        } else {
            Ok(format!("{}\n{}", output.stdout, output.stderr))
        }
    }

    pub fn import_codex_auth(&self, auth_file: &Path) -> Result<CodexImportOutcome, RuntimeError> {
        validate_codex_auth_source(auth_file)?;
        self.require_docker()?;
        let ContainerPresence::Present(container) = self.inspect()? else {
            return Ok(CodexImportOutcome::PendingRuntime);
        };
        if self.classify_container(&container)? != ContainerReconciliation::Current {
            return Err(RuntimeError::CodexImport(
                "the runtime must finish applying its desktop configuration first".to_owned(),
            ));
        }
        if !container.state.running {
            return Ok(CodexImportOutcome::PendingRuntime);
        }

        let mut args = vec![
            "run".to_owned(),
            "--rm".to_owned(),
            "--pull".to_owned(),
            "never".to_owned(),
            "--network".to_owned(),
            "none".to_owned(),
            "--volume".to_owned(),
            format!(
                "{}:{}",
                crate::platform::docker_source(&self.data_dir)
                    .map_err(RuntimeError::InvalidDataPath)?,
                self.runtime_lock.data_mount
            ),
            "--volume".to_owned(),
            format!(
                "{}:{}:ro",
                crate::platform::docker_source(auth_file).map_err(RuntimeError::InvalidDataPath)?,
                self.runtime_lock.codex_auth_mount
            ),
            "--entrypoint".to_owned(),
            self.runtime_lock.codex_import_command[0].clone(),
            self.runtime_lock.image.clone(),
        ];
        args.extend(self.runtime_lock.codex_import_command[1..].iter().cloned());
        args.extend([
            self.runtime_lock.codex_home.clone(),
            self.runtime_lock.codex_auth_store.clone(),
        ]);
        self.run_checked(&args)?;
        Ok(CodexImportOutcome::Imported)
    }

    fn create_and_start(&self) -> Result<(), RuntimeError> {
        fs::create_dir_all(&self.data_dir)
            .map_err(|error| RuntimeError::InvalidDataPath(error.to_string()))?;
        check_port_available(self.runtime_lock.container_ports.web)?;
        check_port_available(self.runtime_lock.container_ports.broker)?;

        self.pull_image(&self.runtime_lock.image)?;
        self.run_container()
    }

    fn pull_image(&self, image: &str) -> Result<(), RuntimeError> {
        crate::progress::report(
            "Fetching runtime image from GHCR. Large images can take several minutes.",
        );
        let registry = image_registry(image);
        if let Some(guidance) = self.runner.missing_credential_helper(registry) {
            warn!(
                "not pulling {image}: Docker's credential helper for {registry} is not installed"
            );
            return Err(RuntimeError::Guidance(guidance));
        }
        info!("pulling locked Universal Agents image {image}");
        self.runner.pull(image).and_then(checked_pull).map_err(|error| {
            if let RuntimeError::DockerCommandFailed { detail, .. } = &error {
                let lower = detail.to_ascii_lowercase();
                if lower.contains("unauthorized") || lower.contains("denied") {
                    warn!("pulling {image} was denied: {detail}");
                    return RuntimeError::Guidance(crate::guidance::guidance(
                        "Image access was denied. Use Private image access to sign in with GitHub CLI or a classic PAT with read:packages, then verify access and retry.",
                    ));
                }
            }
            error
        })?;
        Ok(())
    }

    fn run_container(&self) -> Result<(), RuntimeError> {
        crate::progress::report("Image ready. Starting the Universal Agents runtime…");
        let data_dir = crate::platform::docker_source(&self.data_dir)
            .map_err(RuntimeError::InvalidDataPath)?;
        let mut args = vec![
            "run".to_owned(),
            "--detach".to_owned(),
            "--pull".to_owned(),
            "never".to_owned(),
            "--name".to_owned(),
            self.runtime_lock.container_name.clone(),
            "--restart".to_owned(),
            "unless-stopped".to_owned(),
            "--label".to_owned(),
            format!("{MANAGED_LABEL}=true"),
            "--label".to_owned(),
            format!(
                "{HOST_CONTRACT_LABEL}={}",
                self.runtime_lock.host_contract_version
            ),
            "--label".to_owned(),
            format!("{RUNTIME_VERSION_LABEL}={}", self.runtime_lock.ua_version),
            "--label".to_owned(),
            format!("{CONFIGURATION_LABEL}={}", self.configuration.fingerprint()),
            "--publish".to_owned(),
            format!("127.0.0.1:{}:18090", self.runtime_lock.container_ports.web),
            "--publish".to_owned(),
            format!(
                "127.0.0.1:{}:18600",
                self.runtime_lock.container_ports.broker
            ),
            "--volume".to_owned(),
            format!("{data_dir}:{}", self.runtime_lock.data_mount),
            "--env".to_owned(),
            "NETWORK_PUBLIC_ORIGIN=http://127.0.0.1".to_owned(),
            "--env".to_owned(),
            format!(
                "OPENAI_CODEX_AUTH_STORE_PATH={}",
                self.runtime_lock.codex_auth_store
            ),
            "--env".to_owned(),
            format!(
                "TEAM_REFRESH_EXTERNAL_SKILLS={}",
                self.configuration.refresh_external_skills_on_start
            ),
            "--env".to_owned(),
            format!(
                "INTERNAL_WEBSITE_PUBLIC_PORT={}",
                self.runtime_lock.container_ports.web
            ),
            "--env".to_owned(),
            format!(
                "SECURE_TOOL_BROKER_PUBLIC_PORT={}",
                self.runtime_lock.container_ports.broker
            ),
        ];

        if let Some(workspace) = &self.configuration.external_workspace {
            args.extend([
                "--volume".to_owned(),
                format!(
                    "{}:{}",
                    crate::platform::docker_source(workspace)
                        .map_err(RuntimeError::InvalidDataPath)?,
                    self.runtime_lock.workspace_mount
                ),
            ]);
        }
        args.push(self.runtime_lock.image.clone());
        args.extend(self.runtime_lock.command.iter().cloned());

        info!(
            "creating managed Universal Agents container {}",
            self.runtime_lock.container_name
        );
        self.run_checked(&args)?;
        Ok(())
    }

    fn replace_container(&self, container: &ContainerInspection) -> Result<(), RuntimeError> {
        self.validate_replacement_candidate(container)?;
        let backup_name = format!(
            "{}{}",
            self.runtime_lock.container_name, REPLACEMENT_BACKUP_SUFFIX
        );
        if matches!(
            self.inspect_named(&backup_name)?,
            ContainerPresence::Present(_)
        ) {
            return Err(RuntimeError::RuntimeReplacementFailed {
                cause: format!("backup container name '{backup_name}' is already in use"),
                rollback: "the current runtime was not changed".to_owned(),
            });
        }

        self.pull_image(&self.runtime_lock.image)?;
        let was_running = container.state.running;
        if was_running {
            self.run_checked(&[
                "stop".to_owned(),
                "--time".to_owned(),
                "30".to_owned(),
                self.runtime_lock.container_name.clone(),
            ])?;
        }
        if let Err(error) = self.run_checked(&[
            "rename".to_owned(),
            self.runtime_lock.container_name.clone(),
            backup_name.clone(),
        ]) {
            let rollback = if was_running {
                match self
                    .run_checked(&["start".to_owned(), self.runtime_lock.container_name.clone()])
                {
                    Ok(_) => {
                        "restarted the prior container after its backup rename failed".to_owned()
                    }
                    Err(start_error) => format!(
                        "the prior container kept its name but could not be restarted: {start_error}"
                    ),
                }
            } else {
                "the prior stopped container kept its original name".to_owned()
            };
            return Err(RuntimeError::RuntimeReplacementFailed {
                cause: error.to_string(),
                rollback,
            });
        }

        let replacement_result = check_port_available(self.runtime_lock.container_ports.web)
            .and_then(|_| check_port_available(self.runtime_lock.container_ports.broker))
            .and_then(|_| self.run_container());
        if let Err(error) = replacement_result {
            let rollback = self.rollback_replacement(&backup_name, was_running);
            return Err(RuntimeError::RuntimeReplacementFailed {
                cause: error.to_string(),
                rollback,
            });
        }

        if let Err(error) = self.run_checked(&["rm".to_owned(), backup_name.clone()]) {
            warn!(
                "replacement succeeded but stale backup container {backup_name} could not be removed: {error}"
            );
        }
        Ok(())
    }

    fn rollback_replacement(&self, backup_name: &str, was_running: bool) -> String {
        if let Ok(ContainerPresence::Present(current)) = self.inspect() {
            if self.classify_container(&current).is_ok() {
                let _ = self.run_checked(&[
                    "rm".to_owned(),
                    "--force".to_owned(),
                    self.runtime_lock.container_name.clone(),
                ]);
            }
        }

        if let Err(error) = self.run_checked(&[
            "rename".to_owned(),
            backup_name.to_owned(),
            self.runtime_lock.container_name.clone(),
        ]) {
            return format!("failed to restore the prior container name: {error}");
        }
        if was_running {
            if let Err(error) =
                self.run_checked(&["start".to_owned(), self.runtime_lock.container_name.clone()])
            {
                return format!("restored the prior container but failed to restart it: {error}");
            }
        }
        "restored the prior desktop-managed container; persistent data was unchanged".to_owned()
    }

    fn docker_unavailable_detail(&self) -> Result<Option<String>, RuntimeError> {
        let prerequisite = self.docker_prerequisite()?;
        Ok((!prerequisite.available).then_some(prerequisite.detail))
    }

    fn require_docker(&self) -> Result<(), RuntimeError> {
        if let Some(detail) = self.docker_unavailable_detail()? {
            return Err(RuntimeError::DockerCommandUnavailable(detail));
        }
        Ok(())
    }

    fn inspect(&self) -> Result<ContainerPresence, RuntimeError> {
        self.inspect_named(&self.runtime_lock.container_name)
    }

    fn inspect_named(&self, name: &str) -> Result<ContainerPresence, RuntimeError> {
        let args = [
            "container".to_owned(),
            "inspect".to_owned(),
            name.to_owned(),
        ];
        let output = self.runner.run(&args)?;
        if output.code != 0 {
            let detail = command_detail(&output);
            if detail.contains("No such object") || detail.contains("No such container") {
                return Ok(ContainerPresence::Missing);
            }
            return Err(RuntimeError::DockerCommandFailed {
                command: args.join(" "),
                detail,
            });
        }
        let mut containers: Vec<ContainerInspection> = serde_json::from_str(&output.stdout)
            .map_err(|error| RuntimeError::InvalidInspection(error.to_string()))?;
        if containers.len() != 1 {
            return Err(RuntimeError::InvalidInspection(format!(
                "expected one container, received {}",
                containers.len()
            )));
        }
        Ok(ContainerPresence::Present(containers.remove(0)))
    }

    fn classify_container(
        &self,
        container: &ContainerInspection,
    ) -> Result<ContainerReconciliation, RuntimeError> {
        let identity = self.validate_replacement_candidate(container)?;
        if let ManagedIdentity::Upgrade(source) = identity {
            return Ok(ContainerReconciliation::Replaceable(format!(
                "upgrading Universal Agents {} to {} from reviewed host contract {}",
                source.ua_version, self.runtime_lock.ua_version, source.host_contract_version
            )));
        }
        let labels =
            container
                .config
                .labels
                .as_ref()
                .ok_or_else(|| RuntimeError::UnmanagedContainer {
                    name: self.runtime_lock.container_name.clone(),
                })?;
        let contract = labels
            .get(HOST_CONTRACT_LABEL)
            .and_then(|value| value.parse::<u32>().ok());
        let expected_contract = self.runtime_lock.host_contract_version;
        if contract != Some(expected_contract) {
            return Err(RuntimeError::RuntimeMismatch(format!(
                "container host contract label is {:?}; expected {}",
                labels.get(HOST_CONTRACT_LABEL),
                expected_contract
            )));
        }
        let expected_configuration = self.configuration.fingerprint();
        if labels.get(CONFIGURATION_LABEL).map(String::as_str)
            != Some(expected_configuration.as_str())
        {
            return Ok(ContainerReconciliation::Replaceable(
                "the desktop container configuration changed".to_owned(),
            ));
        }
        Ok(ContainerReconciliation::Current)
    }

    fn validate_replacement_candidate<'a>(
        &'a self,
        container: &ContainerInspection,
    ) -> Result<ManagedIdentity<'a>, RuntimeError> {
        let Some(labels) = container.config.labels.as_ref() else {
            warn!(
                "refusing to manage container {} without desktop ownership label",
                self.runtime_lock.container_name
            );
            return Err(RuntimeError::UnmanagedContainer {
                name: self.runtime_lock.container_name.clone(),
            });
        };
        if labels.get(MANAGED_LABEL).map(String::as_str) != Some("true") {
            warn!(
                "refusing to manage container {} without desktop ownership label",
                self.runtime_lock.container_name
            );
            return Err(RuntimeError::UnmanagedContainer {
                name: self.runtime_lock.container_name.clone(),
            });
        }
        let contract = labels
            .get(HOST_CONTRACT_LABEL)
            .and_then(|value| value.parse::<u32>().ok());
        let version = labels.get(RUNTIME_VERSION_LABEL).map(String::as_str);
        if contract == Some(self.runtime_lock.host_contract_version)
            && version == Some(self.runtime_lock.ua_version.as_str())
            && container.config.image == self.runtime_lock.image
        {
            return Ok(ManagedIdentity::Current);
        }
        if let Some(source) = self.runtime_lock.upgrade_sources.iter().find(|source| {
            contract == Some(source.host_contract_version)
                && version == Some(source.ua_version.as_str())
                && container.config.image == source.image
        }) {
            return Ok(ManagedIdentity::Upgrade(source));
        }
        warn!(
            "managed container matches neither the selected runtime nor the runtime it replaces: contract {:?}, runtime {:?}, image '{}'",
            labels.get(HOST_CONTRACT_LABEL),
            labels.get(RUNTIME_VERSION_LABEL),
            container.config.image
        );
        Err(RuntimeError::UnrecordedContainer {
            name: self.runtime_lock.container_name.clone(),
            found: version.map_or_else(
                || "an unknown UA version".to_owned(),
                |version| format!("UA {version}"),
            ),
            expected: self.runtime_lock.ua_version.clone(),
        })
    }

    fn run_checked(&self, args: &[String]) -> Result<CommandOutput, RuntimeError> {
        let output = self.runner.run(args)?;
        if output.code == 0 {
            Ok(output)
        } else {
            Err(RuntimeError::DockerCommandFailed {
                command: args.join(" "),
                detail: command_detail(&output),
            })
        }
    }

    /// Log a container problem found by a status check once, not on every
    /// poll; a new or different problem is logged again.
    fn log_status_error_once(&self, text: &str) {
        let mut logged = self
            .logged_status_error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if logged.as_deref() != Some(text) {
            log::error!(
                "runtime status inspection failed: {}",
                crate::guidance::plain(text)
            );
            *logged = Some(text.to_owned());
        }
    }

    /// Explain a start that UA refused, quoting UA's own reason when its log
    /// still holds one. Reading logs is best effort: the status stays
    /// `StartupFailed` with a generic explanation if Docker cannot return them.
    fn startup_failure_detail(&self, container: &ContainerInspection) -> String {
        let exit_code = container.state.exit_code;
        let restarts = container.restart_count;
        let output = self.runner.run(&[
            "logs".to_owned(),
            "--tail".to_owned(),
            STARTUP_FAILURE_LOG_TAIL.to_string(),
            self.runtime_lock.container_name.clone(),
        ]);
        let reason = match output {
            Ok(output) if output.code == 0 => {
                startup_refusal_reason(&format!("{}\n{}", output.stdout, output.stderr))
            }
            Ok(output) => {
                warn!(
                    "could not read logs for startup failure: container={} detail={}",
                    self.runtime_lock.container_name,
                    command_detail(&output)
                );
                None
            }
            Err(error) => {
                warn!(
                    "could not read logs for startup failure: container={} error={error}",
                    self.runtime_lock.container_name
                );
                None
            }
        };
        let mut logged = self
            .logged_startup_failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *logged != Some(restarts) {
            *logged = Some(restarts);
            warn!(
                "Universal Agents refused to start: container={} exit_code={exit_code} restart_count={restarts} reason={}",
                self.runtime_lock.container_name,
                reason.as_deref().unwrap_or("not found in recent logs")
            );
        }
        drop(logged);
        let attempts = match restarts {
            0 => "has not restarted it yet".to_owned(),
            1 => "has restarted it once".to_owned(),
            count => format!("has restarted it {count} times"),
        };
        let summary = format!(
            "Universal Agents stopped while starting (exit code {exit_code}), and Docker {attempts}."
        );
        match reason {
            Some(reason) => format!(
                "{summary} Universal Agents reported: {reason} Stop it, fix the cause, then start it again. Download diagnostics for the full log."
            ),
            None => format!("{summary} Stop it, then download diagnostics to read the full log."),
        }
    }

    fn status_value(&self, phase: RuntimePhase, detail: impl Into<String>) -> RuntimeStatus {
        let workspace_dir = self
            .configuration
            .external_workspace
            .clone()
            .unwrap_or_else(|| self.runtime_lock.default_workspace_dir(&self.data_dir));
        RuntimeStatus {
            phase: phase.clone(),
            detail: detail.into(),
            ua_version: self.runtime_lock.ua_version.clone(),
            image: self.runtime_lock.image.clone(),
            web_url: if phase == RuntimePhase::Onboarding {
                self.runtime_lock.onboarding_url()
            } else {
                self.runtime_lock.web_url()
            },
            docs_url: self.runtime_lock.docs_url(),
            data_dir: self.data_dir.display().to_string(),
            workspace_dir: workspace_dir.display().to_string(),
            workspace_is_external: self.configuration.external_workspace.is_some(),
        }
    }
}

fn validate_data_path(path: &Path) -> Result<(), RuntimeError> {
    crate::platform::docker_source(path)
        .map(|_| ())
        .map_err(RuntimeError::InvalidDataPath)
}

fn validate_runtime_configuration(
    configuration: &RuntimeConfiguration,
) -> Result<(), RuntimeError> {
    if let Some(workspace) = &configuration.external_workspace {
        validate_data_path(workspace)?;
    }
    Ok(())
}

fn validate_runtime_sources(configuration: &RuntimeConfiguration) -> Result<(), RuntimeError> {
    let Some(workspace) = configuration.external_workspace.as_deref() else {
        return Ok(());
    };
    let resolved = workspace.canonicalize().map_err(|error| {
        RuntimeError::InvalidDataPath(format!(
            "the selected workspace is unavailable at {}: {error}",
            workspace.display()
        ))
    })?;
    if !resolved.is_dir() {
        return Err(RuntimeError::InvalidDataPath(format!(
            "the selected workspace is not a directory: {}",
            workspace.display()
        )));
    }
    if resolved != workspace {
        return Err(RuntimeError::InvalidDataPath(format!(
            "the selected workspace now resolves to {}. Choose it again to approve the new location",
            resolved.display()
        )));
    }
    Ok(())
}

fn validate_codex_auth_source(auth_file: &Path) -> Result<(), RuntimeError> {
    validate_data_path(auth_file)?;
    let resolved = auth_file.canonicalize().map_err(|error| {
        RuntimeError::CodexImport(format!(
            "the selected auth.json is unavailable at {}: {error}",
            auth_file.display()
        ))
    })?;
    if !resolved.is_file()
        || resolved.file_name().and_then(|name| name.to_str()) != Some("auth.json")
    {
        return Err(RuntimeError::CodexImport(format!(
            "the selected credential is not an existing auth.json file: {}",
            auth_file.display()
        )));
    }
    if resolved != auth_file {
        return Err(RuntimeError::CodexImport(format!(
            "the selected auth.json now resolves to {}. Choose it again to approve the new location",
            resolved.display()
        )));
    }
    Ok(())
}

/// The registry Docker contacts for `image`, as written in Docker's
/// `credHelpers` keys: the host before the first `/` when it looks like one,
/// otherwise Docker Hub.
fn image_registry(image: &str) -> &str {
    match image.split_once('/') {
        Some((host, _)) if host.contains(['.', ':']) || host == "localhost" => host,
        _ => "https://index.docker.io/v1/",
    }
}

fn check_port_available(port: u16) -> Result<(), RuntimeError> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .map(drop)
        .map_err(|_| RuntimeError::PortInUse { port })
}

fn command_detail(output: &CommandOutput) -> String {
    if !output.stderr.is_empty() {
        output.stderr.clone()
    } else if !output.stdout.is_empty() {
        output.stdout.clone()
    } else {
        format!("process exited with code {}", output.code)
    }
}

enum ContainerPresence {
    Missing,
    Present(ContainerInspection),
}

#[derive(Debug, Eq, PartialEq)]
enum ContainerReconciliation {
    Current,
    Replaceable(String),
}

enum ManagedIdentity<'a> {
    Current,
    Upgrade(&'a crate::RuntimeIdentity),
}

#[derive(Debug, Deserialize)]
struct ContainerInspection {
    #[serde(rename = "Config")]
    config: ContainerConfig,
    #[serde(rename = "State")]
    state: ContainerState,
    #[serde(rename = "RestartCount", default)]
    restart_count: u32,
}

#[derive(Debug, Deserialize)]
struct ContainerConfig {
    #[serde(rename = "Image")]
    image: String,
    #[serde(rename = "Labels", default)]
    labels: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct ContainerState {
    #[serde(rename = "Running")]
    running: bool,
    #[serde(rename = "Restarting", default)]
    restarting: bool,
    #[serde(rename = "ExitCode", default)]
    exit_code: i64,
}

/// The last reason UA logged for refusing to start, if any.
fn startup_refusal_reason(logs: &str) -> Option<String> {
    logs.lines().rev().find_map(|line| {
        STARTUP_REFUSAL_MARKERS.iter().find_map(|marker| {
            let (_, reason) = line.split_once(marker)?;
            let reason = reason.trim();
            if reason.is_empty() {
                return None;
            }
            let mut characters = reason.chars();
            let first = characters.next()?.to_uppercase().collect::<String>();
            let mut reason = first + characters.as_str();
            if reason.chars().count() > MAX_STARTUP_REASON_CHARS {
                reason = reason.chars().take(MAX_STARTUP_REASON_CHARS).collect();
                reason.push('…');
            } else if !reason.ends_with(['.', '!', '?']) {
                // The status continues with another sentence after the reason.
                reason.push('.');
            }
            Some(reason)
        })
    })
}

enum HealthState {
    Onboarding,
    Ready,
    Unavailable(String),
}

fn probe_health(runtime_lock: &RuntimeLock) -> HealthState {
    let address = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        runtime_lock.container_ports.web,
    );
    let mut stream = match TcpStream::connect_timeout(&address, HEALTH_TIMEOUT) {
        Ok(stream) => stream,
        Err(error) => {
            return HealthState::Unavailable(format!(
                "The container is running; the Web UI is still starting ({error})."
            ));
        }
    };
    let _ = stream.set_read_timeout(Some(HEALTH_TIMEOUT));
    let _ = stream.set_write_timeout(Some(HEALTH_TIMEOUT));
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
        runtime_lock.health_path, runtime_lock.container_ports.web
    );
    if let Err(error) = stream.write_all(request.as_bytes()) {
        return HealthState::Unavailable(format!(
            "The container is running; its health endpoint is not ready ({error})."
        ));
    }

    let mut response = Vec::new();
    if let Err(error) = stream.take(MAX_HEALTH_BYTES).read_to_end(&mut response) {
        return HealthState::Unavailable(format!(
            "The container is running; its health response is incomplete ({error})."
        ));
    }
    let response = String::from_utf8_lossy(&response);
    let Some((headers, body)) = response.split_once("\r\n\r\n") else {
        return HealthState::Unavailable(
            "The container is running; its health response was malformed.".to_owned(),
        );
    };
    if !headers
        .lines()
        .next()
        .is_some_and(|line| line.contains(" 200 "))
    {
        return HealthState::Unavailable(
            "The container is running; its health endpoint is not ready.".to_owned(),
        );
    }
    let payload: serde_json::Value = match serde_json::from_str(body) {
        Ok(payload) => payload,
        Err(_) => {
            return HealthState::Unavailable(
                "The container is running; its health response was not valid JSON.".to_owned(),
            );
        }
    };
    if payload.get("status").and_then(serde_json::Value::as_str) != Some("ok") {
        return HealthState::Unavailable(
            "The container is running; its health check is not healthy.".to_owned(),
        );
    }
    if payload.get("service").and_then(serde_json::Value::as_str) == Some("onboard-web") {
        HealthState::Onboarding
    } else {
        HealthState::Ready
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn docker_probe_separates_missing_cli_from_stopped_daemon() {
        // CLI missing: the probe must say Docker is not installed and offer an
        // install link without claiming the daemon is merely stopped.
        let temp = TempDir::new().unwrap();
        let runner = FakeRunner::with_outputs(vec![Err(RuntimeError::DockerCommandUnavailable(
            "program not found".to_owned(),
        ))]);
        let manager = RuntimeManager::new(
            runner,
            runtime_lock(),
            temp.path().join("data"),
            RuntimeConfiguration::default(),
        )
        .unwrap();
        let missing = manager.docker_prerequisite().unwrap();
        assert!(!missing.available);
        assert!(!missing.installed);
        assert!(missing.detail.contains("was not found"));
        assert!(missing.install_url.starts_with("https://"));
        assert!(missing.setup_guide_url.starts_with("https://"));
        assert!(manager.runner.calls().iter().all(|args| args[0] == "info"));

        // Daemon stopped: the CLI answered, so Docker is installed but not
        // running. Status still reports the prerequisite failure.
        let temp = TempDir::new().unwrap();
        let runner = FakeRunner::with_outputs(vec![
            Ok(CommandOutput {
                code: 1,
                stdout: String::new(),
                stderr: "Cannot connect to the Docker daemon".to_owned(),
            }),
            Ok(CommandOutput {
                code: 1,
                stdout: String::new(),
                stderr: "Cannot connect to the Docker daemon".to_owned(),
            }),
        ]);
        let manager = RuntimeManager::new(
            runner,
            runtime_lock(),
            temp.path().join("data"),
            RuntimeConfiguration::default(),
        )
        .unwrap();
        let stopped = manager.docker_prerequisite().unwrap();
        assert!(!stopped.available);
        assert!(stopped.installed);
        assert!(stopped.detail.contains("not reachable"));
        let status = manager.status().unwrap();
        assert_eq!(status.phase, RuntimePhase::DockerUnavailable);
        assert!(status.detail.contains("not reachable"));
        // A probe never pulls or creates anything.
        assert!(manager.runner.calls().iter().all(|args| args[0] == "info"));
    }

    #[test]
    fn docker_probe_reports_available_without_mutation() {
        let temp = TempDir::new().unwrap();
        let runner = FakeRunner::with_outputs(vec![Ok(CommandOutput::success("27.0.0"))]);
        let manager = RuntimeManager::new(
            runner,
            runtime_lock(),
            temp.path().join("data"),
            RuntimeConfiguration::default(),
        )
        .unwrap();
        let prerequisite = manager.docker_prerequisite().unwrap();
        assert!(prerequisite.available);
        assert!(prerequisite.installed);
        assert_eq!(
            manager.runner.calls(),
            vec![vec![
                "info".to_owned(),
                "--format".to_owned(),
                "{{.ServerVersion}}".to_owned(),
            ]]
        );
    }

    #[test]
    fn downloading_a_reviewed_image_never_touches_the_running_container() {
        let temp = TempDir::new().unwrap();
        let current = runtime_lock();
        let mut candidate = current.clone();
        candidate.ua_version = "0.5.0".to_owned();
        candidate.image = format!("ghcr.io/example/ua@sha256:{}", "e".repeat(64));
        candidate.upgrade_sources.push(crate::RuntimeIdentity {
            host_contract_version: current.host_contract_version,
            ua_version: current.ua_version.clone(),
            image: current.image.clone(),
        });
        let runner = FakeRunner::with_outputs(vec![
            Ok(CommandOutput::success("27.0.0")),
            Ok(missing_output()),
            Ok(CommandOutput::success("pulled")),
        ]);
        let manager = RuntimeManager::new(
            runner,
            current,
            temp.path().join("data"),
            RuntimeConfiguration::default(),
        )
        .unwrap();

        manager.download_runtime_image(&candidate).unwrap();

        let calls = manager.runner.calls();
        assert_eq!(
            calls[0],
            ["info", "--format", "{{.ServerVersion}}"],
            "download must confirm Docker first"
        );
        assert_eq!(calls[1][0], "image", "download must probe the local store");
        assert_eq!(calls[2], ["pull", candidate.image.as_str()]);
        assert!(
            calls
                .iter()
                .all(|args| !matches!(args[0].as_str(), "run" | "stop" | "rename" | "rm")),
            "downloading must not create, stop, replace, or remove a container"
        );
    }

    #[test]
    fn downloaded_image_is_reused_without_pulling_again() {
        let temp = TempDir::new().unwrap();
        let current = runtime_lock();
        let mut candidate = current.clone();
        candidate.ua_version = "0.5.0".to_owned();
        candidate.image = format!("ghcr.io/example/ua@sha256:{}", "e".repeat(64));
        candidate.upgrade_sources.push(crate::RuntimeIdentity {
            host_contract_version: current.host_contract_version,
            ua_version: current.ua_version.clone(),
            image: current.image.clone(),
        });
        let runner = FakeRunner::with_outputs(vec![
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success("sha256:present")),
        ]);
        let manager = RuntimeManager::new(
            runner,
            current,
            temp.path().join("data"),
            RuntimeConfiguration::default(),
        )
        .unwrap();

        manager.download_runtime_image(&candidate).unwrap();

        assert!(manager.runner.calls().iter().all(|args| args[0] != "pull"));
    }

    #[test]
    fn downloading_rejects_a_candidate_that_changes_the_host_contract() {
        let temp = TempDir::new().unwrap();
        let current = runtime_lock();
        let mut candidate = current.clone();
        candidate.ua_version = "0.5.0".to_owned();
        candidate.image = format!("ghcr.io/example/ua@sha256:{}", "e".repeat(64));
        candidate.web_ui_path = "/different".to_owned();
        let runner = FakeRunner::default();
        let manager = RuntimeManager::new(
            runner,
            current,
            temp.path().join("data"),
            RuntimeConfiguration::default(),
        )
        .unwrap();

        let error = manager.download_runtime_image(&candidate).unwrap_err();

        assert!(
            matches!(error, RuntimeError::RuntimeMismatch(_)),
            "unexpected error: {error}"
        );
        assert!(manager.runner.calls().is_empty());
    }

    #[test]
    fn denied_image_pull_points_to_sign_in_without_creating_container() {
        let temp = tempfile::TempDir::new().unwrap();
        let runner = FakeRunner::with_outputs(vec![
            Ok(CommandOutput::success("27")),
            Ok(missing_output()),
            Ok(CommandOutput {
                code: 1,
                stdout: String::new(),
                stderr: "unauthorized: unauthorized".into(),
            }),
        ]);
        let manager = RuntimeManager::new(
            runner,
            runtime_lock(),
            temp.path().join("data"),
            RuntimeConfiguration::default(),
        )
        .unwrap();
        let error = manager.start().unwrap_err().to_string();
        assert!(error.contains("Private image access"), "{error}");
        // The status window may show this text, so it must not carry Docker's output.
        assert!(crate::guidance::is_guidance(&error), "{error}");
        assert!(!error.contains("unauthorized"), "{error}");
    }

    #[test]
    fn missing_credential_helper_stops_before_pulling_and_explains_the_fix() {
        let temp = tempfile::TempDir::new().unwrap();
        let mut runner =
            FakeRunner::with_outputs(vec![Ok(CommandOutput::success("27")), Ok(missing_output())]);
        runner.missing_helper = Some(crate::guidance::guidance(
            "Install docker-credential-desktop, then retry.",
        ));
        let manager = RuntimeManager::new(
            runner,
            runtime_lock(),
            temp.path().join("data"),
            RuntimeConfiguration::default(),
        )
        .unwrap();

        let error = manager.start().unwrap_err().to_string();

        assert_eq!(
            crate::guidance::plain(&error),
            "Install docker-credential-desktop, then retry."
        );
        assert!(crate::guidance::is_guidance(&error));
        assert!(manager.runner.calls().iter().all(|args| args[0] != "pull"));
    }

    #[test]
    fn image_registry_matches_docker_credential_helper_keys() {
        assert_eq!(
            image_registry("ghcr.io/enzimo/universal-agents@sha256:abc"),
            "ghcr.io"
        );
        assert_eq!(image_registry("localhost:5000/ua:1"), "localhost:5000");
        assert_eq!(
            image_registry("library/python:3"),
            "https://index.docker.io/v1/"
        );
        assert_eq!(image_registry("python:3"), "https://index.docker.io/v1/");
    }

    #[test]
    fn busy_port_is_shown_as_guidance() {
        let error = RuntimeError::PortInUse { port: 18090 }.to_string();
        assert!(crate::guidance::is_guidance(&error));
        assert!(error.contains("Port 18090"), "{error}");
    }

    #[test]
    fn import_while_stopped_waits_without_running_helper() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("auth.json");
        std::fs::write(&source, "{}").unwrap();
        let runner = FakeRunner::with_outputs(vec![
            Ok(CommandOutput::success("27")),
            Ok(CommandOutput::success(inspection_json(true, false))),
        ]);
        let manager = RuntimeManager::new(
            runner,
            runtime_lock(),
            temp.path().join("data"),
            RuntimeConfiguration::default(),
        )
        .unwrap();
        assert_eq!(
            manager
                .import_codex_auth(&source.canonicalize().unwrap())
                .unwrap(),
            CodexImportOutcome::PendingRuntime
        );
    }
    #[test]
    fn import_before_install_waits_without_running_helper() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("auth.json");
        std::fs::write(&source, "{}").unwrap();
        let runner =
            FakeRunner::with_outputs(vec![Ok(CommandOutput::success("27")), Ok(missing_output())]);
        let manager = RuntimeManager::new(
            runner,
            runtime_lock(),
            temp.path().join("data"),
            RuntimeConfiguration::default(),
        )
        .unwrap();
        assert_eq!(
            manager
                .import_codex_auth(&source.canonicalize().unwrap())
                .unwrap(),
            CodexImportOutcome::PendingRuntime
        );
    }
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU16, Ordering};

    use tempfile::TempDir;

    use super::*;
    use crate::{ContainerPorts, RuntimeIdentity};

    const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OLD_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const PREVIOUS_DIGEST: &str =
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    #[derive(Default)]
    struct FakeRunner {
        outputs: Mutex<VecDeque<Result<CommandOutput, RuntimeError>>>,
        calls: Mutex<Vec<Vec<String>>>,
        missing_helper: Option<String>,
    }

    impl FakeRunner {
        fn with_outputs(outputs: Vec<Result<CommandOutput, RuntimeError>>) -> Self {
            Self {
                outputs: Mutex::new(outputs.into()),
                calls: Mutex::new(Vec::new()),
                missing_helper: None,
            }
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().expect("calls lock").clone()
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, args: &[String]) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().expect("calls lock").push(args.to_vec());
            self.outputs
                .lock()
                .expect("outputs lock")
                .pop_front()
                .expect("fake output for every call")
        }

        fn missing_credential_helper(&self, _registry: &str) -> Option<String> {
            self.missing_helper.clone()
        }
    }

    fn runtime_lock() -> RuntimeLock {
        let web_port = available_port();
        let mut broker_port = available_port();
        while broker_port == web_port {
            broker_port = available_port();
        }
        RuntimeLock {
            host_contract_version: 3,
            ua_version: "0.4.2".to_owned(),
            image: format!("ghcr.io/example/ua@sha256:{DIGEST}"),
            upgrade_sources: vec![
                RuntimeIdentity {
                    host_contract_version: 3,
                    ua_version: "0.4.1".to_owned(),
                    image: format!("ghcr.io/example/ua@sha256:{PREVIOUS_DIGEST}"),
                },
                RuntimeIdentity {
                    host_contract_version: 2,
                    ua_version: "0.3.0".to_owned(),
                    image: format!("ghcr.io/example/ua@sha256:{OLD_DIGEST}"),
                },
                RuntimeIdentity {
                    host_contract_version: 3,
                    ua_version: "0.3.0".to_owned(),
                    image: format!("ghcr.io/example/ua@sha256:{OLD_DIGEST}"),
                },
            ],
            command: vec!["run".to_owned()],
            data_mount: "/data".to_owned(),
            workspace_mount: "/data/workspaces/team/team-shared".to_owned(),
            codex_auth_mount: "/codex-home/auth.json".to_owned(),
            codex_home: "/codex-home".to_owned(),
            codex_auth_store: "/data/auth/openai_codex_auth.json".to_owned(),
            instance_config: "/data/instance_config.yaml".to_owned(),
            codex_import_command: RuntimeLock::bundled().unwrap().codex_import_command,
            container_name: "ua-desktop".to_owned(),
            container_ports: ContainerPorts {
                web: web_port,
                broker: broker_port,
            },
            web_ui_path: "/webchat".to_owned(),
            onboarding_ui_path: "/".to_owned(),
            docs_path: "/docs/".to_owned(),
            health_path: "/health".to_owned(),
        }
    }

    /// Pick a loopback port that no other test in this process may reuse.
    ///
    /// Binding port 0 only reports a port that happened to be free at that
    /// instant; dropping the listener frees it again immediately, so two
    /// parallel tests can be handed the same number. The loser then fails its
    /// own `check_port_available` bind, which shifts its fake command outputs
    /// and produces confusing assertion failures elsewhere.
    ///
    /// Candidates come from a process-unique counter so two tests can never
    /// agree on a number, and each candidate is verified to be bindable before
    /// it is returned.
    fn available_port() -> u16 {
        static NEXT_CANDIDATE: AtomicU16 = AtomicU16::new(20_000);
        loop {
            let candidate = NEXT_CANDIDATE.fetch_add(1, Ordering::Relaxed);
            if candidate >= 60_000 {
                panic!("test port allocator exhausted its candidate range");
            }
            if TcpListener::bind((Ipv4Addr::LOCALHOST, candidate)).is_ok() {
                return candidate;
            }
        }
    }

    fn missing_output() -> CommandOutput {
        CommandOutput {
            code: 1,
            stdout: String::new(),
            stderr: "Error: No such object: ua-desktop".to_owned(),
        }
    }

    #[test]
    fn docker_pull_progress_aggregates_layer_byte_counts() {
        let updates = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = updates.clone();
        crate::progress::with_reporter(
            move |update| captured.lock().unwrap().push(update),
            || {
                let mut progress = PullProgress::default();
                observe_pull_line(b"a1b2: Downloading  1.5MB/3MB", &mut progress);
                observe_pull_line(b"c3d4: Downloading  500kB/2MB", &mut progress);
                observe_pull_line(b"a1b2: Pull complete", &mut progress);
            },
        );
        let updates = updates.lock().unwrap();
        let final_update = updates.last().expect("a byte progress update");
        assert_eq!(final_update.current_bytes, Some(3_500_000));
        assert_eq!(final_update.total_bytes, Some(5_000_000));
        assert_eq!(
            final_update.message,
            "Downloading runtime image: 3.5 MB of 5.0 MB"
        );
    }

    #[test]
    fn docker_pull_progress_ignores_unstructured_output() {
        let updates = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = updates.clone();
        crate::progress::with_reporter(
            move |update| captured.lock().unwrap().push(update),
            || {
                let mut progress = PullProgress::default();
                observe_pull_line(b"Digest: sha256:not-a-progress-line", &mut progress);
            },
        );
        assert!(updates.lock().unwrap().is_empty());
    }

    fn inspection_json(managed: bool, running: bool) -> String {
        inspection_json_for(managed, running, 3, &RuntimeConfiguration::default())
    }

    fn inspection_json_for(
        managed: bool,
        running: bool,
        contract: u32,
        configuration: &RuntimeConfiguration,
    ) -> String {
        let managed_label = if managed {
            format!("\"{MANAGED_LABEL}\":\"true\",")
        } else {
            String::new()
        };
        let configuration = configuration.fingerprint();
        format!(
            r#"[{{"Config":{{"Image":"ghcr.io/example/ua@sha256:{DIGEST}","Labels":{{{managed_label}"{HOST_CONTRACT_LABEL}":"{contract}","{RUNTIME_VERSION_LABEL}":"0.4.2","{CONFIGURATION_LABEL}":"{configuration}"}}}},"State":{{"Running":{running},"Status":"running","ExitCode":0}}}}]"#
        )
    }

    fn previous_inspection_json(running: bool, configuration: &RuntimeConfiguration) -> String {
        let configuration = configuration.fingerprint();
        format!(
            r#"[{{"Config":{{"Image":"ghcr.io/example/ua@sha256:{PREVIOUS_DIGEST}","Labels":{{"{MANAGED_LABEL}":"true","{HOST_CONTRACT_LABEL}":"3","{RUNTIME_VERSION_LABEL}":"0.4.1","{CONFIGURATION_LABEL}":"{configuration}"}}}},"State":{{"Running":{running},"Status":"running","ExitCode":0}}}}]"#
        )
    }

    fn upgrade_inspection_json(
        running: bool,
        contract: u32,
        configuration: &RuntimeConfiguration,
    ) -> String {
        let configuration = configuration.fingerprint();
        format!(
            r#"[{{"Config":{{"Image":"ghcr.io/example/ua@sha256:{OLD_DIGEST}","Labels":{{"{MANAGED_LABEL}":"true","{HOST_CONTRACT_LABEL}":"{contract}","{RUNTIME_VERSION_LABEL}":"0.3.0","{CONFIGURATION_LABEL}":"{configuration}"}}}},"State":{{"Running":{running},"Status":"running","ExitCode":0}}}}]"#
        )
    }

    #[test]
    fn new_runtime_pulls_exact_digest_and_publishes_loopback_only() {
        let temp_dir = TempDir::new().expect("temp directory");
        let runtime_lock = runtime_lock();
        let runner = FakeRunner::with_outputs(vec![
            Ok(CommandOutput::success("27.0.0")),
            Ok(missing_output()),
            Ok(CommandOutput::success("pulled")),
            Ok(CommandOutput::success("container-id")),
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(inspection_json(true, true))),
        ]);
        let manager = RuntimeManager::new(
            runner,
            runtime_lock.clone(),
            temp_dir.path().to_owned(),
            RuntimeConfiguration::default(),
        )
        .expect("runtime manager");

        let messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = messages.clone();
        let status = crate::progress::with_reporter(
            move |message| captured.lock().unwrap().push(message),
            || manager.start().expect("start runtime"),
        );
        let messages = messages.lock().unwrap();
        assert!(messages[0].message.starts_with("Fetching runtime image"));
        assert!(messages[1].message.starts_with("Image ready. Starting"));

        assert_eq!(status.phase, RuntimePhase::Starting);
        let calls = manager.runner.calls();
        let pull = calls
            .iter()
            .find(|args| args.first().map(String::as_str) == Some("pull"))
            .expect("pull command");
        assert_eq!(pull, &["pull".to_owned(), runtime_lock.image.clone()]);
        let run = calls
            .iter()
            .find(|args| args.first().map(String::as_str) == Some("run"))
            .expect("run command");
        assert!(
            run.contains(&format!(
                "127.0.0.1:{}:18090",
                runtime_lock.container_ports.web
            )),
            "web port must bind to loopback"
        );
        assert!(
            run.contains(&format!(
                "127.0.0.1:{}:18600",
                runtime_lock.container_ports.broker
            )),
            "broker port must bind to loopback"
        );
        assert!(run.windows(2).any(|pair| pair
            == [
                "--env",
                &format!(
                    "OPENAI_CODEX_AUTH_STORE_PATH={}",
                    runtime_lock.codex_auth_store
                )
            ]));
        assert!(run.contains(&"TEAM_REFRESH_EXTERNAL_SKILLS=false".to_owned()));
        assert!(run.contains(&format!("{MANAGED_LABEL}=true")));
        assert!(!run.iter().any(|argument| argument == "latest"));
    }

    #[test]
    fn launch_url_tracks_onboarding_health_without_requiring_docker() {
        use std::io::{Read, Write};
        for (service, expected_path) in [("onboard-web", "/"), ("internal-website", "/webchat")] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let mut lock = runtime_lock();
            lock.container_ports.web = listener.local_addr().unwrap().port();
            let expected = format!(
                "http://127.0.0.1:{}{expected_path}",
                lock.container_ports.web
            );
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 1024];
                let size = stream.read(&mut request).unwrap();
                assert!(request[..size].starts_with(b"GET /health "));
                let body = format!(r#"{{"status":"ok","service":"{service}"}}"#);
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            });
            let temp = TempDir::new().unwrap();
            let manager = RuntimeManager::new(
                FakeRunner::with_outputs(vec![]),
                lock,
                temp.path().to_owned(),
                RuntimeConfiguration::default(),
            )
            .unwrap();
            assert_eq!(manager.web_url(), expected);
            assert!(manager.runner.calls().is_empty());
            server.join().unwrap();
        }
    }

    /// Docker's view of a container inside its restart policy's back-off.
    fn restarting_inspection_json(exit_code: i64, restart_count: u32) -> String {
        inspection_json(true, true).replace(
            r#""Status":"running","ExitCode":0}"#,
            &format!(
                r#""Restarting":true,"Status":"restarting","ExitCode":{exit_code}}},"RestartCount":{restart_count}"#
            ),
        )
    }

    #[test]
    fn a_leftover_container_from_another_setup_explains_how_to_start_fresh() {
        // The app-data folder was moved aside, so the new selection does not
        // know the UA 0.3.9 container still in Docker.
        let leftover = inspection_json(true, false)
            .replace("\"0.4.2\"", "\"0.3.9\"")
            .replace(DIGEST, &"f".repeat(64));
        let (_temp, manager) = manager_with(vec![
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(leftover.clone())),
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(leftover.clone())),
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(leftover)),
        ]);

        let status = manager.status().unwrap();
        assert_eq!(status.phase, RuntimePhase::Error);
        assert!(
            status.detail.contains("another setup (UA 0.3.9)"),
            "{}",
            status.detail
        );
        assert!(
            status.detail.contains("set up for UA 0.4.2"),
            "{}",
            status.detail
        );
        assert!(
            status.detail.contains("docker rm -f ua-desktop"),
            "{}",
            status.detail
        );
        assert!(
            !crate::guidance::is_guidance(&status.detail),
            "shown as plain text"
        );

        // A poll every few seconds logs the same problem only once.
        let logged = manager.logged_status_error.lock().unwrap().clone();
        manager.status().unwrap();
        assert_eq!(*manager.logged_status_error.lock().unwrap(), logged);

        // Starting refuses with the same guidance and never touches the container.
        let start_error = manager.start();
        assert!(start_error.is_err());
        assert!(
            manager
                .runner
                .calls()
                .iter()
                .all(|args| args[0] != "rm" && args[0] != "stop")
        );
    }

    fn manager_with(
        outputs: Vec<Result<CommandOutput, RuntimeError>>,
    ) -> (TempDir, RuntimeManager<FakeRunner>) {
        let temp_dir = TempDir::new().expect("temp directory");
        let manager = RuntimeManager::new(
            FakeRunner::with_outputs(outputs),
            runtime_lock(),
            temp_dir.path().to_owned(),
            RuntimeConfiguration::default(),
        )
        .expect("runtime manager");
        (temp_dir, manager)
    }

    #[test]
    fn refused_start_reports_ua_reason_and_keeps_the_container() {
        let logs = "2026-10-11 10:00:00,001 INFO launcher: Using instance config\n\
            2026-10-11 10:00:00,002 ERROR __main__: Headless launch refused. This instance still has files in the old single-workspace layout, so it will not start. Reason code: legacy_instance_layout_present.\n\
            # Stop every team runtime first. Then run:\n";
        let (_temp, manager) = manager_with(vec![
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(restarting_inspection_json(2, 4))),
            Ok(CommandOutput {
                code: 0,
                stdout: String::new(),
                stderr: logs.to_owned(),
            }),
        ]);

        let status = manager.status().expect("status");

        assert_eq!(status.phase, RuntimePhase::StartupFailed);
        assert!(status.detail.contains("exit code 2"), "{}", status.detail);
        assert!(
            status.detail.contains("restarted it 4 times"),
            "{}",
            status.detail
        );
        assert!(
            status
                .detail
                .contains("Universal Agents reported: This instance still has files in the old single-workspace layout"),
            "{}",
            status.detail
        );
        let calls = manager.runner.calls();
        assert_eq!(calls[2], ["logs", "--tail", "200", "ua-desktop"]);
        assert!(
            !calls
                .iter()
                .any(|args| ["stop", "rm", "run", "restart"].contains(&args[0].as_str())),
            "reporting a failure must never change the container"
        );
    }

    #[test]
    fn refused_start_without_readable_logs_still_needs_attention() {
        let (_temp, manager) = manager_with(vec![
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(restarting_inspection_json(1, 1))),
            Ok(CommandOutput {
                code: 1,
                stdout: String::new(),
                stderr: "error from daemon".to_owned(),
            }),
        ]);

        let status = manager.status().expect("status");

        assert_eq!(status.phase, RuntimePhase::StartupFailed);
        assert!(
            status.detail.contains("restarted it once"),
            "{}",
            status.detail
        );
        assert!(
            status.detail.contains("download diagnostics"),
            "{}",
            status.detail
        );
    }

    #[test]
    fn restart_after_successful_setup_is_not_a_failure() {
        // UA exits with code 0 after first-time setup so Docker starts the
        // configured team. That restart must read as starting, not failed.
        let (_temp, manager) = manager_with(vec![
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(restarting_inspection_json(0, 1))),
        ]);

        let status = manager.status().expect("status");

        assert_eq!(status.phase, RuntimePhase::Starting);
        assert_eq!(
            manager.runner.calls().len(),
            2,
            "no log read for a clean restart"
        );
    }

    #[test]
    fn startup_refusal_reason_uses_the_latest_ua_explanation() {
        let logs = "x ERROR a: Bundle launch refused. First reason.\n\
            y ERROR b: Headless launch aborted because external skills could not be refreshed: network down\n\
            z INFO c: unrelated line";
        assert_eq!(
            startup_refusal_reason(logs).as_deref(),
            Some("External skills could not be refreshed: network down.")
        );
        assert_eq!(startup_refusal_reason("no refusal here"), None);
        assert_eq!(
            startup_refusal_reason("x ERROR a: Headless launch refused.    "),
            None
        );
        let long = format!("x ERROR a: Headless launch refused. {}", "é".repeat(700));
        let reason = startup_refusal_reason(&long).expect("long reason");
        assert_eq!(reason.chars().count(), MAX_STARTUP_REASON_CHARS + 1);
        assert!(reason.ends_with('…'));
    }

    #[test]
    fn default_workspace_reported_is_the_locked_team_folder() {
        let (temp, manager) = manager_with(vec![
            Ok(CommandOutput::success("27.0.0")),
            Ok(missing_output()),
        ]);

        let status = manager.status().expect("status");

        assert_eq!(
            Path::new(&status.workspace_dir),
            temp.path()
                .join("workspaces")
                .join("team")
                .join("team-shared")
        );
        assert!(!status.workspace_is_external);
    }

    #[test]
    fn refuses_to_start_unmanaged_name_collision() {
        let temp_dir = TempDir::new().expect("temp directory");
        let runner = FakeRunner::with_outputs(vec![
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(inspection_json(false, true))),
        ]);
        let manager = RuntimeManager::new(
            runner,
            runtime_lock(),
            temp_dir.path().to_owned(),
            RuntimeConfiguration::default(),
        )
        .expect("runtime manager");

        let error = manager.start().expect_err("unmanaged container must fail");

        assert!(matches!(error, RuntimeError::UnmanagedContainer { .. }));
        assert_eq!(manager.runner.calls().len(), 2);
    }

    #[test]
    fn stop_never_removes_container_or_data() {
        let temp_dir = TempDir::new().expect("temp directory");
        let runner = FakeRunner::with_outputs(vec![
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(inspection_json(true, true))),
            Ok(CommandOutput::success("ua-desktop")),
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(inspection_json(true, false))),
        ]);
        let manager = RuntimeManager::new(
            runner,
            runtime_lock(),
            temp_dir.path().to_owned(),
            RuntimeConfiguration::default(),
        )
        .expect("runtime manager");

        let status = manager.stop().expect("stop runtime");

        assert_eq!(status.phase, RuntimePhase::Stopped);
        assert!(
            manager
                .runner
                .calls()
                .iter()
                .all(|args| !args.iter().any(|argument| argument == "rm"))
        );
        assert!(temp_dir.path().exists());
    }

    #[test]
    fn selected_workspace_mount_excludes_the_codex_source() {
        let temp_dir = TempDir::new().expect("temp directory");
        let workspace = temp_dir.path().join("project");
        let auth_dir = temp_dir.path().join("codex");
        let auth_file = auth_dir.join("auth.json");
        fs::create_dir_all(&workspace).expect("workspace");
        fs::create_dir_all(&auth_dir).expect("Codex directory");
        fs::write(
            &auth_file,
            r#"{"tokens":"never pass this to Docker arguments"}"#,
        )
        .expect("auth file");
        let workspace = workspace.canonicalize().expect("canonical workspace");
        let configuration = RuntimeConfiguration {
            external_workspace: Some(workspace.clone()),
            ..RuntimeConfiguration::default()
        };
        let runner = FakeRunner::with_outputs(vec![
            Ok(CommandOutput::success("27.0.0")),
            Ok(missing_output()),
            Ok(CommandOutput::success("pulled")),
            Ok(CommandOutput::success("container-id")),
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(inspection_json_for(
                true,
                true,
                3,
                &configuration,
            ))),
        ]);
        let manager = RuntimeManager::new(
            runner,
            runtime_lock(),
            temp_dir.path().join("app-data"),
            configuration,
        )
        .expect("runtime manager");

        manager.start().expect("start runtime");

        let calls = manager.runner.calls();
        let run = calls
            .iter()
            .find(|args| args.first().map(String::as_str) == Some("run"))
            .expect("run command");
        assert!(run.contains(&format!(
            "{}:/data/workspaces/team/team-shared",
            crate::platform::docker_source(&workspace).unwrap()
        )));
        assert!(!run.iter().any(|argument| {
            argument.contains(
                auth_file
                    .to_str()
                    .expect("temporary auth path must be UTF-8"),
            )
        }));
        assert!(!run.iter().any(|argument| argument.contains("/codex-home")));
        assert!(
            !run.iter()
                .any(|argument| argument.contains("never pass this")),
            "credential contents must never enter Docker arguments"
        );
        let label_values: Vec<&String> = run
            .windows(2)
            .filter(|pair| pair[0] == "--label")
            .map(|pair| &pair[1])
            .collect();
        assert!(label_values.iter().all(|value| {
            !value.contains(
                workspace
                    .to_str()
                    .expect("temporary workspace path must be UTF-8"),
            )
        }));
        assert!(label_values.iter().all(|value| {
            !value.contains(
                auth_file
                    .to_str()
                    .expect("temporary auth path must be UTF-8"),
            )
        }));
    }

    #[test]
    fn codex_import_before_onboarding_uses_locked_ua_import_without_config_probe() {
        let temp_dir = TempDir::new().expect("temp directory");
        let auth_file = temp_dir.path().join("auth.json");
        let data_dir = temp_dir.path().join("app-data");
        fs::write(&auth_file, r#"{"access_token":"not-a-command-argument"}"#).expect("auth file");
        let auth_file = auth_file.canonicalize().expect("canonical auth file");
        let configuration = RuntimeConfiguration::default();
        let runner = FakeRunner::with_outputs(vec![
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(inspection_json_for(
                true,
                true,
                3,
                &configuration,
            ))),
            Ok(CommandOutput::success("credentials imported")),
        ]);
        let manager = RuntimeManager::new(runner, runtime_lock(), data_dir.clone(), configuration)
            .expect("runtime manager");

        let outcome = manager
            .import_codex_auth(&auth_file)
            .expect("import credentials");

        assert_eq!(outcome, CodexImportOutcome::Imported);
        let calls = manager.runner.calls();
        assert_eq!(
            calls.len(),
            3,
            "import must not probe instance configuration"
        );
        assert_eq!(
            calls[2],
            [
                "run",
                "--rm",
                "--pull",
                "never",
                "--network",
                "none",
                "--volume",
                &format!(
                    "{}:/data",
                    crate::platform::docker_source(&data_dir).unwrap()
                ),
                "--volume",
                &format!(
                    "{}:/codex-home/auth.json:ro",
                    crate::platform::docker_source(&auth_file).unwrap()
                ),
                "--entrypoint",
                "python",
                &format!("ghcr.io/example/ua@sha256:{DIGEST}"),
                "-c",
                &manager.runtime_lock.codex_import_command[2],
                "/codex-home",
                "/data/auth/openai_codex_auth.json",
            ]
        );
        assert!(
            calls
                .iter()
                .flatten()
                .all(|argument| !argument.contains("not-a-command-argument"))
        );
    }

    #[test]
    fn codex_import_rejects_foreign_containers_and_propagates_helper_failure() {
        for managed in [false, true] {
            let temp_dir = TempDir::new().unwrap();
            let auth_file = temp_dir.path().join("auth.json");
            fs::write(&auth_file, "{}").unwrap();
            let configuration = RuntimeConfiguration::default();
            let mut outputs = vec![
                Ok(CommandOutput::success("27.0.0")),
                Ok(CommandOutput::success(inspection_json_for(
                    managed,
                    true,
                    3,
                    &configuration,
                ))),
            ];
            if managed {
                outputs.push(Ok(CommandOutput {
                    code: 1,
                    stdout: String::new(),
                    stderr: "Choose a current file-backed Codex auth.json and retry.".to_owned(),
                }));
            }
            let manager = RuntimeManager::new(
                FakeRunner::with_outputs(outputs),
                runtime_lock(),
                temp_dir.path().join("app-data"),
                configuration,
            )
            .unwrap();
            let error = manager
                .import_codex_auth(&auth_file.canonicalize().unwrap())
                .unwrap_err();
            if managed {
                assert!(matches!(error, RuntimeError::DockerCommandFailed { .. }));
                assert_eq!(manager.runner.calls().len(), 3);
            } else {
                assert!(matches!(error, RuntimeError::UnmanagedContainer { .. }));
                assert_eq!(manager.runner.calls().len(), 2);
            }
        }
    }

    #[test]
    fn contract_upgrade_replaces_only_the_desktop_container() {
        let temp_dir = TempDir::new().expect("temp directory");
        let current_configuration = RuntimeConfiguration::default();
        let runner = FakeRunner::with_outputs(vec![
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(upgrade_inspection_json(
                true,
                2,
                &current_configuration,
            ))),
            Ok(missing_output()),
            Ok(CommandOutput::success("pulled")),
            Ok(CommandOutput::success("stopped")),
            Ok(CommandOutput::success("renamed")),
            Ok(CommandOutput::success("replacement-id")),
            Ok(CommandOutput::success("removed backup")),
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(inspection_json(true, true))),
        ]);
        let manager = RuntimeManager::new(
            runner,
            runtime_lock(),
            temp_dir.path().to_owned(),
            current_configuration,
        )
        .expect("runtime manager");

        let status = manager.start().expect("upgrade host contract");

        assert_eq!(status.phase, RuntimePhase::Starting);
        let calls = manager.runner.calls();
        assert!(calls.contains(&vec![
            "rename".to_owned(),
            "ua-desktop".to_owned(),
            "ua-desktop-replacement-backup".to_owned(),
        ]));
        assert!(calls.contains(&vec![
            "rm".to_owned(),
            "ua-desktop-replacement-backup".to_owned(),
        ]));
        assert!(
            calls
                .iter()
                .all(|args| !args.iter().any(|value| value == "-v"))
        );
        let replacement = calls
            .iter()
            .find(|args| args.first().map(String::as_str) == Some("run"))
            .expect("replacement run command");
        assert!(replacement.contains(&format!(
            "{}:/data",
            crate::platform::docker_source(temp_dir.path()).unwrap()
        )));
        assert!(temp_dir.path().exists());
    }

    #[test]
    fn pre_onboarding_fix_container_is_refreshed_with_owned_store_environment() {
        let temp_dir = TempDir::new().expect("temp directory");
        let current_configuration = RuntimeConfiguration::default();
        let old_fingerprint = format!(
            "{:x}",
            Sha256::digest(b"universal-agents-desktop-configuration-v1\0")
        );
        let old_inspection = inspection_json(true, true)
            .replace(&current_configuration.fingerprint(), &old_fingerprint);
        let runner = FakeRunner::with_outputs(vec![
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(old_inspection)),
            Ok(missing_output()),
            Ok(CommandOutput::success("pulled")),
            Ok(CommandOutput::success("stopped")),
            Ok(CommandOutput::success("renamed")),
            Ok(CommandOutput::success("replacement-id")),
            Ok(CommandOutput::success("removed backup")),
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(inspection_json(true, true))),
        ]);
        let manager = RuntimeManager::new(
            runner,
            runtime_lock(),
            temp_dir.path().to_owned(),
            current_configuration,
        )
        .expect("runtime manager");

        let status = manager.start().expect("apply owned store environment");

        assert_eq!(status.phase, RuntimePhase::Starting);
        let calls = manager.runner.calls();
        assert!(calls.contains(&vec![
            "rename".to_owned(),
            "ua-desktop".to_owned(),
            "ua-desktop-replacement-backup".to_owned(),
        ]));
        assert!(calls.contains(&vec![
            "rm".to_owned(),
            "ua-desktop-replacement-backup".to_owned(),
        ]));
        assert!(
            calls
                .iter()
                .all(|args| !args.iter().any(|value| value == "-v"))
        );
        let replacement = calls
            .iter()
            .find(|args| args.first().map(String::as_str) == Some("run"))
            .expect("replacement run command");
        assert!(replacement.contains(&format!(
            "{}:/data",
            crate::platform::docker_source(temp_dir.path()).unwrap()
        )));
        assert!(replacement.contains(
            &"OPENAI_CODEX_AUTH_STORE_PATH=/data/auth/openai_codex_auth.json".to_owned()
        ));
        assert!(replacement.contains(&format!(
            "{CONFIGURATION_LABEL}={}",
            manager.configuration.fingerprint()
        )));
        assert!(temp_dir.path().exists());
    }

    #[test]
    fn reviewed_runtime_upgrade_is_replaceable_on_current_host_contract() {
        let temp_dir = TempDir::new().expect("temp directory");
        let configuration = RuntimeConfiguration::default();
        let runner = FakeRunner::with_outputs(vec![
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(upgrade_inspection_json(
                true,
                3,
                &configuration,
            ))),
        ]);
        let manager = RuntimeManager::new(
            runner,
            runtime_lock(),
            temp_dir.path().to_owned(),
            configuration,
        )
        .expect("runtime manager");

        let status = manager.status().expect("reviewed upgrade status");

        assert_eq!(status.phase, RuntimePhase::Starting);
        assert!(status.detail.contains("Universal Agents 0.3.0 to 0.4.2"));
        assert_eq!(manager.runner.calls().len(), 2);
    }

    #[test]
    fn approved_new_runtime_uses_new_digest_and_keeps_persistent_mounts() {
        let dir = TempDir::new().unwrap();
        let old = runtime_lock();
        let mut next = old.clone();
        next.ua_version = "0.5.0".into();
        next.image = format!("ghcr.io/example/ua@sha256:{}", "d".repeat(64));
        next.upgrade_sources.push(RuntimeIdentity {
            host_contract_version: old.host_contract_version,
            ua_version: old.ua_version,
            image: old.image,
        });
        crate::updates::validate_update(&runtime_lock_with_ports(&next), &next).unwrap();
        let new_inspection = inspection_json(true, true)
            .replace(DIGEST, &"d".repeat(64))
            .replace("0.4.2", "0.5.0");
        let runner = FakeRunner::with_outputs(vec![
            Ok(CommandOutput::success("27")),
            Ok(CommandOutput::success(inspection_json(true, true))),
            Ok(missing_output()),
            Ok(CommandOutput::success("pulled")),
            Ok(CommandOutput::success("stopped")),
            Ok(CommandOutput::success("renamed")),
            Ok(CommandOutput::success("created")),
            Ok(CommandOutput::success("removed backup")),
            Ok(CommandOutput::success("27")),
            Ok(CommandOutput::success(new_inspection)),
        ]);
        let manager = RuntimeManager::new(
            runner,
            next.clone(),
            dir.path().to_owned(),
            RuntimeConfiguration::default(),
        )
        .unwrap();
        let status = manager.start().unwrap();
        assert_eq!(status.ua_version, "0.5.0");
        let calls = manager.runner.calls();
        assert!(calls.contains(&vec!["pull".into(), next.image.clone()]));
        let run = calls.iter().find(|args| args[0] == "run").unwrap();
        assert!(run.contains(&next.image));
        assert!(run.contains(&format!(
            "{}:/data",
            crate::platform::docker_source(dir.path()).unwrap()
        )));
        assert!(!calls.iter().flatten().any(|arg| arg == "-v"));
    }

    fn runtime_lock_with_ports(next: &RuntimeLock) -> RuntimeLock {
        let mut old = runtime_lock();
        old.container_ports = next.container_ports.clone();
        old
    }

    #[test]
    fn previous_runtime_release_is_replaceable() {
        let temp_dir = TempDir::new().expect("temp directory");
        let configuration = RuntimeConfiguration::default();
        let runner = FakeRunner::with_outputs(vec![
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(previous_inspection_json(
                true,
                &configuration,
            ))),
        ]);
        let manager = RuntimeManager::new(
            runner,
            runtime_lock(),
            temp_dir.path().to_owned(),
            configuration,
        )
        .expect("runtime manager");

        let status = manager.status().expect("previous release status");

        assert_eq!(status.phase, RuntimePhase::Starting);
        assert!(status.detail.contains("Universal Agents 0.4.1 to 0.4.2"));
        assert_eq!(manager.runner.calls().len(), 2);
    }

    #[test]
    fn failed_replacement_restores_the_prior_running_container() {
        let temp_dir = TempDir::new().expect("temp directory");
        let current_configuration = RuntimeConfiguration::default();
        let runner = FakeRunner::with_outputs(vec![
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(upgrade_inspection_json(
                true,
                2,
                &current_configuration,
            ))),
            Ok(missing_output()),
            Ok(CommandOutput::success("pulled")),
            Ok(CommandOutput::success("stopped")),
            Ok(CommandOutput::success("renamed")),
            Ok(CommandOutput {
                code: 1,
                stdout: String::new(),
                stderr: "replacement rejected".to_owned(),
            }),
            Ok(missing_output()),
            Ok(CommandOutput::success("restored name")),
            Ok(CommandOutput::success("restarted")),
        ]);
        let manager = RuntimeManager::new(
            runner,
            runtime_lock(),
            temp_dir.path().to_owned(),
            current_configuration,
        )
        .expect("runtime manager");

        let error = manager.start().expect_err("replacement must fail");

        assert!(matches!(
            error,
            RuntimeError::RuntimeReplacementFailed { .. }
        ));
        assert!(error.to_string().contains("restored the prior"));
        let calls = manager.runner.calls();
        assert!(calls.contains(&vec![
            "rename".to_owned(),
            "ua-desktop-replacement-backup".to_owned(),
            "ua-desktop".to_owned(),
        ]));
        assert_eq!(
            calls.last().expect("rollback restart"),
            &["start".to_owned(), "ua-desktop".to_owned()]
        );
        assert!(
            calls
                .iter()
                .all(|args| !args.iter().any(|value| value == "rm"))
        );
        assert!(temp_dir.path().exists());
    }

    #[test]
    fn failed_backup_rename_restarts_the_prior_container() {
        let temp_dir = TempDir::new().expect("temp directory");
        let current_configuration = RuntimeConfiguration::default();
        let runner = FakeRunner::with_outputs(vec![
            Ok(CommandOutput::success("27.0.0")),
            Ok(CommandOutput::success(upgrade_inspection_json(
                true,
                2,
                &current_configuration,
            ))),
            Ok(missing_output()),
            Ok(CommandOutput::success("pulled")),
            Ok(CommandOutput::success("stopped")),
            Ok(CommandOutput {
                code: 1,
                stdout: String::new(),
                stderr: "rename rejected".to_owned(),
            }),
            Ok(CommandOutput::success("restarted")),
        ]);
        let manager = RuntimeManager::new(
            runner,
            runtime_lock(),
            temp_dir.path().to_owned(),
            current_configuration,
        )
        .expect("runtime manager");

        let error = manager.start().expect_err("replacement must fail");

        assert!(
            error
                .to_string()
                .contains("restarted the prior container after its backup rename failed")
        );
        assert_eq!(
            manager.runner.calls().last().expect("rollback restart"),
            &["start".to_owned(), "ua-desktop".to_owned()]
        );
        assert!(temp_dir.path().exists());
    }

    #[test]
    fn docker_discovery_uses_host_search_path() {
        assert!(!crate::platform::command_path().is_empty());
        assert!(
            !crate::platform::program_path("docker")
                .as_os_str()
                .is_empty()
        );
    }

    #[test]
    fn unavailable_workspace_blocks_start_before_docker_is_called() {
        let temp_dir = TempDir::new().expect("temp directory");
        let missing_workspace = temp_dir.path().join("moved-project");
        let manager = RuntimeManager::new(
            FakeRunner::default(),
            runtime_lock(),
            temp_dir.path().join("app-data"),
            RuntimeConfiguration {
                external_workspace: Some(missing_workspace),
                ..RuntimeConfiguration::default()
            },
        )
        .expect("runtime manager retains the saved path");

        let error = manager
            .start()
            .expect_err("missing workspace must block start");

        assert!(
            error
                .to_string()
                .contains("selected workspace is unavailable")
        );
        assert!(manager.runner.calls().is_empty());
    }

    #[test]
    fn unavailable_auth_source_blocks_import_before_docker_is_called() {
        let temp_dir = TempDir::new().expect("temp directory");
        let manager = RuntimeManager::new(
            FakeRunner::default(),
            runtime_lock(),
            temp_dir.path().join("app-data"),
            RuntimeConfiguration::default(),
        )
        .expect("runtime manager");

        let error = manager
            .import_codex_auth(&temp_dir.path().join("moved/auth.json"))
            .expect_err("missing auth file must block import");

        assert!(error.to_string().contains("auth.json is unavailable"));
        assert!(manager.runner.calls().is_empty());
    }
}
