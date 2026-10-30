# Update center (redesign concept D), phase 1

Status: phase 1 implemented, originally approved 2026-10-14. Layout and interaction notes refreshed 2026-10-30 against the bundled launcher and [desktop design contract](../../ui/DESIGN.md). The [concept D mockup](mockups/d-update-center.html) records the original proposal; use the design contract for the current UI.

Changes from the proposal, made during implementation:
- Newer rows under Other versions have **Choose** only, not Download and Choose. Choosing makes the version the lane's target and its Download step then fetches it; a separate per-row Download would have silently changed the target too.
- `review_for` builds the review for a chosen version from the cached check, so Choose needs no second GitHub request and cannot see a different release list than the one shown.
- One command, `open_release_page(product, version)`, opens both the launcher release page and UA release notes (older rows' **Notes**), instead of `open_desktop_release`. The address still comes from the last check.
- `DesktopRelease` has no notes; the launcher lane links to the release page instead.
- The catalog is empty while an update is pending or when the compatibility list cannot be read; the drawer says why.
- Errors from drawer actions show inside the drawer, because the main notice is behind it.

## Goal

Make updates something the person drives, with no dead ends:

- the launcher shows when a newer launcher exists and where to get it;
- the person can see the recent UA versions and move UA to any newer one, not only the newest;
- every update shows its next step until it is done, instead of relying on a notification that can disappear;
- the header is one aligned row.

## Decisions already made

- Layout: concept D, with the current home layout described below. An **Updates** drawer holds version review and update actions. The other mockups remain design alternatives, not approved next steps.
- UA versions: **newer only.** Older versions are listed for reference without Download or Switch, because UA has no guard against an older version opening data a newer one changed.
- Launcher self-update: **notify now, install later.** Phase 1 shows the newer launcher and opens its GitHub release page. Phase 2 (separate plan) adds in-app download and install with Tauri's updater.

## What the person sees

### Header (one row)

On the left: the project mark, **Universal Agents Launcher** with its runtime state immediately after the title, and the independent Launcher and UA version chips below. On the right, all 38 px high and vertically centred: **Updates** with a count, one joined Start / Restart / Stop group, a joined **Close** / **Quit** group, and the sun/moon theme switch. Controls wrap together on narrow windows.

The operation progress bar and its result text occupy a full-width row under the header. The first automatic startup also uses this row, with stage messages, download sizes when available, and a reminder that the first installation can take several minutes. Native verification follows the [platform acceptance checklist](../../docs/PLATFORM_TESTING.md).

There are no duplicate lifecycle buttons below the header. **Close** exits the launcher and leaves UA running; **Quit** stops UA before exiting and stays unavailable when UA is not running.

### Home

- Left: the launch pane with its explanation, **Open in app**, **Open in browser**, and **Read The Docs**. The two open actions become available when UA is ready for setup or running.
- Right: **Agent Workspace**, showing the effective host folder, whether it is app-managed or selected, and the folder controls. These panes stack below 850 px.
- **Advanced Settings** follows, with **Private Image Access** and **OpenAI Subscription (optional)** side by side, then **Recent Runtime Logs** across the full width. All three sections start collapsed. An unavailable saved subscription source opens its section for recovery.
- **Controls** is the last pane and starts expanded. It contains runtime facts, the web address, data folder, start-at-login and refresh-skills switches. There is no footer or manual status-refresh button.

The title intro appears before the launcher. On first use, the notice about keeping the computer awake follows it, then the Docker prerequisite dialog if needed. These dialogs keep the launcher and update actions out of reach until dismissed.

### Updates drawer

Opens from the header **Updates** button, the floating badge, or a "downloaded" notice. It slides in from the right as a modal dialog (focus moves in, Escape closes, focus returns).

Top: "Checked N minutes ago" and **Check now**.

**Desktop launcher lane**
1. *Found launcher X*: release date.
2. *Download and install*: **Open download page** opens the release page returned by the last check in the browser.
3. *Reopen the launcher*: "UA keeps running while you install."

When the launcher is current: "Launcher X is up to date." A failed launcher check reports its own error without blocking the UA lane.

**Universal Agents lane** (target = recommended version, or the newer version the person picked)
1. *Found UA X*, or *Chose UA X*: date and **Works with this launcher**, or the unchecked warning.
2. *Download*: **Download** button, then a percentage, then "Downloaded". It never restarts UA.
3. *Review the notes*: every release between the running version and the target as selectable plain text, breaking-change headings called out in the warning, and an acknowledgment switch. Notes start collapsed for each new review.
4. *Switch*: **Switch and restart**. It restarts UA, interrupting work; data stays.

Special states shown in the lane instead of the steps:
- update pending, "Start finishes or retries it";
- cannot install, with the reason (needs a newer launcher, not on the list yet, list unreadable);
- check failed, with **Check now**.

**Other versions** (inside the UA lane)

The last five versions for this launcher, newest first, always including the running one. Each row shows:
- version and release date;
- **Works with this launcher** or **Unchecked**;
- **Downloaded** when the image is on this computer;
- **Running** for the current one.

Newer rows have **Choose**, which makes that version the lane's target. Older rows only say "Older than the running version" and link to their notes.

The **Offer pre-release and unchecked versions** switch is the final row, off by default. Changing it rechecks available versions. Choosing is disabled during a check, download, or other operation; older versions offer only **Notes**.

### Floating badge

The bottom-right badge briefly shows the single most useful update item. Clicking it opens the drawer. In priority order:

1. UA downloading, with a percentage ring;
2. UA downloaded, "Review and switch";
3. UA newer version available;
4. launcher newer version available.

It hides after five seconds, on dismissal, or when the drawer opens. The timer pauses while the pointer or keyboard focus is inside the badge. It reappears only when the update reaches its next stage, not after an unchanged recheck. The header count remains available after the badge hides.

The badge also stays hidden while Docker is unavailable, during the intro or first-run notice, and while the Updates drawer is open.

## Implemented backend changes

`crates/desktop-core/src/updates.rs`:

- Read the UA release list once per check, as today (all pages), keeping `published_at`. Derive both the newer-release notes and the catalog from it.
- `RuntimeCatalogEntry { version, published_at, listed, prerelease, relation (newer | running | older), notes_url, image, downloaded }`. The catalog has:
  - the compatibility-list entries for this host contract and image repository;
  - when the person offers unchecked versions, newer unlisted releases whose `container-image.txt` names an image (same lookup and limit as today);
  - newest first, cut to five, always including the running version.
- `review_for(base_review, current, target_version)` builds an `UpdateReview` for one chosen newer catalog version from the cached check. It uses the same notes range, `validate_update`, `upgrade_sources = [current identity]` and review-id hash as today. Older or equal targets are refused with "Only a newer runtime can be installed."
- `check` returns the review for the recommended target, as today, plus `catalog` and `desktop`.
- `DesktopRelease { current_version, latest_version, newer, published_at, error }` comes from `repos/enzimo/universal-agents-desktop/releases/latest` through the same `gh` source. Drafts and pre-releases are ignored. If this check fails, only the launcher lane says so; the UA check is unaffected.

`src-tauri/src/controller.rs` and `lib.rs`:
- `check_runtime_updates` fills `downloaded` for each catalog entry with `image_present`, which is Docker only, never a pull.
- `review_runtime_version(version)` stores and returns the review for that target. `download_runtime_update` and `install_runtime_update` take a review id.
- `open_release_page(product, version)` opens the launcher release page or a UA notes page from the last check. It accepts no arbitrary URL.
- Both commands are added to `permissions/runtime-control.toml`.

Unchanged:
- downloads never restart;
- switching needs the acknowledgment and a current review;
- only newer versions;
- a pending update retries through Start;
- first install picks the newest listed version.

## Implemented frontend changes (`ui/desktop-shell`)

- `index.html`: new header row and progress row, drawer and badge markup. Removed: the updates pane, the lower lifecycle buttons, the update banner and the old floating button.
- `app.js`: `renderUpdateCenter(review)` draws the lanes, steps, catalog rows and badge, and computes the header count. The existing download and install flows feed it. The drawer focus handling and Escape behave like the Docker dialog.
- `styles.css`: header row, segmented control, drawer, step list, catalog rows, badge states (with reduced-motion fallbacks).
- `ui/DESIGN.md`: rewrite "Layout and hierarchy", the update parts of "Interaction rules", and "Runtime updates".

## Impact

- Removed from the window: the Universal Agents updates pane, the duplicate lower Start / Restart / Stop, the green "update downloaded" banner and the old floating button. Their jobs move to the drawer and badge.
- One more GitHub call per check (desktop releases), plus up to five local `docker image inspect` calls.
- No change to Docker, data, the container, the compatibility list, the release workflow or UA.
- Older launchers keep working as before; nothing on GitHub changes.

## Verification

- Rust unit tests (`updates.rs`, `controller.rs`):
  - catalog order and the five-row cut, with the running version always present;
  - listed vs unchecked, and pre-releases only with the switch;
  - older rows have no review, and a review for a chosen newer target uses the right notes range;
  - older or equal targets are refused, and stale review ids are refused;
  - desktop release comparison: newer, equal, older, pre-release ignored, bad tag;
  - a desktop check failure does not block the UA lane.
- `ui/desktop-shell/app.test.cjs`:
  - lane and step rendering for none, available, downloading, downloaded, pending, blocked and unchecked;
  - header count and badge priority;
  - older rows offer Notes but no Choose, Download, or Switch action;
  - drawer open and close with focus return;
  - lifecycle gates on the single header group.
- `src-tauri/examples/ui_driver.rs` fake backend: new response shapes. `tests/ui/launcher.mjs` native suite on macOS and Windows: open the drawer, both lanes, choose a newer version, switch toggle, Escape.
- Manual on each supported platform with real UA: check for an available newer listed version, download it without restarting, review the notes, then switch. Confirm the launcher lane opens its reviewed release page. Use disposable test data when a real runtime switch is needed. The checks above describe required coverage, not a claim that native validation has run for the current working changes.

## Phase 2 (separate plan, later)

In-app launcher download and install with `tauri-plugin-updater`:
- an update-signing key pair, with the private key as a GitHub secret;
- CI builds updater artifacts and `latest.json`;
- public installer downloads remain available without a GitHub account; decide any restricted-repository authentication separately;
- decide the install-complete message and **Restart launcher** action in that future design, including how they interact with the badge's five-second dismissal.
