# Update center (redesign concept D), phase 1

Status: phase 1 implemented on branch `feat/update-center` (approved 2026-10-14). Mockup: `mockups/d-update-center.html`.

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

- Layout: concept D (home stays about the team; an **Updates** drawer holds all versions). Concept B is the likely next step if the launcher grows.
- UA versions: **newer only.** Older versions are listed for reference without Download or Switch, because UA has no guard against an older version opening data a newer one changed.
- Launcher self-update: **notify now, install later.** Phase 1 shows the newer launcher and opens its GitHub release page. Phase 2 (separate plan) adds in-app download and install with Tauri's updater.

## What the person sees

### Header (one row)

Brand and version chips (`Launcher 0.5.0`, `UA 0.9.2`) on the left. On the right, all 38 px high and vertically centred: **Updates** with a count, one Start / Restart / Stop group, the status pill, the theme switch. The operation progress bar and its result text move to a full-width row under the header, so the buttons never shift.

The duplicate Start / Restart / Stop buttons in the lower pane go away.

### Home

- Left: status, explanation, **Open in app**, **Open in browser**, **Read The Docs** (unchanged).
- Right: recent runtime logs (moved up from the lower row).
- Below, unchanged in content: private image access, Access (workspace and OpenAI subscription), then a **Settings** pane with the runtime facts and the two switches (start at login, refresh skills), then the footer.

### Updates drawer

Opens from the header **Updates** button, the floating badge, or a "downloaded" notice. It slides in from the right as a modal dialog (focus moves in, Escape closes, focus returns).

Top: "Checked N minutes ago" and **Check now**.

**Desktop launcher lane**
1. *Found launcher 0.5.5*: release date, **What's new** (opens the release page).
2. *Download and install*: **Open download page** opens `https://github.com/enzimo/universal-agents-desktop/releases/tag/v0.5.5` in the browser.
3. *Reopen the launcher*: "UA keeps running while you install."

When the launcher is current: "Launcher 0.5.5 is up to date."

**Universal Agents lane** (target = recommended version, or the newer version the person picked)
1. *Found UA 0.9.3*: date; ✓ works with this launcher, or the amber unchecked warning.
2. *Download*: **Download** button, then a percentage, then "Downloaded". It never restarts UA.
3. *Review the notes*: every release between the running version and the target, breaking-change headings highlighted, the acknowledgment switch. (Implemented with the notes collapsed by default.)
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

The **Offer pre-release and unchecked versions** switch moves here.

### Floating badge

It replaces today's floating button and green banner. It shows the single most useful item and stays until that item is resolved. Clicking it opens the drawer. In priority order:

1. UA downloading, with a percentage ring;
2. UA downloaded, "Review and switch";
3. UA newer version available;
4. launcher newer version available.

Docker missing hides it, as today.

## Backend changes

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
- New command `review_runtime_version(version)` stores and returns the review for that target. `download_runtime_update` and `install_runtime_update` keep taking a review id, unchanged.
- New command `open_release_page(product, version)` opens only `https://github.com/enzimo/universal-agents-desktop/releases/tag/v{version}` for the launcher version the last check returned, or the notes page of a UA version in the last check.
- Both commands are added to `permissions/runtime-control.toml`.

Unchanged:
- downloads never restart;
- switching needs the acknowledgment and a current review;
- only newer versions;
- a pending update retries through Start;
- first install picks the newest listed version.

## Frontend changes (`ui/desktop-shell`)

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
  - older rows have no actions;
  - drawer open and close with focus return;
  - lifecycle gates on the single header group.
- `src-tauri/examples/ui_driver.rs` fake backend: new response shapes. `tests/ui/launcher.mjs` native suite on macOS and Windows: open the drawer, both lanes, choose a newer version, switch toggle, Escape.
- Manual on the user's Mac with real UA: check, download 0.9.3, review, switch; the launcher lane links to the 0.5.5 release.

## Phase 2 (separate plan, later)

In-app launcher download and install with `tauri-plugin-updater`:
- an update-signing key pair, with the private key as a GitHub secret;
- CI builds updater artifacts and `latest.json`;
- downloads from the private repository through the person's GitHub CLI login;
- the badge's Ready state becomes **Restart launcher**.
