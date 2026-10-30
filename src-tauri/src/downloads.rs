//! Native downloads for unprivileged runtime windows. The webview retains its
//! cookies and resolves blob URLs; no HTTP client or file-writing IPC is exposed.
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};
use tauri::{AppHandle, webview::DownloadEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

static SAVE_DESTINATION: OnceLock<fn(&str) -> Option<PathBuf>> = OnceLock::new();

/// The native probe substitutes a destination without an interactive dialog.
pub fn use_save_destination(picker: fn(&str) -> Option<PathBuf>) {
    let _ = SAVE_DESTINATION.set(picker);
}

struct Pending {
    url: String,
    name: String,
    file: PathBuf,
    _directory: tempfile::TempDir,
}

impl Pending {
    fn save(self, destination: Option<tauri_plugin_dialog::FilePath>) -> Result<(), String> {
        let Some(destination) = destination else {
            return Ok(());
        };
        let path = destination.into_path().map_err(|error| error.to_string())?;
        std::fs::copy(&self.file, path)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

#[derive(Clone, Default)]
pub struct Downloads(Arc<Mutex<HashMap<PathBuf, Pending>>>);

fn take_download(
    pending: &mut HashMap<PathBuf, Pending>,
    url: &str,
    path: Option<PathBuf>,
) -> Option<Pending> {
    if let Some(path) = path {
        // WebKitGTK reports a file URL; WebView2 reports an ordinary path.
        let path = tauri::Url::parse(&path.to_string_lossy())
            .ok()
            .filter(|url| url.scheme() == "file")
            .and_then(|url| url.to_file_path().ok())
            .unwrap_or(path);
        if let Some(item) = pending.remove(&path) {
            return Some(item);
        }
    }
    // WKWebView supplies no completed path. Never guess between concurrent
    // downloads of the same URL, which could save the wrong config contents.
    let matching: Vec<_> = pending
        .iter()
        .filter(|(_, item)| item.url == url)
        .map(|(path, _)| path.clone())
        .collect();
    (matching.len() == 1)
        .then(|| pending.remove(&matching[0]))
        .flatten()
}

fn report_failure(app: &AppHandle, error: impl std::fmt::Display) {
    log::error!("embedded download failed: {error}");
    app.dialog().message("The file could not be downloaded or saved. Try the download again. If it keeps failing, open setup in your browser or download diagnostics from the launcher.")
        .title("Download failed").kind(MessageDialogKind::Error).show(|_| {});
}

impl Downloads {
    pub fn handle(&self, app: &AppHandle, event: DownloadEvent<'_>) -> bool {
        match event {
            DownloadEvent::Requested { url, destination } => {
                // Do not block the webview callback on a native dialog. In
                // particular, WebView2 calls this on the Windows message thread.
                let directory = match tempfile::tempdir() {
                    Ok(directory) => directory,
                    Err(error) => {
                        report_failure(app, error);
                        return false;
                    }
                };
                let name = destination
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("download")
                    .to_owned();
                // WebKitGTK refuses an existing destination. A private directory
                // provides a fresh path without exposing partially downloaded secrets.
                let file = directory.path().join("download");
                *destination = file.clone();
                self.0.lock().unwrap().insert(
                    destination.clone(),
                    Pending {
                        url: url.to_string(),
                        name,
                        file,
                        _directory: directory,
                    },
                );
                true
            }
            DownloadEvent::Finished { url, path, success } => {
                let mut pending = self.0.lock().unwrap();
                let item = take_download(&mut pending, url.as_str(), path);
                if !success {
                    // Some engines omit the failed path. Only remove by URL
                    // when unambiguous; simultaneous downloads may share a URL.
                    // Ambiguous partial files remain private until window close.
                    drop(pending);
                    report_failure(app, "the webview did not complete the download");
                    return true;
                }
                drop(pending);
                let Some(item) = item else {
                    report_failure(app, "the webview returned an unknown download destination");
                    return true;
                };
                let app = app.clone();
                let name = item.name.clone();
                let dialog = app.dialog().file().set_file_name(&name);
                // Holding Pending through the callback removes the private
                // source after save, cancellation, or failure.
                let save = move |destination: Option<tauri_plugin_dialog::FilePath>| {
                    if let Err(error) = item.save(destination) {
                        report_failure(&app, error);
                    }
                };
                if let Some(picker) = SAVE_DESTINATION.get() {
                    save(picker(&name).map(Into::into));
                } else {
                    dialog.save_file(save);
                }
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending() -> Pending {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("download");
        std::fs::write(&file, b"FIXTURE=only\n").unwrap();
        Pending {
            url: "blob:http://127.0.0.1:18090/fixture".into(),
            name: "team.env".into(),
            file,
            _directory: directory,
        }
    }

    #[test]
    fn saving_preserves_bytes_and_removes_staging_after_save_cancel_and_error() {
        let target = tempfile::tempdir().unwrap();
        let saved = target.path().join("team.env");
        let item = pending();
        let source = item.file.clone();
        item.save(Some(saved.clone().into())).unwrap();
        assert_eq!(std::fs::read(saved).unwrap(), b"FIXTURE=only\n");
        assert!(!source.exists());
        let item = pending();
        let source = item.file.clone();
        item.save(None).unwrap();
        assert!(!source.exists());
        let item = pending();
        let source = item.file.clone();
        assert!(
            item.save(Some(target.path().join("missing/team.env").into()))
                .is_err()
        );
        assert!(!source.exists());
    }

    #[test]
    fn completed_downloads_match_windows_paths_linux_file_urls_and_macos_missing_paths() {
        for mode in 0..3 {
            let item = pending();
            let source = item.file.clone();
            let url = item.url.clone();
            let path = match mode {
                0 => Some(source.clone()),
                1 => Some(PathBuf::from(
                    tauri::Url::from_file_path(&source).unwrap().as_str(),
                )),
                _ => None,
            };
            let mut items = HashMap::from([(source.clone(), item)]);
            let matched = take_download(&mut items, &url, path).unwrap();
            assert_eq!(matched.file, source);
            assert!(items.is_empty());
        }
    }

    #[test]
    fn missing_path_never_selects_an_ambiguous_download() {
        let first = pending();
        let second = pending();
        let url = first.url.clone();
        let mut items = HashMap::from([(first.file.clone(), first), (second.file.clone(), second)]);
        assert!(take_download(&mut items, &url, None).is_none());
        assert_eq!(items.len(), 2);
    }
}
