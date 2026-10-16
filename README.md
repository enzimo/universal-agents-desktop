# Universal Agents Desktop

Universal Agents Desktop runs [Universal Agents](https://github.com/enzimo/universal-agents)
in a local Docker container and opens its Web Chat. The desktop window provides
start, stop, logs, workspace selection, and update controls.

## Get started

Follow [QUICKSTART.md](QUICKSTART.md), or download and open the
[offline setup guide](https://github.com/enzimo/universal-agents/releases/latest/download/setup-guide.html) for platform-specific commands and initial settings.

- **macOS:** universal Intel/Apple Silicon DMG; use Colima and the Docker CLI.
- **Windows 11 x64:** EXE or MSI; enable WSL 2 and Docker Desktop's Linux engine.
- **Linux x64:** DEB or AppImage; use Docker Engine with access for your normal user.

These installers are ad-hoc signed on macOS and unsigned on Windows, and neither
build is notarized yet, so Gatekeeper or SmartScreen may ask you to confirm the
first launch. The first start downloads the runtime image.
Public downloads and images need no account; the guide includes optional private-access setup.

Desktop and runtime versions are independent. The reviewed
[`runtime-lock.json`](runtime-lock.json) is the reviewed host contract, and [`compatible-runtimes.json`](compatible-runtimes.json) lists the UA versions and immutable image digests that implement it ([Compatibility list](docs/COMPATIBILITY_LIST.md)).
Persistent runtime data survives stopping, restarting, and upgrading the app.
See [Usage](docs/USAGE.md) for workspace selection, subscription login import, and updates.

## Safety model

The host publishes Web Chat and secure-broker ports only on `127.0.0.1`. It manages only the container named `universal-agents-desktop` carrying its exact ownership, runtime, and host-contract labels. A name collision, altered image, or unsupported contract fails closed. Approved workspace changes use a recoverable container-replacement transaction that preserves `/data`.

The chosen workspace is the only additional personal folder mounted for agents. A selected Codex `auth.json` is mounted as an exact read-only file in a network-disabled, one-shot helper container that imports it into `/data/auth`; it is never mounted in the long-running agent container, and the desktop host never parses credential contents.

The embedded Web Chat is a remote loopback webview with no Tauri capabilities. Only the bundled Status window can invoke desktop lifecycle commands. See [`SECURITY.md`](SECURITY.md) and [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Development

```bash
rustup component add rustfmt clippy
cargo test -p ua-desktop-core
cargo fmt --all -- --check
cargo clippy -p ua-desktop-core --all-targets -- -D warnings
```

Run every repository shell command through RTK when working as an agent, as described in [`AGENTS.md`](AGENTS.md).

The macOS GitHub Actions workflow performs the real Tauri compilation and produces a universal `.app` and `.dmg`. Releases are ad-hoc signed and not notarized; describing a build as production-signed requires Apple Developer ID signing and notarization.

## Repository map

- `crates/desktop-core/`: testable runtime lock, Docker ownership, lifecycle, health, and logs
- `src-tauri/`: cross-platform application, native windows, menu bar, autostart, and command boundary
- `ui/desktop-shell/`: bundled Status window
- `ui/DESIGN.md`: desktop-only visual and interaction contract
- `runtime-lock.json`: reviewed UA host contract and image repository
- `compatible-runtimes.json`: UA versions and exact image digests known to work with each host contract
- `compatible-runtimes.json`, `compatibility/`, `scripts/check_compatibility_list.py`: UA versions the launcher may offer, and the CI check that verifies and merges UA's release entries; see [`docs/COMPATIBILITY_LIST.md`](docs/COMPATIBILITY_LIST.md)
- `docs/`: architecture, runtime integration, troubleshooting, and release operations
