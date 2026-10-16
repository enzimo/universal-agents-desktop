//! Operation-scoped progress, independent of the serialized controller mutex.
//! Only fixed application messages belong here, never command output or credentials.
use std::cell::RefCell;

use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProgressUpdate {
    pub message: String,
    pub current_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
}

type Reporter = Box<dyn Fn(ProgressUpdate)>;
thread_local! {
    static REPORTER: RefCell<Option<Reporter>> = RefCell::new(None);
}

pub fn report(message: impl Into<String>) {
    emit(ProgressUpdate {
        message: message.into(),
        current_bytes: None,
        total_bytes: None,
    });
}

pub fn report_download(current_bytes: u64, total_bytes: u64) {
    emit(ProgressUpdate {
        message: format!(
            "Downloading runtime image: {} of {}",
            display_bytes(current_bytes),
            display_bytes(total_bytes)
        ),
        current_bytes: Some(current_bytes),
        total_bytes: Some(total_bytes),
    });
}

fn display_bytes(bytes: u64) -> String {
    if bytes < 1_000_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    }
}

fn emit(update: ProgressUpdate) {
    REPORTER.with(|slot| {
        if let Some(reporter) = slot.borrow().as_ref() {
            reporter(update);
        }
    });
}

pub fn with_reporter<T>(
    reporter: impl Fn(ProgressUpdate) + 'static,
    operation: impl FnOnce() -> T,
) -> T {
    struct Restore(Option<Reporter>);
    impl Drop for Restore {
        fn drop(&mut self) {
            REPORTER.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(REPORTER.with(|slot| slot.replace(Some(Box::new(reporter)))));
    operation()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn reporter_is_scoped_to_worker_and_removed_after_panic() {
        let messages = Arc::new(Mutex::new(Vec::new()));
        let captured = messages.clone();
        let _ = std::panic::catch_unwind(move || {
            with_reporter(
                move |message| captured.lock().unwrap().push(message),
                || {
                    report("Fetching runtime image");
                    std::thread::spawn(|| report("Unrelated operation"))
                        .join()
                        .unwrap();
                    panic!("failed operation");
                },
            );
        });
        report("After operation");
        assert_eq!(
            messages.lock().unwrap()[0].message,
            "Fetching runtime image"
        );
        assert_eq!(messages.lock().unwrap().len(), 1);
    }

    #[test]
    fn download_progress_carries_bytes_and_readable_megabytes() {
        let updates = Arc::new(Mutex::new(Vec::new()));
        let captured = updates.clone();
        with_reporter(
            move |update| captured.lock().unwrap().push(update),
            || report_download(12_500_000, 50_000_000),
        );
        assert_eq!(
            *updates.lock().unwrap(),
            [ProgressUpdate {
                message: "Downloading runtime image: 12.5 MB of 50.0 MB".to_owned(),
                current_bytes: Some(12_500_000),
                total_bytes: Some(50_000_000),
            }]
        );
    }
}
