# Native launcher UI tests

On a Mac, from the repository root:

```sh
npm ci --ignore-scripts
npm run test:ui:build
npm run test:ui
```

On Windows, keep the build account signed into its desktop, then run from the repository root:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .github/scripts/windows-ui-tests.ps1
```

The Windows wrapper builds directly and runs the GUI test through a temporary limited-privilege interactive task. It removes that task afterward. The manual Windows runner workflow invokes the same wrapper and uploads evidence even if a test fails. No extra remote desktop service or external WebDriver installation is required.

Results, native WebView screenshots and driver logs are saved under `target/ui-test-results/<platform>/`. A passing `results.json` lists the completed checks. `intro.png` captures the intro once **Click to Begin** shows; `always-on-notice.png` captures the first-run notice; `launcher-initial.png` captures the initial layout; `updates-drawer.png` captures the Updates drawer and `launcher.png` the window after it closes. Failures attempt to save `failure.png`.

The driver runs in the `ui_driver` Cargo example, gated by the `ui-test` feature and rejected in release mode. The production entry point never registers it. It uses the actual bundled launcher HTML, CSS and JavaScript, native Tauri IPC, and in-memory runtime responses. The fixture has a separate application identifier and never constructs the production controller, starts Docker, reads credentials, writes UA data, changes login settings or opens external sites. Its driver listens only on loopback and the test process shuts it down afterward.

The suite first checks that the intro covers the launcher, keeps it out of reach, and leaves after **Click to Begin**, then that the first-run notice about keeping the computer on follows it (the suite forgets the saved **Got it** first, because the fixture keeps its storage between runs), shows the fixture's sleep hint, and hands focus to the launcher once closed; every later check needs both gone because they block clicks. The window must be visible on screen: the intro waits for a visible window before it plays. The suite then checks the sun/moon theme switch and the saved choice (leaving the fixture on dark), version rendering, private-access disclosure, that the header controls share one height and centre line, header lifecycle actions and availability, that logs start collapsed and Controls starts open, automatic log refresh once the logs are open and its pause and resume, the Close and Quit buttons and their order, log tools on the logs title row, the project link on the mark, the icon expand control, and the Updates drawer: the header count, opening and closing with Escape and focus return, the launcher lane's release page, a background download that does not restart and the badge (with its dismiss button) announcing it, choosing a newer catalog version while older ones offer no install, release notes that start collapsed and show as plain text, acknowledgment gating, and the pre-release switch. It also checks that a healthy Docker probe shows no prerequisite dialog. The simulated releases are labeled `99.0.0-test`, `98.0.0-test`, `99.1.0-beta.1` and launcher `99.9.9`; none is a published or installable version, and the fixture reports which release page it would open instead of opening a browser.

These checks do not establish that real Docker lifecycle, native file dialogs, tray/autostart, browser login, or real-account onboarding work. Keep the core integration suite, native popup probe and platform acceptance checklist. WebdriverIO drives the embedded WebView rather than a separate Chrome preview. No extra Tauri capability is granted to hosted Web Chat.

The standalone WebdriverIO client uses the embedded Tauri driver directly. We do not need the separate WDIO command-mocking plugin or its additional IPC permissions. Versions are pinned in both lockfiles.

`npm audit` currently reports an upstream `extract-zip` issue through WebdriverIO's browser-download dependencies. This suite never downloads browsers or extracts archives: it connects to our already-built local driver. Dependencies are development-only and installed with scripts disabled. Do not run a forced major-version downgrade as an audit fix; review upstream remediation when available.

References: [Tauri WebDriver](https://v2.tauri.app/develop/tests/webdriver/), [embedded driver setup](https://webdriver.io/docs/desktop-testing/tauri/plugin-setup/).
