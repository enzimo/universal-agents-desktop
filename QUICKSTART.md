# Quickstart

Download [Desktop 0.6.1](https://github.com/enzimo/universal-agents-desktop/releases/tag/v0.6.1) and follow the steps below. For provider signup, configuration fields, and server hosting, download [the comprehensive HTML setup guide](https://github.com/enzimo/universal-agents/releases/latest/download/setup-guide.html) and open it in your browser.

Already using a hosted service? Open its URL in your browser and sign in. You do not need local Docker or the desktop launcher; choose **Hosted server** in the guide.

## 0. Get access

The desktop app, the UA runtime image and the setup guide are private. You need a GitHub account that the maintainer has invited.

1. Accept both invitations, to `enzimo/universal-agents-desktop` and `enzimo/universal-agents`, from the invitation emails or [github.com/notifications](https://github.com/notifications).
2. Install [GitHub CLI](https://cli.github.com/) and sign in with permission to download packages:

   ```sh
   gh auth login --hostname github.com --web --scopes read:packages
   ```

   The app uses this login to download the runtime image and to check for updates.

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

Download the installer for your computer from the [latest release](https://github.com/enzimo/universal-agents-desktop/releases/latest) while signed in to GitHub in your browser, or with GitHub CLI. Each command downloads the newest release's installer and its `.sha256` checksum file into the current folder:

```sh
# macOS (Apple Silicon or Intel)
gh release download --repo enzimo/universal-agents-desktop --pattern '*-macOS-universal.dmg*'
# Windows 11 x64
gh release download --repo enzimo/universal-agents-desktop --pattern '*-Windows-x64-setup.exe*'
# Ubuntu 24.04 x64
gh release download --repo enzimo/universal-agents-desktop --pattern '*-Linux-x64.deb*'
```

Verify the SHA-256 checksum as the HTML guide describes, then install:

| Platform | Installer |
| --- | --- |
| macOS, Apple Silicon or Intel | Universal DMG; open it and drag Universal Agents to Applications |
| Windows 11 x64 | Setup EXE; use the MSI if you prefer Windows Installer |
| Ubuntu 24.04 x64 desktop | DEB; run `sudo apt install ./Universal-Agents-Desktop-0.6.1-Linux-x64.deb` |

The macOS app is ad-hoc signed and not notarized, and Windows installers are unsigned. If macOS blocks a trusted download, use **System Settings > Privacy & Security > Open Anyway**; on Windows, choose **More info** then **Run anyway**. Do not disable Gatekeeper or SmartScreen system-wide. Linux AppImage is also available; see the HTML guide for FUSE setup.

## 3. Open Universal Agents

1. Open the app and choose **Click to Begin** on the title screen. Expand **Advanced Settings > Private Image Access** and choose **Use GitHub CLI** to give Docker your GitHub login from step 0. Then choose **Start**. Leave the Status window open while it downloads the runtime image.
2. Under **Agent workspace**, optionally choose a project folder agents may read and change. Agents see it as the team's shared folder, `team/team-shared`. With Colima on macOS, pick a folder inside your home folder. Otherwise keep the app-managed folder.
3. At **Setup ready**, choose **Open in app**. Enter your user and assistant names, select a model provider and model, add its credential, and use **Test Connection**. Keep local storage and Web Chat defaults for your first run.
4. Save the access token shown during setup, choose **Launch team**, sign in, and send a message. Type `/tour` for a walkthrough.

**Refresh external skills on start** is on by default: UA pulls the repositories approved during onboarding before agents launch. Turn it off in the Status window for reproducible offline startup or manual skill updates. The setting applies on the next Start or Restart.

<details>
<summary>If the runtime image download is refused</summary>

Docker needs a credential helper before it can save your GitHub login; the app says so and explains the fix. On macOS with Colima, run `brew install docker-credential-helper` and set `"credsStore": "osxkeychain"` in `~/.docker/config.json`. The HTML guide has instructions for each platform.

In **Advanced Settings > Private Image Access**, **Sign in with gh** opens GitHub's browser login and continues installation afterward. **Use GitHub CLI** reuses an existing login. If Docker is already signed in, choose **Verify access and retry**. Access is still refused if your account has not accepted the `enzimo/universal-agents` invitation or your GitHub CLI login lacks `read:packages`.

</details>

Closing the window keeps the app in the background. **Stop** stops UA while preserving your data. In the header, **Close** quits only the launcher and leaves the runtime running, and **Quit** stops UA first, then quits the launcher.

If an operation fails and the app knows the fix, such as installing a missing Docker credential helper, the Status window says what to do. Otherwise choose **Download diagnostics** beside Recent runtime logs to save the exact error and logs.

See [using the launcher](docs/USAGE.md) for workspace changes, subscription credential import, updates, and data locations. For startup errors, see [troubleshooting](docs/TROUBLESHOOTING.md).
