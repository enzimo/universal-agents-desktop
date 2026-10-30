# Quickstart

Install Universal Agents Desktop, start its local Docker container, and set up
your team. Download the app from the
[latest public release](https://github.com/enzimo/universal-agents-desktop/releases/latest).
You do not need a GitHub account to download the installer.

## Watch the desktop setup

<iframe width="560" height="315" style="width:100%;height:auto;aspect-ratio:16/9;border:0" src="https://www.youtube-nocookie.com/embed/Ia_zs5fkPqM" title="Desktop setup: install Universal Agents and launch your team" loading="lazy" allow="encrypted-media;picture-in-picture;web-share" referrerpolicy="strict-origin-when-cross-origin" allowfullscreen></iframe>

[Watch the desktop installation video on YouTube](https://youtu.be/Ia_zs5fkPqM?list=PLPO_4fTKUE6I).
The steps below follow the same sequence: prepare Docker, install the app,
start UA, complete the setup pages, then open your team's chat.

Use the [setup and configuration guide](https://enzimo.github.io/universal-agents/docker-compose-setup.html)
alongside the video. Choose **Desktop app** and your operating system. Its
commands reflect the CPU, memory and disk settings you choose. The video opens
a downloaded copy; the website provides the same guide, and an
[offline copy](https://github.com/enzimo/universal-agents/releases/latest/download/setup-guide.html)
is also available.

Already using a hosted service? Open its URL in your browser and sign in.
Choose **Hosted server** in the guide; you do not need the desktop app or local Docker.

## 1. Prepare your computer

Have an account and API key for your chosen model provider, such as OpenRouter,
OpenAI, Anthropic or Google, ready for the setup pages. The guide links to their
signup instructions. A supported local model server or subscription sign-in is
another option; see the guide for its requirements.

**Keep this computer awake.** Agents run here and stop working while it sleeps.
The launcher names the sleep setting to change on your system. You can also
run UA on a server and use it from a browser, as the setup guide explains.

Install and start Docker:

- **macOS:** install [Homebrew](https://brew.sh), then run in Terminal:

  ```sh
  brew install colima docker
  colima start universal-agents --cpu 2 --memory 4 --disk 40
  docker context use colima-universal-agents
  ```

  Run `colima start universal-agents` again after restarting the Mac.
- **Windows 11 x64:** run `wsl --install` in administrator PowerShell and restart
  if requested. Install [Docker Desktop](https://docs.docker.com/desktop/setup/install/windows-install/),
  choose the WSL 2 backend, and start it in **Linux containers** mode. Install
  the native Windows desktop app outside WSL.
- **Ubuntu 24.04 x64 desktop:** install [Docker Engine](https://docs.docker.com/engine/install/ubuntu/)
  and enable [access for your normal user](https://docs.docker.com/engine/install/linux-postinstall/).
  Sign out and back in after changing group membership.

Run `docker info` in your normal terminal. Continue when it completes without
an error. If the launcher cannot reach Docker, it offers **Install Docker**,
**Setup guide** and **Proceed**; it checks again once Docker starts.

## 2. Download and install the app

Open the [latest release](https://github.com/enzimo/universal-agents-desktop/releases/latest)
in your browser and expand **Assets**. Download the installer for your computer
and its matching `.sha256` file into the same folder. These downloads are public;
GitHub CLI and GitHub sign-in are not required.

| Computer | Download | Install |
| --- | --- | --- |
| macOS, Apple Silicon or Intel | File ending in `-macOS-universal.dmg` | Open the DMG and drag Universal Agents to Applications. |
| Windows 11 x64 | File ending in `-Windows-x64-setup.exe` | Run the setup EXE. An MSI is also available. |
| Ubuntu 24.04 x64 desktop | File ending in `-Linux-x64.deb` | Follow the guide's `sudo apt install` command. An AppImage is also available. |

Use the guide's verification commands to compare the download with its SHA-256
checksum before installing. Select **Desktop app** and the same operating
system so it shows commands for the right installer.

The macOS app is ad-hoc signed and not notarized. If a trusted download is
blocked, use **System Settings > Privacy & Security > Open Anyway**, then confirm
the prompt. Windows installers are unsigned; SmartScreen may offer **More info > Run anyway**.
Keep Gatekeeper and SmartScreen enabled.

## 3. Start Universal Agents

1. Open Universal Agents, choose **Click to Begin**, then **Got it** on the
   reminder about keeping the computer awake.
2. Under **Agent Workspace**, optionally choose a folder agents may read and
   change. Everyone on your UA team can use this shared folder. With Colima,
   choose a folder inside your home folder. Otherwise keep the app-managed folder.
3. The launcher checks Docker and starts UA automatically. Leave it open while
   the progress row shows the current stage and download sizes; the first
   download can take several minutes. If startup needs a retry, choose
   **Install & start**, or **Start** on later launches. Public images need no registry sign-in; leave
   **Private Image Access** closed unless you use a private image.
4. When the launcher says **Setup ready**, choose **Open in app**. **Open in browser**
   opens the same setup in your browser.

## 4. Complete the setup pages

Configuration downloads inside the app open a native save dialog. Choose a
destination for each file, or cancel to discard it. A download or save failure
shows an error; **Open in browser** offers the same setup as an alternative.

Follow the pages shown in the video:

1. **Welcome:** choose a theme, then continue.
2. **Choose Your Starting Point:** choose **I am a new user** for your first
   team. Use the import option when transferring an existing team instead.
3. **Identity:** enter your name and role, name your assistant, and review its
   purpose and response style.
4. **Set Up Your Runtime:** choose your model provider, its credential and a
   model that provider supports. Fresh setups select OpenRouter by default;
   change the model too if you choose another provider. Choose **Test Connection**
   and continue after it succeeds. Keep the local storage defaults for your first run.
5. **Safety:** keep the generated service credentials and other defaults for
   your first setup.
6. **Let's Talk:** keep Web Chat enabled. Create or generate its access token
   and save it in your password manager; you need it to sign in after launch.
   Other chat channels can wait.
7. **Preferences, Team and Secrets:** set quiet hours, keep or choose specialists,
   and optionally add Exa or Brave search credentials. Review which services
   you share with other people on your team.
8. **Review:** choose **Finalize Configuration**, then download both configuration
   files and keep them somewhere private as your backup. Choose **Launch**.
9. Wait for the container to restart and **Your Team Has Started** to appear.
   Choose **Open Status**, enter the saved Web Chat token, then choose **Unlock Chat**.

If you use a ChatGPT/Codex subscription, follow
[subscription login import](docs/USAGE.md#use-a-chatgptcodex-subscription)
before testing that provider. Choosing a subscription provider does not import
your login automatically.

## 5. Talk to your team

Send a message or type `/tour` for a walkthrough. The launcher offers **Open in app**
and **Open in browser** whenever UA is ready.

Closing a window leaves the team running. **Stop** stops the team and preserves
its data; **Start** resumes it. In the header, **Close** quits the launcher while
leaving UA running, and **Quit** stops UA and quits the launcher. Agents cannot
work while UA is stopped or the computer sleeps.

**Refresh external skills on start** is on by default. Turn it off in the
launcher for reproducible offline startup or manual skill updates; the change
applies on the next Start or Restart.

If a failure does not explain its fix, choose **Download diagnostics** beside
Recent Runtime Logs. See [using the launcher](docs/USAGE.md) and
[troubleshooting](docs/TROUBLESHOOTING.md) for further help.

## Optional GitHub CLI and private images

The current app uses signed-in [GitHub CLI](https://cli.github.com/) for
**Updates**, even when repositories are public. This is an app dependency, not
a restriction on public downloads. Without it, a public runtime can still
start using the version built into the launcher, but checking the compatibility
list and in-app updates fails. You can download newer desktop installers from
the public release page without signing in.

To enable the current in-app update checker, install GitHub CLI and run
`gh auth login --hostname github.com --web`. Public runtime images do not need
the extra `read:packages` permission.

<details>
<summary>Only if you use a private runtime image</summary>

Your account needs access to that package. Sign in with package-read permission:

```sh
gh auth login --hostname github.com --web --scopes read:packages
```

Docker also needs a credential helper to store that login. On macOS with Colima,
install it with `brew install docker-credential-helper`, then set
`"credsStore": "osxkeychain"` in `~/.docker/config.json` as the setup guide describes.

In **Advanced Settings > Private Image Access**, choose **Use GitHub CLI** to
reuse your login, or **Sign in with gh** to sign in through a visible terminal.
Both verify access, then download and start UA automatically. If Docker already
has a working login, choose **Verify access and retry**.

</details>

If Docker reports a missing credential-helper program while pulling a public
image, repair the existing helper setting as
[troubleshooting](docs/TROUBLESHOOTING.md) describes. Public downloads do not
require adding a GitHub login.
