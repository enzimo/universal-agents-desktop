//! Native launcher UI fixture. No production controller, Docker, credentials or autostart.
#[cfg(not(debug_assertions))]
compile_error!("The UI driver must not be built in release mode.");

use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tauri::ipc::InvokeBody;

/// Marks fixture errors as app-written guidance, so the launcher shows them
/// word for word (`ua_desktop_core::guidance::MARKER`).
const GUIDANCE: char = '\u{1f}';
/// The newer versions the fixture offers, newest first. All are labelled as
/// test versions; none is a published or installable runtime.
const BETA: &str = "99.1.0-beta.1";
const OFFERED: &str = "99.0.0-test";
const OLDER_NEWER: &str = "98.0.0-test";
const LAUNCHER: &str = "99.9.9";

fn review_id(target: &str) -> String {
    format!("ui-fixture-review-{target}")
}

/// The update review for `target`, with the catalog and launcher release the
/// real check returns.
fn fixture_review(running: &str, target: &str, unlisted: bool, downloaded: bool) -> Value {
    let release = |version: &str, breaking: bool, notes: &str| {
        json!({"version": version, "url": format!("https://example.invalid/{version}"),
            "breaking_changes": breaking, "prerelease": version == BETA, "notes": notes,
            "published_at": "2026-10-13T08:00:00Z"})
    };
    let all = [
        release(BETA, false, "UI TEST ONLY: beta"),
        release(OFFERED, true, "UI TEST ONLY: <b>must remain plain text</b>"),
        release(OLDER_NEWER, false, "UI TEST ONLY: older newer release"),
    ];
    let skip = [BETA, OFFERED, OLDER_NEWER]
        .iter()
        .position(|version| *version == target)
        .expect("fixture target");
    let releases: Vec<Value> = all[skip..].to_vec();
    let entry = |version: &str, relation: &str, listed: bool, recommended: bool| {
        json!({"version": version, "published_at": "2026-10-13T08:00:00Z", "listed": listed,
            "prerelease": version == BETA, "relation": relation, "recommended": recommended,
            "notes_url": format!("https://example.invalid/{version}"),
            "image": format!("example.invalid/ua@sha256:{version}"),
            "downloaded": relation == "running" || (downloaded && version == target)})
    };
    let mut catalog = Vec::new();
    if unlisted {
        catalog.push(entry(BETA, "newer", false, true));
    }
    catalog.extend([
        entry(OFFERED, "newer", true, !unlisted),
        entry(OLDER_NEWER, "newer", true, false),
        entry(running, "running", true, false),
        entry("0.0.1-test", "older", true, false),
    ]);
    json!({
        "desktop_version": env!("CARGO_PKG_VERSION"),
        "current_version": running, "latest_version": target,
        "update_pending": false,
        "breaking_changes": releases.iter().any(|release| release["breaking_changes"] == true),
        "image_downloaded": downloaded, "listed": target != BETA, "prerelease": target == BETA,
        "newer_unlisted": if unlisted { Value::Null } else { json!(BETA) },
        "blocked_reason": null, "review_id": review_id(target),
        "releases": releases, "catalog": catalog,
        "desktop": {"current_version": env!("CARGO_PKG_VERSION"), "latest_version": LAUNCHER,
            "newer": true, "published_at": "2026-10-13T08:00:00Z", "error": null}
    })
}

fn main() {
    let lock = ua_desktop_core::RuntimeLock::bundled().expect("reviewed runtime lock");
    let phase = Arc::new(Mutex::new("stopped"));
    let logs = Arc::new(Mutex::new(0_u32));
    // The launcher shows backend errors only as a fixed notice, so an accepted
    // update is reported through the status explanation instead.
    let update_acknowledged = Arc::new(Mutex::new(false));
    let offer_unlisted = Arc::new(Mutex::new(false));
    // The update target (the offered version or one chosen from the catalog)
    // and whether its image was "downloaded".
    let target = Arc::new(Mutex::new((OFFERED, false)));
    let mut context = tauri::generate_context!();
    context.config_mut().identifier = "com.universalagents.desktop.ui-test".into();
    context.config_mut().app.windows[0].title = "UA Launcher UI Test — simulated runtime".into();
    // The fixture skips the production fit-to-screen step, so it keeps the
    // previous height and the suite's geometry is the same on every display.
    context.config_mut().app.windows[0].height = 920.0;
    tauri::Builder::default()
        .plugin(tauri_plugin_wdio_webdriver::init())
        .invoke_handler(move |invoke| {
            let command = invoke.message.command();
            let args = match invoke.message.payload() {
                InvokeBody::Json(value) => value.clone(),
                _ => Value::Null,
            };
            let mut phase = phase.lock().unwrap();
            let mut update_acknowledged = update_acknowledged.lock().unwrap();
            let mut offer_unlisted = offer_unlisted.lock().unwrap();
            let mut target = target.lock().unwrap();
            let current_review = review_id(target.0);
            match command {
                "start_runtime" | "restart_runtime" => *phase = "ready",
                "stop_runtime" => *phase = "stopped",
                "set_unlisted_runtime_offers" => *offer_unlisted = args["enabled"] == true,
                "check_runtime_updates" => {
                    *target = (if *offer_unlisted { BETA } else { OFFERED }, false);
                }
                "review_runtime_version" => {
                    let choices: &[&'static str] = if *offer_unlisted {
                        &[BETA, OFFERED, OLDER_NEWER]
                    } else {
                        &[OFFERED, OLDER_NEWER]
                    };
                    match choices.iter().find(|version| args["version"] == **version) {
                        Some(version) => *target = (*version, false),
                        None => {
                            invoke.resolver.reject(format!(
                                "{GUIDANCE}Only a newer runtime can be installed."
                            ));
                            return true;
                        }
                    }
                }
                "download_runtime_update" => {
                    if args["reviewId"] != current_review.as_str() {
                        invoke.resolver.reject("UI fixture: download rejected without a current review.");
                        return true;
                    }
                    // No image or container is actually changed by the fixture.
                    target.1 = true;
                }
                "install_runtime_update" => {
                    if args["acknowledged"] != true || args["reviewId"] != current_review.as_str() {
                        invoke
                            .resolver
                            .reject("UI fixture: update rejected without review acknowledgment.");
                        return true;
                    }
                    *update_acknowledged = true;
                }
                _ => {}
            }
            let detail = if *update_acknowledged {
                "UI fixture: acknowledged update reached the backend; no image was changed."
            } else {
                "UI fixture: no real runtime is running."
            };
            let response = match command {
                "desktop_version" => json!(env!("CARGO_PKG_VERSION")),
                "runtime_status" | "start_runtime" | "restart_runtime" | "stop_runtime"
                | "install_runtime_update" => json!({
                    "phase": *phase, "detail": detail,
                    "ua_version": lock.ua_version,
                    "web_url": "http://127.0.0.1:18090/webchat",
                    "docs_url": "http://127.0.0.1:18090/docs/",
                    "data_dir": "UI fixture — no persistent data"
                }),
                "integration_status" | "set_unlisted_runtime_offers" => json!({
                    "workspace_dir": "UI fixture workspace", "workspace_available": true,
                    "workspace_is_external": false, "workspace_issue": null,
                    "openai_subscription_state": "disconnected",
                    "openai_subscription_detail": "UI fixture: no credentials are read.",
                    "pending_codex_auth_file": null,
                    "refresh_external_skills_on_start": true,
                    "offer_unlisted_runtimes": *offer_unlisted
                }),
                "autostart_status" => json!(false),
                "docker_prerequisite" => json!({
                    "available": true, "installed": true,
                    "detail": "UI fixture: Docker is simulated as available.",
                    "setup_hint": "UI fixture setup hint",
                    "install_url": "https://docs.docker.com/desktop/setup/install/mac-install/",
                    "setup_guide_url": "https://example.invalid/setup-guide.html"
                }),
                "open_docker_help" => {
                    invoke.resolver.reject("UI fixture: opening Docker help is not exercised.");
                    return true;
                }
                "check_runtime_updates" | "review_runtime_version" | "download_runtime_update" => {
                    fixture_review(&lock.ua_version, target.0, *offer_unlisted, target.1)
                }
                // Report which page would open, without opening a browser.
                "open_release_page" => {
                    invoke.resolver.reject(format!(
                        "{GUIDANCE}UI fixture: would open the {} {} release page.",
                        args["product"].as_str().unwrap_or("?"),
                        args["version"].as_str().unwrap_or("?")
                    ));
                    return true;
                }
                "open_ua_repository" => {
                    invoke.resolver.reject(format!(
                        "{GUIDANCE}UI fixture: would open the Universal Agents project page."
                    ));
                    return true;
                }
                "open_workspace_folder" | "open_docs_browser"
                | "open_webchat_browser" | "open_webchat_window" => {
                    invoke.resolver.reject("UI fixture: opening windows and folders is not exercised.");
                    return true;
                }
                "stop_runtime_and_quit" => {
                    // Report the stopped runtime as the shipping command does,
                    // without exiting the fixture process.
                    *phase = "stopped";
                    json!({
                        "phase": *phase, "detail": "UI fixture: runtime stop requested; the fixture does not quit.",
                        "ua_version": lock.ua_version,
                        "web_url": "http://127.0.0.1:18090/webchat",
                        "docs_url": "http://127.0.0.1:18090/docs/",
                        "data_dir": "UI fixture — no persistent data"
                    })
                }
                "quit_desktop_app" => {
                    invoke.resolver.reject("UI fixture: quitting is not exercised.");
                    return true;
                }
                "set_autostart" => args["enabled"].clone(),
                "recent_logs" => {
                    let mut count = logs.lock().unwrap();
                    *count += 1;
                    json!(format!("Simulated log refresh {count}"))
                }
                _ => {
                    invoke.resolver.reject(format!("UI fixture: {command} is not enabled; no host action was taken."));
                    return true;
                }
            };
            invoke.resolver.resolve(response);
            true
        })
        .run(context)
        .expect("native UI test fixture");
}
