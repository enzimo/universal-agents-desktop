//! Host-only GHCR authentication. Secrets never cross into runtime settings or containers.
use crate::guidance::guidance;
use crate::platform::{self, Host};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::RuntimeLock;

const REGISTRY: &str = "ghcr.io";
const GITHUB: &str = "github.com";

/// This boundary deliberately has no Debug implementation or command-output errors.
pub trait RegistryRunner {
    fn run(&self, program: &str, args: &[&str], input: Option<&str>) -> Result<String, String>;
    fn require_credential_helper(&self) -> Result<(), String>;
    fn launch(&self, program: &str, args: &[&str]) -> Result<(), String> {
        self.run(program, args, None).map(|_| ())
    }
}

pub struct ProcessRegistry;

/// OS-aware executable search path for GUI launches.
pub fn host_command_path() -> std::ffi::OsString {
    platform::command_path()
}

impl RegistryRunner for ProcessRegistry {
    fn run(&self, program: &str, args: &[&str], input: Option<&str>) -> Result<String, String> {
        let mut child = platform::command(program)
            .args(args)
            .env("PATH", host_command_path())
            // 'Use GitHub CLI' means its stored account, not an inherited CI token.
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| format!("Could not run {program}. Install it and try again."))?;
        if let Some(input) = input {
            let result = child
                .stdin
                .take()
                .expect("piped stdin")
                .write_all(input.as_bytes());
            if result.is_err() {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Could not send credentials to the credential helper.".to_owned());
            }
        }
        let output = child
            .wait_with_output()
            .map_err(|_| "Could not wait for authentication.".to_owned())?;
        if !output.status.success() {
            // Never echo subprocess output: it can contain the supplied credential.
            return Err(format!("{program} authentication command failed."));
        }
        String::from_utf8(output.stdout)
            .map(|value| value.trim().to_owned())
            .map_err(|_| "Invalid authentication response.".to_owned())
    }

    fn launch(&self, program: &str, args: &[&str]) -> Result<(), String> {
        let mut command = std::process::Command::new(platform::program_path(program));
        command
            .args(args)
            .env("PATH", host_command_path())
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x00000010); // CREATE_NEW_CONSOLE
            // Let PowerShell attach its standard streams to the new console.
            command
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
        }
        let mut child = command.spawn().map_err(|_| "Could not open a terminal. Run gh auth login --hostname github.com --web --scopes read:packages in your terminal, then choose Use GitHub CLI.".to_owned())?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }

    fn require_credential_helper(&self) -> Result<(), String> {
        let path = docker_config_path()
            .ok_or_else(|| guidance("Docker configuration directory is unavailable."))?;
        let bytes = std::fs::read(&path)
            .map_err(|_| guidance("Configure a Docker credential helper for your OS first."))?;
        let config: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| {
            guidance("Docker's config.json is invalid. Repair it in Docker Desktop.")
        })?;
        let host = Host::current();
        require_helper(&config, host)?;
        match missing_helper_guidance(&config, REGISTRY, &path, host, helper_installed) {
            Some(guidance) => Err(guidance),
            None => Ok(()),
        }
    }
}

/// Docker's settings file: `$DOCKER_CONFIG/config.json`, else `~/.docker/config.json`.
fn docker_config_path() -> Option<PathBuf> {
    std::env::var_os("DOCKER_CONFIG")
        .map(PathBuf::from)
        .or_else(|| platform::home_dir().map(|home| home.join(".docker")))
        .map(|directory| directory.join("config.json"))
}

/// The credential helper Docker asks for `registry`: a per-registry
/// `credHelpers` entry wins over the global `credsStore`, and an empty name
/// means none.
fn configured_helper<'a>(config: &'a serde_json::Value, registry: &str) -> Option<&'a str> {
    config
        .get("credHelpers")
        .and_then(|value| value.get(registry))
        .or_else(|| config.get("credsStore"))
        .and_then(|value| value.as_str())
}

fn require_helper(config: &serde_json::Value, host: Host) -> Result<(), String> {
    if configured_helper(config, REGISTRY)
        .is_some_and(|name| host.credential_helpers().contains(&name))
    {
        Ok(())
    } else {
        Err(guidance(format!(
            "Configure a Docker credential helper first: {}. The app will not save a token as plaintext in Docker config.json.",
            host.credential_helpers().join(", ")
        )))
    }
}

/// Guidance when Docker's settings name a credential helper whose program is
/// not installed. Docker asks that helper before every download from
/// `registry`, even of a public image, so every pull would fail with
/// "error getting credentials". This happens when Docker Desktop is removed
/// and Docker is then provided by Colima or another engine: its settings still
/// name `desktop`.
fn missing_helper_guidance(
    config: &serde_json::Value,
    registry: &str,
    config_path: &Path,
    host: Host,
    installed: impl Fn(&str) -> bool,
) -> Option<String> {
    let helper = configured_helper(config, registry).filter(|name| !name.is_empty())?;
    if installed(helper) {
        return None;
    }
    Some(guidance(format!(
        "Docker cannot download images because its settings file ({}) names the credential helper docker-credential-{helper}, which is not installed. A credential helper is the program Docker uses to keep registry passwords. {} Then retry.",
        config_path.display(),
        host.credential_helper_fix()
    )))
}

fn helper_installed(name: &str) -> bool {
    let program = platform::program_path(&format!("docker-credential-{name}"));
    program.is_absolute() && program.is_file()
}

/// Guidance when the credential helper Docker would use for `registry` is not
/// installed; `None` when it is, or when Docker has no readable settings file
/// (Docker then reports its own error).
pub fn missing_credential_helper(registry: &str) -> Option<String> {
    let path = docker_config_path()?;
    let config: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
    missing_helper_guidance(&config, registry, &path, Host::current(), helper_installed)
}

pub fn login_pat(runner: &impl RegistryRunner, username: &str, token: &str) -> Result<(), String> {
    if username.is_empty()
        || username.len() > 39
        || username.starts_with('-')
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err("Enter a valid GitHub username.".to_owned());
    }
    if token.is_empty() || token.len() > 4096 || token.chars().any(char::is_whitespace) {
        return Err(
            "Enter a GitHub classic PAT without whitespace, with read:packages access.".to_owned(),
        );
    }
    runner.require_credential_helper()?;
    runner
        .run(
            "docker",
            &[
                "login",
                REGISTRY,
                "--username",
                username,
                "--password-stdin",
            ],
            Some(token),
        )
        .map(|_| ())
        .map_err(|_| {
            "GHCR sign-in failed. Check the token, username, and Docker Desktop credential helper."
                .to_owned()
        })
}

pub fn login_gh(runner: &impl RegistryRunner) -> Result<(), String> {
    runner.require_credential_helper()?;
    let username = runner
        .run(
            "gh",
            &["api", "user", "--hostname", GITHUB, "--jq", ".login"],
            None,
        )
        .map_err(|_| {
            "GitHub CLI is unavailable or not signed in. Install gh, then choose Sign in with gh."
                .to_owned()
        })?;
    let token = runner
        .run("gh", &["auth", "token", "--hostname", GITHUB], None)
        .map_err(|_| "GitHub CLI has no usable token. Choose Sign in with gh.".to_owned())?;
    login_pat(runner, &username, &token)
}

pub fn verify_image(runner: &impl RegistryRunner, lock: &RuntimeLock) -> Result<(), String> {
    lock.validate().map_err(|error| error.to_string())?;
    if !lock.image.starts_with("ghcr.io/") {
        return Err("This runtime is not hosted on GHCR.".to_owned());
    }
    runner.run("docker", &["manifest", "inspect", &lock.image], None)
        .map(|_| ())
        .map_err(|_| "Cannot read the pinned runtime image. Check network access and package permissions. Your token needs read:packages and may need organization SSO authorization. GitHub CLI users can choose Sign in with gh to grant the scope.".to_owned())
}

/// Launch a visible login terminal with fixed arguments and no credential values.
pub fn complete_gh_login(runner: &impl RegistryRunner) -> Result<(), String> {
    runner.require_credential_helper()?;
    let session = tempfile::Builder::new()
        .prefix("ua-github-login-")
        .tempdir()
        .map_err(|_| "Could not create the temporary GitHub login session.".to_owned())?;
    let result = session.path().join("result");
    launch_gh_login(runner, &result)?;
    wait_for_login(&result, std::time::Duration::from_secs(300))
}

fn wait_for_login(result: &std::path::Path, timeout: std::time::Duration) -> Result<(), String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match std::fs::read_to_string(result) {
            Ok(value) if value.trim() == "0" => return Ok(()),
            Ok(value) if !value.trim().is_empty() => return Err("GitHub sign-in did not complete. Try Sign in with gh again, or use an existing GitHub CLI account.".to_owned()),
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err("Could not read the GitHub login result. Try again.".to_owned()),
            _ => (),
        }
        if std::time::Instant::now() >= deadline {
            return Err("GitHub sign-in timed out after five minutes. Close the login terminal and retry. If you completed sign-in, choose Use GitHub CLI.".to_owned());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn login_script(host: Host, executable: &str, result: &str) -> String {
    match host {
        Host::Windows => format!(
            "Remove-Item Env:GH_TOKEN,Env:GITHUB_TOKEN -ErrorAction SilentlyContinue; $code = 1; try {{ & '{}' auth login --hostname github.com --web --scopes read:packages; $code = $LASTEXITCODE }} finally {{ if (Test-Path -LiteralPath '{}') {{ [System.IO.File]::WriteAllText('{}', [string]$code) }} }}; exit $code",
            executable.replace('\'', "''"),
            std::path::Path::new(result)
                .parent()
                .unwrap()
                .to_string_lossy()
                .replace('\'', "''"),
            result.replace('\'', "''"),
        ),
        _ => format!(
            "unset GH_TOKEN GITHUB_TOKEN; {} auth login --hostname github.com --web --scopes read:packages; code=$?; if test -d {}; then printf '%s' \"$code\" > {}; fi; exit \"$code\"",
            shell_quote(executable),
            shell_quote(
                &std::path::Path::new(result)
                    .parent()
                    .unwrap()
                    .to_string_lossy()
            ),
            shell_quote(result),
        ),
    }
}

fn launch_gh_login(runner: &impl RegistryRunner, result: &std::path::Path) -> Result<(), String> {
    runner.run("gh", &["--version"], None).map_err(|_| {
        "Install GitHub CLI, then choose Sign in with gh again, or use a classic PAT.".to_owned()
    })?;
    let script = login_script(
        Host::current(),
        &platform::program_path("gh").to_string_lossy(),
        &result.to_string_lossy(),
    );
    match Host::current() {
        Host::Mac => runner.run("osascript", &[
            "-e", "on run argv", "-e", "tell application \"Terminal\"",
            "-e", "activate", "-e", "do script (item 1 of argv)", "-e", "end tell", "-e", "end run", "--",
            &format!("sh -c {}", shell_quote(&script)),
        ], None).map(|_| ()),
        Host::Windows => runner.launch("powershell", &["-NoLogo", "-NoProfile", "-Command", &script]),
        Host::Linux => {
            let terminal = ["x-terminal-emulator", "gnome-terminal", "konsole", "xfce4-terminal", "xterm"]
                .into_iter().find(|name| platform::program_path(name).is_file())
                .ok_or("No supported terminal found. Run gh auth login --hostname github.com --web --scopes read:packages in your terminal, then choose Use GitHub CLI.")?;
            let separator = if terminal == "gnome-terminal" { "--" } else { "-e" };
            runner.launch(terminal, &[separator, "sh", "-c", &script])
        }
    }.map_err(|_| "Could not open the login terminal. Run gh auth login --hostname github.com --web --scopes read:packages in your terminal, then choose Use GitHub CLI.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    type Call = (String, Vec<String>, Option<String>);
    struct Fake {
        calls: RefCell<Vec<Call>>,
        fail: bool,
        helper: bool,
    }
    impl RegistryRunner for Fake {
        fn run(&self, program: &str, args: &[&str], input: Option<&str>) -> Result<String, String> {
            self.calls.borrow_mut().push((
                program.into(),
                args.iter().map(|s| s.to_string()).collect(),
                input.map(str::to_owned),
            ));
            if self.fail {
                return Err("secret-should-not-escape".into());
            }
            Ok(if args.first() == Some(&"api") {
                "octocat"
            } else {
                "secret-should-not-escape"
            }
            .into())
        }
        fn require_credential_helper(&self) -> Result<(), String> {
            if self.helper {
                Ok(())
            } else {
                Err("helper required".into())
            }
        }
    }
    fn fake() -> Fake {
        Fake {
            calls: RefCell::new(vec![]),
            fail: false,
            helper: true,
        }
    }
    #[test]
    fn pat_only_uses_stdin() {
        let runner = fake();
        login_pat(&runner, "octocat", "secret-should-not-escape").unwrap();
        let calls = runner.calls.borrow();
        assert_eq!(
            calls[0].1,
            [
                "login",
                "ghcr.io",
                "--username",
                "octocat",
                "--password-stdin"
            ]
        );
        assert_eq!(calls[0].2.as_deref(), Some("secret-should-not-escape"));
    }
    #[test]
    fn no_plaintext_fallback_or_secret_in_errors() {
        let mut runner = fake();
        runner.helper = false;
        assert!(login_pat(&runner, "octocat", "secret-should-not-escape").is_err());
        assert!(runner.calls.borrow().is_empty());
        runner.helper = true;
        runner.fail = true;
        assert!(
            !login_pat(&runner, "octocat", "secret-should-not-escape")
                .unwrap_err()
                .contains("secret-should-not-escape")
        );
    }
    #[test]
    fn gh_token_is_backend_only_and_registry_is_fixed() {
        let runner = fake();
        login_gh(&runner).unwrap();
        let calls = runner.calls.borrow();
        assert_eq!(calls[1].1, ["auth", "token", "--hostname", "github.com"]);
        assert_eq!(calls[2].2.as_deref(), Some("secret-should-not-escape"));
        assert!(!format!("{:?}", calls[2].1).contains("secret-should-not-escape"));
    }
    #[test]
    fn named_but_missing_helper_explains_the_fix_for_this_os() {
        let path = Path::new("/Users/me/.docker/config.json");
        let config = serde_json::json!({"credsStore":"desktop"});

        let error = missing_helper_guidance(&config, REGISTRY, path, Host::Mac, |_| false)
            .expect("a missing helper is reported");

        assert!(crate::guidance::is_guidance(&error));
        assert!(
            error.contains("docker-credential-desktop, which is not installed"),
            "{error}"
        );
        assert!(error.contains("/Users/me/.docker/config.json"), "{error}");
        assert!(error.contains(Host::Mac.credential_helper_fix()), "{error}");
        assert!(
            missing_helper_guidance(&config, REGISTRY, path, Host::Mac, |name| name == "desktop")
                .is_none()
        );
    }

    #[test]
    fn missing_helper_check_follows_the_helper_docker_would_use() {
        let path = Path::new("config.json");
        let installed = |name: &str| name == "osxkeychain";
        // A per-registry helper wins over the global store.
        let per_registry =
            serde_json::json!({"credsStore":"desktop","credHelpers":{"ghcr.io":"osxkeychain"}});
        assert!(
            missing_helper_guidance(&per_registry, REGISTRY, path, Host::Mac, installed).is_none()
        );
        let other_registry =
            serde_json::json!({"credsStore":"desktop","credHelpers":{"docker.io":"osxkeychain"}});
        assert!(
            missing_helper_guidance(&other_registry, REGISTRY, path, Host::Mac, installed)
                .is_some()
        );
        // No helper means Docker keeps no stored password to look up.
        for config in [
            serde_json::json!({}),
            serde_json::json!({"credsStore":""}),
            serde_json::json!({"credsStore":"desktop","credHelpers":{"ghcr.io":""}}),
        ] {
            assert!(
                missing_helper_guidance(&config, REGISTRY, path, Host::Mac, |_| false).is_none(),
                "{config}"
            );
        }
    }

    #[test]
    fn every_os_has_its_own_helper_fix() {
        assert!(Host::Mac.credential_helper_fix().contains("osxkeychain"));
        assert!(Host::Windows.credential_helper_fix().contains("wincred"));
        assert!(
            Host::Linux
                .credential_helper_fix()
                .contains("secretservice")
        );
    }

    #[test]
    fn helper_override_cannot_enable_plaintext_storage() {
        for config in [
            serde_json::json!({}),
            serde_json::json!({"credsStore":""}),
            serde_json::json!({"credsStore":"desktop", "credHelpers":{"ghcr.io":""}}),
        ] {
            assert!(require_helper(&config, Host::Mac).is_err());
        }
        assert!(require_helper(&serde_json::json!({"credsStore":"desktop"}), Host::Mac).is_ok());
        assert!(
            require_helper(
                &serde_json::json!({"credHelpers":{"ghcr.io":"osxkeychain"}}),
                Host::Mac
            )
            .is_ok()
        );
    }

    #[test]
    fn credential_helpers_are_os_specific_and_overrides_cannot_bypass_validation() {
        for host in [Host::Mac, Host::Windows, Host::Linux] {
            for helper in host.credential_helpers() {
                assert!(require_helper(&serde_json::json!({"credsStore":helper}), host).is_ok());
                assert!(
                    require_helper(
                        &serde_json::json!({"credsStore":helper,"credHelpers":{"ghcr.io":""}}),
                        host
                    )
                    .is_err()
                );
            }
            assert!(require_helper(&serde_json::json!({"credsStore":"plain"}), host).is_err());
        }
        assert!(
            require_helper(
                &serde_json::json!({"credsStore":"osxkeychain"}),
                Host::Windows
            )
            .is_err()
        );
        assert!(require_helper(&serde_json::json!({"credsStore":"wincred"}), Host::Linux).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_login_launches_powershell_without_profile_or_token_arguments() {
        let runner = fake();
        launch_gh_login(&runner, &std::env::temp_dir().join("ua-test-result")).unwrap();
        let calls = runner.calls.borrow();
        assert_eq!(calls[1].0, "powershell");
        assert!(calls[1].1.contains(&"-NoProfile".to_owned()));
        assert!(!calls[1].1.contains(&"-NoExit".to_owned()));
        assert!(calls[1].2.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn windows_process_uses_stdin_and_discards_secret_output_on_failure() {
        let output = ProcessRegistry.run("powershell", &["-NoProfile", "-NonInteractive", "-Command", "$value = [Console]::In.ReadLine(); if ($env:GH_TOKEN -or $env:GITHUB_TOKEN) {exit 1}; [Console]::Write($value)"], Some("test-only-secret\n")).unwrap();
        assert_eq!(output, "test-only-secret");
        let error = ProcessRegistry
            .run(
                "powershell",
                &[
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "Write-Output 'test-only-secret'; exit 1",
                ],
                None,
            )
            .unwrap_err();
        assert!(!error.contains("test-only-secret"));
    }

    #[test]
    fn verifies_exact_locked_digest_and_handles_denial() {
        let lock = RuntimeLock::bundled().unwrap();
        let mut runner = fake();
        verify_image(&runner, &lock).unwrap();
        assert_eq!(
            runner.calls.borrow()[0].1,
            ["manifest", "inspect", &lock.image]
        );
        runner.fail = true;
        let error = verify_image(&runner, &lock).unwrap_err();
        assert!(error.contains("read:packages"));
        assert!(!error.contains("secret-should-not-escape"));
    }

    #[cfg(unix)]
    #[test]
    fn real_process_passes_stdin_and_discards_failure_output() {
        let output = ProcessRegistry.run("/bin/sh", &["-c", "IFS= read -r value; test -z \"${GH_TOKEN+x}\" && test -z \"${GITHUB_TOKEN+x}\" && printf '%s' \"$value\""], Some("test-only-secret\n")).unwrap();
        assert_eq!(output, "test-only-secret");
        let error = ProcessRegistry
            .run(
                "/bin/sh",
                &[
                    "-c",
                    "printf test-only-secret; printf test-only-secret >&2; exit 1",
                ],
                None,
            )
            .unwrap_err();
        assert!(!error.contains("test-only-secret"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn browser_login_uses_fixed_command_without_credentials() {
        let runner = fake();
        launch_gh_login(&runner, &std::env::temp_dir().join("ua-test-result")).unwrap();
        let calls = runner.calls.borrow();
        assert_eq!(calls[0].0, "gh");
        assert_eq!(calls[1].0, "osascript");
        assert!(
            calls[1]
                .1
                .last()
                .unwrap()
                .contains("--scopes read:packages")
        );
        assert!(calls[1].2.is_none());
    }

    #[test]
    fn login_result_requires_this_sessions_success_and_has_a_deadline() {
        let session = tempfile::tempdir().unwrap();
        let result = session.path().join("result");
        let zero = std::time::Duration::ZERO;
        assert!(
            wait_for_login(&result, zero)
                .unwrap_err()
                .contains("timed out")
        );
        for code in ["1", "130", "invalid"] {
            std::fs::write(&result, code).unwrap();
            assert!(wait_for_login(&result, zero).is_err());
        }
        std::fs::write(&result, "0").unwrap();
        assert!(wait_for_login(&result, zero).is_ok());
    }

    #[test]
    fn real_login_wrapper_records_success_and_failure_with_quoted_paths() {
        let session = tempfile::Builder::new()
            .prefix("ua login's ")
            .tempdir()
            .unwrap();
        let result = session.path().join("result's file");
        for code in [0, 1] {
            let executable = session.path().join(if cfg!(windows) {
                "fake gh.ps1"
            } else {
                "fake gh"
            });
            #[cfg(windows)]
            std::fs::write(
                &executable,
                format!("if ($env:GH_TOKEN -or $env:GITHUB_TOKEN) {{exit 99}}; exit {code}"),
            )
            .unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::write(&executable, format!("#!/bin/sh\ntest -z \"${{GH_TOKEN+x}}\" && test -z \"${{GITHUB_TOKEN+x}}\" || exit 99\nexit {code}\n")).unwrap();
                std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
                    .unwrap();
            }
            let script = login_script(
                Host::current(),
                executable.to_str().unwrap(),
                result.to_str().unwrap(),
            );
            let mut command = if cfg!(windows) {
                let mut cmd = platform::command("powershell");
                cmd.args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-Command",
                    &script,
                ]);
                cmd
            } else {
                let mut cmd = platform::command("sh");
                cmd.args(["-c", &script]);
                cmd
            };
            let status = command
                .env("GH_TOKEN", "test-token")
                .env("GITHUB_TOKEN", "test-token")
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(code));
            assert_eq!(std::fs::read_to_string(&result).unwrap(), code.to_string());
        }
    }

    #[test]
    fn invalid_credentials_never_reach_a_process() {
        let runner = fake();
        for (username, token) in [
            ("--help", "secret"),
            ("octocat", "secret\n"),
            ("", "secret"),
            ("octocat", ""),
        ] {
            assert!(login_pat(&runner, username, token).is_err());
        }
        assert!(runner.calls.borrow().is_empty());
    }
}
