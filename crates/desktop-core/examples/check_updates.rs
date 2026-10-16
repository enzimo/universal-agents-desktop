//! Read-only check using the same release source as the desktop app.
use ua_desktop_core::{
    RuntimeLock,
    updates::{self, GitHubReleases},
};
fn main() -> Result<(), String> {
    let lock = RuntimeLock::bundled().map_err(|e| e.to_string())?;
    let review = updates::check(&GitHubReleases, &lock, false, false)?;
    // CLI output only; app diagnostics use log.
    println!(
        "Desktop {} / selected UA {} / latest UA {}",
        review.desktop_version, review.current_version, review.latest_version
    );
    if let Some(reason) = review.blocked_reason {
        println!("{reason}");
    }
    Ok(())
}
