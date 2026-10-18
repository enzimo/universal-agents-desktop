# Universal Agents runtime integration

`runtime-lock.json` is the reviewed host contract: everything the launcher provides to UA (mounts, ports, paths, commands) plus the image repository UA is published in. It names no UA version. `compatible-runtimes.json` lists the UA versions and exact OCI digests that implement each contract ([Compatibility list](COMPATIBILITY_LIST.md)); the selected runtime is the contract combined with one list entry. The Docker image's `run` command owns both first-time onboarding and established runtime startup.

## Container contract

The host creates `universal-agents-desktop` with:

- restart policy `unless-stopped`;
- ownership label `io.universal-agents.desktop.managed=true`;
- exact host-contract and UA-version labels;
- a SHA-256 workspace-configuration fingerprint label that does not expose the host path;
- loopback publications for container ports `18090` and `18600`;
- the desktop application-data directory mounted at `/data`;
- an optional user-selected directory mounted read/write at `/data/workspaces/team/team-shared`;
- `NETWORK_PUBLIC_ORIGIN=http://127.0.0.1`;
- public port translations matching the locked host ports; and
- the exact digest-pinned image followed by `run`.

Host contract version 3 also locks the workspace destination, Codex source mount, Codex home, UA-owned credential store, and UA instance configuration paths. Runtime code must use those lock fields rather than duplicating container paths. An update's candidate names the exact running identity as its only replacement source; code contains no broad version fallback.

### Why host contract version 3

UA 0.6.0 split the single `/data/workspace` folder into `control/`, `dbs/`, `state/`, `runtime/`, and `workspaces/{users,team,agents}`. UA treats a non-empty `/data/workspace` as the old layout and refuses to start (`legacy_instance_layout_present`), so contract 2's folder mount stopped UA from starting, and an empty one was invisible to agents. Contract 3 mounts the selected folder at UA's shared team folder, `team/team-shared`. UA's agent instructions already describe that folder as everyone's shared working folder, every person and agent on the team can use it, and no access registration is needed. The lock validator only accepts a workspace mount below `<data_mount>/workspaces/`, the one area UA lets agents reach.

Contract 3 also removed the `assistant_cli` path, which no desktop code used. Desktop 0.4.x installs used contract 2 and are not migrated: their saved runtime selection is refused with plain guidance to start fresh, and their container fails closed as an unknown contract.

UA onboarding owns the approved external skill repository list. The desktop host exposes only the stable refresh policy: **Refresh external skills on start** passes `TEAM_REFRESH_EXTERNAL_SKILLS=true` or `false` to the long-running container. UA reads its own opaque configuration and, when enabled, pulls the configured repositories before launching agents. The desktop host never reads or edits that configuration. Changing the switch updates the host configuration fingerprint and safely replaces the managed container on the next start or restart while preserving `/data`.

## Workspace contract

The default host workspace is `<app-data>/ua-data/workspaces/team/team-shared`, already inside the `/data` bind mount. The desktop derives it from the locked `workspace_mount` rather than hard-coding it, so the Agent Workspace pane and **Open folder** always show the folder agents see. UA creates it on first start. A native folder selection replaces that container path with a nested read/write bind mount. The host canonicalizes the selected directory, rejects paths that overlap application data, and rejects a workspace containing the selected Codex credential source.

The mount grants the UA runtime, and therefore authorized agents and tools running inside it, filesystem access to the chosen directory. Because it is the team's shared folder, every person on the UA team and their agents can read and change it. Agents' private per-person folders (`workspaces/users/<person>/`) stay inside app data. The desktop app does not mount the user's home directory or broad filesystem roots implicitly.

UA's own maintenance renames files only in its control and runtime areas, never across the workspace mount, so the separate mount does not break UA's file moves. Agent shell and Python commands run in UA's Landlock code sandbox, which can open the mounted folder like any other team folder.

The settings store preserves a canonical saved path even when it becomes unavailable. Runtime start and restart revalidate that path and fail before calling Docker. The Agent Workspace pane then requires the user to choose a replacement or return to the app workspace.

## OpenAI subscription import

The desktop host accepts only an existing file named `auth.json`. It never opens, deserializes, or logs the file. Once the managed runtime is running, during onboarding or afterward, it starts a network-disabled, one-shot copy of the locked UA image with:

- `--rm`, `--pull never`, and `--network none`;
- the existing desktop `ua-data` directory mounted read/write at `/data`;
- `<selected-auth.json>:/codex-home/auth.json:ro`;
- `python` as the explicit entry point; and
- the locked `codex_import_command`, which calls UA's `import_shared_codex_auth` with the locked Codex home and credential store paths.

The helper writes `/data/auth/openai_codex_auth.json` as the protected destination and exits. UA owns validation, file permissions, token refresh, and error semantics. A successful import clears the desktop source setting. The long-running agent container is not recreated and never receives the source mount.

If onboarding is incomplete, the source remains read-only and the UI reports a pending import. The UI retries after the health service changes from onboarding to ready; the user can also request a retry.

If the source moves before import, only the import is blocked. The UI preserves the saved path and offers replacement or cancellation. The long-running runtime configuration does not depend on that temporary source.

## Managed replacement transaction

Containers are eligible for replacement only when they match the current identity, the exact identity the selected update replaces (the runtime that was running when the person approved it), or the current identity with a different desktop-configuration fingerprint. Contract 2 data is not migrated. The transaction is:

1. Confirm the fixed backup container name is unused.
2. Pull the exact locked image before mutating the current runtime.
3. Stop the current container if running and rename it to the backup name.
4. Check the locked ports and create the replacement with the same `/data` source.
5. Remove the backup container without volume deletion after successful creation.

If step 4 fails, the host removes only a recognized partial replacement, restores the prior name, and restarts it when it was previously running. Unknown contracts, images, versions, labels, or name collisions fail closed.

## Refused starts

UA refuses to start, and exits with a non-zero code, when its data or configuration needs operator action. Examples are an old folder layout, missing signing keys, or an unfinished team import. UA logs one line containing `launch refused.` or `launch aborted because` with its reason. Docker's `unless-stopped` policy then restarts the container in a loop, so the Web UI never answers.

Docker reports such a container as running and restarting, and keeps the last exit code until the next start. When the container is restarting with a non-zero exit code, the status becomes **Stopped while starting** (`startup_failed`). It shows the exit code, Docker's restart count, and UA's latest refusal reason from the last 200 log lines. Stop and Restart stay available. Reporting never changes the container. UA's own restart after first-time setup exits with code 0 and is shown as starting, not failed. Each new restart count is logged once as `Universal Agents refused to start` with the container name, exit code, restart count, and reason.

## Runtime upgrade procedure

A UA release that keeps the contract needs no desktop change: its release opens a pull request adding it to `compatible-runtimes.json`, and the compatibility workflow merges it after checking the digest and testing the published image on both native architectures ([Compatibility list](COMPATIBILITY_LIST.md)). When that check stops for a person, or a UA release changes the contract:

1. Read the failing native runtime check's report and reproduce it with `tests/runtime_import_smoke.py <version>`.
2. If the change keeps the contract, fix the runtime test or the runtime defect and let both native checks pass. Internal file changes alone require no approval.
3. If it changes the contract, raise `SUPPORTED_HOST_CONTRACT_VERSION` and `runtime-lock.json` in a desktop release, add list entries with the new contract number, and decide how data from the previous contract is handled.
4. Test a fresh install, an in-place update from the previous listed version, rollback after failed creation, and an existing data directory on macOS and Windows.

A digest or version that is not on the list is installed only when the person turns on unchecked versions and acknowledges the warning. Other drift still fails closed.
