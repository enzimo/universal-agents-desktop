# Architecture

## Decision

Universal Agents Desktop is a separate repository and release surface because its change drivers differ from the UA runtime. Native windows, menu bar behavior, autostart, signing, notarization, and future Windows/Linux packaging change for platform reasons. Agent execution, onboarding, Web Chat, authorization, configuration, and persisted domain state change for UA product reasons.

This is a deliberate process boundary, not a second implementation of UA.

```text
macOS user
  ├─ bundled Status window ── typed Tauri commands ── desktop host
  │                                                ├─ Docker lifecycle
  │                                                ├─ workspace selection
  │                                                ├─ credential import orchestration
  │                                                ├─ menu bar / autostart
  │                                                └─ health and logs
  └─ embedded or browser Web Chat ── 127.0.0.1:18090 ── UA container
                                                        ├─ onboarding / gateway
                                                        ├─ NATS and agents
                                                        ├─ secure tool broker
                                                        └─ opaque /data volume
```

## Ownership

| Concern | Desktop host | Universal Agents |
| --- | --- | --- |
| Native windows, tray, login startup | Owns | Does not own |
| Container name, labels, host ports | Owns | Consumes translated environment |
| Runtime image and command selection | Pins reviewed contract | Publishes image and entry point |
| Web Chat and onboarding | Opens exact loopback URL | Owns all behavior and UI |
| Selected host workspace | Records the explicit path and mounts it at the locked destination | Treats `/data/workspaces/team/team-shared` as the team's shared working folder |
| OpenAI subscription credentials | Gives one network-disabled helper the selected `auth.json` read-only and invokes the locked UA import command; never opens credential contents | Validates, imports, refreshes, and protects the owned credential store |
| Runtime configuration | Does not interpret or mutate UA configuration files | Owns schemas and validation |
| `/data` contents | Mounts and preserves opaquely | Owns all files and migrations |
| Agent, task, authorization, messaging state | Does not own | Owns |

## Components

`ua-desktop-core` contains the platform-neutral runtime host contract. A `CommandRunner` boundary makes Docker behavior testable without a daemon. It validates the immutable lock and desktop settings, checks Docker availability, inspects ownership, constructs commands, performs recoverable container replacement, invokes UA's fixed credential-import command, probes the loopback health endpoint, and returns serializable status.

`src-tauri` adapts that core to platform application data, native folder and file pickers, native windows, the menu bar, login startup, structured logging, and Tauri commands. Blocking Docker operations run on worker threads. It stores desktop-only settings in `desktop-settings.json` beside `ua-data`, not inside `/data`.

The bundled Status UI contains no runtime logic. Its window label receives the explicit `runtime-control` permission and invokes only that narrow Tauri command set. It does not receive the broad Tauri core default. The embedded Web Chat loads the hosted UI as a top-level remote webview with no capability or remote URL grant, preserving UA's cookies, CSRF behavior, and application routes without an iframe or copied frontend. Same-origin popup pages use a unique `ua-page-*` window label and WebKit's originating webview configuration, preserving session cookies and deferred `about:blank` navigation. Their navigation remains limited to the exact runtime origin, plus UA's secure broker origin (the runtime lock's broker port) for credential and sign-in forms, which therefore open in an app window instead of the browser. The main Web UI window never navigates to the broker. Other HTTP(S) destinations open in the system browser; a child window that was opened blank and whose first navigation leaves the app is closed, so the adapter's blank window is never left behind. Child windows receive no capability. A document-start adapter converts `_blank` anchor clicks into blank-window creation followed by navigation because WKWebView can suppress those anchors before the native delegate. It preserves `noopener` and `noreferrer` behavior, introduces no IPC, and leaves URL authorization to native handlers.

## Lifecycle and persistence

On app setup, the host starts a background idempotent runtime check. A missing runtime triggers an exact digest pull followed by container creation. A valid stopped container is started. A valid running container is left alone. Any ownership or contract mismatch fails closed. A container that Docker keeps restarting after a non-zero exit is reported as **Stopped while starting** with UA's logged refusal reason; see [Refused starts](RUNTIME_INTEGRATION.md#refused-starts).

Desktop settings load separately from source availability. This lets the Status window retain and display a saved path after a folder or pending credential file moves. The host revalidates selected sources before each operation. A missing workspace blocks start, restart, and the embedded Web Chat action until the user repairs the selection. The browser action still opens the fixed loopback URL, as required by the desktop UI contract. A missing credential file blocks only the one-shot import and can be replaced or cancelled.

The host maps its platform application-data subdirectory `ua-data` to `/data` and never interprets that directory. Without an external selection, the effective workspace is `ua-data/workspaces/team/team-shared`, derived from the locked `workspace_mount`. An explicitly selected existing directory is mounted read/write over `/data/workspaces/team/team-shared`, UA's shared team folder; app data and credential-source paths are rejected as workspace choices. Host contract 3 chose that destination because UA 0.6 and later refuse to start with files in the old `/data/workspace` and only let agents reach `/data/workspaces/`. See [Runtime integration](RUNTIME_INTEGRATION.md#why-host-contract-version-3).

A workspace change changes a configuration fingerprint. The host then pulls the locked image, stops the old desktop-managed container, renames it as a temporary backup, starts the replacement against the same `/data`, and removes the backup only after Docker accepts the replacement. A creation failure restores the old name and running state. Stop, restart, app quit, container refresh, and desktop upgrades preserve `/data`.

For subscription setup, the host imports as soon as the runtime reaches onboarding or ready status. It starts a network-disabled, one-shot copy of the locked UA image. That helper receives `/data` read/write and the exact selected `auth.json` read-only at `/codex-home/auth.json`. The reviewed `codex_import_command` in `runtime-lock.json` invokes UA's `import_shared_codex_auth` function with the locked source home and destination store as positional arguments. UA owns validation, locking, permissions, and writes. The invocation does not require an instance config. The long-running container receives `OPENAI_CODEX_AUTH_STORE_PATH` from the locked `codex_auth_store`, making onboarding connection tests and normal runtime calls use the same owned store. Configuration fingerprint v3 also records the explicit external-skill refresh policy and triggers a safe replacement of older managed containers. Refreshing the approved external skill repositories before agents launch is the default; the switch turns it off for reproducible offline startup. The default value is not written to `desktop-settings.json`, so "never chosen" stays distinct from an explicit off choice without a migration: a missing key means default, and only a deliberate choice is persisted. Deselecting the switch is what keeps a long-lived installation offline-reproducible. Import errors return fixed guidance without subprocess token details. The long-running agent container never receives the source. On success the host clears the source path from desktop settings, leaving only UA's protected `/data/auth/openai_codex_auth.json` store available to the runtime.

## Failure containment

### Host registry authentication

`desktop-core::registry` owns GHCR login and exact-digest manifest verification behind a separate command boundary. The local Status window alone can invoke its two Tauri commands. The remote Web Chat receives no credential capability. Existing Docker credentials remain usable without enrolling in the new login flow.

PAT submission and `gh auth token --hostname github.com` feed `docker login --password-stdin`. The host suppresses authentication subprocess diagnostics because helpers can echo credentials. Tokens are transient process memory only; desktop settings and UA containers never receive them. Login requires the effective GHCR helper in Docker's existing config to be `desktop` or `osxkeychain`, and that helper's program to be installed; the host refuses to rewrite that config or accept plaintext persistence. Before every image pull the host also checks that the helper Docker would ask for the image's registry is installed, because Docker consults it even for public images and fails every pull without it.

Errors reach the Status window as plain strings. Most can carry raw Docker or OS output, so the window shows a fixed message and points to diagnostics. Errors the host wrote in full, with no command output, start with an invisible marker character (`ua_desktop_core::guidance`), and the window shows their text. A marker that is not at the start of the string, for example inside a wrapped error, is ignored, so a mistake can only hide guidance, never reveal raw output. Logs and the startup dialog strip the marker. Docker owns credential storage and sign-in changes its shared GHCR identity for other clients too.

GitHub CLI use ignores inherited `GH_TOKEN` and `GITHUB_TOKEN` to select its stored account. Interactive sign-in is delegated to the platform terminal with a fixed `gh auth login --web --scopes read:packages` command. This preserves visible device-code, account, and credential-store prompts. Installation continues automatically after successful terminal login. No pasted input enters this shell command. GH CLI remains responsible for its own credential storage.

Finder-launched authentication and lifecycle children receive standard Homebrew and Docker Desktop CLI/helper directories on PATH. Authentication and lifecycle commands serialize through the controller mutex because they share Docker credentials. Login is followed by manifest inspection of the exact runtime-lock digest before startup; login success is not treated as proof of private package access. Registry denial does not clear a saved OpenAI import path or choose a different image.

The bundle declares an Apple Events usage description and automation entitlement for the explicit Terminal sign-in action. macOS may request consent to control Terminal. Denial leaves PAT and existing-Docker authentication available and reports the manual gh command. No remote-content capability or arbitrary command parameter is added.

Selecting a Codex source now only saves intent. Readiness gates UI import actions, the controller rechecks runtime state, and the core returns `PendingRuntime` if a container is missing or stopped during a race. Ownership and lock checks remain enforced. Onboarding and ready status both permit import; the one-shot helper no longer waits for an instance config.

### Runtime failures

- Docker absent or stopped: status reports an actionable prerequisite failure.
- Registry authentication failure: the pull fails visibly; no alternative image is tried.
- Port conflict: creation stops before Docker mutation.
- Container name conflict: no start, stop, restart, logs, or delete command is issued against the foreign container.
- Locked runtime drift: the container remains untouched and status reports the mismatch.
- Replacement failure: the prior container name and running state are restored; `/data` is never removed.
- Missing onboarding configuration: subscription import uses UA's config-independent import function so setup connection tests can proceed. Desktop code never reads or copies credential contents.
- Missing saved workspace: start and restart remain blocked until the user selects a valid replacement or the app workspace.
- Missing pending credential source: UA lifecycle remains available while import waits for replacement or cancellation.
- Health not ready: the container remains running and status stays transitional; no fallback URL is used.

## Testing emphasis

The core leans on integration-style tests around the fake Docker command boundary because command order, immutable arguments, ownership gates, and preservation rules matter more than isolated helper coverage. macOS CI compiles and bundles the real Tauri application because platform APIs, configuration, resources, and universal binary assembly cannot be proven on Linux. A signed/notarized installation smoke test remains a release gate once credentials exist.

## Desktop launch routes and appearance

The reviewed lock distinguishes onboarding `/`, chat `/webchat`, and docs `/docs/`. Health identifies onboarding for launch URL selection. After onboarding, `/` redirects to docs, so it is never used as the normal chat entry point. Read The Docs invokes a fixed desktop command; remote content cannot invoke it.

The local Status page opens dark by default and offers a sun/moon theme switch whose choice is saved in the page's local storage; it does not follow the system appearance. It uses vendored UA background renderers on a still canvas that fills in once with the intro and then redraws only on resize or a theme change, and plays a vendored UA intro when the page loads, followed on first run by the always-on notice (see `ui/DESIGN.md`). The hosted Web UI continues to own its own appearance.

## Independent desktop and runtime updates

Desktop version comes from the Cargo workspace and Tauri bundle; UA identity comes only from the compatibility list (`compatible-runtimes.json`) combined with the host contract in `runtime-lock.json`. `updates` in desktop-core owns release discovery, compatibility validation, approval, and persistence through a fakeable `ReleaseSource`. The controller owns serialization and Docker application; bundled UI owns review and consent. Remote Web Chat gains no capabilities.

The authenticated GitHub CLI reads releases from `enzimo/universal-agents`, with bounded responses and pagination. It ignores drafts and, unless **Offer pre-release and unchecked versions** is on, pre-releases; it sorts by semantic version, where `0.9.2-beta.1` and `0.9.2-nightly.202610120317` come after 0.9.1 and before 0.9.2. Notes include every skipped release up to the offered one; headings containing “breaking” trigger a warning. This is publisher metadata, not an automatic compatibility analysis, and unlabeled breaking changes cannot be detected. Bodies are displayed as plain text.

The compatibility list decides what is offered. `compatible-runtimes.json` on this repository's `main` branch lists each UA version known to work with a host contract, with its exact image digest. The launcher offers the newest listed stable version for its own contract that is newer than the running one. Any listed version can replace any older one with the same contract, so the list has no per-version upgrade paths. Each stable UA release opens a pull request that appends itself; the repository's compatibility workflow merges it only when the digest matches the UA release and the UA files that define the contract are unchanged since the previous listed version ([Compatibility list](COMPATIBILITY_LIST.md)). Earlier launchers pinned one version in `runtime-lock.json` and needed a desktop change for every UA release; the list removes that step for compatible releases while keeping every install bound to an exact digest.

With **Offer pre-release and unchecked versions** on, the launcher also offers newer GitHub releases that are not on the list: betas, rcs, nightlies, and stable releases whose list entry has not merged yet. It reads the exact image from the release's `container-image.txt` (`Immutable image:` line), requires the same image repository, and assumes this launcher's contract, which nothing has checked; the status window says so before the person acknowledges. The setting is off by default, is saved only when turned on, and is not part of the container configuration fingerprint, so changing it never restarts UA. Turning it on or off drops the current review.

Runtime-only updates cannot change image repository, command, import code, mounts, ports, URLs, or host contract. Such changes need desktop development first. No tags are resolved or substituted at install time. An update replaces only the exact running selection: the candidate names it as its only upgrade source.

A first install on a fresh computer (no container, no files in the data folder, no pending update) installs the newest stable listed version from `main` instead of the version built into the launcher, so an old launcher still installs a current UA. If GitHub cannot be reached, it installs the built-in version and says so; the built-in version is the newest entry of the list copy compiled into the launcher.

On first launch the bundled lock is saved beside desktop-settings.json, outside UA's opaque `/data`. Later launches retain that selected lock instead of silently applying a newly bundled version. It must still match the binary's supported host profile. Future host-profile changes need an explicit selection migration; incompatible saved selections fail closed.

Checks run outside the controller mutex and cache a review tied to the full source lock, candidate, and changelog. The install command accepts only the cached review ID and explicit acknowledgment, revalidates the source and contract, and persists the approved selection with `update_pending` before any Docker mutation. Atomic rename commits the selection; directory-sync errors after commit are logged without pretending the old selection remains active. All subsequent settings and lifecycle operations use the selected lock.

Existing managed-container replacement preserves `/data` and workspace and restores the old container on Docker creation failure. It does not roll back runtime migrations or guarantee health after Docker accepts creation. Failed/interrupted updates retain the approved target as pending; Start retries that digest. Pending clears only when status validates the target container and reports ready/onboarding. Release checks never change Docker state, credentials, or UA files.

Each check also returns a **catalog**: the last five UA versions this launcher can run, newest first, always including the running one. It holds the compatibility-list entries for the launcher's contract and image repository (pre-release entries only when unchecked versions are offered), the unchecked releases found through `container-image.txt` when they are offered, and the running version; release dates and note links come from the same release history the check already reads, older versions included. The controller marks each entry it finds in the local Docker image store (`docker image inspect`, read-only). Only newer entries can become the target: `review_runtime_version` builds the review for a chosen version with `updates::review_for` from the cached check (no second GitHub request), with the same notes range, single upgrade source and review-id hash as the offered version, and replaces the cached review, so the previous review id can no longer download or install. Older entries are listed for reference only, because UA has no guard against an older version opening data a newer one changed; `validate_update` refuses them anyway. While an update is pending the catalog is empty.

The same check reads this repository's latest release (`releases/latest`, which excludes drafts and pre-releases) and reports whether it is newer than the running launcher. A failure there is reported in the launcher result only and never blocks the UA review. The launcher does not install itself yet: `open_release_page` opens the GitHub release page in the browser. The renderer passes only a product (`launcher` or `runtime`) and a version; the address comes from the last check, so no arbitrary URL can be opened.

Downloading and applying are separate steps. `download_runtime_update` validates the cached review and exact digest through `approve_download`, which requires a current review but no release-notes acknowledgment because fetching an image changes neither the selected runtime, the container, nor UA data. The pull is a plain `docker pull` guarded by a read-only `docker image inspect`; it never runs, stops, replaces, or removes a container. Applying still requires the reviewed Switch and restart path with its acknowledgment gate. `image_downloaded` reports local image presence so the launcher can offer or hide the download without pulling on every check.

## Docker prerequisite

The launcher treats Docker as a hard prerequisite for anything that talks to the daemon. `docker_prerequisite` is a read-only probe (`docker info` only) that distinguishes a missing CLI from a stopped daemon and returns the per-OS install page and the shared setup-guide URL from `desktop-core::platform`. It runs on a 30-second timer while Docker is missing, and never pulls, creates, or writes.

On first detection the launcher shows a blocking dialog with Install Docker, Setup guide, and Proceed. Proceed is not a claim that Docker works: it reveals the launcher with a persistent amber banner ("I am still unable to detect Docker…") and disables Start, Stop, Restart, private image access, and update install. Quitting, refresh status, logs, diagnostics, docs, autostart, and recording a workspace or credential intent stay available so a user can recover. When the probe reports Docker, the banner turns green ("Docker detected, good to go.") until the first user interaction. `open_docker_help` accepts only the fixed `install` and `guide` selectors, so the renderer never supplies an arbitrary URL.

## First-run always-on notice

The launcher installs UA on the person's own computer, but agents only work while that computer is awake, and personal computers sleep. The first time the Status window shows, after the intro and before the Docker dialog, a notice says so. It offers two choices: keep this computer awake (the exact sleep setting comes from `keep_awake_hint`, fixed text per OS in `desktop-core::platform`, like the Docker hint), or run UA on a server with the shared setup guide (the same `open_docker_help` `guide` selector, so no new address can be opened). The notice is renderer-only state: **Got it** is saved in the page's local storage, beside the theme choice, so it shows once per computer and once after upgrading from a launcher without it. It is reminder text, not a setting, so it does not belong in `desktop-settings.json`. If storage cannot be read, the notice shows and a warning is logged.

## Host platform boundary

`desktop-core::platform` owns OS-specific home/PATH discovery, executable lookup, console flags, credential-helper names, and Docker source-path normalization. Registry commands, release discovery, settings validation, and Docker invocation share that boundary. Windows background commands hide console windows; interactive login deliberately creates a visible PowerShell console. Linux terminal selection is explicit and reports a manual login command if no supported terminal exists.

Windows canonical verbatim drive paths become ordinary `C:/...` Docker sources. UNC/device paths and alternate-data-stream colons are rejected. Settings retain canonical filesystem paths for validation. The host does not change UA mount destinations, ownership labels, image identity, or published ports. Native settings files live in Tauri's per-user application data directory and inherit Windows user-profile ACLs; Unix settings retain mode 0600. File handles close before atomic replacement.

The bundled Status window is the only privileged surface. It can open the effective agent workspace folder in the platform file manager, quit the host while leaving UA running, or stop UA and then quit. Both quit paths preserve `/data` and the selected workspace; only the second changes runtime state, and it stays unavailable until the runtime is running.

Tauri's platform config overlays select NSIS/MSI on Windows and DEB/AppImage on Linux. The existing `window_features` API preserves WebView2 environment, WebKitGTK related view, or WKWebView configuration respectively. Native CI runs the same popup/cookie/capability probe on Windows and Linux. Docker-dependent end-to-end tests remain a separate manual acceptance gate.

On Windows, WebView2 does not allow a webview to be created reentrantly from the message thread. Tauri runs synchronous commands, tray/menu handlers, and event handlers on that thread, and `WebviewWindowBuilder::build` there deadlocks in `CreateCoreWebView2Controller`. The host therefore opens the embedded Web UI only from `async` commands, and the tray item routes through the same worker path. A blocked message thread leaves the embedded window blank and stops the host from answering any other command, so this constraint is load-bearing for every lifecycle action. Tauri documents it on `WebviewWindowBuilder::build` and tracks it as [WRY 583](https://github.com/tauri-apps/wry/issues/583).

On Linux, WebKitGTK must permit script-created windows so the native new-window URL policy can handle deferred login and file-browser popups. The host enables that setting only for its unprivileged runtime webviews. The same-origin/external-navigation checks still apply; desktop capabilities remain restricted to the bundled control window. The native popup probe reproduces the default suppression and passes with this setting.

A missing desktop runtime selection is not permission to upgrade existing UA data. Controller initialization rejects a nonempty data directory when the host selection is absent, before writing a new selection. Desktop 0.4.3 can establish the prior 0.4.2 selection for legacy installations; otherwise restore the host-owned selection file. Existing saved selections continue to require release review and explicit update approval.

## Native UI test driver

The `ui_driver` Cargo example registers an embedded WebDriver only when explicitly built with `ui-test`, and refuses release builds. The shipping entry point does not register the plugin. The example uses the production bundled launcher assets and native IPC with an in-memory fixture instead of `DesktopController`; it cannot operate Docker or read user settings. It has a separate app identifier. No production capability changes are needed. WebdriverIO connects on loopback, records evidence, and terminates the fixture. See `tests/ui/README.md` for coverage and limitations.

## CI ownership

Native build and runtime-check workflows run on explicitly labeled local runners. Windows and macOS packaging remains native; WSL 2 supplies Linux builds and runtime container CI. Installed host hooks isolate job state, retain compiler/package-download caches within a per-runner size and age budget, recover interrupted jobs, and reclaim recorded job resources after action post steps. Main and pull-request cache scopes are separate. Pull requests compile and test native apps without packaging installers; main builds retain packaging for release promotion. They preserve UA data and unrelated Docker resources. Documentation is the exception: it builds and deploys on disposable GitHub-hosted runners so public contributions cannot execute on personal hosts. MkDocs reads the existing guides through `scripts/docs_site.py`; source paths stay intact for link resolution, while published page URLs are lowercase. The site has no desktop command access and changes no launcher UI. See [Local CI](LOCAL_CI.md) for provisioning, retention, publishing, and operational limits.


GitHub terminal login completion uses a unique temporary directory containing only the command exit code. The launcher waits at most five minutes, then removes the session on success, failure, or timeout. A successful result continues through the existing credential-helper and locked-image validation path. An older stored login does not count as completion. Closing the terminal without a result reaches the deadline; the user can retry or use the stored CLI account. The terminal is user-visible and may require manual closing on macOS or after a timeout.
