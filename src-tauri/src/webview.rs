use std::sync::{
    OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;

pub use crate::downloads::use_save_destination;

static NEXT_PAGE: AtomicU64 = AtomicU64::new(1);

/// UA selects subpages with query parameters, such as /webchat?view=status.
/// Explicitly opening chat must reset those views even when the path matches.
pub fn needs_navigation(current: &Url, target: &Url) -> bool {
    current != target
}

/// Replaces the system browser for links that leave the app. Only the native
/// popup probe sets this, so it can check external links without opening a
/// browser on the machine running it.
static EXTERNAL_OPENER: OnceLock<fn(&Url)> = OnceLock::new();

/// Route external links to `opener` instead of the system browser (probe only).
pub fn use_external_opener(opener: fn(&Url)) {
    let _ = EXTERNAL_OPENER.set(opener);
}

/// The loopback ports of one UA runtime. The Web UI (`web`) may load in any
/// app window. UA's secure broker (`broker`) serves the credential and sign-in
/// forms the Web Chat links to; those open in their own app window, like other
/// UA pages, but never replace the main Web UI window.
#[derive(Clone, Copy, Debug)]
pub struct RuntimePorts {
    pub web: u16,
    pub broker: u16,
}

fn is_runtime(url: &Url, port: u16) -> bool {
    url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port() == Some(port)
        && url.username().is_empty()
        && url.password().is_none()
}

fn is_blank(url: &Url) -> bool {
    url.as_str() == "about:blank"
}

fn is_runtime_download(url: &Url, ports: RuntimePorts) -> bool {
    is_runtime(url, ports.web)
        || (url.scheme() == "blob"
            && Url::parse(url.path()).is_ok_and(|origin| is_runtime(&origin, ports.web)))
}

/// Whether `url` may load inside an app window. `child` is true for windows
/// a UA page opened, false for the main Web UI window.
fn loads_in_app(url: &Url, ports: RuntimePorts, child: bool) -> bool {
    is_runtime_download(url, ports) || is_blank(url) || (child && is_runtime(url, ports.broker))
}

fn open_external(app: &AppHandle, url: &Url) {
    if let Some(opener) = EXTERNAL_OPENER.get() {
        opener(url);
        return;
    }
    if matches!(url.scheme(), "http" | "https") {
        if let Err(error) = app.opener().open_url(url.as_str(), None::<&str>) {
            log::warn!("failed to open external Web UI link: {error}");
        }
    } else {
        log::warn!("blocked Web UI navigation with scheme {}", url.scheme());
    }
}

/// Build the main Web UI window at `runtime_url`, whose port is the Web UI port.
pub fn build_runtime_window(
    app: &AppHandle,
    label: &str,
    runtime_url: Url,
    broker_port: u16,
) -> Result<WebviewWindow, String> {
    let ports = RuntimePorts {
        web: runtime_url
            .port()
            .ok_or("runtime URL must include a port")?,
        broker: broker_port,
    };
    let window = build_page(app, label, ports, None, runtime_url)?;
    crate::window_size::fit_to_screen(&window);
    Ok(window)
}

fn build_page(
    app: &AppHandle,
    label: &str,
    ports: RuntimePorts,
    features: Option<tauri::webview::NewWindowFeatures>,
    target: Url,
) -> Result<WebviewWindow, String> {
    let child = features.is_some();
    let navigation_app = app.clone();
    let popup_app = app.clone();
    let page_label = label.to_owned();
    // A page opened as about:blank is navigated by its opener afterwards
    // (popup-links.js, deferred login windows). If that first navigation
    // leaves the app, the empty window would stay on screen, so it is closed.
    let loaded = AtomicBool::new(!is_blank(&target));
    let downloads = crate::downloads::Downloads::default();
    // Start each page at its requested URL, including deferred about:blank pages.
    // window_features preserves the opener's WebView2 environment, WebKitGTK
    // related view, or WKWebView configuration, depending on the host OS.
    let mut builder = WebviewWindowBuilder::new(app, label, WebviewUrl::External(target))
        .title("Universal Agents")
        .initialization_script(include_str!("popup-links.js"))
        .inner_size(1280.0, crate::window_size::WEB_UI_HEIGHT)
        .min_inner_size(860.0, 620.0)
        .on_navigation(move |url| {
            log::debug!(
                "Web UI navigation: scheme={}, allowed={}",
                url.scheme(),
                loads_in_app(url, ports, child)
            );
            if loads_in_app(url, ports, child) {
                if !is_blank(url) {
                    loaded.store(true, Ordering::SeqCst);
                }
                return true;
            }
            open_external(&navigation_app, url);
            if child && !loaded.load(Ordering::SeqCst) {
                close_empty_page(&navigation_app, &page_label);
            }
            false
        })
        .on_new_window(move |url, features| {
            log::debug!("Web UI new window requested with scheme {}", url.scheme());
            if loads_in_app(&url, ports, true) {
                let label = format!("ua-page-{}", NEXT_PAGE.fetch_add(1, Ordering::Relaxed));
                match build_page(&popup_app, &label, ports, Some(features), url) {
                    Ok(window) => tauri::webview::NewWindowResponse::Create { window },
                    Err(error) => {
                        log::error!("failed to create Web UI page: {error}");
                        tauri::webview::NewWindowResponse::Deny
                    }
                }
            } else {
                open_external(&popup_app, &url);
                tauri::webview::NewWindowResponse::Deny
            }
        });
    // WebKitGTK registers download callbacks on the shared web context, so
    // the main window's handler also handles its children. Registering it on
    // every child would process each download several times.
    if !cfg!(target_os = "linux") || !child {
        builder = builder.on_download(move |view, event| {
            match &event {
                tauri::webview::DownloadEvent::Requested { url, .. } => log::debug!("Web UI download requested: scheme={}", url.scheme()),
                tauri::webview::DownloadEvent::Finished { url, success, .. } => log::debug!("Web UI download finished: scheme={}, success={success}", url.scheme()),
                _ => {},
            }
            if matches!(&event, tauri::webview::DownloadEvent::Requested { url, .. } if !is_runtime_download(url, ports)) {
                log::warn!("blocked download outside the runtime origin");
                return false;
            }
            downloads.handle(view.app_handle(), event)
        });
    }
    if let Some(features) = features {
        builder = builder.window_features(features);
    }
    let window = builder.build().map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    window
        .with_webview(|view| {
            use webkit2gtk::{SettingsExt, WebViewExt};
            // WebKitGTK otherwise suppresses script-created windows before our
            // native URL policy runs, including deferred login windows.
            if let Some(settings) = view.inner().settings() {
                settings.set_javascript_can_open_windows_automatically(true);
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(window)
}

/// Close a window that never showed a page, outside the navigation callback
/// that found it empty.
fn close_empty_page(app: &AppHandle, label: &str) {
    let app = app.clone();
    let label = label.to_owned();
    tauri::async_runtime::spawn(async move {
        let Some(window) = app.get_webview_window(&label) else {
            return;
        };
        log::info!("closing empty Web UI window {label}: its first page opened in the browser");
        if let Err(error) = window.close() {
            log::warn!("failed to close empty Web UI window {label}: {error}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reopening_chat_resets_subpages_but_preserves_the_current_chat() {
        let target = Url::parse("http://127.0.0.1:18090/webchat").unwrap();
        assert!(!needs_navigation(&target, &target));
        for current in [
            "http://127.0.0.1:18090/webchat?view=status",
            "http://127.0.0.1:18090/webchat?view=files",
            "http://127.0.0.1:18090/webchat#status",
            "http://127.0.0.1:18090/",
            "http://127.0.0.1:18091/webchat",
        ] {
            assert!(
                needs_navigation(&Url::parse(current).unwrap(), &target),
                "{current}"
            );
        }
    }
    #[test]
    fn downloads_allow_only_runtime_http_and_runtime_created_blobs() {
        let ports = RuntimePorts {
            web: 18090,
            broker: 18600,
        };
        for url in [
            "http://127.0.0.1:18090/config",
            "blob:http://127.0.0.1:18090/id",
        ] {
            assert!(
                is_runtime_download(&Url::parse(url).unwrap(), ports),
                "{url}"
            );
        }
        for url in [
            "blob:https://example.com/id",
            "blob:null/id",
            "blob:http://127.0.0.1:18600/id",
            "http://127.0.0.1:18600/config",
            "file:///tmp/config",
            "data:text/plain,config",
        ] {
            assert!(
                !is_runtime_download(&Url::parse(url).unwrap(), ports),
                "{url}"
            );
        }
    }
    #[test]
    fn remote_pages_are_limited_to_the_exact_runtime_origin() {
        for url in [
            "http://127.0.0.1:18090/files",
            "http://127.0.0.1:18090/webchat",
        ] {
            assert!(is_runtime(&Url::parse(url).unwrap(), 18090));
        }
        for url in [
            "http://127.0.0.1:18600/",
            "https://example.com/",
            "http://localhost:18090/",
            "http://user@127.0.0.1:18090/",
            "tauri://localhost/",
            "file:///etc/passwd",
            "javascript:alert(1)",
        ] {
            assert!(!is_runtime(&Url::parse(url).unwrap(), 18090));
        }
        assert!(is_blank(&Url::parse("about:blank").unwrap()));
        assert!(!is_blank(&Url::parse("about:config").unwrap()));
    }

    #[test]
    fn broker_forms_open_only_in_windows_ua_pages_opened() {
        let ports = RuntimePorts {
            web: 18090,
            broker: 18600,
        };
        let form = Url::parse("http://127.0.0.1:18600/credential-collection/abc").unwrap();
        assert!(loads_in_app(&form, ports, true));
        assert!(!loads_in_app(&form, ports, false));
        for url in [
            "http://localhost:18600/credential-collection/abc",
            "https://127.0.0.1:18600/credential-collection/abc",
            "http://127.0.0.1:18601/",
            "https://example.com/",
        ] {
            assert!(
                !loads_in_app(&Url::parse(url).unwrap(), ports, true),
                "{url}"
            );
        }
        let chat = Url::parse("http://127.0.0.1:18090/webchat").unwrap();
        assert!(loads_in_app(&chat, ports, false));
        assert!(loads_in_app(&chat, ports, true));
    }
}
