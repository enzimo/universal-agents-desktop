# Security

Report private security issues directly to the repository owner rather than filing a public issue.

## Trust boundaries

The bundled Status UI is trusted local application content. It alone receives the explicit `runtime-control` permission for the app's lifecycle, status, native selection, integration, window, and autostart commands; it does not receive Tauri's broad default core permission set. The UA Web Chat is served from `http://127.0.0.1:18090` in a separate webview and receives no Tauri capability or remote URL grant. Navigation away from the exact runtime origin is denied in that webview and normal HTTP links are handed to the system browser.

The app executes only fixed Docker subcommands assembled from a compile-time reviewed host contract (`runtime-lock.json`), a digest-pinned image from the compatibility list or, when the person opts in, from a UA release's `container-image.txt`, validated native-picker paths, and the application-data directory. Paths are passed as process arguments, never interpolated into a shell string. Docker credentials remain in Docker Desktop's credential store.

## Filesystem and credential access

Selecting a workspace is explicit consent to grant the UA container read/write access to that directory. The app canonicalizes the path, rejects application-data overlap, and mounts it only at the locked `/data/workspaces/team/team-shared` destination, UA's shared team folder. Every person on the UA team and their agents can read and change it. It does not scan or automatically mount the user's home directory.

OpenAI subscription setup accepts only an exact file-backed Codex `auth.json`. The source must be outside application data and the selected workspace. The app does not open, parse, display, copy, or log credential contents. It starts a network-disabled, one-shot container with the single file mounted read-only and `/data` mounted read/write, invokes UA's fixed import operation, and removes the helper automatically when UA reports success. The long-running agent container never receives the source mount.

Desktop settings record the canonical source path while an import is pending, never token contents, and are written owner-only on Unix. UA owns the imported store at `/data/auth/openai_codex_auth.json` and is responsible for token validation, refresh, and protected file permissions. Agents receive the chosen project workspace, not the temporary source path. Docker administrators can still inspect mounts or access container data and remain inside the trusted local computing base.

The app preserves unavailable saved paths so the user can repair them, but it revalidates canonical identity before each start or import. A moved workspace cannot trigger a silent fallback, and a moved `auth.json` never reaches Docker.

## Runtime containment

- Host publication is limited to `127.0.0.1:18090` and `127.0.0.1:18600`.
- The UA image is locked by SHA-256 digest.
- Container ownership requires all desktop labels plus the exact locked image.
- Unmanaged collisions and runtime drift fail closed.
- Container replacement accepts only the current exact identity or the exact identity an approved update replaces, and preserves `/data` through rollback.

## Who decides which UA versions are offered

The compatibility list on this repository's `main` branch decides which UA images are offered. Each stable UA release opens a pull request that appends itself; the compatibility workflow merges it without a person when the image digest matches the UA release's `container-image.txt` and the UA files that define the host contract are unchanged since the previous listed version. Anyone who can publish a UA release or push to this repository's `main` can therefore put a new image in front of launcher users. UA's release attestations are waived while its repository is private, so the launcher cannot verify the image cryptographically; it relies on the exact digest, the fixed image repository, and the person's acknowledgment of the release notes before every update.

With **Offer pre-release and unchecked versions** on, the launcher also offers UA releases that are not on the list, reading their digest from the release's `container-image.txt`. Nothing has checked those against the host contract. The setting is off by default, and the review marks such offers as unchecked.
- Persistent `/data` is never deleted by lifecycle or access-setting actions.

Docker Desktop remains a privileged local dependency. A compromised Docker daemon or a user with Docker control can alter containers and mounted data outside this app's guarantees.

## Distribution

Desktop releases are published as full GitHub releases so the newest stable version, its installers, and its checksums are discoverable from the repository and the releases feed.

The macOS build is currently ad-hoc signed and is not notarized, and Windows installers are unsigned. Gatekeeper and SmartScreen warnings therefore remain expected until an Apple Developer ID Application certificate, hardened runtime, and Apple notarization are configured. Do not ask users to disable Gatekeeper or SmartScreen system-wide; direct them to the per-app Open Anyway flow instead.

A notarized build is still required before the desktop app can be described as production-signed.
