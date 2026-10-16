# Using the launcher

Start with [Quickstart](../QUICKSTART.md) or download the [HTML setup guide](https://github.com/enzimo/universal-agents/releases/latest/download/setup-guide.html).

## Choose an agent workspace

In the Status window, choose **Choose folder** under **Agent workspace** and select the project directory agents may read and change. The app mounts that folder read/write as UA's shared team folder, `team/team-shared` (`/data/workspaces/team/team-shared` inside the container). UA tells agents to use that folder when people and agents work on the same files, and everyone on your UA team can use it. Each person's private agent folder stays in app data. The app does not grant access to other personal folders.

If you do not select a folder, the shared team folder is `<app-data>/ua-data/workspaces/team/team-shared`. UA creates it the first time it starts.

With Colima on macOS, choose a folder inside your home folder. Colima shares only your home folder by default; a folder elsewhere appears empty to agents, and their files never reach your Mac.
The app-data directory normally is:

| Platform | App data |
| --- | --- |
| macOS | `~/Library/Application Support/com.universalagents.desktop` |
| Windows | `%APPDATA%\com.universalagents.desktop` |
| Linux | `${XDG_DATA_HOME:-~/.local/share}/com.universalagents.desktop` |

Back up `ua-data` while the runtime is stopped. Desktop settings are stored
beside it in `desktop-settings.json`.

Changing the selection safely recreates the desktop-managed container. The app preserves `/data` and does not delete either workspace.

If the selected folder moves or is disconnected, the app opens to a recovery state instead of silently using another workspace. Choose the replacement folder or **Use app workspace** before starting UA again.

## Use a ChatGPT/Codex subscription

The desktop app imports an existing file-backed Codex login. If `~/.codex/auth.json` already exists, choose **Choose auth.json** under **OpenAI subscription** and select it.

If Codex stores your login in the macOS Keychain and no file exists, create a file-backed login in Terminal:

```bash
codex login --device-auth -c 'cli_auth_credentials_store="file"'
```

Then select `~/.codex/auth.json`. Selection saves the path without starting Docker. If image access is blocked, use the optional private-access instructions in the setup guide and start UA first. On a new installation, the app imports automatically as soon as the setup server is ready, before onboarding finishes. Leave the Codex access token blank and use **Test Connection** in setup. **Finish import** is available during setup and normal operation for a manual retry.

If a pending source moves, choose its replacement or cancel the pending import. UA can keep running while the credential import waits.

The source file is mounted read-only in a network-disabled, one-shot helper container during import. The helper shares UA's `/data`, runs the supported importer, and exits. The long-running agent container never receives the source mount. Do not put `auth.json` inside the selected agent workspace.

Closing the window leaves the desktop host in the menu bar. Stopping the runtime is explicit. **Close** (in the header, next to Stop) quits the launcher without stopping the container; **Quit** stops the runtime and quits the launcher in one step. The tray menu's **Quit Desktop App** and **Stop UA and Quit** do the same. Both preserve your workspace and persistent data.

**Open folder** under Agent workspace reveals the folder agents are currently using in your file manager. It does not change the selection.

## Refresh external skills on start

During UA onboarding, keep or change the approved external skill repository URLs. **Refresh external skills on start** is on by default, so UA pulls those repositories before agents launch rather than starting from a possibly stale local copy. The setting applies on the next Start or Restart. Turn it off when you need reproducible offline startup or want to update skills manually.

If you are upgrading from an earlier desktop build, the switch appears on and the next Start or Restart applies it, unless you had already turned it off. Turn the switch off to keep startup offline-reproducible.

## If Docker is not running

Universal Agents runs in a local Docker container, so the launcher needs Docker. When it cannot reach Docker at launch, it shows a blocking dialog with **Install Docker**, **Setup guide**, and **Proceed**.

**Proceed** opens the launcher with the controls that need Docker disabled and an amber banner explaining that it will keep checking every 30 seconds. Quitting, refreshing status and logs, exporting diagnostics, opening the docs, changing autostart, and choosing a workspace or credential source stay available, so you can recover without restarting the app. Once Docker answers, a green banner confirms it and the controls turn back on.

## Updates

Choose **Updates** in the header, or the floating badge at the bottom right, to open the Updates drawer. The header button shows how many updates are waiting. The badge briefly shows the most useful one (a download in progress, a downloaded update, an available UA version, or a newer launcher): it hides after 5 seconds or when you close it, and shows again when that update reaches its next stage.

The drawer has two parts:

- **Desktop launcher.** When a newer launcher is released, choose **Open download page**. It opens that release on GitHub in your browser. Download the installer for your computer and run it, then reopen the launcher. Universal Agents keeps running while you install.
- **Universal Agents.** The steps show where you are: found, downloaded, notes reviewed, switched. **Download** fetches the new version in the background while you keep working; it never restarts UA. When your work is finished, open and read **Release notes**, turn on the acknowledgment switch and choose **Switch and restart**. Switching restarts UA and interrupts work in progress; your workspace and data stay. If you skip the download, Switch downloads first.

## If macOS blocks the app

Desktop releases and CI artifacts are ad-hoc signed and not notarized, so Gatekeeper may block the first launch. For a build you trust from the project release page, use **System Settings > Privacy & Security > Open Anyway**, then confirm the macOS prompt. Do not disable Gatekeeper system-wide. Windows installers are unsigned, so SmartScreen can offer **More info** then **Run anyway** instead. Developer ID signing and notarization are still required before a build can be described as production-signed.

See [`TROUBLESHOOTING.md`](TROUBLESHOOTING.md) for Docker, registry, port, and ownership errors.

## Update Universal Agents

The desktop app and UA runtime have independent versions. Open **Updates**, check the steps in the **Universal Agents** part, and read the notes for every skipped release. Follow any migration instructions, finish active work, acknowledge the notes, then choose **Switch and restart**. Checks run at launch, every six hours while the window is visible, and when you choose **Check now**.

**Other versions** lists the last five UA versions for this launcher, including the one you run. Choose **Choose** on a newer version to switch to it instead of the newest one; the steps then follow that version. Older versions are listed for reference with a link to their notes, but cannot be installed: UA does not guarantee that an older version can open data a newer one has changed.

The in-app checker uses GitHub CLI, installed and signed in. Public installers can also be downloaded directly from the releases page. Restricted repositories and images require an account with access. A release becomes installable once it is on the compatibility list in the desktop repository for this desktop's host contract; stable UA releases add themselves after an automatic check. Otherwise the drawer explains why it is blocked.

**Offer pre-release and unchecked versions**, at the bottom of **Other versions**, also offers UA betas, nightly builds, and releases not yet on the list. They are for testing: they may not start with this launcher, and once one has run, your data may not work with older UA versions. The drawer marks such a version as unchecked and asks for the same acknowledgment. Turning the switch off does not change the running version; the next listed version newer than it is offered as usual.

A brand-new install uses the newest listed UA version. If GitHub cannot be reached at that moment, the version built into the desktop app is installed and the progress text says so; update it afterwards from the Updates drawer.
