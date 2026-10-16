//! Recovery guidance written entirely by this app, which the status window may
//! show word for word.
//!
//! Most errors carry raw Docker or operating-system output, which can include
//! account names, paths, or registry responses. The status window therefore
//! replaces them with a fixed "download diagnostics" message. Guidance is the
//! exception: its whole text comes from this app and contains no command output,
//! so the window shows it in place of the fixed message.
//!
//! Errors cross the desktop controller and the Tauri command boundary as plain
//! strings, so guidance carries a leading marker character instead of a type.
//! The status window checks for the same character (`GUIDANCE_MARKER` in
//! `ui/desktop-shell/app.js`). A guidance string wrapped inside another message
//! no longer starts with the marker and is shown as the fixed message, so a
//! mistake can only hide guidance, never reveal raw output.

/// ASCII unit separator: never produced by Docker, the OS, or UA output.
pub const MARKER: char = '\u{1f}';

/// Mark app-written text as guidance. Already-marked text is returned unchanged.
pub fn guidance(text: impl Into<String>) -> String {
    let text = text.into();
    if is_guidance(&text) {
        text
    } else {
        format!("{MARKER}{text}")
    }
}

pub fn is_guidance(error: &str) -> bool {
    error.starts_with(MARKER)
}

/// The error without any marker, for logs and native dialogs.
pub fn plain(error: &str) -> String {
    error.replace(MARKER, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guidance_is_marked_once_and_plain_removes_the_marker() {
        let marked = guidance("Quit the other program, then retry.");
        assert!(is_guidance(&marked));
        assert_eq!(guidance(marked.clone()), marked);
        assert_eq!(plain(&marked), "Quit the other program, then retry.");
    }

    #[test]
    fn wrapped_guidance_is_no_longer_guidance() {
        let wrapped = format!("The approved update is pending: {}", guidance("Sign in."));
        assert!(!is_guidance(&wrapped));
        assert_eq!(plain(&wrapped), "The approved update is pending: Sign in.");
    }

    #[test]
    fn raw_errors_are_not_guidance() {
        assert!(!is_guidance("Docker command failed: docker pull: denied"));
    }
}
