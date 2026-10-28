# Troubleshooting

## Docker unavailable

Start Colima on macOS (`colima start universal-agents`), Docker Desktop with its WSL 2 Linux engine on Windows, or Docker Engine on Linux, then refresh status. The app uses the normal `docker` CLI and Docker context. Confirm `docker info` works in Terminal.

When launched from Finder or Login Items, the app checks PATH, standard Homebrew paths, and Docker Desktop CLI locations. A custom Docker CLI location is not part of the v1 host contract.

## "Docker cannot download images because its settings file … names the credential helper"

Docker's settings file (`~/.docker/config.json`, or `config.json` in `DOCKER_CONFIG`) names a credential helper, the program Docker uses to keep registry passwords, and that program is not installed. Docker asks the helper before every download, even of a public image, so nothing can be installed until this is fixed. It usually happens after Docker Desktop is removed and Docker is then provided by Colima: the settings still say `"credsStore": "desktop"`. On macOS with Colima, run `brew install docker-credential-helper`, change `credsStore` to `osxkeychain`, then retry. Logins saved by Docker Desktop live in the same macOS Keychain, so they usually keep working. The app checks this before downloading and does not edit Docker's settings itself.

## Image pull is denied

Public images do not need a login. If the registry requires access, use **Advanced Settings > Private Image Access** in the Status window. Choose **Use GitHub CLI** for the active stored `github.com` account, or supply your GitHub username and a classic PAT. The token needs `read:packages`, access to the private package, and any required organization SSO authorization. **Sign in with gh** opens the interactive browser-login flow in Terminal; complete it and the launcher signs Docker in and resumes installation automatically. If you close the terminal early, waiting ends after five minutes; retry or choose **Use GitHub CLI** if you finished login separately. Environment-only `GH_TOKEN`/`GITHUB_TOKEN` credentials are deliberately not used by this flow.

If Docker is already signed in, choose **Verify access and retry**. Verification inspects the exact locked image manifest; a successful login alone is not sufficient. Network failures and missing package permissions can both prevent verification. The app never switches to a public or mutable image.

If the app requests a credential helper, follow the optional private-access section of the [setup guide](https://github.com/enzimo/universal-agents/releases/latest/download/setup-guide.html). With Colima on macOS, install `docker-credential-helper` through Homebrew and configure `osxkeychain`; with Docker Desktop, repair its credential-store configuration. GHCR's effective `credHelpers` override or `credsStore` must be `desktop` or `osxkeychain`. The app refuses plaintext token storage and does not rewrite Docker configuration. Signing in replaces the shared Docker GHCR account, so other Docker clients will use it too.

## A runtime port is already in use

The first creation requires loopback ports `18090` and `18600`. The Status window names the busy port. Stop the process using it, then start again. The host does not pick random ports because Web Chat origin and broker callbacks are part of the reviewed runtime contract.

## Container name is unmanaged

The status says Docker already has a container named `universal-agents-desktop` that this app did not create. The app refuses to alter it. Inspect that container manually and decide whether to rename or remove it; the app will not make that destructive decision.

## "Docker still has a Universal Agents container from another setup"

The app-data folder was moved aside or deleted, but the container created with it is still in Docker. The new setup has no record of that container, so the app shows **Needs attention** and does not start, stop or replace it. If you meant to start fresh, run `docker rm -f universal-agents-desktop`, then choose **Start**. That removes only the container: its data lived in the app-data folder you moved, and no folder on this computer is deleted. To go back to the old setup instead, put the old app-data folder back.

## Runtime mismatch

The existing desktop-managed container no longer matches the selected runtime. The app replaces only the current runtime, the exact runtime an approved update replaces, or a current container whose workspace setting changed. Any other drift remains untouched. Preserve the UA data directory and perform a reviewed upgrade rather than deleting state casually.

## A selected workspace will not start

Confirm the directory still exists and that your Docker VM can share it. Colima shares your home directory by default; put workspaces there. A folder Colima does not share is not an error: Docker mounts an empty folder inside the Colima VM instead, so agents see none of your files and their changes stay in the VM. On macOS, Docker Desktop may require the location under **Settings → Resources → File sharing**, and macOS may ask the desktop app or Docker for access to protected folders such as Documents, Desktop, or removable volumes.

The app rejects a workspace that contains its own application data or the selected Codex `auth.json`. Choose the narrow project directory agents need, not your home directory or an enclosing personal folder.

If a previously selected workspace moved or a removable drive is disconnected, the Agent Workspace pane shows **Folder unavailable**. Choose the replacement or **Use app workspace**. The app does not fall back to a different workspace on its own.

## No Codex auth.json exists

Codex may be storing your login in the macOS Keychain. The desktop import deliberately requires a file-backed login so UA can use its supported import path. In Terminal, run:

```bash
codex login --device-auth -c 'cli_auth_credentials_store="file"'
```

Then select `~/.codex/auth.json` in **Advanced Settings > OpenAI Subscription (optional)**. OpenAI subscription setup never asks you to paste an OpenAI token; the separate Private Image Access section accepts GitHub PATs only.

## OpenAI subscription import is pending

Selecting `auth.json` saves the path without requiring Docker or a container. If the runtime is not installed, start it first, resolving image access only if required. If stopped, start it. On a fresh installation, the import runs as soon as the setup server is ready, before onboarding finishes. **Finish import** is enabled during onboarding and normal operation. If Runtime Basics reports missing Codex credentials, finish the import in the desktop window, leave the access token blank, and retry **Test Connection**. If import still fails, confirm the selected file is a current file-backed Codex login and remains readable by Docker. Import failures appear in the desktop notice; the one-shot helper is separate from Recent runtime logs.

After a successful import, the OpenAI Subscription section reports **Imported** and no source path. The one-shot helper has exited; the long-running agent container continues from UA's protected credential store under `/data/auth`.

If the saved source moved, the OpenAI Subscription section opens and reports **Source unavailable**. Choose the replacement `auth.json` or cancel the pending import. This does not stop the main UA runtime.

## Container replacement reports a backup name conflict

The fixed backup name is `universal-agents-desktop-replacement-backup`. The app refuses to overwrite it. Inspect both desktop containers and recent logs before removing anything. A failed replacement normally restores the original container automatically; preserve the `ua-data` directory regardless of container cleanup.

## Status shows Stopped while starting

Universal Agents exited with an error while starting, and Docker keeps restarting it. The status shows the exit code, how many times Docker restarted it, and the reason UA logged, for example an old folder layout or missing signing keys. Choose **Stop**, fix the cause UA describes, then **Start**. **Download diagnostics** exports the full log. The app never deletes or moves UA data to recover.

## "Universal Agents can't start" appears when the app opens

The desktop app could not prepare itself, so it shows the reason in this dialog and quits when you choose OK. The status window never opens. The same text is written to the desktop log as a line starting `desktop app could not start`. "Data from an older Universal Agents desktop app" means this computer still has data from Desktop 0.4.x, which this version cannot upgrade. The desktop log line before it records the technical detail as `saved_contract=2 supported_contract=3`. Nothing was changed. To start fresh, run `docker rm -f universal-agents-desktop`, then move the app-data folder listed in [Usage](USAGE.md) aside before reopening. Keep the old folder until you no longer need it.

## Web UI remains in Starting

Open **Recent Runtime Logs**. The desktop host waits for `http://127.0.0.1:18090/health` to return healthy JSON and does not guess readiness from container state alone. First-time startup can take several minutes.

## App closes but UA keeps running

This is expected for **Close**, which exits only the native host. Use **Quit** in the header (stops UA, then the launcher), **Stop**, or the tray's **Stop Runtime** when you intend to stop UA. Both stop paths preserve your workspace and persistent data.

## The embedded Web UI is blank and the launcher buttons stop responding

If the Status window freezes at the same time the Universal Agents window is blank, restart the desktop app and use the current build. Earlier desktop versions created the embedded window from the Windows message thread, which WebView2 cannot complete; that one blocked thread also stopped the launcher from answering Start, Stop, Open in browser, and Refresh. The fix creates the window from a separate worker. If the window is still blank after updating, choose **Download diagnostics** for the exact desktop error and runtime logs.

## An update is available but cannot be installed

The update check lists every newer UA release on GitHub, but the app offers only versions on the compatibility list (`compatible-runtimes.json` on this repository's `main` branch) unless **Offer pre-release and unchecked versions** is on. The warning under the status line says why there is no install button:

- "not on this launcher's compatibility list yet": the release has not been added to the list. A stable UA release adds itself within minutes once its image is published and the compatibility check passes; if the check found a change to the contract files, a person has to decide. To try it sooner, turn on **Offer pre-release and unchecked versions**.
- "needs a newer desktop app": the new UA release changes what the desktop app must provide. Install the latest desktop app first.
- "has no published image yet": with unchecked versions on, the newest release has no `container-image.txt` yet because its image is still being built. Check again later.
- "Could not read the compatibility list": GitHub CLI could not reach this repository, or `main` has no list yet. Check your connection and that `gh auth status` shows an account with access. With **Offer pre-release and unchecked versions** on, the newest published release is still offered, marked unchecked.

A note under the status line, "UA X is newer but not on this launcher's compatibility list yet", means a listed version is offered while a newer release waits for its list entry.

## Installation or update is taking time

The progress bar beneath Install/Start shows the current operation. “Signing Docker into GHCR” means authentication is running. While Docker pulls an image, the text and bar show aggregate downloaded and total bytes when Docker reports layer sizes. “Image ready” means the pull succeeded and runtime startup is next.

When the app knows the cause and how to fix it, for example a missing credential helper, denied image access, or a port in use, the Status window shows those steps directly. Other failures show a short message instead, because the exact error can contain raw Docker or system output. For an exact failure, choose **Download diagnostics** beside Recent runtime logs. The exported text includes desktop logs, runtime status, and recent container logs. Review it before retrying or attach it to an issue after checking it for account, workspace, or conversation details you do not want to share.
