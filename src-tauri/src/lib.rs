mod controller;
mod downloads;
pub mod webview;
mod window_size;

use std::sync::Arc;

use controller::{DesktopController, IntegrationStatus};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Runtime, State, Url, WebviewWindow, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as AutostartManagerExt};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use ua_desktop_core::{RuntimePhase, RuntimeStatus, guidance};

const CONTROL_WINDOW: &str = "control";
const WEBCHAT_WINDOW: &str = "webchat";

struct AppState {
    controller: Arc<DesktopController>,
    startup: Arc<ua_desktop_core::progress::StartupTracker>,
}

#[tauri::command]
fn startup_progress(state: State<'_, AppState>) -> ua_desktop_core::progress::StartupProgress {
    state.startup.snapshot()
}

#[tauri::command]
async fn registry_access(
    app: AppHandle,
    state: State<'_, AppState>,
    method: String,
    username: String,
    token: String,
) -> Result<RuntimeStatus, String> {
    let controller = Arc::clone(&state.controller);
    run_with_progress(app, move || {
        controller.registry_access(&method, &username, &token)
    })
    .await
}

#[tauri::command]
async fn begin_github_login(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<RuntimeStatus, String> {
    let controller = Arc::clone(&state.controller);
    run_with_progress(app, move || {
        let status = controller.runtime_status()?;
        if status.phase == RuntimePhase::DockerUnavailable {
            return Err(status.detail);
        }
        ua_desktop_core::progress::report("Complete GitHub sign-in in the terminal and browser. Installation will continue automatically.");
        ua_desktop_core::registry::complete_gh_login(&ua_desktop_core::registry::ProcessRegistry)
            .map_err(guidance::guidance)?;
        controller.registry_access("gh", "", "")
    }).await
}

#[tauri::command]
async fn check_runtime_updates(
    state: State<'_, AppState>,
) -> Result<ua_desktop_core::updates::UpdateReview, String> {
    let controller = Arc::clone(&state.controller);
    run_blocking(move || controller.check_runtime_updates()).await
}

#[tauri::command]
async fn docker_prerequisite(
    state: State<'_, AppState>,
) -> Result<ua_desktop_core::DockerPrerequisite, String> {
    let controller = Arc::clone(&state.controller);
    run_blocking(move || controller.docker_prerequisite()).await
}

#[tauri::command]
async fn download_runtime_update(
    app: AppHandle,
    state: State<'_, AppState>,
    review_id: String,
) -> Result<ua_desktop_core::updates::UpdateReview, String> {
    let controller = Arc::clone(&state.controller);
    run_with_progress(app, move || controller.download_runtime_update(&review_id)).await
}

#[tauri::command]
async fn install_runtime_update(
    app: AppHandle,
    state: State<'_, AppState>,
    review_id: String,
    acknowledged: bool,
) -> Result<RuntimeStatus, String> {
    let controller = Arc::clone(&state.controller);
    run_with_progress(app, move || {
        controller.install_runtime_update(&review_id, acknowledged)
    })
    .await
}

/// Make one of the newer UA versions from the last check the update target.
#[tauri::command]
async fn review_runtime_version(
    state: State<'_, AppState>,
    version: String,
) -> Result<ua_desktop_core::updates::UpdateReview, String> {
    let controller = Arc::clone(&state.controller);
    run_blocking(move || controller.review_runtime_version(&version)).await
}

/// Open the GitHub release page of the newest launcher or of a UA version the
/// last check returned. The renderer names a product and version; the address
/// comes from the check, so no arbitrary URL can be opened.
#[tauri::command]
async fn open_release_page(
    app: AppHandle,
    state: State<'_, AppState>,
    product: String,
    version: String,
) -> Result<(), String> {
    let controller = Arc::clone(&state.controller);
    let url = run_blocking(move || controller.release_page(&product, &version)).await?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| error.to_string())
}

/// Open the Universal Agents project page on GitHub. The address is fixed.
#[tauri::command]
async fn open_ua_repository(app: AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(ua_desktop_core::updates::ua_repository_url(), None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn desktop_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The sleep setting to change on this computer, for the first-run notice
/// that agents need a computer that stays on.
#[tauri::command]
fn keep_awake_hint() -> &'static str {
    ua_desktop_core::platform::Host::current().keep_awake_hint()
}

#[tauri::command]
async fn runtime_status(state: State<'_, AppState>) -> Result<RuntimeStatus, String> {
    let controller = Arc::clone(&state.controller);
    run_blocking(move || controller.runtime_status()).await
}

#[tauri::command]
async fn start_runtime(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<RuntimeStatus, String> {
    let controller = Arc::clone(&state.controller);
    run_with_progress(app, move || controller.start_runtime()).await
}

#[tauri::command]
async fn stop_runtime(app: AppHandle, state: State<'_, AppState>) -> Result<RuntimeStatus, String> {
    let controller = Arc::clone(&state.controller);
    run_with_progress(app, move || controller.stop_runtime()).await
}

#[tauri::command]
async fn restart_runtime(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<RuntimeStatus, String> {
    let controller = Arc::clone(&state.controller);
    run_with_progress(app, move || controller.restart_runtime()).await
}

#[tauri::command]
async fn recent_logs(state: State<'_, AppState>, tail: usize) -> Result<String, String> {
    let controller = Arc::clone(&state.controller);
    run_blocking(move || controller.recent_logs(tail)).await
}

#[tauri::command]
async fn download_diagnostics(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let dialog_app = app.clone();
    let selected = run_blocking(move || {
        Ok(dialog_app
            .dialog()
            .file()
            .set_file_name("universal-agents-diagnostics.txt")
            .add_filter("Text log", &["txt"])
            .blocking_save_file())
    })
    .await?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|_| "The selected diagnostics destination is not a local file.".to_owned())?;
    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|error| error.to_string())?;
    let controller = Arc::clone(&state.controller);
    let saved_path = path.clone();
    run_blocking(move || {
        let status = controller
            .runtime_status()
            .map(|status| format!("{status:#?}"))
            .unwrap_or_else(|error| format!("Status error: {error}"));
        let runtime_logs = controller
            .recent_logs(1_000)
            .unwrap_or_else(|error| format!("Runtime logs unavailable: {error}"));
        let desktop_logs = read_desktop_logs(&log_dir);
        let report = format!(
            "Universal Agents Desktop diagnostics\nDesktop version: {}\nOS: {}\nArchitecture: {}\n\nRuntime status\n{}\n\nDesktop logs\n{}\n\nRuntime logs\n{}\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            status,
            desktop_logs,
            runtime_logs
        );
        std::fs::write(&saved_path, report).map_err(|error| error.to_string())?;
        Ok(())
    })
    .await?;
    Ok(Some(path.display().to_string()))
}

fn read_desktop_logs(log_dir: &std::path::Path) -> String {
    const MAX_EXPORTED_DESKTOP_LOG_BYTES: usize = 1024 * 1024;
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return "Desktop log directory is unavailable.".to_owned();
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|value| value == "log"))
        .collect();
    paths.sort();
    let mut output = String::new();
    for path in paths {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let start = bytes.len().saturating_sub(MAX_EXPORTED_DESKTOP_LOG_BYTES);
        output.push_str(&format!("\n--- {} ---\n", path.display()));
        output.push_str(&String::from_utf8_lossy(&bytes[start..]));
    }
    if output.is_empty() {
        "No desktop log entries are available.".to_owned()
    } else {
        output
    }
}

#[tauri::command]
async fn integration_status(state: State<'_, AppState>) -> Result<IntegrationStatus, String> {
    let controller = Arc::clone(&state.controller);
    run_blocking(move || controller.integration_status()).await
}

#[tauri::command]
async fn set_unlisted_runtime_offers(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<IntegrationStatus, String> {
    let controller = Arc::clone(&state.controller);
    run_blocking(move || controller.set_unlisted_runtime_offers(enabled)).await
}

#[tauri::command]
async fn set_external_skill_refresh(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<IntegrationStatus, String> {
    let controller = Arc::clone(&state.controller);
    run_blocking(move || controller.set_external_skill_refresh(enabled)).await
}

#[tauri::command]
async fn choose_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<IntegrationStatus>, String> {
    let selected = app
        .dialog()
        .file()
        .set_title("Choose an agent workspace")
        .blocking_pick_folder();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected.into_path().map_err(|error| error.to_string())?;
    let controller = Arc::clone(&state.controller);
    run_blocking(move || controller.select_workspace(path).map(Some)).await
}

#[tauri::command]
async fn use_internal_workspace(state: State<'_, AppState>) -> Result<IntegrationStatus, String> {
    let controller = Arc::clone(&state.controller);
    run_blocking(move || controller.use_internal_workspace()).await
}

#[tauri::command]
async fn connect_openai_subscription(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<IntegrationStatus>, String> {
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Choose Codex auth.json")
        .add_filter("Codex credentials", &["json"]);
    if let Ok(home_dir) = app.path().home_dir() {
        let codex_home = home_dir.join(".codex");
        if codex_home.is_dir() {
            dialog = dialog.set_directory(codex_home);
        }
    }
    let Some(selected) = dialog.blocking_pick_file() else {
        return Ok(None);
    };
    let auth_file = selected.into_path().map_err(|error| error.to_string())?;
    let controller = Arc::clone(&state.controller);
    run_blocking(move || controller.connect_openai_subscription(auth_file).map(Some)).await
}

#[tauri::command]
async fn finish_openai_subscription(
    state: State<'_, AppState>,
) -> Result<IntegrationStatus, String> {
    let controller = Arc::clone(&state.controller);
    run_blocking(move || controller.finish_openai_subscription()).await
}

#[tauri::command]
async fn cancel_openai_subscription_import(
    state: State<'_, AppState>,
) -> Result<IntegrationStatus, String> {
    let controller = Arc::clone(&state.controller);
    run_blocking(move || controller.cancel_openai_subscription_import()).await
}

/// Resolve the launch URL once the runtime reports a ready phase.
///
/// The readiness probe runs Docker and opens a loopback socket, so it must stay
/// off the UI thread on every platform.
fn ready_web_url(controller: &DesktopController) -> Result<String, String> {
    let status = controller.runtime_status()?;
    if !matches!(status.phase, RuntimePhase::Ready | RuntimePhase::Onboarding) {
        return Err(format!(
            "The Web UI is not ready yet. Current state: {}",
            status.detail
        ));
    }
    controller.web_url()
}

/// The ready Web UI URL plus the broker port its credential forms use.
fn ready_webchat(controller: &DesktopController) -> Result<(String, u16), String> {
    Ok((ready_web_url(controller)?, controller.broker_port()?))
}

/// Open the embedded Web UI.
///
/// Tauri runs synchronous commands and menu handlers on the Windows message
/// thread, and creating a WebView2 window there re-enters the message loop
/// (https://github.com/tauri-apps/wry/issues/583). That reentrancy leaves the
/// embedded window blank and stops the message thread from answering any other
/// work, so this command is async and both the readiness probe and window
/// creation happen on worker threads.
#[tauri::command]
async fn open_webchat_window(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let controller = Arc::clone(&state.controller);
    let (url, broker_port) = run_blocking(move || ready_webchat(&controller)).await?;
    show_webchat(&app, url, broker_port)
}

#[tauri::command]
async fn open_webchat_browser(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let controller = Arc::clone(&state.controller);
    // Same readiness rule as the embedded window, so a browser is never sent
    // to a page that cannot load.
    let url = run_blocking(move || ready_web_url(&controller)).await?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn open_docs_browser(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let controller = Arc::clone(&state.controller);
    let url = run_blocking(move || controller.docs_url()).await?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Open one of the fixed Docker prerequisite links in the default browser.
///
/// `target` selects between the per-OS Docker install page and the shared setup
/// guide. The renderer cannot pass an arbitrary URL, so this adds no new
/// navigation capability beyond two reviewed addresses.
#[tauri::command]
async fn open_docker_help(app: AppHandle, target: String) -> Result<(), String> {
    let url = match target.as_str() {
        "install" => ua_desktop_core::platform::Host::current()
            .docker_install_url()
            .to_owned(),
        "guide" => ua_desktop_core::platform::DOCKER_SETUP_GUIDE_URL.to_owned(),
        _ => return Err("Unknown Docker help destination.".to_owned()),
    };
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| error.to_string())
}

/// Reveal the effective agent workspace in the platform file manager.
///
/// An unavailable saved folder reports an actionable error instead of opening
/// a different location, matching the runtime's refusal to switch workspaces
/// silently.
#[tauri::command]
async fn open_workspace_folder(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let controller = Arc::clone(&state.controller);
    let workspace = run_blocking(move || controller.workspace_dir()).await?;
    if !workspace.is_dir() {
        return Err(guidance::guidance(
            "The agent workspace folder is not available yet. Start Universal Agents so it can create the app workspace, or choose an existing folder.",
        ));
    }
    app.opener()
        .open_path(workspace.display().to_string(), None::<&str>)
        .map_err(|error| error.to_string())
}

/// Quit the desktop host and leave the runtime running.
///
/// `Quit Desktop App` never stops Universal Agents, so the wording and the
/// behavior stay explicit in both the Status window and the tray menu.
#[tauri::command]
fn quit_desktop_app(app: AppHandle) {
    app.exit(0);
}

/// Stop Universal Agents and then quit the desktop host.
///
/// Stopping preserves `/data` and the selected workspace; only the container
/// process ends. Failures keep the app open so the user can see the error.
#[tauri::command]
async fn stop_runtime_and_quit(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<RuntimeStatus, String> {
    let controller = Arc::clone(&state.controller);
    let status = run_with_progress(app.clone(), move || controller.stop_runtime()).await?;
    // Report the stopped runtime before exiting so the caller sees the result
    // rather than a dropped response.
    app.exit(0);
    Ok(status)
}

#[tauri::command]
async fn autostart_status(app: AppHandle) -> Result<bool, String> {
    read_autostart(app).await
}

#[tauri::command]
async fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let write_app = app.clone();
    run_blocking(move || write_autostart(&write_app, enabled)).await?;
    read_autostart(app).await
}

async fn read_autostart(app: AppHandle) -> Result<bool, String> {
    run_blocking(move || {
        app.autolaunch()
            .is_enabled()
            .map_err(|error| error.to_string())
    })
    .await
}

fn write_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if enabled {
        app.autolaunch()
            .enable()
            .map_err(|error| error.to_string())?;
    } else {
        app.autolaunch()
            .disable()
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

async fn run_with_progress<T, F>(app: AppHandle, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    run_blocking(move || {
        ua_desktop_core::progress::with_reporter(
            move |message| {
                let _ = app.emit_to(CONTROL_WINDOW, "runtime-progress", message);
            },
            operation,
        )
    })
    .await
}

async fn run_blocking<T, F>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    let result = tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| format!("desktop worker failed: {error}"))?
        .map_err(|error| error.to_string());
    if let Err(error) = &result {
        log::error!("desktop operation failed: {}", guidance::plain(error));
    }
    result
}

fn show_webchat(app: &AppHandle, url: String, broker_port: u16) -> Result<(), String> {
    let target = Url::parse(&url).map_err(|e| e.to_string())?;
    if let Some(window) = app.get_webview_window(WEBCHAT_WINDOW) {
        if webview::needs_navigation(&window.url().map_err(|e| e.to_string())?, &target) {
            window.navigate(target).map_err(|e| e.to_string())?;
        }
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }

    let window = webview::build_runtime_window(app, WEBCHAT_WINDOW, target, broker_port)?;
    hide_on_close(&window);
    Ok(())
}

fn hide_on_close<R: Runtime>(window: &WebviewWindow<R>) {
    let window_to_hide = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Err(error) = window_to_hide.hide() {
                log::warn!("failed to hide window on close: {error}");
            }
        }
    });
}

fn show_control<R: Runtime>(app: &AppHandle<R>) {
    // After a failed start only the error dialog is shown; the status window
    // cannot work without the controller, so a second launch must not reveal it.
    if app.try_state::<AppState>().is_none() {
        return;
    }
    if let Some(window) = app.get_webview_window(CONTROL_WINDOW) {
        if let Err(error) = window.show().and_then(|_| window.set_focus()) {
            log::warn!("failed to show desktop control window: {error}");
        }
    }
}

fn start_in_background(app: &AppHandle) {
    let controller = Arc::clone(&app.state::<AppState>().controller);
    let startup = Arc::clone(&app.state::<AppState>().startup);
    tauri::async_runtime::spawn(async move {
        let reporter = Arc::clone(&startup);
        let result = run_blocking(move || {
            ua_desktop_core::progress::with_reporter(
                move |update| reporter.report(update),
                || controller.start_runtime(),
            )
        })
        .await;
        match result {
            Ok(status) => {
                log::info!("startup runtime check finished: {:?}", status.phase);
                startup.finish(Ok(status.detail));
            }
            Err(error) => {
                log::warn!(
                    "automatic runtime start failed: {}",
                    guidance::plain(&error)
                );
                startup.finish(Err(error));
            }
        }
    });
}

fn run_tray_lifecycle(app: &AppHandle, action: &'static str) {
    let controller = Arc::clone(&app.state::<AppState>().controller);
    tauri::async_runtime::spawn(async move {
        let result = tauri::async_runtime::spawn_blocking(move || match action {
            "start" => controller.start_runtime(),
            "restart" => controller.restart_runtime(),
            "stop" => controller.stop_runtime(),
            _ => unreachable!("tray action is defined by the application"),
        })
        .await;
        match result {
            Ok(Ok(status)) => log::info!("tray {action} finished: {:?}", status.phase),
            Ok(Err(error)) => {
                log::warn!("tray {action} failed: {}", guidance::plain(&error))
            }
            Err(error) => log::error!("tray {action} worker failed: {error}"),
        }
    });
}

/// Stop Universal Agents and then quit, matching the Status window action.
fn run_tray_stop_and_quit(app: &AppHandle) {
    let quit_app = app.clone();
    let controller = Arc::clone(&app.state::<AppState>().controller);
    tauri::async_runtime::spawn(async move {
        let result = tauri::async_runtime::spawn_blocking(move || controller.stop_runtime()).await;
        match result {
            Ok(Ok(status)) => {
                log::info!("tray stop-and-quit finished: {:?}", status.phase);
                quit_app.exit(0);
            }
            Ok(Err(error)) => {
                log::warn!("tray stop-and-quit failed: {}", guidance::plain(&error))
            }
            Err(error) => log::error!("tray stop-and-quit worker failed: {error}"),
        }
    });
}

/// Open the embedded Web UI from a tray or menu handler without blocking the
/// Windows message thread. Any failure falls back to showing the Status window.
fn open_webchat_from_tray(app: &AppHandle) {
    let open_app = app.clone();
    let controller = Arc::clone(&app.state::<AppState>().controller);
    tauri::async_runtime::spawn(async move {
        let target = tauri::async_runtime::spawn_blocking(move || ready_webchat(&controller))
            .await
            .map_err(|error| format!("desktop worker failed: {error}"))
            .and_then(|result| result.map_err(|error| error.to_string()));
        match target {
            Ok((url, broker_port)) => {
                if let Err(error) = show_webchat(&open_app, url, broker_port) {
                    log::warn!("failed to open embedded Web UI: {error}");
                    show_control(&open_app);
                }
            }
            Err(error) => {
                log::warn!("failed to resolve the runtime Web UI URL: {error}");
                show_control(&open_app);
            }
        }
    });
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open_web = MenuItem::with_id(app, "open-web", "Open Universal Agents", true, None::<&str>)?;
    let show_status = MenuItem::with_id(app, "show-status", "Status & Logs", true, None::<&str>)?;
    let start = MenuItem::with_id(app, "start", "Start Runtime", true, None::<&str>)?;
    let restart = MenuItem::with_id(app, "restart", "Restart Runtime", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop Runtime", true, None::<&str>)?;
    let stop_and_quit =
        MenuItem::with_id(app, "stop-and-quit", "Stop UA and Quit", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Desktop App", true, None::<&str>)?;
    let separator_one = PredefinedMenuItem::separator(app)?;
    let separator_two = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &open_web,
            &show_status,
            &separator_one,
            &start,
            &restart,
            &stop,
            &stop_and_quit,
            &separator_two,
            &quit,
        ],
    )?;

    let tray_app = app.clone();
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Universal Agents")
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            // Menu handlers run on the Windows message thread, so the readiness
            // probe and window creation are moved to a worker thread for the same
            // reason as the open_webchat_window command.
            "open-web" => open_webchat_from_tray(app),
            "show-status" => show_control(app),
            "start" => run_tray_lifecycle(app, "start"),
            "restart" => run_tray_lifecycle(app, "restart"),
            "stop" => run_tray_lifecycle(app, "stop"),
            "stop-and-quit" => run_tray_stop_and_quit(app),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = tray_app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn start_desktop(
    app: &mut tauri::App,
    runtime_lock: ua_desktop_core::RuntimeLock,
) -> Result<(), String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("The desktop app could not find its app-data folder: {error}"))?;
    let data_dir = app_data_dir.join("ua-data");
    let controller = DesktopController::new(
        runtime_lock,
        data_dir,
        app_data_dir.join("desktop-settings.json"),
    )?;
    app.manage(AppState {
        controller: Arc::new(controller),
        startup: Arc::default(),
    });

    let control = app
        .get_webview_window(CONTROL_WINDOW)
        .expect("control window is declared in tauri.conf.json");
    window_size::fit_to_screen(&control);
    hide_on_close(&control);
    build_tray(app.handle())
        .map_err(|error| format!("The menu bar icon could not be created: {error}"))?;
    start_in_background(app.handle());

    if std::env::args().any(|argument| argument == "--background") {
        control
            .hide()
            .map_err(|error| format!("The status window could not be hidden: {error}"))?;
    }
    Ok(())
}

/// Explains a failed start in a native dialog, then exits once it is dismissed.
/// The status window stays hidden because its commands need the controller that
/// failed to start.
fn report_startup_failure(app: &tauri::App, error: String) {
    let error = guidance::plain(&error);
    log::error!("desktop app could not start: {error}");
    if let Some(control) = app.get_webview_window(CONTROL_WINDOW) {
        if let Err(hide_error) = control.hide() {
            log::warn!("failed to hide the status window after a failed start: {hide_error}");
        }
    }
    let exit_app = app.handle().clone();
    app.dialog()
        .message(error)
        .title("Universal Agents can't start")
        .kind(MessageDialogKind::Error)
        .show(move |_| exit_app.exit(1));
}

pub fn run() {
    let runtime_lock = ua_desktop_core::RuntimeLock::bundled()
        .expect("runtime-lock.json must satisfy the desktop host contract");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_control(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            registry_access,
            begin_github_login,
            runtime_status,
            desktop_version,
            keep_awake_hint,
            check_runtime_updates,
            download_runtime_update,
            install_runtime_update,
            review_runtime_version,
            open_release_page,
            open_ua_repository,
            docker_prerequisite,
            start_runtime,
            stop_runtime,
            restart_runtime,
            recent_logs,
            download_diagnostics,
            integration_status,
            set_external_skill_refresh,
            set_unlisted_runtime_offers,
            choose_workspace,
            use_internal_workspace,
            connect_openai_subscription,
            finish_openai_subscription,
            cancel_openai_subscription_import,
            open_webchat_window,
            open_webchat_browser,
            open_docs_browser,
            open_docker_help,
            open_workspace_folder,
            startup_progress,
            quit_desktop_app,
            stop_runtime_and_quit,
            autostart_status,
            set_autostart
        ])
        .setup(move |app| {
            // An error returned from this hook makes Tauri panic inside the macOS
            // launch callback, which cannot unwind, so the app aborts with a crash
            // report and the user never sees why. Startup errors are shown in a
            // native dialog instead, and the app exits once it is dismissed.
            if let Err(error) = start_desktop(app, runtime_lock) {
                report_startup_failure(app, error);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Universal Agents Desktop");
}

#[cfg(test)]
mod tests {
    use super::read_desktop_logs;

    #[test]
    fn diagnostics_include_desktop_log_files_only() {
        let directory = tempfile::tempdir().expect("temporary log directory");
        std::fs::write(
            directory.path().join("desktop.log"),
            "exact desktop failure",
        )
        .expect("write desktop log");
        std::fs::write(directory.path().join("unrelated.txt"), "do not export")
            .expect("write unrelated file");

        let report = read_desktop_logs(directory.path());

        assert!(report.contains("desktop.log"));
        assert!(report.contains("exact desktop failure"));
        assert!(!report.contains("do not export"));
    }
}
