//! Native desktop integration check: cargo run -p universal-agents-desktop --example popup_smoke
//! Uses only temporary loopback servers; never starts Docker or reads desktop settings.
//! External links go to a recording hook instead of the system browser.
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tauri::{Manager, Url};

/// Result bits: same-origin `_blank` link, deferred `about:blank` window,
/// broker form in its own app window, external link sent to the browser.
const SAME_ORIGIN: usize = 1;
const DEFERRED: usize = 2;
const BROKER: usize = 4;
const EXTERNAL: usize = 8;
const HTTP_DOWNLOAD: usize = 16;
const BLOB_DOWNLOAD: usize = 32;
const CANCEL_DOWNLOAD: usize = 64;
const ALL: usize =
    SAME_ORIGIN | DEFERRED | BROKER | EXTERNAL | HTTP_DOWNLOAD | BLOB_DOWNLOAD | CANCEL_DOWNLOAD;
static DOWNLOAD_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();

fn save_download(name: &str) -> Option<std::path::PathBuf> {
    println!("Popup event: save requested for {name}");
    match name {
        "cancel.txt" => {
            PASSED.fetch_or(CANCEL_DOWNLOAD, Ordering::SeqCst);
            None
        }
        "instance_config.yaml" | "team.env" => Some(DOWNLOAD_DIR.get().unwrap().join(name)),
        _ => {
            eprintln!("FAIL: unexpected download filename");
            None
        }
    }
}
/// The three links that load inside the app; the external one must not leave
/// an empty window behind.
const EXPECTED_PAGES: usize = 3;
const EXTERNAL_URL: &str = "https://example.invalid/external";

static PASSED: AtomicUsize = AtomicUsize::new(0);

/// WebView2 limits successive automatic downloads without a user gesture.
/// Exercise each button with a simulated gesture, as a person does in setup.
/// This stays in the probe; the shipping app retains the browser's policy.
#[cfg(windows)]
fn click_download(window: &tauri::WebviewWindow, kind: &str) {
    let parameters = serde_json::json!({
        "expression": format!("window.uaSmokeDownload({})", serde_json::to_string(kind).unwrap()),
        "userGesture": true,
    })
    .to_string();
    window
        .with_webview(move |view| unsafe {
            use windows::core::HSTRING;
            view.controller()
                .CoreWebView2()
                .unwrap()
                .CallDevToolsProtocolMethod(
                    &HSTRING::from("Runtime.evaluate"),
                    &HSTRING::from(parameters),
                    None,
                )
                .expect("simulate the download button's user gesture");
        })
        .expect("access the probe webview");
}

#[tauri::command]
fn runtime_status() -> String {
    "UNEXPECTED PRIVILEGE".into()
}

fn record_external(url: &Url) {
    if url.as_str() == EXTERNAL_URL {
        println!("Popup event: external link sent to the browser hook");
        PASSED.fetch_or(EXTERNAL, Ordering::SeqCst);
    } else {
        eprintln!("Popup check failed: unexpected external URL {url}");
    }
}

/// Answer each request on `listener` with the body `respond` returns for its path.
fn serve(listener: TcpListener, respond: impl Fn(&str) -> String + Send + 'static) {
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut bytes = [0; 8192];
            let size = stream.read(&mut bytes).unwrap();
            let request = String::from_utf8_lossy(&bytes[..size]);
            let path = request.split_whitespace().nth(1).unwrap_or("/");
            let body = respond(path);
            let content_type = if path == "/download" {
                "application/octet-stream\r\nContent-Disposition: attachment; filename=instance_config.yaml"
            } else {
                "text/html"
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
}

fn main() {
    let download_dir = tempfile::tempdir().unwrap();
    DOWNLOAD_DIR.set(download_dir.path().to_path_buf()).unwrap();
    universal_agents_desktop_lib::webview::use_save_destination(save_download);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let broker = TcpListener::bind("127.0.0.1:0").unwrap();
    let broker_port = broker.local_addr().unwrap().port();

    // UA's secure broker serves credential forms on its own port. The form
    // reports back to the Web UI server, since the two origins differ.
    serve(broker, move |_path| {
        format!(
            r#"<h1>Credential form</h1><script>
            (async () => {{
              let denied = false;
              try {{ await window.__TAURI_INTERNALS__.invoke('runtime_status'); }}
              catch (e) {{ denied = /not allowed|denied|permissions/i.test(String(e)); }}
              new Image().src = 'http://127.0.0.1:{port}/broker-result?denied=' + denied;
            }})();
            </script>"#
        )
    });

    serve(listener, move |path| {
        if path == "/download" {
            "fixture: config\n".into()
        } else if path.starts_with("/event") {
            println!("Popup event: {path}");
            "ok".into()
        } else if path.starts_with("/broker-result") {
            if path.contains("denied=true") {
                println!("Popup event: broker form opened in an app window");
                PASSED.fetch_or(BROKER, Ordering::SeqCst);
            } else {
                eprintln!("Popup check failed: {path}");
            }
            "ok".into()
        } else if path.starts_with("/result") {
            if path.contains("cookie=true") && path.contains("denied=true") {
                PASSED.fetch_or(
                    if path.contains("deferred") {
                        DEFERRED
                    } else {
                        SAME_ORIGIN
                    },
                    Ordering::SeqCst,
                );
            } else {
                eprintln!("Popup check failed: {path}");
            }
            "ok".into()
        } else if path.starts_with("/child") {
            r#"<script>
            (async () => {
              let denied = false;
              try { await window.__TAURI_INTERNALS__.invoke('runtime_status'); }
              catch (e) { denied = /not allowed|denied|permissions/i.test(String(e)); }
              fetch('/result?cookie=' + document.cookie.includes('ua_smoke=shared') + '&denied=' + denied + '&kind=' + encodeURIComponent(location.search));
            })();
            </script>"#
                .into()
        } else {
            format!(
                r#"<h1>Native popup smoke test</h1><script>
            document.cookie = 'ua_smoke=shared; path=/';
            function clickBlank(href) {{
              let link = document.createElement('a');
              link.href = href; link.target = '_blank'; link.rel = 'noopener noreferrer';
              document.body.append(link); link.click();
            }}
            setTimeout(() => {{
              clickBlank('/child');
              setTimeout(() => {{
                fetch('/event?phase=deferred-timer-fired');
                let child = window.open('about:blank', '_blank');
                fetch('/event?phase=deferred-open&child=' + Boolean(child));
                if (child) setTimeout(() => {{
                  fetch('/event?phase=deferred-navigate');
                  child.location='/child?deferred';
                }}, 100);
              }}, 2500);
              setTimeout(() => clickBlank('http://127.0.0.1:{broker_port}/credential-collection/smoke'), 4000);
              setTimeout(() => clickBlank('{EXTERNAL_URL}'), 5500);
              function download(href, name) {{
                const link = document.createElement('a');
                link.href = href; link.download = name;
                document.body.append(link); link.click(); link.remove();
              }}
              window.uaSmokeDownload = (kind) => {{
                if (kind === 'http') download('/download', 'instance_config.yaml');
                if (kind === 'blob') download(URL.createObjectURL(new Blob(['FIXTURE=only\n'], {{type: 'text/plain'}})), 'team.env');
                if (kind === 'cancel') download(URL.createObjectURL(new Blob(['cancel me'], {{type: 'text/plain'}})), 'cancel.txt');
              }};
              if (!{native_gestures}) {{
                setTimeout(() => window.uaSmokeDownload('http'), 7000);
                setTimeout(() => window.uaSmokeDownload('blob'), 9000);
                setTimeout(() => window.uaSmokeDownload('cancel'), 11000);
              }}
            }}, 800);
            </script>"#,
                native_gestures = cfg!(windows),
            )
        }
    });

    universal_agents_desktop_lib::webview::use_external_opener(record_external);
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![runtime_status])
        .setup(move |app| {
            universal_agents_desktop_lib::webview::build_runtime_window(
                app.handle(),
                "webchat-smoke",
                format!("http://127.0.0.1:{port}/").parse().unwrap(),
                broker_port,
            )
            .unwrap();
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                for tick in 0..450 {
                    #[cfg(windows)]
                    if let Some(kind) = match tick {
                        80 => Some("http"),
                        100 => Some("blob"),
                        120 => Some("cancel"),
                        _ => None,
                    } {
                        let window = handle.get_webview_window("webchat-smoke").unwrap();
                        click_download(&window, kind);
                    }
                    #[cfg(not(windows))]
                    let _ = tick;
                    for (name, expected, bit) in [
                        ("instance_config.yaml", "fixture: config\n", HTTP_DOWNLOAD),
                        ("team.env", "FIXTURE=only\n", BLOB_DOWNLOAD),
                    ] {
                        if std::fs::read_to_string(DOWNLOAD_DIR.get().unwrap().join(name)).ok().as_deref() == Some(expected) {
                            PASSED.fetch_or(bit, Ordering::SeqCst);
                        }
                    }
                    if PASSED.load(Ordering::SeqCst) == ALL {
                        // Give the empty window from the external link time to close.
                        std::thread::sleep(Duration::from_millis(1500));
                        let pages = handle
                            .webview_windows()
                            .keys()
                            .filter(|label| label.starts_with("ua-page-"))
                            .count();
                        if pages != EXPECTED_PAGES {
                            eprintln!(
                                "FAIL: {pages} UA page windows are open (expected {EXPECTED_PAGES}); an external link left an empty window"
                            );
                            std::process::exit(1);
                        }
                        println!(
                            "PASS: popups share cookies and deny desktop commands; broker and external links follow policy; HTTP and blob downloads save exact contents and cancellation is handled"
                        );
                        handle.exit(0);
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                eprintln!(
                    "FAIL: popup check bitmask {} (expected {ALL})",
                    PASSED.load(Ordering::SeqCst)
                );
                std::process::exit(1);
            });
            Ok(())
        })
        .run(context)
        .expect("native popup smoke app");
}
