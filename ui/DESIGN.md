# Universal Agents Desktop design

This document owns the visual and interaction contract for the standalone desktop host. The hosted Universal Agents Web Chat keeps its own design system in the Universal Agents repository; this file applies only to desktop-owned windows, menus, and native affordances.

## Product posture

The desktop host should feel quiet, trustworthy, and operational. It is a small control surface around the full Web Chat, not a competing dashboard. The Web Chat remains the primary place for agent interaction.

The desktop app uses two intentionally separate surfaces:

- The local **Status** window reports runtime state, lifecycle controls, autostart, paths, and logs.
- The remote **Universal Agents** window renders the exact loopback-hosted Web Chat. It does not receive desktop command privileges.

## Visual language

The Status window follows current Universal Agents (UA's `ui/DESIGN.md`, at the commit recorded in `ui/desktop-shell/vendor/README.md`): frosted panes over UA's background, Inter type, ocean primary actions, and UA's Deep Ocean + Ember dark theme. Desktop-specific choices below exist because the launcher is a small, always-available window with dense text.

- **Dark by default.** The window opens dark whatever the system setting. A small **Dark | Light** button pair in the header switches themes at once; the choice is saved on this computer in `localStorage` under `ua-desktop.status-window.color-mode` (not sensitive) and used the next time the window loads. The native window background is UA's dark canvas `#070b0e` so the window never flashes white while it loads.
- **Still background.** Dark mode shows UA's metal plates (the studio green metal) and light mode UA's diffuse polygons, both from the vendored UA renderers on a still canvas. They fill in once as the intro starts (1.8 s; the dark plates fade in from the middle outward, as in UA), then draw only on resize or a theme change; no animation frame loop keeps running, because the launcher may stay open all day. Without WebGL2 the dark background uses UA's basic 2D drawing. A faint scrim darkens the bottom of the dark background, as in UA.
- **Panes.** Dark panes use UA's glass: a `rgba(23, 52, 64, 0.74)`→`rgba(11, 21, 27, 0.70)` gradient over `rgba(7, 11, 14, 0.72)`, a cyan `rgba(98, 213, 243, 0.24)` border, 24 px blur. Light panes use UA's pale green-white glass but more opaque (0.88→0.80, UA Web Chat uses about 0.3–0.5) so the launcher's small text keeps WCAG AA contrast over the dark blue corner of the light background. Adjacent panes touch, with square interior corners.
- **Corner brackets.** In dark mode, short accent brackets mark the top-left of the header and the bottom-right of the last pane, as on UA's dark panels; the dialog carries them in both themes. They do not move.
- **Type.** Inter with system fallbacks; no downloaded fonts. Headings weight 880, controls 800–820, no negative letter spacing. Use the system monospace stack only for logs and machine identifiers.
- **Corners.** Controls and inputs 7 px; callouts, logs and notes 10 px; the outer pane group and dialog 14 px.
- **Buttons.** Minimum height 44 px (quiet buttons and every header control 38 px, compact buttons 34 px). Primary: UA's ocean gradient in light mode, ending at `#0369a1` instead of UA's `#0284c7` so white labels keep 4.5:1; Deep Ocean `#087697` in dark mode. Secondary: a visible border on the control fill; in dark mode hover and focus turn the border Ember `#f0a35a`. Destructive: solid `#b91c1c` (light) or `#b94850` (dark) with pale text. Disabled controls change to a muted fill and label instead of fading the whole control.
- **Status words.** States (the runtime state in the header, Live or Paused, Imported, Chosen folder, Running, Selected) are a short word in its colour, with no border, pill or fill: green for ready or done, amber for in progress or paused, red for problems, ocean for a selection, muted otherwise. The colours are darker in light mode so the words keep 4.5:1. The word always carries the meaning.
- **Tooltips.** Every button has a `title` tooltip saying what it does and what it changes (for example, that Restart interrupts work in progress). A button that is unavailable says why in its tooltip instead, and gets its normal tooltip back once it is available. A unit test fails if a button in `index.html` has none.
- **Focus.** A 2 px outline, offset 2 px, in ocean `#075985` (light) or `#62d5f3` (dark). Kickers, the version line and links use the same colors.
- Reuse the established Universal Agents mark. Do not synthesize a second logo.
- **Motion** is brief and functional and stops with `prefers-reduced-motion`. The background never moves. Motion is limited to the intro and its one-time background fill, the launcher's 600 ms fade-up after it, button hover lifts, and the update badge's download ring.

## Intro

Each time the Status window loads (app start or reload), an intro modeled on UA's onboarding intro covers it:

1. The background fills in once over 1.8 s, as in UA, and then stays still.
2. After 0.5 s the title "Universal Agents" forms letter by letter as glass facets (UA's polygon title), with small theme-colored meshes flashing inside letters. It is complete after about 3.6 s.
3. **Click to Begin** appears, takes keyboard focus and glows slowly. Keyboard focus lightens its fill rather than adding a ring, as in UA. In light mode the pill is light glass (UA uses its dark pill in both modes) so its text stays readable.
4. A click anywhere, Enter, Space or Escape dismisses the intro at any point. It fades out in 450 ms while the launcher fades up in 600 ms, and focus moves to the launcher heading.

- With reduced motion the finished title and button show at once and dismissal has no fades.
- If the app starts hidden at login, the intro waits until the window is first shown.
- After 30 seconds unattended, the title stops flashing and the button stops glowing, so an open window draws no frames.
- While the intro shows, the launcher and the update badge are hidden and inert. The overlay is a modal dialog named by its title and the title stays text for screen readers. On dismissal the overlay leaves the accessibility tree at once.
- Status polling and log refresh run underneath. The first-run notice and then the Docker prerequisite dialog wait until the intro is dismissed, and the dismissing click or key does not count as having seen the Docker recovery banner or as closing the notice.

## First-run notice

UA runs on the person's own computer, and agents only work while it is awake. The first time the launcher shows on a computer, right after the intro, a dialog in the Docker dialog's style says so:

- Kicker **Before you start**, title **Agents need a computer that stays on**, and one paragraph: the app installs UA on this computer, agents need it running 24/7, and while it sleeps agents stop and scheduled tasks can run late or be missed.
- Two choices, each a boxed row like the Docker dialog's hint: **Keep this computer awake.** followed by the sleep setting for this operating system (fixed text from the native side; general advice stays if it cannot be read), and **Or run Universal Agents on a server** for people comfortable with Docker.
- **Got it** (primary, first, focused) closes it; Escape does the same. **Open setup guide** (secondary) opens the shared setup guide in the browser and leaves the notice open. A small note says it shows once on this computer.
- Closing it is saved in `localStorage` under `ua-desktop.status-window.always-on-notice-seen`, so it shows once per computer, including once after upgrading. A reload before **Got it** shows it again. If storage cannot be read, it shows anyway.
- While it shows, the launcher is inert, the update badge and Updates drawer wait, and so does the Docker prerequisite dialog, which then takes focus. Otherwise focus goes to the launcher heading. Clicks and keys on the notice do not count as having seen the Docker recovery banner behind it.

## Layout and hierarchy

The Status header is one row. On the left: the mark (a button that opens the Universal Agents project on GitHub in the browser; the renderer passes no address), **Universal Agents Launcher** with the runtime state right after it as a coloured word (for example **Running** in green), and below them two version chips (`Launcher 0.5.5`, `UA 0.9.3`), because the two versions are independent. On the right, all 38 px high and centred on one line: **Updates** with a count of items waiting for the person, one joined Start | Restart | Stop group (the only lifecycle buttons in the window), a joined **Close** | **Quit** group, and the theme switch (a sun in dark mode, which switches to light, and a moon in light mode). The controls wrap as a group onto a second line in narrow windows. The operation progress bar and its result text sit in a full-width row under the header, so nothing in the header moves while an operation runs.

Below the header, the launch pane (state, explanation, **Open in app**, **Open in browser**, **Read The Docs**) sits beside **Agent Workspace** (the shared folder, its path and the folder buttons); they stack below 850 px. Next is **Advanced Settings**, three collapsible sections, all collapsed initially: **Private Image Access** and **OpenAI Subscription (optional)** side by side, and **Recent Runtime Logs** across the full width below them. "(optional)" tells the person UA works without the subscription. A section's state word sits right after its title so it shows while collapsed (the subscription's Not connected, Import pending, Imported or Source unavailable; the logs' Live or Paused). The subscription section opens by itself once when its saved `auth.json` goes missing. Each title has a chevron that points right when closed and down when open. The last pane, **Controls**, also collapses but starts open: UA runtime, web address and data folder beside the start-at-login and refresh-skills switches. Status refreshes on its own, so there is no refresh button. There is no footer. Panes have no external gaps.

The launcher opens 1180 × 900 px and the main Web UI window 1280 × 1176 px (about 40 % taller than the earlier 840 px, so more of UA shows without scrolling). Each is then fitted to the work area of its display (the screen without the menu bar, Dock or taskbar) and centred if it had to shrink.

The window leads with three facts: current state, a plain-language explanation, and the two ways to open the Web Chat. Versions and updates live in the Updates drawer, so the home screen stays about running the team.

Status color is never the only signal. Every state includes a label and explanation:

- green: ready or onboarding ready;
- amber: starting or transitional;
- neutral: stopped or not yet created;
- red: Docker unavailable, ownership conflict, runtime mismatch, or **Stopped while starting** (UA refused to start and Docker keeps restarting it; the explanation quotes UA's reason).

## Interaction rules

- Private Image Access and the OpenAI subscription are separate sections of Advanced Settings. Keep Docker/GitHub authentication visibly separate from OpenAI subscription import.
- Offer verification with existing Docker credentials, use of the active stored GitHub CLI account, and a collapsible username/classic-PAT form. Mask the token, clear it on submission, and never put it in errors or persistent frontend storage.
- Explain that GHCR sign-in replaces Docker's shared registry account. Require a macOS credential helper; no plaintext-storage fallback.
- New GitHub CLI sign-in opens a visible terminal for browser/device prompts. Wait for an exit-code-only result in a unique temporary session, then authenticate Docker, verify the locked image, and start UA automatically. Do not mistake a pre-existing GitHub login for completion. A five-minute deadline stops waiting and removes the temporary session. Show failures if the terminal or gh is unavailable. Never run interactive login behind an invisible pipe.
- File selection records OpenAI import intent without starting UA. Pending copy names the next prerequisite: runtime start. **Finish import** is enabled during onboarding and normal operation; the first onboarding or ready refresh attempts import automatically. Changing the source permits a new automatic attempt.

- “Open in app” is enabled only when the health endpoint is ready.
- “Open in browser” opens the locked onboarding path while setup is active and the locked `/webchat` route otherwise; it never accepts arbitrary input. Like **Open in app**, it is disabled unless the status is **Setup ready** or **Running**, because UA's web server does not answer in any other state; the native command refuses with the same "not ready yet" message. **Read The Docs** is a separate anchor that opens the locked `/docs/` path in the browser.
- Installation, registry authentication, restart, and runtime updates share a progress bar in the row under the header. Subscribe before starting work and show backend stage messages independently of the controller lock. Docker pulls switch the bar to determinate progress and show aggregate downloaded and total bytes when Docker reports layer sizes; other stages remain indeterminate. Stop the bar on success or failure and retain the result text. Progress events contain fixed application text and numeric byte counts, never credentials or raw command output. Respect reduced motion. The status refreshes automatically after the operation.
- Workspace selection uses the native folder picker. The UI always shows the effective host path and whether it is the app-managed default or an explicitly selected folder. **Open folder** reveals that same effective folder in the platform file manager without changing the selection; it reports an actionable error when the saved folder is unavailable instead of opening somewhere else.
- If a saved workspace moves, the Status window still opens and shows the unavailable path. Start and restart remain disabled until the user chooses a replacement or returns to the app workspace. The host never switches workspaces silently.
- OpenAI subscription setup uses the native file picker and accepts only a file-backed Codex `auth.json`. A pending import stays visible until import succeeds. Credentials must be imported before onboarding connection tests. The UI explains that a one-shot helper receives the source read-only only for import; the long-running agent container never receives it.
- If a pending `auth.json` moves, the OpenAI Subscription section opens and offers replacement and cancellation. This state does not disable the UA lifecycle because the long-running container never depends on the temporary source.
- Access changes may recreate the desktop-managed container. The persistent `/data` bind mount and workspace contents are never presented as disposable UI state.
- The external-skills switch persists in desktop settings and is on by default. Start and restart pass its explicit value to UA, which refreshes configured external skill repositories before the team launches. Turn it off for reproducible offline startup or manual skill updates. A changed value is applied through the same safe managed-container replacement as other host configuration.
- Start is available only when the runtime is absent, stopped, or needs attention; it reads **Install & start** before the first install. Stop and Restart are available only while the runtime is running, starting, or stopped while starting.
- Closing the Status or main Web UI window hides it. Child Web UI pages close normally. Same-origin new-page links share the Web UI session in unprivileged child windows. UA's secure credential and sign-in forms (the broker port) also open in their own unprivileged app window, never in place of the main Web UI window. Other HTTP(S) links open in the default browser. Blank child windows may navigate later for login flows; one whose first page opens in the browser is closed, so no empty window is left behind. Runtime stop is always explicit.
- **Close** quits the launcher without stopping Universal Agents; **Quit** stops the runtime first, then quits the launcher. They are a joined pair next to Stop in the header, Close first, and their tooltips say exactly that. Quit stays unavailable until the runtime is running, and a stop failure keeps the app open with the error visible. The tray menu offers the same two outcomes. While UA stops, **Quit** shows a spinning ring and reads “Stopping…” (the ring stops with reduced motion; the words stay); a failure restores it.
- **Recent Runtime Logs** is a full-width section of Advanced Settings and refreshes only while open; opening it fetches at once. Its tools, left to right: a **Lines** number field (10 to 5000, default 250, kept on this computer under `ua-desktop.status-window.log-lines`), a **Live** or **Paused** tag, a pause button that becomes a play button while paused (its label says what a press does: “Pause log refresh” or “Resume log refresh”), and icon buttons to expand or collapse the logs and to download diagnostics, each with an accessible label and tooltip. While the section is open its tools sit on its title row, at the right (they are positioned there, because a summary must not contain buttons). Changing the line count fetches at once, even while paused. Expanding makes the log view taller; Escape collapses it.
- When Docker is unavailable at launch, a blocking dialog offers **Install Docker**, **Setup guide**, and **Proceed**. If the intro or the first-run notice is still showing, the dialog appears once they are closed. Proceed opens the launcher with daemon-dependent controls disabled and an amber banner: “I am still unable to detect Docker. Please install and run Docker to proceed. I will run an automatic check every 30 sec.” The launcher checks every 30 seconds and, once Docker answers, shows a green “Docker detected, good to go.” banner until the first user interaction, then re-enables the controls. Quitting, refresh, logs, diagnostics, docs, autostart, and recording a workspace or credential choice stay available while blocked. Status color is never the only signal; each banner carries its text.
- Updates are driven by the person from the Updates drawer (see Runtime updates). Downloading never restarts the runtime; switching always waits for the acknowledgment and the **Switch and restart** button.
- Destructive data deletion is not part of the normal control surface.
- Errors remain visible until the next user action. When the desktop app wrote the whole message itself and it says how to recover (a missing Docker credential helper, denied image access, a busy port, or a private-access sign-in problem), the notice shows that text. Every other error shows the fixed “The operation did not complete. Download diagnostics…” text, because it may contain raw Docker or OS output. **Download diagnostics** exports the exact desktop error log, runtime status, and recent runtime logs to a user-selected text file.

## Accessibility

- Maintain WCAG AA contrast for text and controls.
- Every icon-only control has an accessible label. The theme switch is labelled with the theme it switches to ("Switch to light theme" or "Switch to dark theme").
- All actions remain keyboard reachable with visible focus treatment. The intro can be dismissed from the keyboard at any point and hands focus to the launcher.
- Logs are keyboard focusable and selectable. Refresh them on opening the window, on returning to it, and every three seconds while visible, unless the person paused them. Permit only one pending log request; automatic refreshes must not clear errors from other actions. Docker lifecycle serialization may delay a read until the current operation finishes.
- Do not convey status through color alone.

## Native surfaces

The menu bar uses plain platform labels rather than branded prose. Its stable order is: open Web UI, show status, lifecycle controls, and quit. Start-at-login uses a macOS LaunchAgent through Tauri's reviewed autostart plugin.

## Runtime updates

All updates live in the **Updates** drawer: a modal dialog that slides in from the right, opened from the header **Updates** button or the floating badge. Opening it moves focus to its title; Escape, the close button, or a click on the dimmed backdrop closes it and returns focus to what opened it. The launcher behind it is inert. The Docker prerequisite dialog closes it. Errors from drawer actions show in a notice inside the drawer.

The top of the drawer says when the last check ran ("Checked 4 minutes ago"), with **Check now**, and that downloads never restart anything. Checks also run at launch and every six hours while the window is visible, and never interrupt the runtime. Below are two lanes. Each update is a short list of steps: a green ring with a tick for done steps, an ocean ring for the next step, a grey ring for later ones. Screen readers hear "Done:" or "Next:" before the step. A step waiting on the person has a button below the list.

**Desktop launcher lane.** When a newer launcher release exists: *Found launcher X* (with its date), *Download and install* (**Open download page** opens that release's GitHub page in the browser), *Reopen the launcher* (UA keeps running). Otherwise "Launcher X is up to date", or the reason the check failed; a failed launcher check never affects the UA lane. Installing a launcher inside the app is a later phase.

**Universal Agents lane.** The status line says what is true now: checking, up to date, available (or available as an unchecked pre-release or release), downloading, downloaded, pending (Start finishes or retries it), can't install yet (the reason is in an amber warning), or the check failed. For an installable version the steps are *Found UA X* (date, and "Works with this launcher" or "Unchecked: not on this launcher's compatibility list"), *Download* (with the percentage while it runs; optional, because Switch downloads first), *Review the notes* (number of releases since the running version, and whether any has breaking changes), and *Switch to UA X* (restarts UA; workspace and data stay). The release notes of every skipped release show as selectable plain text in a **Release notes** section that starts collapsed for each new review (a new check or a chosen version), with publisher breaking-change headings called out in the warning; the acknowledgment is a switch row under them, like the switches in Controls. **Download** and **Switch and restart** follow. Switch is enabled only after the acknowledgment and closes the drawer so the restart progress under the header is visible. When a listed version is offered while a newer release is not listed yet, a quiet note names the newer one. No automatic installation.

**Other versions** (inside the UA lane) lists the last five UA versions for this launcher, newest first, always including the running one. Each row shows the version, release date, "Works with this launcher" or "Unchecked", "Pre-release", and "Downloaded" when the image is on this computer. Newer rows have **Choose**, which makes that version the lane's target (labelled *Chose UA X*); the target row shows **Selected**. The running row shows **Running**. Older rows say "Older than the running version" and offer only **Notes** (the release page in the browser), because UA has no guard against an older version opening data a newer one changed. Choose is disabled while a check, download or other operation runs. The **Offer pre-release and unchecked versions** switch is the last row, off by default; its description names the risks in plain words (may not start; data may not work with older versions). Changing it rechecks immediately.

**Floating badge.** Bottom right, it briefly shows the single most useful update item, in priority order: UA downloading (a ring that fills with the percentage, or spins while the size is unknown), UA downloaded ("Review the notes and switch when your work is finished"), UA available (prefixed "Unchecked" when it is not on the list), and a newer launcher. Clicking it opens the drawer. It hides after 5 seconds, when its ✕ is pressed (focus moves to the header **Updates** button), or once the drawer is opened. The 5 seconds pause while the pointer is over it or focus is inside it. It shows again only when the update reaches its next stage (for example, a download finishes), not on a recheck of the same stage, so the header count, not the badge, is the lasting pointer. It is also hidden while the drawer is open, while Docker is unavailable, during the intro, and while the first-run notice shows. The header count adds one for a UA update (available or pending) and one for a newer launcher.

## Platform behavior

Use OS-neutral wording for terminal login, credential storage, and background runtime operation. macOS uses Terminal and Keychain-compatible Docker helpers; Windows opens PowerShell and uses desktop/wincred; Linux opens an available terminal and requires desktop/secretservice/pass. Keep the same launcher layout and remote-window capability boundary on all platforms.

## Shared setup guide

The comprehensive setup guide is maintained in `universal-agents` as
`docker-compose-setup.html`. Desktop documentation and releases link to its
`setup-guide.html` download. Do not maintain or package a desktop copy.
