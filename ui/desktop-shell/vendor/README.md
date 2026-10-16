# Vendored Universal Agents assets

These files come from Universal Agents (`enzimo/universal-agents`) so the Status window looks like current UA. UA is licensed under Apache-2.0; `LICENSE` is UA's license file, unchanged. UA has no `NOTICE` file at this tag, so none is carried.

Source: UA tag `v0.9.4`. All six files are byte-identical to that tag. (They were first copied from UA `main` on 2026-10-10 and last updated on 2026-10-12; UA's repository was recreated in October 2026 without its old history, so those commits no longer exist.) `intro_word.js` gained two fixes since the first copy. The first draws the settled facets of letters still forming on their own canvas (`ua-intro-word-settled`), so a letter no longer shows a flickering outline before it has formed. `../styles.css` positions that canvas like the other title layers, as UA's `styles.css` does.

| File | UA source path | Changes |
| --- | --- | --- |
| `metallic_mesh.js` | `assistant_gateway/onboarding_web/assets/metallic_mesh.js` | None |
| `static-mesh.js` | `user_docs_source/assets/static-mesh.js` | None |
| `ua-runtime-theme.css` | `user_docs_source/assets/ua-runtime-theme.css` | None |
| `glass_mesh.js` | `assistant_gateway/onboarding_web/assets/glass_mesh.js` | None |
| `intro_word.js` | `assistant_gateway/onboarding_web/assets/intro_word.js` | None |
| `ui_motion.js` | `assistant_gateway/onboarding_web/assets/ui_motion.js` | None |

Every vendored file is byte-identical to UA. Desktop adaptations live in desktop-owned files, so re-vendoring is a copy followed by a review of the items below.

The second gives the two blurred title layers (the slab and the turning triangles) a transparent margin of three blur radii. WebKit repaints only a canvas's own box when its drawing changes, so the part of a CSS blur that spread past the box stayed on screen as a faint shadow framing the word after the slab had dissolved.

## How the desktop uses them

- **Background.** The page canvas has the class `ua-doc-static-mesh`, which UA's renderers treat as a still surface (as on UA's Docs pages). `metallic_mesh.js` draws the dark metal once on its own layer and redraws only on resize or a theme change; `static-mesh.js` draws the light polygons the same way. Without WebGL2 the dark background uses UA's basic 2D drawing and logs a console warning. Nothing calls `UAMetallicMesh.reveal`, so no frame loop runs.
- **Theme settings.** `ua-runtime-theme.css` supplies the background settings as CSS variables. The desktop no longer adds `--ua-light-mesh-seed: 4147`: `static-mesh.js` defaults to that same seed when the variable is missing.
- **Intro.** `intro_word.js` (the polygon title) and `glass_mesh.js` (its glass facets) run unchanged. `ui_motion.js` is included only for `UAMeshGlyph` (the flat mesh that flashes inside letters) and `UAMotion.palette` (glint colors). The desktop never calls its `playIntro`, `playGreeting`, `fillBackground`, `ensureIntroOutline` or `arrive`; its message-arrival code is unused. `intro.js` repeats `fillBackground` itself (below).

## Desktop replacements for UA files that are not vendored

- `../theme.js` replaces UA's `appearance.js`. It does not fetch `/api/web-chat/appearance` or offer customization. It reads the dark and light settings from `ua-runtime-theme.css` the way `appearance.js` builds its CSS baseline for Docs, and copies `appearance.js`'s motion accent (`--ua-motion-accent`) and motion palette rules. Dark is the default instead of `--ua-runtime-color-mode`, and the Dark/Light choice is saved in `localStorage` under `ua-desktop.status-window.color-mode`.
- `../intro.js` replaces the intro portion of `ui_motion.js` (`playIntro`). Differences:
  - The background fills in once as the intro starts, as UA's `fillBackground` does: `UAMetallicMesh.reveal(1800)` in dark mode and the `ua-intro-filling` fade in light mode, with `data-ua-background-hold` keeping it blank until then. Afterwards it stays still: the reveal's frame loop stops when the fill ends because the canvas is a still surface. With reduced motion there is no hold and no fill.
  - UA's `ensureIntroOutline` sets the `feFlood` color and the hidden SVG's position with `style` attributes, which the Status window's Content Security Policy (`style-src 'self'`) blocks. `intro.js` builds the same filter with the same id and classes, and `../styles.css` sets `flood-color` and the position. Because the id already exists, UA's version would do nothing even if called.
  - It runs at the top of `<body>` so the overlay exists before the launcher is drawn, then starts the title once the page has loaded and the window is visible.
  - Enter and Space dismiss at any time, not only after **Click to Begin** has focus.
  - After 30 seconds unattended, the title stops flashing (`showStill`) and the button stops glowing, so an always-open launcher draws no frames.
  - Every other element on the page is inert while the intro shows; on dismissal the overlay is hidden from assistive technology at once, focus moves to the launcher heading, and the Docker prerequisite dialog may appear.
- `../styles.css` adapts UA's intro styles (`.ua-intro*`, `ua-beacon`, `ua-assemble`) and glass variables (`--ua-glass-*`, `--ua-intro-word-*`) from UA's `assistant_gateway/onboarding_web/assets/styles.css`. The light "Click to Begin" pill is light glass instead of UA's dark pill so its text stays readable, the dark pill is slightly more opaque (0.62) for WCAG AA over bright plates, and the beacon glow uses a fixed color instead of `color-mix()`.

## Updating

1. Copy the six files from the UA paths above and update the commit and version here.
2. Compare UA's `playIntro`, `ensureIntroOutline` and intro styles with `../intro.js` and `../styles.css`, and `appearance.js`'s motion accent, motion palette and dark CSS variable names with `../theme.js`.
3. Search the new files for `setAttribute("style"`, `style=`, `eval` and `new Function`; the Status window's CSP blocks inline styles and scripts.
4. Run `node --test ui/desktop-shell/app.test.cjs` (it reads the vendored CSS and runs `metallic_mesh.js` without WebGL2) and the native UI suite in `tests/ui/`.
