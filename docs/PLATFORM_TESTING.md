# Desktop platform testing

For end-user installation, use [Quickstart](../QUICKSTART.md) or the [offline setup guide](setup.html).

For Windows, Linux and macOS build-machine provisioning, open the [interactive setup guide](remote-build-setup.html). It covers SSH, native toolchains, Docker build/run checks, self-hosted runners, desktop sessions and the connection handoff.

The active **Local runner preflight** workflow checks the managed local runners. All active workflows now require local labels and installed cleanup hooks. See [Local CI](LOCAL_CI.md) for toolchains, WSL, runner registration, and resource reclamation. GitHub organization permissions are needed to activate the complete fleet.

## Supported starting points

- Windows 11 x64 with Docker Desktop running **Linux containers**, and the WebView2 runtime. The installer can provision WebView2. Run the launcher as your normal user. Choose local drive folders; UNC shares, WSL network paths, and device paths are not supported as mounts.
- Ubuntu 24.04 x64 desktop VM with a working graphical session, Docker Engine. Verify that `docker info` succeeds as your normal user. A console-only VM is insufficient for testing the launcher UI.
- A dedicated Mac on a Docker-supported macOS version with Command Line Tools, Rust, Node.js, GitHub CLI and Colima with the Docker CLI. Keep the runner and Docker under the signed-in build account for GUI tests. A universal app build includes Intel and Apple Silicon binaries; execution tests cover only the host architecture.

The locked UA image supports Linux amd64 and arm64. An ARM Linux VM on an Apple Silicon Mac can test an ARM build from source, but it cannot directly install the x64 CI package. Use a matching architecture.

Docker CLI builds and runs work on all three hosts. GitHub container actions and job service containers require a Linux runner. Use one native runner per host and a separate Linux runner inside WSL 2. Each runner owns its work directory. Serialize full UA lifecycle tests per Docker daemon, including tests launched over SSH. They share fixed container names and loopback ports. Keep runner caches outside UA data, and preserve persistent test data until an explicit disposable-data cleanup design exists.

## Obtain builds

The manual **Windows runner build** workflow targets the signed-in Windows runner with labels `self-hosted`, `Windows`, `X64`, `ua-build`, and `ua-windows`. It runs the native tests and popup probe, then uploads development installers and checksums. It does not start or change a UA runtime. Keep the runner in the interactive build account.

The **Windows and Linux builds** workflow produces `universal-agents-windows-x64-development` and `universal-agents-linux-x64-development` artifacts. Windows contains an NSIS EXE and MSI; Linux contains a DEB and AppImage. Use one installer per platform. These are development previews; the Windows packages are unsigned.

On Ubuntu, prefer the DEB to install declared dependencies through `sudo apt install ./package.deb`. AppImage may require FUSE 2 or extraction on newer distributions. For GNOME tray access, enable an AppIndicator extension. Closing the launcher hides it; relaunching it restores the same window even without a tray extension.

## Optional private-access prerequisites

Public downloads and images need no authentication. The following applies when a repository or image requires access.

Configure a Docker credential helper before using the launcher's GitHub CLI or PAT sign-in. Supported helpers are `desktop`/`wincred` on Windows and `desktop`/`secretservice`/`pass` on Linux. For Linux, initialize the selected helper and unlock its keyring before testing. The launcher reads Docker's configuration and refuses plaintext credential storage. It does not configure or overwrite helper settings.

`Sign in with gh` opens PowerShell on Windows, or x-terminal-emulator, GNOME Terminal, Konsole, Xfce Terminal, or xterm on Linux. If no supported terminal is available, run `gh auth login --hostname github.com --web --scopes read:packages` manually and then choose **Use GitHub CLI**. Successful in-app terminal login must continue automatically through Docker login, image verification, and startup. A failed or timed-out login must leave retry available. Update checks use the same stored GitHub CLI account.

## End-to-end checklist

For automated native launcher checks on Mac and Windows, follow [the UI test setup](../tests/ui/README.md). It records screenshots and tests the real bundled UI against a simulated runtime boundary. The checklist below covers the remaining real-host acceptance work.

1. Install and open the launcher. Confirm the title intro and **Click to Begin**, the one-time background fill, the dark default appearance and the sun/moon theme switch, both versions, collapsed image access, and header controls.
2. With Docker stopped, verify actionable status; start Docker and verify recovery.
3. Choose a workspace whose path includes spaces. Choose an `auth.json` outside that workspace and app data. Verify overlapping paths are rejected.
4. Install/start the runtime (sign into GHCR only if access is required). Confirm the local setup page opens and Codex import allows the connection test. Complete onboarding and send a chat message.
5. Open Workspace Files in the embedded app. Verify session continuity, browser authentication links, Open in Browser, and Read The Docs.
   Ask an agent to list `team/team-shared` and to create a small file there with a shell or Python command. Confirm it sees the chosen folder's existing files and that the new file appears in that folder on the host. This checks the contract 3 mount and UA's Landlock code sandbox together.
   After onboarding, stop UA, put a file in `ua-data/workspace`, and start UA. The status must read **Stopped while starting** and quote UA's old-layout reason, with Stop available. Stop UA and remove that file afterward.
6. Confirm logs refresh every three seconds while visible. Stop/restart and verify the workspace, history, and imported subscription remain.
7. Toggle Start at login, sign out/in, and verify a single background instance. Relaunch to show the controls. Turn it off and verify the preference persists.
8. Open **Updates** and choose **Check now**. Verify the header row stays aligned, the launcher lane names the newest launcher release and **Open download page** opens it in the browser, and the UA lane shows release notes and approval gating. Under **Other versions**, confirm the running version says Running, older versions offer only Notes, and **Choose** on a newer version moves the steps to it. Only test an upgrade when a newer listed version is available; don't alter runtime data or fabricate a release. Turn on **Offer pre-release and unchecked versions** and confirm the offer is marked unchecked with its warning, then turn it off again.
9. Quit and reopen, then replace the desktop installation with a newer build. Confirm persistent data and the selected runtime remain intact.

UA's agent sandbox was verified on 2026-10-11 with Docker's default security profile on Colima 0.10.3 (Ubuntu 24.04 VM, kernel 6.8, Landlock ABI 4) and Docker Desktop 29.8.2 on Windows (WSL2 kernel 6.18, ABI 7). On each Docker VM, a sandboxed command could write only inside its folder, network access was blocked when requested, and a host folder mounted inside the team folder was readable and writable. Docker Desktop on macOS was not tested.

Report the app version, OS/architecture, Docker version/context, failed step, and visible error. Do not include tokens or `auth.json` contents.

## Build from source

Install Rust, Node.js, and Tauri's platform prerequisites. From the repository root run `cargo test --workspace --locked`, `node --test ui/desktop-shell/app.test.cjs`, and `npx --yes @tauri-apps/cli@2.11.4 build`. Tauri merges `tauri.windows.conf.json` or `tauri.linux.conf.json` automatically.

On Windows, run authenticated Docker builds and popup tests through the foreground runner in the signed-in desktop. Key-based SSH can build native installers, but its logon session may not unlock the desktop credential store. The popup example receives its own Common Controls v6 manifest from the build script.

The native popup probe needs a graphical desktop: `cargo run -p universal-agents-desktop --example popup_smoke --locked`. On a Linux CI host use `xvfb-run -a dbus-run-session -- cargo run -p universal-agents-desktop --example popup_smoke --locked`. It uses temporary loopback fixtures, checks shared cookies and rejected desktop commands, checks that a broker-port form opens in an app window and that an external link leaves no empty window (external links go to a recording hook, not the browser), and never starts Docker or reads saved credentials. CI runs it on Windows and Linux; run it by hand on macOS after changing window or link handling.

References: [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/), [Windows installers](https://v2.tauri.app/distribute/windows-installer/), [Docker credential stores](https://docs.docker.com/reference/cli/docker/login/).
