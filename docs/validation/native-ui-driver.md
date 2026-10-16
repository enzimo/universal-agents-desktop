# Native UI driver validation

Goal alignment: satisfied for installing and exercising a native UI driver on the current Mac and Windows build machine. Consequence risk: moderate while the test fixture is running, low for normal builds. Evidence strength: strong for the four native UI scenarios; full real-host acceptance remains separate.

The review baseline is `7f441ce`. The user's request authorizes configuring the driver after granting Mac permissions. Constraints are unchanged: preserve UA data, keep remote Web Chat unprivileged, and exclude test automation from normal builds.

## Evidence and boundaries

- Both macOS WKWebView and Windows WebView2 passed version/disclosure rendering, header lifecycle IPC, automatic log refresh, and update acknowledgment/plain-text release notes. Screenshots were captured by the native embedded driver.
- Mac: 56 existing Rust tests, 11 existing JavaScript tests and Clippy with the `ui-test` feature passed. The normal native application compiled after dependency changes.
- The default application's normal dependency tree contains neither the WebDriver plugin nor its HTTP server. Registration exists only in `src-tauri/examples/ui_driver.rs`, gated by a required feature; release compilation of that example is explicitly rejected.
- The fixture loads the real bundled assets but does not construct `DesktopController`. Runtime state and update responses are in memory. Unimplemented commands reject without opening files, browsers or credentials. Production capability files and the application entry point are unchanged.
- The driver dependency binds to `127.0.0.1` on desktop platforms. WebdriverIO starts a uniquely addressed local session and terminates its fixture afterward. Windows launches the script in a limited-privilege interactive task and removes that task in `finally`.
- The Windows workflow calls the same wrapper used for the passing remote run and collects UI evidence. The workflow change itself has not yet been dispatched through GitHub Actions.

## Remaining obligations

Keep the Windows account signed into its desktop. Native file dialogs, tray/autostart, real-account authentication, actual Docker lifecycle and data upgrades are outside this fixture. Linux execution is not verified by this change. Screenshots establish the captured layout, not a complete visual regression baseline.

The npm audit reports six high-severity dependency entries rooted in the upstream `extract-zip` advisory through WebdriverIO's browser-download tooling. No fixed `extract-zip` release is currently offered. This suite connects to an existing local embedded driver and never invokes browser download or archive extraction. Dependencies are development-only and installed with scripts disabled. Track upstream remediation; do not interpret the audit as clean.

The driver provides control of its test WebView to local clients while running, so it must remain confined to the fixture. Do not enable it in the shipping application or grant WDIO capabilities to hosted Web Chat. No skill templates or runtime skills changed.
