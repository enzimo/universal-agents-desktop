//! Native desktop integration check: cargo run -p universal-agents-desktop --example popup_smoke
//! Uses only temporary loopback servers; never starts Docker or reads desktop settings.
//! External links go to a recording hook instead of the system browser.
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tauri::{Manager, Url};

/// Result bits: same-origin `_blank` link, deferred `about:blank` window,
/// broker form in its own app window, external link sent to the browser.
const SAME_ORIGIN: usize = 1;
const DEFERRED: usize = 2;
const BROKER: usize = 4;
const EXTERNAL: usize = 8;
const ALL: usize = SAME_ORIGIN | DEFERRED | BROKER | EXTERNAL;
/// The three links that load inside the app; the external one must not leave
/// an empty window behind.
const EXPECTED_PAGES: usize = 3;
const EXTERNAL_URL: &str = "https://example.invalid/external";

static PASSED: AtomicUsize = AtomicUsize::new(0);

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
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
}

fn main() {
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
        if path.starts_with("/event") {
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
            }}, 800);
            </script>"#
            )
        }
    });

    universal_agents_desktop_lib::webview::use_external_opener(record_external);
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
                for _ in 0..450 {
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
                            "PASS: same-origin and deferred popups share cookies and deny desktop commands; the broker form opens in an app window; external links leave no empty window"
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
