const invoke = window.__TAURI__.core.invoke;

const elements = {
  desktopVersion: document.querySelector("#desktop-version"),
  dockerBanner: document.querySelector("#docker-banner"),
  dockerBannerText: document.querySelector("#docker-banner-text"),
  dockerGate: document.querySelector("#docker-gate"),
  dockerGateDetail: document.querySelector("#docker-gate-detail"),
  dockerGateHint: document.querySelector("#docker-gate-hint"),
  dockerInstall: document.querySelector("#docker-install"),
  dockerGuide: document.querySelector("#docker-guide"),
  dockerProceed: document.querySelector("#docker-proceed"),
  alwaysOnNotice: document.querySelector("#always-on-notice"),
  alwaysOnSleepHint: document.querySelector("#always-on-sleep-hint"),
  alwaysOnGotIt: document.querySelector("#always-on-got-it"),
  alwaysOnGuide: document.querySelector("#always-on-guide"),
  launcherTitle: document.querySelector("#launcher-title"),
  shell: document.querySelector(".shell"),
  openUpdates: document.querySelector("#open-updates"),
  updatesCount: document.querySelector("#updates-count"),
  updateBadge: document.querySelector("#update-badge"),
  updateBadgeOpen: document.querySelector("#update-badge-open"),
  updateBadgeDismiss: document.querySelector("#update-badge-dismiss"),
  updateBadgeRing: document.querySelector("#update-badge-ring"),
  updateBadgeLabel: document.querySelector("#update-badge-label"),
  updateBadgeDetail: document.querySelector("#update-badge-detail"),
  updatesDrawer: document.querySelector("#updates-drawer"),
  updatesTitle: document.querySelector("#updates-title"),
  closeUpdates: document.querySelector("#close-updates"),
  drawerNotice: document.querySelector("#drawer-notice"),
  updateChecked: document.querySelector("#update-checked"),
  checkUpdates: document.querySelector("#check-updates"),
  launcherLaneVersions: document.querySelector("#launcher-lane-versions"),
  launcherStatus: document.querySelector("#launcher-status"),
  launcherSteps: document.querySelector("#launcher-steps"),
  launcherActions: document.querySelector("#launcher-actions"),
  openLauncherDownload: document.querySelector("#open-launcher-download"),
  runtimeLaneVersions: document.querySelector("#runtime-lane-versions"),
  updateStatus: document.querySelector("#update-status"),
  updateWarning: document.querySelector("#update-warning"),
  runtimeSteps: document.querySelector("#runtime-steps"),
  updateReview: document.querySelector("#update-review"),
  updateNotes: document.querySelector("#update-notes"),
  updateNotesSection: document.querySelector("#update-notes-section"),
  updateAck: document.querySelector("#update-ack"),
  runtimeActions: document.querySelector("#runtime-actions"),
  downloadUpdate: document.querySelector("#download-update"),
  installUpdate: document.querySelector("#install-update"),
  catalogNote: document.querySelector("#catalog-note"),
  runtimeCatalog: document.querySelector("#runtime-catalog"),
  registryExisting: document.querySelector("#registry-existing"),
  registryGh: document.querySelector("#registry-gh"),
  registryLogin: document.querySelector("#registry-login"),
  registryPat: document.querySelector("#registry-pat"),
  registryForm: document.querySelector("#registry-form"),
  registryUser: document.querySelector("#registry-user"),
  registryToken: document.querySelector("#registry-token"),
  registryDetail: document.querySelector("#registry-detail"),
  authSource: document.querySelector("#auth-source"),
  authSourceRow: document.querySelector("#auth-source-row"),
  authWarning: document.querySelector("#auth-warning"),
  autostart: document.querySelector("#autostart"),
  cancelSubscription: document.querySelector("#cancel-subscription"),
  chooseWorkspace: document.querySelector("#choose-workspace"),
  connectSubscription: document.querySelector("#connect-subscription"),
  dataDir: document.querySelector("#data-dir"),
  downloadDiagnostics: document.querySelector("#download-diagnostics"),
  finishSubscription: document.querySelector("#finish-subscription"),
  logs: document.querySelector("#logs"),
  notice: document.querySelector("#notice"),
  expandLogs: document.querySelector("#expand-logs"),
  openApp: document.querySelector("#open-app"),
  openBrowser: document.querySelector("#open-browser"),
  openWorkspaceFolder: document.querySelector("#open-workspace-folder"),
  quitDesktopApp: document.querySelector("#quit-desktop-app"),
  toggleLogRefresh: document.querySelector("#toggle-log-refresh"),
  logLines: document.querySelector("#log-lines"),
  logsState: document.querySelector("#logs-state"),
  refreshExternalSkills: document.querySelector("#refresh-external-skills"),
  offerUnlistedRuntimes: document.querySelector("#offer-unlisted-runtimes"),
  updateNote: document.querySelector("#update-note"),
  restartRuntime: document.querySelector("#restart-runtime"),
  runtimeVersion: document.querySelector("#runtime-version"),
  startRuntime: document.querySelector("#start-runtime"),
  statusDetail: document.querySelector("#status-detail"),
  statusLabel: document.querySelector("#status-label"),
  statusPill: document.querySelector("#status-pill"),
  stopRuntime: document.querySelector("#stop-runtime"),
  stopRuntimeAndQuit: document.querySelector("#stop-runtime-and-quit"),
  stopAndQuitLabel: document.querySelector("#stop-and-quit-label"),
  subscriptionAccess: document.querySelector("#subscription-access"),
  logsSection: document.querySelector("#logs-section"),
  openUaRepository: document.querySelector("#open-ua-repository"),
  subscriptionBadge: document.querySelector("#subscription-badge"),
  subscriptionDetail: document.querySelector("#subscription-detail"),
  useInternalWorkspace: document.querySelector("#use-internal-workspace"),
  webUrl: document.querySelector("#web-url"),
  workspaceBadge: document.querySelector("#workspace-badge"),
  workspaceDir: document.querySelector("#workspace-dir"),
  workspaceWarning: document.querySelector("#workspace-warning"),
};

const phaseLabels = {
  docker_unavailable: "Docker unavailable",
  error: "Needs attention",
  not_created: "Not installed",
  onboarding: "Setup ready",
  ready: "Running",
  starting: "Starting",
  startup_failed: "Stopped while starting",
  stopped: "Stopped",
};

// Phases in which UA's web server answers, so the Web UI can open in the app
// or a browser. The native open commands check the same phases.
const WEB_UI_PHASES = ["ready", "onboarding"];

function webUiAvailable() {
  return WEB_UI_PHASES.includes(latestStatus?.phase);
}

const subscriptionLabels = {
  disconnected: "Not connected",
  imported: "Imported",
  pending_import: "Import pending",
  source_unavailable: "Source unavailable",
};

let latestUpdate = null;
let checkingUpdates = false;
// The last check failed outright (no review at all), as opposed to a review
// that explains why nothing can be installed.
let lastCheckFailed = false;
let lastCheckedAt = null;
let latestStatus = null;
let latestIntegration = null;
let operationInFlight = false;
let logsRefreshInFlight = false;
let automaticImportAttempted = false;
// Tooltips written in index.html. Some buttons swap them for the reason they
// are unavailable, then restore these.
const defaultTitles = {
  start: elements.startRuntime.title,
  install: elements.startRuntime.dataset?.installTitle,
  restart: elements.restartRuntime.title,
  finish: elements.finishSubscription.title,
};
// The subscription state last rendered, to open its section once when it needs action.
let lastSubscriptionState = null;
// Automatic log refresh is paused by the person.
let logsPaused = false;
// How many log lines to fetch; the person's choice is kept on this computer.
const LOG_LINES_KEY = "ua-desktop.status-window.log-lines";
const LOG_LINES = { min: 10, max: 5000, fallback: 250 };
// The update badge hides after this long, or when dismissed, and shows again
// when the update moves to its next stage.
const BADGE_VISIBLE_MS = 5_000;
let badgeKey = null;
let badgeDone = false;
let badgeTimer = null;
// The pointer is over the badge or focus is inside it, so it stays.
let badgeHeld = false;
// Docker prerequisite state. The gate is blocking only until the user chooses
// Proceed; after that the launcher stays usable for recovery actions while a
// 30-second check watches for Docker to appear.
let dockerAvailable = null;
let dockerGateDismissed = false;
let dockerBannerText = "";
// Set once Docker becomes reachable so the green confirmation banner shows until
// the first user interaction, then never again in this session.
let dockerRecoveryBanner = false;
let dockerRecoveryAcknowledged = false;
// Update download state, independent of the apply path.
let downloadingUpdate = false;
// Whole percent of a running download, when Docker reports layer sizes.
let downloadPercent = null;
// A "Choose" request for another catalog version is in flight.
let choosingVersion = false;
// The catalog's Choose and Notes buttons, redrawn with each review.
let catalogButtons = [];
// Where focus returns when the Updates drawer closes.
let drawerReturnFocus = null;
// The Docker prerequisite dialog is showing.
let dockerGateVisible = false;
let latestDockerPrerequisite = null;
// The intro (intro.js) covers the launcher when the window first loads. The
// Docker prerequisite dialog waits until it is dismissed so the two never
// stack, and the click or key that dismisses it does not count as noticing
// the Docker recovery banner hidden behind it.
const intro = window.UADesktopIntro;

function introOpen() {
  return intro?.isOpen() === true;
}

// Universal Agents runs on this computer, and agents only work while it is
// awake. The first time the launcher shows, after the intro and before the
// Docker dialog, a notice says so and offers the setup guide for running UA on
// a server instead. "Got it" is kept on this computer, like the theme choice,
// so the notice shows once. "pending" until the intro is gone, then "showing"
// or "done".
const ALWAYS_ON_SEEN_KEY = "ua-desktop.status-window.always-on-notice-seen";
let alwaysOnNotice = "pending";

function alwaysOnNoticeVisible() {
  return alwaysOnNotice === "showing";
}

// Read the saved choice only once the intro is gone, so a reload shows the
// notice again until the person closes it. Unreadable storage shows the notice:
// repeating it is better than never telling the person.
function showAlwaysOnNoticeIfDue() {
  if (alwaysOnNotice !== "pending" || introOpen()) return;
  let seen = false;
  try {
    seen = window.localStorage.getItem(ALWAYS_ON_SEEN_KEY) !== null;
  } catch (error) {
    console.warn(`ua.desktop.always-on-notice: whether the notice was seen could not be read, so it shows: ${error}`);
  }
  alwaysOnNotice = seen ? "done" : "showing";
  elements.alwaysOnNotice.hidden = !alwaysOnNoticeVisible();
  if (alwaysOnNoticeVisible()) {
    closeUpdates({ returnFocus: false });
    applyShellInert();
    elements.alwaysOnGotIt.focus?.();
  }
}

function dismissAlwaysOnNotice() {
  if (!alwaysOnNoticeVisible()) return;
  alwaysOnNotice = "done";
  elements.alwaysOnNotice.hidden = true;
  try {
    window.localStorage.setItem(ALWAYS_ON_SEEN_KEY, new Date().toISOString());
  } catch (error) {
    console.warn(`ua.desktop.always-on-notice: closing the notice could not be saved, so it shows again next time: ${error}`);
  }
  // The Docker dialog that waited shows now and takes focus; otherwise focus
  // goes to the launcher, which this render released from inert.
  renderDockerGate();
  renderUpdateBadge();
  if (!dockerGateVisible) elements.launcherTitle.focus?.();
}

async function loadKeepAwakeHint() {
  try {
    elements.alwaysOnSleepHint.textContent = await invoke("keep_awake_hint");
  } catch (error) {
    // The general advice written in index.html stays.
    console.warn(`ua.desktop.always-on-notice: the sleep setting for this computer could not be read: ${error}`);
  }
}

// Marks an error written entirely by the desktop app, with no Docker or OS
// output, so it is safe to show (ua_desktop_core::guidance::MARKER). Every
// other error can carry raw command output and stays in the diagnostics file.
const GUIDANCE_MARKER = "\u001f";
const GENERIC_ERROR =
  "The operation did not complete. Download diagnostics for the exact error and logs, then retry.";

// Errors appear where the person is looking: inside the Updates drawer while
// it is open, otherwise under the launch panes.
function showError(error) {
  const text = String(error);
  const notice = updatesOpen() ? elements.drawerNotice : elements.notice;
  notice.textContent = text.startsWith(GUIDANCE_MARKER)
    ? text.slice(GUIDANCE_MARKER.length)
    : GENERIC_ERROR;
  notice.hidden = false;
}

function clearError() {
  for (const notice of [elements.notice, elements.drawerNotice]) {
    notice.hidden = true;
    notice.textContent = "";
  }
}

function setBusy(busy) {
  operationInFlight = busy;
  document.body.classList.toggle("operation-busy", busy);
  updateActionAvailability();
}

// Subscribe before invoking so fast authentication/download transitions are not lost.
// `onProgress` also receives each update, for progress shown elsewhere.
async function invokeWithProgress(command, args, initialMessage, onProgress) {
  const panel = document.querySelector("#operation-progress");
  const bar = document.querySelector("#operation-bar");
  const detail = document.querySelector("#operation-detail");
  panel.hidden = false;
  bar.hidden = false;
  bar.removeAttribute("value");
  bar.removeAttribute("max");
  detail.textContent = initialMessage;
  let unlisten;
  try {
    unlisten = await window.__TAURI__.event.listen("runtime-progress", event => {
      const update = typeof event.payload === "string"
        ? { message: event.payload }
        : event.payload;
      detail.textContent = update.message;
      if (Number.isFinite(update.current_bytes) && Number.isFinite(update.total_bytes) && update.total_bytes > 0) {
        bar.max = update.total_bytes;
        bar.value = Math.min(update.current_bytes, update.total_bytes);
      } else {
        bar.removeAttribute("value");
        bar.removeAttribute("max");
      }
      onProgress?.(update);
    });
    const status = await invoke(command, args);
    detail.textContent = status?.detail || phaseLabels[status?.phase] || "Operation finished.";
    return status;
  } catch (error) {
    detail.textContent = "Operation failed. See the error below, then retry.";
    throw error;
  } finally {
    if (unlisten) unlisten();
    bar.hidden = true;
  }
}

// Docker prerequisite copy is fixed application text, never command output.
const DOCKER_WAITING_BANNER =
  "I am still unable to detect Docker. Please install and run Docker to proceed. I will run an automatic check every 30 sec.";
const DOCKER_READY_BANNER = "Docker detected, good to go.";
const UPDATE_CHECK_FAILED =
  "Could not check for updates. Download diagnostics for details, then choose Check now.";

function renderDockerGate() {
  const blocked = dockerAvailable === false;
  document.body.classList.toggle("docker-blocked", blocked);
  // The dialog is blocking on first detection and after a failed recovery, but
  // a Proceed click leaves the launcher usable for recovery actions.
  const showGate = blocked && !dockerGateDismissed && !introOpen() && !alwaysOnNoticeVisible();
  const wasHidden = elements.dockerGate.hidden;
  elements.dockerGate.hidden = !showGate;
  dockerGateVisible = showGate;
  // The Docker dialog takes over from the Updates drawer.
  if (showGate) closeUpdates({ returnFocus: false });
  applyShellInert();
  if (showGate && wasHidden) {
    // Move focus into the dialog so keyboard users start on the prerequisite
    // choices instead of the disabled launcher behind it.
    elements.dockerInstall.focus?.();
  }
  if (showGate && latestDockerPrerequisite) {
    elements.dockerGateDetail.textContent = latestDockerPrerequisite.installed
      ? "Docker is installed but I cannot reach it. Start Docker and it will connect automatically."
      : "Universal Agents runs in a local Docker container. Docker was not found on this computer.";
    elements.dockerGateHint.textContent = latestDockerPrerequisite.setup_hint ?? "";
  }
  // A status banner sits under the header once the blocking dialog is out of
  // the way: amber while Docker is missing, green until the first interaction
  // after it returns.
  const bannerText = showGate ? "" : dockerBannerText;
  elements.dockerBanner.hidden = bannerText === "";
  elements.dockerBannerText.textContent = bannerText;
  elements.dockerBanner.className = `banner ${
    bannerText === DOCKER_READY_BANNER ? "banner-success" : "banner-warning"
  }`;
}

// Make the launcher genuinely unreachable behind anything modal: while the
// Docker dialog, the always-on notice, the intro or the Updates drawer is open,
// the launcher is inert, so keyboard focus cannot reach it. Proceed, Got it,
// dismissing the intro and closing the drawer each release it.
function applyShellInert() {
  if (elements.shell) {
    elements.shell.inert = dockerGateVisible || alwaysOnNoticeVisible() || introOpen() || updatesOpen();
  }
}

// A single entry point keeps the modal, banner, and button gates consistent
// whether Docker state arrives from the prerequisite probe or a status phase.
function applyDockerState(available) {
  const wasUnavailable = dockerAvailable === false;
  dockerAvailable = available;
  if (available === false) {
    // A fresh failure re-opens the blocking dialog; a continuing one stays
    // dismissed so the launcher does not re-block on every poll.
    if (!wasUnavailable) dockerGateDismissed = false;
    dockerBannerText = DOCKER_WAITING_BANNER;
    dockerRecoveryBanner = false;
    dockerRecoveryAcknowledged = false;
  } else if (wasUnavailable && !dockerRecoveryAcknowledged) {
    // Docker came back after being unavailable: confirm recovery until the user
    // touches the UI, then stop showing the confirmation.
    dockerRecoveryBanner = true;
    dockerBannerText = DOCKER_READY_BANNER;
  } else if (available === true && !dockerRecoveryBanner) {
    dockerBannerText = "";
  }
  renderDockerGate();
  renderUpdateBadge();
  updateActionAvailability();
}

function acknowledgeDockerRecovery() {
  // The banner is behind the intro or the always-on notice, so clicks and keys
  // there do not count as seeing it.
  if (introOpen() || alwaysOnNoticeVisible() || !dockerRecoveryBanner || dockerRecoveryAcknowledged) return;
  dockerRecoveryAcknowledged = true;
  dockerRecoveryBanner = false;
  dockerBannerText = "";
  renderDockerGate();
}

function updateActionAvailability() {
  const updateBusy = operationInFlight || checkingUpdates || choosingVersion;
  elements.checkUpdates.disabled = updateBusy || downloadingUpdate;
  elements.updateAck.disabled = updateBusy;
  elements.installUpdate.disabled = updateBusy || downloadingUpdate ||
    dockerAvailable === false || !latestUpdate?.review_id || !elements.updateAck.checked ||
    latestIntegration?.workspace_available !== true;
  elements.downloadUpdate.disabled = updateBusy || downloadingUpdate ||
    dockerAvailable === false || !latestUpdate?.review_id;
  for (const button of catalogButtons) {
    // Choosing another version replaces the review a download or switch uses.
    button.disabled = button.dataset.action === "choose" && (updateBusy || downloadingUpdate);
  }
  const workspaceBlocked = latestIntegration?.workspace_available !== true;
  // A refused start is still a Docker-running container in a restart loop:
  // Stop and Restart must reach it, and Start would do nothing.
  const running = ["ready", "onboarding", "starting", "startup_failed"].includes(latestStatus?.phase);
  const startable = ["not_created", "stopped", "error"].includes(latestStatus?.phase);
  // Docker is a hard prerequisite for anything that talks to the daemon. The
  // remaining controls stay available so a user can quit, refresh, read logs,
  // export diagnostics, and record a workspace or credential choice while they
  // install or start Docker.
  const dockerBlocked = dockerAvailable === false;
  for (const button of [
    elements.registryExisting,
    elements.registryGh,
    elements.registryLogin,
    elements.registryPat,
    elements.registryUser,
    elements.registryToken,
    elements.finishSubscription,
    elements.stopRuntime,
    elements.stopRuntimeAndQuit,
  ]) {
    button.disabled = operationInFlight || dockerBlocked;
  }
  for (const button of [
    elements.cancelSubscription,
    elements.chooseWorkspace,
    elements.connectSubscription,
    elements.downloadDiagnostics,
    elements.openWorkspaceFolder,
    elements.refreshExternalSkills,
    elements.useInternalWorkspace,
  ]) {
    button.disabled = operationInFlight;
  }
  // A check in progress would return an offer for the old choice.
  elements.offerUnlistedRuntimes.disabled = updateBusy || downloadingUpdate;
  // Quitting the host is always available; quitting by stopping first needs a
  // running runtime so the action cannot silently do nothing.
  elements.quitDesktopApp.disabled = false;
  elements.stopRuntimeAndQuit.disabled = operationInFlight || dockerBlocked || !running;
  elements.startRuntime.disabled = operationInFlight || dockerBlocked || workspaceBlocked || !startable;
  elements.stopRuntime.disabled = operationInFlight || dockerBlocked || !running;
  elements.restartRuntime.disabled = operationInFlight || dockerBlocked || workspaceBlocked || !running;
  const importReady = webUiAvailable() &&
    latestIntegration?.openai_subscription_state === "pending_import";
  elements.finishSubscription.disabled = operationInFlight || !importReady;
  elements.finishSubscription.title = importReady ? defaultTitles.finish : "Start UA first. Your selection is saved.";
  if (latestIntegration?.openai_subscription_state === "pending_import") {
    elements.subscriptionDetail.textContent = latestStatus?.phase === "ready"
      ? latestIntegration.openai_subscription_detail
      : latestStatus?.phase === "onboarding"
        ? "Your auth.json selection is saved. Import will run automatically so you can test the Codex connection during onboarding."
        : "Your auth.json selection is saved. Start the runtime first. If image access was denied, sign in above. Import will run when setup is ready.";
  }
  const workspaceTitle =
    latestIntegration === null
      ? "Checking the saved workspace."
      : workspaceBlocked
        ? "Repair the saved workspace before starting the runtime."
        : "";
  elements.startRuntime.title = workspaceTitle ||
    (latestStatus?.phase === "not_created" ? defaultTitles.install : defaultTitles.start);
  elements.restartRuntime.title = workspaceTitle || defaultTitles.restart;
}

function renderStatus(status) {
  latestStatus = status;
  // The status phase already distinguishes Docker-unavailable, so the launcher
  // reflects it on every poll without waiting for the slower prerequisite probe.
  applyDockerState(status.phase !== "docker_unavailable");
  // Only fetch the dialog's install/start detail when it is missing. The
  // 30-second timer owns ongoing re-checks, so a 3-second status poll must not
  // double the backend's `docker info` load.
  if (status.phase === "docker_unavailable" && latestDockerPrerequisite === null) {
    refreshDockerPrerequisite({ quiet: true });
  }
  document.querySelector("#read-docs").href = status.docs_url ?? "#";
  elements.statusLabel.textContent = phaseLabels[status.phase] ?? status.phase;
  elements.statusDetail.textContent = status.detail;
  elements.runtimeVersion.textContent = status.ua_version;
  document.querySelector("#header-runtime-version").textContent = status.ua_version;
  elements.webUrl.textContent = status.web_url;
  elements.dataDir.textContent = status.data_dir;
  elements.statusPill.className = `status-text status-${status.phase}`;
  elements.openApp.disabled = !webUiAvailable();
  elements.openBrowser.disabled = !webUiAvailable();
  elements.startRuntime.textContent = status.phase === "not_created" ? "Install & start" : "Start";
  updateActionAvailability();
}

function renderIntegration(status) {
  latestIntegration = status;
  const subscriptionState = status.openai_subscription_state;
  elements.workspaceDir.textContent = status.workspace_dir;
  elements.workspaceBadge.textContent = status.workspace_available
    ? status.workspace_is_external
      ? "Chosen folder"
      : "App workspace"
    : "Folder unavailable";
  elements.workspaceBadge.className = `mini-badge ${
    status.workspace_available
      ? status.workspace_is_external
        ? "badge-external"
        : ""
      : "badge-unavailable"
  }`;
  elements.workspaceWarning.hidden = status.workspace_available;
  elements.workspaceWarning.textContent = status.workspace_issue ?? "";
  elements.chooseWorkspace.textContent = status.workspace_available
    ? "Choose folder"
    : "Choose replacement";
  elements.useInternalWorkspace.hidden = !status.workspace_is_external;

  elements.subscriptionBadge.textContent = subscriptionLabels[subscriptionState] ?? subscriptionState;
  elements.subscriptionBadge.className = `mini-badge badge-${subscriptionState}`;
  elements.subscriptionDetail.textContent = status.openai_subscription_detail;
  elements.authSourceRow.hidden = !status.pending_codex_auth_file;
  elements.authSource.textContent = status.pending_codex_auth_file ?? "—";
  elements.finishSubscription.hidden = subscriptionState !== "pending_import";
  elements.cancelSubscription.hidden = !["pending_import", "source_unavailable"].includes(
    subscriptionState,
  );
  elements.authWarning.hidden = subscriptionState !== "source_unavailable";
  elements.authWarning.textContent =
    subscriptionState === "source_unavailable"
      ? "The desktop app will not read or mount this path until you choose an available auth.json."
      : "";
  elements.connectSubscription.textContent =
    subscriptionState === "disconnected"
      ? "Choose auth.json"
      : subscriptionState === "source_unavailable"
        ? "Choose replacement auth.json"
        : "Replace auth.json";
  elements.refreshExternalSkills.checked = status.refresh_external_skills_on_start === true;
  elements.offerUnlistedRuntimes.checked = status.offer_unlisted_runtimes === true;
  if (subscriptionState !== "pending_import") automaticImportAttempted = false;
  // A saved auth.json that went missing needs a choice, so its section opens.
  if (subscriptionState === "source_unavailable" && lastSubscriptionState !== subscriptionState) {
    elements.subscriptionAccess.open = true;
  }
  lastSubscriptionState = subscriptionState;
  updateActionAvailability();
}

async function refreshIntegration({ quiet = false } = {}) {
  if (!quiet) clearError();
  try {
    renderIntegration(await invoke("integration_status"));
  } catch (error) {
    if (!quiet) showError(error);
  }
}

async function attemptPendingImport() {
  if (
    operationInFlight ||
    automaticImportAttempted ||
    !webUiAvailable() ||
    latestIntegration?.openai_subscription_state !== "pending_import"
  ) {
    return;
  }
  automaticImportAttempted = true;
  setBusy(true);
  try {
    renderIntegration(await invoke("finish_openai_subscription"));
    renderStatus(await invoke("runtime_status"));
  } catch (error) {
    showError(error);
  } finally {
    setBusy(false);
  }
}

async function refreshStatus({ quiet = false } = {}) {
  // A background download holds the Docker serialization lock, so polling
  // status or logs would only queue blocked calls behind it.
  if (operationInFlight || downloadingUpdate) return;
  if (!quiet) clearError();
  try {
    renderStatus(await invoke("runtime_status"));
    await refreshIntegration({ quiet: true });
    await attemptPendingImport();
    if (latestUpdate?.update_pending && webUiAvailable()) checkUpdates();
  } catch (error) {
    if (!quiet) showError(error);
  }
}

async function runLifecycle(command) {
  clearError();
  setBusy(true);
  try {
    renderStatus(await invokeWithProgress(command, undefined, command === "stop_runtime" ? "Stopping runtime…" : "Checking Docker and preparing runtime…"));
    await refreshIntegration({ quiet: true });
    await refreshLogs({ quiet: true });
  } catch (error) {
    showError(error);
  } finally {
    setBusy(false);
  }
}

async function runIntegration(command) {
  clearError();
  setBusy(true);
  if (command === "connect_openai_subscription") automaticImportAttempted = false;
  try {
    renderIntegration(await invoke(command));
    renderStatus(await invoke("runtime_status"));
    await refreshLogs({ quiet: true });
  } catch (error) {
    showError(error);
  } finally {
    setBusy(false);
  }
}

async function authenticateRegistry(method) {
  if (operationInFlight) return;
  clearError();
  const token = method === "pat" ? elements.registryToken.value : "";
  elements.registryToken.value = "";
  setBusy(true);
  elements.registryDetail.textContent = "Checking image access, then starting UA. The first pull can take several minutes.";
  try {
    renderStatus(await invokeWithProgress("registry_access", {
      method, username: method === "pat" ? elements.registryUser.value.trim() : "", token,
    }, method === "existing" ? "Verifying image access…" : "Signing Docker into GHCR…"));
    elements.registryDetail.textContent = "Image access verified. Runtime start completed.";
    await refreshIntegration({ quiet: true });
  } catch (error) {
    elements.registryDetail.textContent = "Setup did not complete. See the error above; saved auth.json selections are unchanged.";
    showError(error);
  } finally {
    setBusy(false);
  }
}

elements.registryExisting.addEventListener("click", () => authenticateRegistry("existing"));
elements.registryGh.addEventListener("click", () => authenticateRegistry("gh"));
elements.registryForm.addEventListener("submit", (event) => {
  event.preventDefault();
  return authenticateRegistry("pat");
});
elements.registryLogin.addEventListener("click", async () => {
  if (operationInFlight) return;
  clearError();
  setBusy(true);
  try {
    elements.registryDetail.textContent = "Complete GitHub sign-in in the terminal and browser. Installation will continue automatically. You have five minutes to finish.";
    renderStatus(await invokeWithProgress("begin_github_login", undefined, elements.registryDetail.textContent));
    elements.registryDetail.textContent = "GitHub and image access verified. " + (latestStatus?.detail || "Runtime start requested.");
    await refreshIntegration({ quiet: true });
  } catch (error) {
    showError(error);
  } finally {
    setBusy(false);
  }
});

// Automatic refreshes run only while the logs section is open and not paused;
// `force` fetches once anyway, for a request the person just made.
async function refreshLogs({ quiet = false, force = false } = {}) {
  if (logsRefreshInFlight || downloadingUpdate) return;
  if (!force && (logsPaused || elements.logsSection.open !== true)) return;
  logsRefreshInFlight = true;
  if (!quiet) clearError();
  try {
    const logs = await invoke("recent_logs", { tail: logLineCount() });
    elements.logs.textContent = logs || "The runtime has not emitted any logs yet.";
    elements.logs.scrollTop = elements.logs.scrollHeight;
  } catch (error) {
    if (!quiet) showError(error);
  } finally {
    logsRefreshInFlight = false;
  }
}

// The requested line count, kept within the allowed range.
function logLineCount() {
  const value = Math.round(Number(elements.logLines.value));
  if (!Number.isFinite(value) || value <= 0) return LOG_LINES.fallback;
  return Math.min(LOG_LINES.max, Math.max(LOG_LINES.min, value));
}

function loadLogLines() {
  let saved = null;
  try {
    saved = window.localStorage?.getItem(LOG_LINES_KEY) ?? null;
  } catch (error) {
    console.warn(`ua.desktop.logs: the saved line count could not be read: ${error}`);
  }
  elements.logLines.value = String(saved === null ? LOG_LINES.fallback : saved);
  elements.logLines.value = String(logLineCount());
}

function renderLogRefresh() {
  const label = logsPaused ? "Resume log refresh" : "Pause log refresh";
  elements.toggleLogRefresh.setAttribute("aria-label", label);
  elements.toggleLogRefresh.title = label;
  elements.toggleLogRefresh.className = `icon-button${logsPaused ? " is-paused" : ""}`;
  elements.logsState.textContent = logsPaused ? "Paused" : "Live";
  elements.logsState.className = `mini-badge ${logsPaused ? "badge-paused" : "badge-running"}`;
}

async function loadAutostart() {
  try {
    elements.autostart.checked = await invoke("autostart_status");
  } catch (error) {
    showError(error);
  }
}

elements.openApp.addEventListener("click", async () => {
  clearError();
  try {
    await invoke("open_webchat_window");
  } catch (error) {
    showError(error);
  }
});

elements.openBrowser.addEventListener("click", async () => {
  clearError();
  try {
    await invoke("open_webchat_browser");
  } catch (error) {
    showError(error);
  }
});

elements.chooseWorkspace.addEventListener("click", () => runIntegration("choose_workspace"));
elements.openWorkspaceFolder.addEventListener("click", async () => {
  clearError();
  elements.openWorkspaceFolder.disabled = true;
  try {
    await invoke("open_workspace_folder");
  } catch (error) {
    showError(error);
  } finally {
    updateActionAvailability();
  }
});
elements.expandLogs.addEventListener("click", () => {
  const expanded = document.body.classList.toggle("logs-expanded");
  const label = expanded ? "Collapse logs" : "Expand logs";
  elements.expandLogs.setAttribute("aria-expanded", String(expanded));
  elements.expandLogs.setAttribute("aria-label", label);
  elements.expandLogs.title = label;
  if (expanded) elements.logs.scrollTop = elements.logs.scrollHeight;
});
// Opening the section shows current logs at once.
elements.logsSection.addEventListener("toggle", () => {
  if (elements.logsSection.open) refreshLogs({ quiet: true, force: true });
});
elements.toggleLogRefresh.addEventListener("click", () => {
  logsPaused = !logsPaused;
  renderLogRefresh();
  if (!logsPaused) refreshLogs({ force: true });
});
elements.logLines.addEventListener("change", () => {
  const lines = logLineCount();
  elements.logLines.value = String(lines);
  try {
    window.localStorage?.setItem(LOG_LINES_KEY, String(lines));
  } catch (error) {
    console.warn(`ua.desktop.logs: the line count could not be saved: ${error}`);
  }
  // A new count is a request to see it, even while refresh is paused.
  refreshLogs({ force: true });
});
elements.quitDesktopApp.addEventListener("click", async () => {
  clearError();
  try {
    await invoke("quit_desktop_app");
  } catch (error) {
    showError(error);
  }
});
// The button spins and says so while UA stops, until the app exits.
function showStopping(stopping) {
  elements.stopRuntimeAndQuit.className = `button button-danger header-control${stopping ? " is-working" : ""}`;
  elements.stopRuntimeAndQuit.setAttribute("aria-busy", String(stopping));
  elements.stopAndQuitLabel.textContent = stopping ? "Stopping…" : "Quit";
}

elements.stopRuntimeAndQuit.addEventListener("click", async () => {
  if (elements.stopRuntimeAndQuit.disabled) return;
  clearError();
  showStopping(true);
  setBusy(true);
  try {
    // A successful call stops the runtime and exits the host, so there is no
    // follow-up state to render. A failure keeps the window open with details.
    await invokeWithProgress("stop_runtime_and_quit", undefined, "Stopping Universal Agents…");
  } catch (error) {
    showStopping(false);
    showError(error);
  } finally {
    setBusy(false);
  }
});
elements.connectSubscription.addEventListener("click", () =>
  runIntegration("connect_openai_subscription"),
);
elements.cancelSubscription.addEventListener("click", () =>
  runIntegration("cancel_openai_subscription_import"),
);
elements.finishSubscription.addEventListener("click", () => {
  automaticImportAttempted = false;
  return runIntegration("finish_openai_subscription");
});
elements.useInternalWorkspace.addEventListener("click", () =>
  runIntegration("use_internal_workspace"),
);
elements.startRuntime.addEventListener("click", () => runLifecycle("start_runtime"));
elements.restartRuntime.addEventListener("click", () => runLifecycle("restart_runtime"));
elements.stopRuntime.addEventListener("click", () => runLifecycle("stop_runtime"));
elements.openUaRepository.addEventListener("click", () => {
  clearError();
  invoke("open_ua_repository").catch(showError);
});
loadLogLines();
renderLogRefresh();
elements.downloadDiagnostics.addEventListener("click", async () => {
  clearError();
  elements.downloadDiagnostics.disabled = true;
  try {
    const path = await invoke("download_diagnostics");
    if (path) elements.notice.textContent = `Diagnostics saved to ${path}`;
    elements.notice.hidden = !path;
  } catch (error) {
    showError(error);
  } finally {
    elements.downloadDiagnostics.disabled = false;
  }
});
elements.refreshExternalSkills.addEventListener("change", async () => {
  clearError();
  setBusy(true);
  try {
    renderIntegration(await invoke("set_external_skill_refresh", {
      enabled: elements.refreshExternalSkills.checked,
    }));
    renderStatus(await invoke("runtime_status"));
  } catch (error) {
    showError(error);
    await refreshIntegration({ quiet: true });
  } finally {
    setBusy(false);
  }
});
elements.offerUnlistedRuntimes.addEventListener("change", async () => {
  clearError();
  elements.offerUnlistedRuntimes.disabled = true;
  try {
    renderIntegration(await invoke("set_unlisted_runtime_offers", {
      enabled: elements.offerUnlistedRuntimes.checked,
    }));
    // The backend dropped the review made under the other choice.
    await checkUpdates();
  } catch (error) {
    showError(error);
    await refreshIntegration({ quiet: true });
  } finally {
    updateActionAvailability();
  }
});
elements.autostart.addEventListener("change", async () => {
  clearError();
  elements.autostart.disabled = true;
  try {
    elements.autostart.checked = await invoke("set_autostart", {
      enabled: elements.autostart.checked,
    });
  } catch (error) {
    showError(error);
    await loadAutostart();
  } finally {
    elements.autostart.disabled = false;
  }
});

document.addEventListener("keydown", (event) => {
  // The intro marks the Escape that dismisses it, which must not also close
  // the notice that appears in its place.
  if (event.key !== "Escape" || event.defaultPrevented) return;
  if (alwaysOnNoticeVisible()) {
    dismissAlwaysOnNotice();
  } else if (updatesOpen()) {
    closeUpdates();
  } else if (document.body.classList.contains("logs-expanded")) {
    elements.expandLogs.click();
  }
});

document.addEventListener("visibilitychange", () => {
  if (!document.hidden) {
    refreshStatus({ quiet: true });
    refreshLogs({ quiet: true });
    if (dockerAvailable === false) refreshDockerPrerequisite({ quiet: true });
  }
});

Promise.all([refreshStatus(), loadAutostart(), refreshLogs({ quiet: true })]);
setInterval(() => {
  if (!document.hidden) {
    refreshStatus({ quiet: true });
    refreshLogs({ quiet: true });
    if (updatesOpen()) renderCheckedTime();
  }
}, 3_000);

// While Docker is missing the launcher keeps checking so the blocking dialog
// and disabled controls recover on their own once Docker answers.
setInterval(() => {
  if (!document.hidden && dockerAvailable === false) refreshDockerPrerequisite({ quiet: true });
}, 30_000);

document.querySelector("#read-docs").addEventListener("click", async (event) => {
  event.preventDefault();
  clearError();
  try { await invoke("open_docs_browser"); } catch (error) { showError(error); }
});

async function refreshDockerPrerequisite({ quiet = false } = {}) {
  try {
    const prerequisite = await invoke("docker_prerequisite");
    latestDockerPrerequisite = prerequisite;
    applyDockerState(prerequisite.available === true);
  } catch (error) {
    if (!quiet) showError(error);
  }
}

// ---------- Updates drawer and floating badge ----------
// The drawer is the one place for updates: a lane for the launcher, a lane
// for Universal Agents with its steps, and the recent UA versions. The header
// count and the floating badge only point to it.

function updatesOpen() {
  return elements.updatesDrawer.hidden === false;
}

// `event` is the click on the Updates button or badge. WebKit does not focus
// a clicked button, so the button itself, not the focused element, is where
// focus returns.
function openUpdates(event) {
  if (updatesOpen() || introOpen() || dockerGateVisible || alwaysOnNoticeVisible()) return;
  drawerReturnFocus = event?.currentTarget ?? document.activeElement ?? null;
  // Opening the drawer deals with what the badge pointed to.
  badgeDone = true;
  elements.drawerNotice.hidden = true;
  elements.updatesDrawer.hidden = false;
  applyShellInert();
  renderUpdateCenter();
  elements.updatesTitle.focus?.();
}

function closeUpdates({ returnFocus = true } = {}) {
  if (!updatesOpen()) return;
  elements.updatesDrawer.hidden = true;
  applyShellInert();
  renderUpdateBadge();
  const target = drawerReturnFocus;
  drawerReturnFocus = null;
  if (!returnFocus) return;
  // The badge that opened the drawer may be gone by now, and WebKit does not
  // focus a clicked button (focus stays on the page); the header button stays.
  const hiddenBadge = target === elements.updateBadgeOpen && elements.updateBadge.hidden !== false;
  const usable = target && target !== document.body && !hiddenBadge && !target.hidden && target.isConnected !== false;
  (usable ? target : elements.openUpdates).focus?.();
}

function releaseDate(value) {
  const date = value ? new Date(value) : null;
  if (!date || Number.isNaN(date.getTime())) return "";
  return date.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

function plural(count, word) {
  return `${count} ${word}${count === 1 ? "" : "s"}`;
}

// Release text is untrusted, so every node gets plain text, never HTML.
function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

// The icon shows a step's state; screen readers get the same as words.
const STEP_PREFIX = { done: "Done: ", now: "Next: ", todo: "" };

function renderSteps(list, steps) {
  list.hidden = steps.length === 0;
  list.replaceChildren(...steps.map(({ state, title, detail }) => {
    const item = element("li", `step is-${state}`);
    if (STEP_PREFIX[state]) item.append(element("span", "visually-hidden", STEP_PREFIX[state]));
    item.append(element("b", "", title), element("small", "", detail));
    return item;
  }));
}

// Finished steps are done and the first unfinished one is next.
function stepStates(finished) {
  const next = finished.indexOf(false);
  return finished.map((done, index) => (done ? "done" : index === next ? "now" : "todo"));
}

function percentText() {
  return downloadPercent === null ? "…" : ` · ${downloadPercent}%`;
}

function renderCheckedTime() {
  let text;
  if (checkingUpdates) {
    text = "Checking now…";
  } else if (lastCheckFailed) {
    text = "The last check did not finish.";
  } else if (lastCheckedAt === null) {
    text = "Not checked yet.";
  } else {
    const minutes = Math.floor((Date.now() - lastCheckedAt) / 60_000);
    text = minutes < 1
      ? "Checked just now."
      : minutes < 60
        ? `Checked ${plural(minutes, "minute")} ago.`
        : `Checked ${plural(Math.floor(minutes / 60), "hour")} ago.`;
  }
  elements.updateChecked.textContent = text;
}

function renderLauncherLane() {
  const desktop = latestUpdate?.desktop;
  let status;
  let steps = [];
  if (!desktop) {
    status = lastCheckFailed && !checkingUpdates
      ? "Could not check for a newer launcher."
      : "Checking for a newer launcher…";
  } else if (desktop.error) {
    status = desktop.error;
  } else if (desktop.newer) {
    status = `Launcher ${desktop.latest_version} is available. You have ${desktop.current_version}.`;
    const date = releaseDate(desktop.published_at);
    const [found, install, reopen] = stepStates([true, false, false]);
    steps = [
      { state: found, title: `Found launcher ${desktop.latest_version}`, detail: date ? `Released ${date}.` : "Published on GitHub." },
      { state: install, title: "Download and install", detail: "Open download page shows the release in your browser. Download the installer for this computer and run it." },
      { state: reopen, title: "Reopen the launcher", detail: "Universal Agents keeps running while you install." },
    ];
  } else {
    status = `Launcher ${desktop.current_version} is up to date.`;
  }
  elements.launcherStatus.textContent = status;
  elements.launcherLaneVersions.textContent = desktop?.newer
    ? `${desktop.current_version} → ${desktop.latest_version}`
    : "";
  renderSteps(elements.launcherSteps, steps);
  elements.launcherActions.hidden = desktop?.newer !== true;
}

function runtimeLaneState(review) {
  if (!review) return lastCheckFailed && !checkingUpdates ? "failed" : "checking";
  if (review.update_pending) return "pending";
  if (review.review_id) return "available";
  return review.releases.length ? "blocked" : "current";
}

function renderRuntimeLane() {
  const review = latestUpdate;
  const state = runtimeLaneState(review);
  const target = review?.latest_version;
  const current = review?.current_version;
  const downloaded = review?.image_downloaded === true;
  const unchecked = review?.listed === false
    ? `an unchecked ${review.prerelease ? "pre-release" : "release"}`
    : "";
  const available = downloadingUpdate
    ? `Downloading UA ${target} in the background. UA keeps running.`
    : downloaded
      ? `UA ${target} is downloaded. Review the notes, then switch when your work is finished.`
      : unchecked
        ? `UA ${target} is available as ${unchecked}. Running: ${current}.`
        : `UA ${target} is available. Running: ${current}.`;
  elements.updateStatus.textContent = {
    checking: "Checking GitHub for UA releases…",
    failed: UPDATE_CHECK_FAILED,
    pending: `UA ${current} update is pending. Use Start to finish or retry.`,
    // The reason is shown in the warning below this line.
    blocked: `UA ${target} is available, but this launcher can't install it yet. Running: ${current}.`,
    current: `UA ${current} is up to date.`,
    available,
  }[state];
  elements.runtimeLaneVersions.textContent = state === "available" ? `${current} → ${target}` : "";

  const warnings = [];
  if (review?.review_id && review.listed === false) {
    warnings.push(`UA ${target} is not on this launcher's compatibility list${review.prerelease ? " and is a pre-release for testing" : ""}. It may not start with this launcher, and after it runs your data may not work with older UA versions.`);
  }
  if (review?.breaking_changes) warnings.push("Breaking changes are listed in this update. Review every skipped release and follow its migration instructions before continuing.");
  if (review?.blocked_reason) warnings.push(review.blocked_reason);
  elements.updateWarning.textContent = warnings.join(" ");
  elements.updateWarning.hidden = warnings.length === 0;
  elements.updateNote.textContent = review?.newer_unlisted
    ? `UA ${review.newer_unlisted} is newer but not on this launcher's compatibility list yet.`
    : "";
  elements.updateNote.hidden = !review?.newer_unlisted;

  let steps = [];
  if (state === "available") {
    const recommended = review.catalog?.find(entry => entry.recommended)?.version;
    const date = releaseDate(review.releases.find(release => release.version === target)?.published_at);
    const fits = review.listed === false
      ? "Unchecked: not on this launcher's compatibility list."
      : "Works with this launcher.";
    const [found, download, notes, change] =
      stepStates([true, downloaded, elements.updateAck.checked === true, false]);
    steps = [
      { state: found, title: `${!recommended || recommended === target ? "Found" : "Chose"} UA ${target}`, detail: date ? `Released ${date}. ${fits}` : fits },
      {
        state: download,
        title: downloaded ? "Downloaded" : downloadingUpdate ? `Downloading${percentText()}` : "Download",
        detail: downloaded
          ? "It is on this computer. UA kept running."
          : "Runs in the background while UA keeps running. If you skip it, Switch downloads first.",
      },
      {
        state: notes,
        title: "Review the notes",
        detail: `${plural(review.releases.length, "release")} since UA ${current}${review.breaking_changes ? ", including breaking changes. Follow their migration steps." : "."}`,
      },
      { state: change, title: `Switch to UA ${target}`, detail: "Restarts UA, which interrupts work in progress. Your workspace and data stay." },
    ];
  }
  renderSteps(elements.runtimeSteps, steps);

  // Notes stay readable when a release cannot be installed yet.
  elements.updateReview.hidden = !review?.releases?.length || state === "pending";
  elements.updateNotes.textContent = (review?.releases ?? []).map(release =>
    `UA ${release.version}${release.prerelease ? " (pre-release)" : ""}${release.breaking_changes ? " — BREAKING CHANGES" : ""}\n${release.url}\n\n${release.notes || "No release notes provided."}`
  ).join("\n\n────────────────────\n\n");
  if (elements.updateAck.parentElement) elements.updateAck.parentElement.hidden = state !== "available";
  elements.runtimeActions.hidden = state !== "available";
  elements.downloadUpdate.hidden = downloaded;
  elements.downloadUpdate.textContent = downloadingUpdate ? `Downloading${percentText()}` : "Download";
}

function renderCatalog() {
  const review = latestUpdate;
  const catalog = review?.catalog ?? [];
  catalogButtons = [];
  elements.catalogNote.textContent = !review
    ? "Recent versions appear after a check."
    : review.update_pending
      ? "Recent versions show again once the pending update finishes."
      : catalog.length === 0
        ? "Recent versions could not be listed because the compatibility list could not be read. Choose Check now to try again."
        : "Choose a newer version to make it the update. Older versions are listed for reference only, because data a newer version changed may not work with them.";
  elements.runtimeCatalog.replaceChildren(...catalog.map(entry => catalogRow(entry, review)));
}

function catalogRow(entry, review) {
  const selected = entry.relation === "newer" && Boolean(review.review_id) &&
    entry.version === review.latest_version;
  const row = element("li", `catalog-row is-${entry.relation}${selected ? " is-selected" : ""}`);
  const facts = [
    releaseDate(entry.published_at),
    entry.listed ? "Works with this launcher" : "Unchecked",
    entry.prerelease ? "Pre-release" : "",
    entry.downloaded ? "Downloaded" : "",
  ].filter(Boolean);
  row.append(element("b", "catalog-version", entry.version), element("span", "catalog-facts", facts.join(" · ")));
  const action = element("span", "catalog-action");
  if (entry.relation === "running") {
    action.append(element("span", "mini-badge badge-running", "Running"));
  } else if (selected) {
    action.append(element("span", "mini-badge badge-selected", "Selected"));
  } else if (entry.relation === "newer") {
    action.append(catalogButton("Choose", "choose", `Make UA ${entry.version} the update`, () => chooseVersion(entry.version)));
  } else {
    action.append(element("span", "catalog-older", "Older than the running version"));
    if (entry.notes_url) {
      action.append(catalogButton("Notes", "notes", `Open the UA ${entry.version} release notes in your browser`, () => openReleasePage("runtime", entry.version)));
    }
  }
  row.append(action);
  return row;
}

function catalogButton(label, action, description, onClick) {
  const button = element("button", "button button-quiet button-compact", label);
  button.type = "button";
  button.dataset.action = action;
  button.setAttribute("aria-label", description);
  button.addEventListener("click", onClick);
  catalogButtons.push(button);
  return button;
}

function renderUpdatesCount() {
  const review = latestUpdate;
  const count = (review?.review_id || review?.update_pending ? 1 : 0) + (review?.desktop?.newer ? 1 : 0);
  elements.updatesCount.hidden = count === 0;
  elements.updatesCount.textContent = String(count);
  elements.openUpdates.setAttribute("aria-label", count ? `Updates, ${count} waiting for you` : "Updates");
}

// The badge shows the single most useful update item, in this order.
function badgeItem() {
  const review = latestUpdate;
  const target = review?.latest_version;
  if (downloadingUpdate) {
    return { kind: "downloading", key: `downloading:${target}`, label: `Downloading UA ${target}${percentText()}`, detail: "Keep working. Nothing restarts until you choose." };
  }
  if (review?.review_id && review.image_downloaded) {
    return { kind: "downloaded", key: `downloaded:${target}`, label: `UA ${target} is downloaded`, detail: "Review the notes and switch when your work is finished." };
  }
  if (review?.review_id) {
    return { kind: "available", key: `available:${target}`, label: `${review.listed === false ? "Unchecked " : ""}UA ${target} is available`, detail: "Open Updates to download it in the background." };
  }
  if (review?.desktop?.newer) {
    return { kind: "launcher", key: `launcher:${review.desktop.latest_version}`, label: `Launcher ${review.desktop.latest_version} is available`, detail: `You have ${review.desktop.current_version}. Universal Agents keeps running while you update.` };
  }
  return null;
}

function renderUpdateBadge() {
  const item = badgeItem();
  // A new stage shows again; a recheck of the same stage does not.
  if (item && item.key !== badgeKey) {
    badgeKey = item.key;
    badgeDone = false;
  }
  const show = item !== null && !badgeDone && dockerAvailable !== false && !updatesOpen() && !introOpen() &&
    !alwaysOnNoticeVisible();
  const wasHidden = elements.updateBadge.hidden !== false;
  elements.updateBadge.hidden = !show;
  if (!show) {
    stopBadgeTimer();
  } else if (wasHidden) {
    startBadgeTimer();
  }
  elements.updateBadge.className = `update-badge${item ? ` is-${item.kind}` : ""}${downloadPercent === null ? "" : " has-percent"}`;
  if (!item) return;
  elements.updateBadgeLabel.textContent = item.label;
  elements.updateBadgeDetail.textContent = item.detail;
  elements.updateBadgeRing.style?.setProperty("--badge-progress", `${downloadPercent ?? 0}%`);
}

function startBadgeTimer() {
  stopBadgeTimer();
  if (!badgeHeld) badgeTimer = setTimeout(() => hideBadge(), BADGE_VISIBLE_MS);
}

function stopBadgeTimer() {
  if (badgeTimer !== null) clearTimeout(badgeTimer);
  badgeTimer = null;
}

// Timed out, dismissed, or acted on: hidden until the next stage.
function hideBadge() {
  badgeDone = true;
  renderUpdateBadge();
}

function holdBadge(held) {
  badgeHeld = held;
  if (held) {
    stopBadgeTimer();
  } else if (elements.updateBadge.hidden === false) {
    startBadgeTimer();
  }
}

function renderUpdateCenter() {
  renderCheckedTime();
  renderLauncherLane();
  renderRuntimeLane();
  renderCatalog();
  renderUpdatesCount();
  renderUpdateBadge();
  updateActionAvailability();
}

// A new review starts unacknowledged with its release notes collapsed.
function resetReview() {
  elements.updateAck.checked = false;
  elements.updateNotesSection.open = false;
}

function renderUpdate(review) {
  // A finished download returns the same review; keep its acknowledgment
  // and leave the notes as the reader left them.
  if (!review.review_id || review.review_id !== latestUpdate?.review_id) resetReview();
  latestUpdate = review;
  lastCheckFailed = false;
  elements.desktopVersion.textContent = review.desktop_version;
  renderUpdateCenter();
}

async function checkUpdates() {
  // A background download holds the Docker lock; a concurrent release check
  // would only queue behind it.
  if (operationInFlight || checkingUpdates || downloadingUpdate || choosingVersion) return;
  checkingUpdates = true;
  latestUpdate = null;
  resetReview();
  renderUpdateCenter();
  try {
    const review = await invoke("check_runtime_updates");
    lastCheckedAt = Date.now();
    checkingUpdates = false;
    renderUpdate(review);
  } catch (error) {
    lastCheckFailed = true;
  } finally {
    checkingUpdates = false;
    renderUpdateCenter();
  }
}

// Make another newer version from the catalog the update. Download and Switch
// then use its review.
async function chooseVersion(version) {
  if (choosingVersion || downloadingUpdate || operationInFlight || checkingUpdates) return;
  clearError();
  choosingVersion = true;
  updateActionAvailability();
  try {
    renderUpdate(await invoke("review_runtime_version", { version }));
    // The pressed row was redrawn; continue at the lane's next action.
    (elements.downloadUpdate.hidden ? elements.updateAck : elements.downloadUpdate).focus?.();
  } catch (error) {
    showError(error);
  } finally {
    choosingVersion = false;
    renderUpdateCenter();
  }
}

async function openReleasePage(product, version) {
  clearError();
  try {
    await invoke("open_release_page", { product, version });
  } catch (error) {
    showError(error);
  }
}

async function installUpdate() {
  if (elements.installUpdate.disabled || !latestUpdate?.review_id) return;
  const reviewId = latestUpdate.review_id;
  const acknowledged = elements.updateAck.checked;
  // The restart shows in the header progress row and the status, so the
  // drawer steps aside.
  closeUpdates();
  clearError();
  setBusy(true);
  try {
    renderStatus(await invokeWithProgress("install_runtime_update", { reviewId, acknowledged }, "Preparing the reviewed runtime update…"));
  } catch (error) {
    showError(error);
  } finally {
    setBusy(false);
    await checkUpdates();
  }
}

// Download the reviewed image in the background. This never restarts UA, so it
// is safe to run while the user keeps working; switching stays a separate,
// explicitly acknowledged action.
async function downloadUpdate() {
  if (downloadingUpdate || operationInFlight || !latestUpdate?.review_id || latestUpdate.image_downloaded) return;
  const reviewId = latestUpdate.review_id;
  clearError();
  downloadingUpdate = true;
  downloadPercent = null;
  renderUpdateCenter();
  try {
    renderUpdate(await invokeWithProgress(
      "download_runtime_update",
      { reviewId },
      "Downloading the reviewed runtime image…",
      (update) => {
        if (Number.isFinite(update.current_bytes) && Number.isFinite(update.total_bytes) && update.total_bytes > 0) {
          downloadPercent = Math.min(100, Math.floor((update.current_bytes / update.total_bytes) * 100));
          renderRuntimeLane();
          renderUpdateBadge();
        }
      },
    ));
  } catch (error) {
    showError(error);
  } finally {
    downloadingUpdate = false;
    downloadPercent = null;
    renderUpdateCenter();
  }
}

elements.dockerInstall.addEventListener("click", () => invoke("open_docker_help", { target: "install" }).catch(showError));
elements.dockerGuide.addEventListener("click", () => invoke("open_docker_help", { target: "guide" }).catch(showError));
elements.alwaysOnGotIt.addEventListener("click", dismissAlwaysOnNotice);
// The same fixed guide as the Docker dialog; the renderer passes no address.
elements.alwaysOnGuide.addEventListener("click", () => invoke("open_docker_help", { target: "guide" }).catch(showError));
elements.dockerProceed.addEventListener("click", () => {
  // Proceeding is not a claim that Docker works: it reveals the launcher with
  // runtime controls disabled and keeps the 30-second check running.
  dockerGateDismissed = true;
  renderDockerGate();
  renderUpdateBadge();
  updateActionAvailability();
});
elements.openUpdates.addEventListener("click", openUpdates);
elements.updateBadgeOpen.addEventListener("click", openUpdates);
elements.updateBadgeDismiss.addEventListener("click", () => {
  badgeHeld = false;
  hideBadge();
  // The dismissed control is gone; the header button leads to the same place.
  elements.openUpdates.focus?.();
});
elements.updateBadge.addEventListener("pointerenter", () => holdBadge(true));
elements.updateBadge.addEventListener("pointerleave", () => holdBadge(false));
elements.updateBadge.addEventListener("focusin", () => holdBadge(true));
elements.updateBadge.addEventListener("focusout", () => holdBadge(false));
elements.closeUpdates.addEventListener("click", () => closeUpdates());
// A click on the dimmed backdrop, outside the drawer itself, closes it.
elements.updatesDrawer.addEventListener("click", (event) => {
  if (event.target === elements.updatesDrawer) closeUpdates();
});
elements.downloadUpdate.addEventListener("click", downloadUpdate);
elements.openLauncherDownload.addEventListener("click", () => {
  const version = latestUpdate?.desktop?.latest_version;
  if (version) openReleasePage("launcher", version);
});
// The green recovery confirmation is a one-time acknowledgment: the first real
// interaction clears it and normal status colors take over. The listeners stay
// registered because an interaction before recovery must not consume the
// acknowledgment.
for (const type of ["pointerdown", "keydown"]) {
  document.addEventListener(type, acknowledgeDockerRecovery, { capture: true });
}
// Show what waited for the intro: the always-on notice first, then the
// prerequisite dialog (or its banner).
intro?.onDismiss(() => {
  showAlwaysOnNoticeIfDue();
  renderDockerGate();
  renderUpdateBadge();
});
// Without an intro (it failed to start, or was already dismissed) the notice
// shows at once.
showAlwaysOnNoticeIfDue();
loadKeepAwakeHint();

elements.checkUpdates.addEventListener("click", () => {
  clearError();
  return checkUpdates();
});
elements.updateAck.addEventListener("change", () => {
  renderUpdateCenter();
  updateActionAvailability();
});
elements.installUpdate.addEventListener("click", installUpdate);
invoke("desktop_version").then(version => { elements.desktopVersion.textContent = version; }).catch(showError);
checkUpdates();
setInterval(() => { if (!document.hidden) checkUpdates(); }, 6 * 60 * 60 * 1000);
