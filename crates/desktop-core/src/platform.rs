//! Host OS mechanics; runtime identity and container policy live elsewhere.
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

/// Shared setup guide maintained in the `universal-agents` repository.
///
/// The desktop host links to the published download instead of packaging its
/// own copy, so the setup instructions stay in one reviewed place.
pub const DOCKER_SETUP_GUIDE_URL: &str =
    "https://github.com/enzimo/universal-agents/releases/latest/download/setup-guide.html";

#[derive(Clone, Copy, Debug)]
pub enum Host {
    Mac,
    Windows,
    Linux,
}
impl Host {
    pub const fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Linux
        }
    }
    pub fn docker_setup_hint(self) -> &'static str {
        match self {
            Self::Mac => {
                "Install Colima and Docker, run `colima start universal-agents`, then `docker context use colima-universal-agents`."
            }
            Self::Windows => {
                "Install and start Docker Desktop with the WSL 2 backend in Linux containers mode. Complete WSL setup and restart Windows if required."
            }
            Self::Linux => {
                "Install and start Docker Engine, and grant your normal user Docker access. Sign out and back in after changing group membership."
            }
        }
    }

    /// Where to stop this computer from sleeping, for the first-run notice
    /// that agents need a computer that stays on. Fixed text per OS, like the
    /// Docker hint, so the renderer never guesses the system it runs on.
    pub fn keep_awake_hint(self) -> &'static str {
        match self {
            Self::Mac => {
                "On a MacBook, open System Settings > Battery > Options and turn on \"Prevent automatic sleeping on power adapter when the display is off\", then keep it plugged in with the lid open. On a desktop Mac, use System Settings > Energy."
            }
            Self::Windows => {
                "Open Settings > System > Power & battery > Screen, sleep & hibernate timeouts and set sleep to Never when plugged in. On a laptop, also set closing the lid to Do nothing in Control Panel > Power Options."
            }
            Self::Linux => {
                "Open Settings > Power and turn off Automatic Suspend (on a laptop, at least when plugged in)."
            }
        }
    }

    /// Official Docker installation instructions for this host.
    ///
    /// A fixed per-OS page keeps the prerequisite dialog actionable without
    /// letting the renderer open an arbitrary address.
    pub fn docker_install_url(self) -> &'static str {
        match self {
            Self::Mac => "https://docs.docker.com/desktop/setup/install/mac-install/",
            Self::Windows => "https://docs.docker.com/desktop/setup/install/windows-install/",
            Self::Linux => "https://docs.docker.com/engine/install/",
        }
    }

    pub fn credential_helpers(self) -> &'static [&'static str] {
        match self {
            Self::Mac => &["desktop", "osxkeychain"],
            Self::Windows => &["desktop", "wincred"],
            Self::Linux => &["desktop", "secretservice", "pass"],
        }
    }

    /// How to repair Docker settings that name a credential helper which is
    /// not installed, phrased to follow a sentence naming the settings file.
    pub fn credential_helper_fix(self) -> &'static str {
        match self {
            Self::Mac => {
                "If you use Colima or another Docker without Docker Desktop, run \"brew install docker-credential-helper\" and change \"credsStore\" to \"osxkeychain\" in that file. Otherwise reinstall Docker Desktop, which includes its helper."
            }
            Self::Windows => {
                "Reinstall or repair Docker Desktop, which includes its helpers, or change \"credsStore\" to \"wincred\" in that file if docker-credential-wincred.exe is installed."
            }
            Self::Linux => {
                "Install that helper, or change \"credsStore\" in that file to \"secretservice\" or \"pass\" after installing the matching docker-credential package."
            }
        }
    }
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

pub fn command_path() -> OsString {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let mut paths: Vec<PathBuf> = std::env::split_paths(&inherited).collect();
    match Host::current() {
        Host::Mac => paths.extend(
            [
                "/opt/homebrew/bin",
                "/usr/local/bin",
                "/usr/bin",
                "/bin",
                "/Applications/Docker.app/Contents/Resources/bin",
            ]
            .map(PathBuf::from),
        ),
        Host::Linux => {
            paths.extend(["/usr/local/bin", "/usr/bin", "/bin", "/snap/bin"].map(PathBuf::from));
            if let Some(home) = home_dir() {
                paths.push(home.join(".local/bin"));
            }
        }
        Host::Windows => {
            if let Some(root) = std::env::var_os("ProgramFiles") {
                paths.push(PathBuf::from(&root).join("Docker/Docker/resources/bin"));
                paths.push(PathBuf::from(root).join("GitHub CLI"));
            }
            if let Some(root) = std::env::var_os("SystemRoot") {
                paths.push(PathBuf::from(&root).join("System32"));
                paths.push(PathBuf::from(root).join("System32/WindowsPowerShell/v1.0"));
            }
        }
    }
    std::env::join_paths(paths).unwrap_or(inherited)
}

pub fn program_path(program: &str) -> PathBuf {
    let name = if cfg!(windows) && !program.ends_with(".exe") {
        format!("{program}.exe")
    } else {
        program.to_owned()
    };
    std::env::split_paths(&command_path())
        .map(|p| p.join(&name))
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

/// GUI launches on Windows should not flash a console for each status poll.
/// Visible login terminals deliberately do not use this constructor.
pub fn command(program: &str) -> Command {
    let mut command = Command::new(program_path(program));
    command.env("PATH", command_path());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    command
}

pub fn docker_source(path: &Path) -> Result<String, String> {
    if !path.is_absolute() {
        return Err("Docker mount source must be absolute.".into());
    }
    let source = path
        .to_str()
        .ok_or("Docker mount source must be valid Unicode.")?;
    normalize_source(source, Host::current())
}

fn normalize_source(source: &str, host: Host) -> Result<String, String> {
    let source = match host {
        Host::Windows => {
            // Rust canonicalize returns verbatim drive paths. Docker CLI expects
            // ordinary drive-letter paths; UNC/device paths are not supported.
            let source = source
                .strip_prefix(r"\\?\")
                .unwrap_or(source)
                .replace('\\', "/");
            let bytes = source.as_bytes();
            if bytes.len() < 3
                || !bytes[0].is_ascii_alphabetic()
                || &bytes[1..3] != b":/"
                || source[2..].contains(':')
            {
                return Err(
                    "Choose a local Windows drive folder, not a network share or device path."
                        .into(),
                );
            }
            source
        }
        _ => {
            if source.contains(':') {
                return Err("Docker mount source cannot contain a colon.".into());
            }
            source.to_owned()
        }
    };
    if source.chars().any(|c| c == '\0' || c == '\r' || c == '\n') {
        return Err("Docker mount source cannot contain control characters.".into());
    }
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_mount_paths_handle_drive_letters_spaces_and_verbatim_prefix() {
        assert_eq!(
            normalize_source(r"C:\Users\Jane Doe\workspace", Host::Windows).unwrap(),
            "C:/Users/Jane Doe/workspace"
        );
        assert_eq!(
            normalize_source(r"\\?\D:\UA Data", Host::Windows).unwrap(),
            "D:/UA Data"
        );
        for source in [
            r"\\server\share",
            r"\\?\UNC\server\share",
            r"C:relative",
            r"C:\data:stream",
            r"\\.\device",
        ] {
            assert!(normalize_source(source, Host::Windows).is_err(), "{source}");
        }
    }
    #[test]
    fn docker_prerequisite_links_are_https_and_per_os() {
        let urls = [
            Host::Mac.docker_install_url(),
            Host::Windows.docker_install_url(),
            Host::Linux.docker_install_url(),
        ];
        for url in urls {
            assert!(url.starts_with("https://"), "{url}");
        }
        assert_ne!(
            Host::Mac.docker_install_url(),
            Host::Windows.docker_install_url()
        );
        assert_ne!(
            Host::Windows.docker_install_url(),
            Host::Linux.docker_install_url()
        );
        assert!(DOCKER_SETUP_GUIDE_URL.starts_with("https://"));
    }

    #[test]
    fn keep_awake_hints_name_each_system_settings_app() {
        assert!(Host::Mac.keep_awake_hint().contains("System Settings"));
        assert!(Host::Windows.keep_awake_hint().contains("Power & battery"));
        assert!(Host::Linux.keep_awake_hint().contains("Automatic Suspend"));
    }

    #[test]
    fn unix_mount_paths_preserve_names_and_reject_volume_syntax_injection() {
        for host in [Host::Mac, Host::Linux] {
            assert_eq!(
                normalize_source("/home/jane/UA Data", host).unwrap(),
                "/home/jane/UA Data"
            );
            assert!(normalize_source("/home/jane/work:/extra", host).is_err());
        }
    }
}
