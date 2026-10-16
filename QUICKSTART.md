# Quickstart

Download [Desktop 0.6.0](https://github.com/enzimo/universal-agents-desktop/releases/tag/v0.6.0) and follow the three steps below. For provider signup, configuration fields, and server hosting, download [the comprehensive HTML setup guide](https://github.com/enzimo/universal-agents/releases/latest/download/setup-guide.html) and open it in your browser.

Already using a hosted service? Open its URL in your browser and sign in. You do not need local Docker or the desktop launcher; choose **Hosted server** in the guide.

## 1. Start Docker

- **macOS:** install [Homebrew](https://brew.sh), then run:

  ```sh
  brew install colima docker
  colima start universal-agents --cpu 2 --memory 4 --disk 40
  docker context use colima-universal-agents
  ```

- **Windows 11 x64:** run `wsl --install` in administrator PowerShell and restart if prompted. Install [Docker Desktop](https://docs.docker.com/desktop/setup/install/windows-install/), choose its WSL 2 backend, and start it in **Linux containers** mode. Install the native Windows app outside WSL.
- **Ubuntu 24.04 x64 desktop:** install [Docker Engine](https://docs.docker.com/engine/install/ubuntu/) and enable [access for your normal user](https://docs.docker.com/engine/install/linux-postinstall/). Sign out and back in after changing group membership.

Confirm `docker info` succeeds in your normal terminal. On macOS, run `colima start universal-agents` again after restarting the computer. On Windows, start Docker Desktop before the launcher.

## 2. Install the app

Choose one installer from the release and verify its SHA-256 checksum using the HTML guide.

| Platform | Installer |
| --- | --- |
| macOS, Apple Silicon or Intel | Universal DMG; open it and drag Universal Agents to Applications |
| Windows 11 x64 | Setup EXE; use the MSI if you prefer Windows Installer |
| Ubuntu 24.04 x64 desktop | DEB; run `sudo apt install ./Universal-Agents-Desktop-0.6.0-Linux-x64.deb` |

The macOS app is ad-hoc signed and not notarized, and Windows installers are unsigned. If macOS blocks a trusted download, use **System Settings > Privacy & Security > Open Anyway**; on Windows, choose **More info** then **Run anyway**. Do not disable Gatekeeper or SmartScreen system-wide. Linux AppImage is also available; see the HTML guide for FUSE setup.

## 3. Open Universal Agents

1. Open the app, choose **Click to Begin** on the title screen, then choose **Start**. Leave the Status window open while it downloads the runtime image.
2. Under **Agent workspace**, optionally choose a project folder agents may read and change. Agents see it as the team's shared folder, `team/team-shared`. With Colima on macOS, pick a folder inside your home folder. Otherwise keep the app-managed folder.
3. At **Setup ready**, choose **Open in app**. Enter your user and assistant names, select a model provider and model, add its credential, and use **Test Connection**. Keep local storage and Web Chat defaults for your first run.
4. Save the access token shown during setup, choose **Launch team**, sign in, and send a message. Type `/tour` for a walkthrough.

**Refresh external skills on start** is on by default: UA pulls the repositories approved during onboarding before agents launch. Turn it off in the Status window for reproducible offline startup or manual skill updates. The setting applies on the next Start or Restart.

<details>
<summary>Optional: GitHub authentication for restricted downloads</summary>

Public installers and runtime images need no special access. If downloads are restricted, install [GitHub CLI](https://cli.github.com/) and run `gh auth login --hostname github.com --web --scopes read:packages`. Your account needs access to both repositories and the runtime package.

In the app, expand **Advanced Settings > Private Image Access**. Choose **Sign in with gh** to log in through your browser and continue installation automatically, or **Use GitHub CLI** if you already signed in. A Docker credential helper must be configured first; the HTML guide has platform-specific instructions. If Docker is already signed in, choose **Verify access and retry**.

The current in-app update checker also uses GitHub CLI. Public updates can always be downloaded from the releases page without signing in.

</details>

Closing the window keeps the app in the background. **Stop** stops UA while preserving your data. In the header, **Close** quits only the launcher and leaves the runtime running, and **Quit** stops UA first, then quits the launcher.

If an operation fails and the app knows the fix, such as installing a missing Docker credential helper, the Status window says what to do. Otherwise choose **Download diagnostics** beside Recent runtime logs to save the exact error and logs.

See [using the launcher](docs/USAGE.md) for workspace changes, subscription credential import, updates, and data locations. For startup errors, see [troubleshooting](docs/TROUBLESHOOTING.md).
