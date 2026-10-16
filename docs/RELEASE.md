# Desktop releases

Do not publish a release without an explicit user request.

Desktop versions are independent from UA versions. A desktop release states both its own semantic version and the locked UA runtime version/digest.

## Development artifact

The macOS workflow builds a universal `.app` and `.dmg` for review. These artifacts are not published releases and are not notarized.

## Release

A requested release promotes Windows EXE/MSI, Linux DEB/AppImage, and a universal macOS DMG built by local runners for the exact main-branch commit. Push main and wait for **CI** and **Windows and Linux builds** to succeed before creating the signed version tag. Use the deterministic watcher in [Local CI](LOCAL_CI.md). The tag workflow fails immediately if builds are unfinished or their three-day artifacts have expired. It verifies GitHub artifact SHA-256 digests and the complete five-installer set without rebuilding. Each installer has a SHA-256 sidecar. Link to the [shared setup guide](https://github.com/enzimo/universal-agents/releases/latest/download/setup-guide.html) maintained and published by Universal Agents; desktop releases do not package a second guide. The release is published as a full GitHub release and is marked latest so the repository sidebar, the releases feed, and `releases/latest` all resolve to it.

Windows builds are unsigned. Until Developer ID credentials are configured, the macOS DMG uses ad-hoc signing. Release notes must say the installers are unsigned or ad-hoc signed and not notarized, and must point users at the per-app Open Anyway flow instead of disabling Gatekeeper. Apple signing requirements below remain the gate for calling a build production-signed.

Publishing a full release is a stability statement about the listed fixes, not a signing attestation. Do not describe a release as production-signed until notarization passes.

## Apple signing prerequisites

- Apple Developer Program membership
- Developer ID Application certificate available to CI
- Apple notarization credentials
- hardened-runtime verification
- installation smoke tests on a clean Apple Silicon Mac and a clean Intel Mac or equivalent hosted machine

## Release checks

1. Confirm `Cargo.toml` workspace version and `tauri.conf.json` version match. Update versioned Quickstart links and `data-desktop-version` in the HTML guide. Update `data-runtime-version` only when the approved runtime changes; the two versions are independent. Coordinate the sibling runtime guide when publishing both projects. Write the release notes in `docs/releases/v<version>.md`; the tag workflow publishes that file as the release description and fails without it.
2. Confirm `compatible-runtimes.json` ends with the UA version new installs should get when GitHub cannot be reached (the newest stable entry for this launcher's contract is built in), and that core tests pass.
3. Run format, Clippy, core tests, workspace tests on macOS, and the universal bundle build.
4. For a notarized release, sign the app and nested code, submit for notarization, staple the ticket, and verify with `codesign` and `spctl`. For an unsigned or ad-hoc release, verify the ad-hoc signature and both universal architectures, and state the limitation in the release notes.
5. Install the DMG on a clean Mac with Colima, authenticate to GHCR only if required, complete a fresh onboarding run, restart, and confirm persistence.
6. Test an existing supported `/data` directory and confirm stop, quit, login startup, embedded Web Chat, browser Web Chat, and logs.

Automatic replacement of desktop installers remains out of scope. The existing runtime update flow uses the reviewed immutable runtime lock and explicit user acknowledgment.
