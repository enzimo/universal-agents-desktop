(() => {
  "use strict";

  // Status window appearance. Dark is the default; the header's sun/moon
  // button switches themes and the choice is remembered on this computer.
  //
  // The vendored background renderers (vendor/static-mesh.js,
  // vendor/metallic_mesh.js) and the intro (intro.js, vendor/intro_word.js,
  // vendor/glass_mesh.js) read `window.UAAppearance` the way UA's Docs pages
  // do: background settings come from the CSS variables in
  // vendor/ua-runtime-theme.css, and every change is announced with the
  // `ua:appearance-change` event. The motion accent and palette follow UA's
  // appearance.js (see vendor/README.md). There is no server preference and
  // no theme customization in the desktop app.
  //
  // This script runs in <head> after the stylesheets, so the color mode is
  // set before the body is drawn and the window never flashes the other theme.

  const STORAGE_KEY = "ua-desktop.status-window.color-mode";
  const DARK = "dark";
  const LIGHT = "light";
  const MODES = [DARK, LIGHT];
  const DEFAULT_MODE = DARK;
  const CHANGE_EVENT = "ua:appearance-change";
  const TOGGLE_ATTRIBUTE = "data-theme-toggle";
  // The switch names the mode it switches to; its icon (sun or moon) follows.
  const TOGGLE_LABELS = { [DARK]: "Switch to light theme", [LIGHT]: "Switch to dark theme" };
  const LOG_PREFIX = "ua.desktop.theme:";

  // Dark background settings and their CSS variables, as in UA's
  // appearance.js DARK_FIELDS. metallic_mesh.js clamps and fills the rest.
  const DARK_FIELDS = [
    ["detail", "--ua-dark-detail", "text"],
    ["surfaceColor", "--ua-dark-surface", "text"],
    ["metalColor", "--ua-dark-metal", "text"],
    ["facetSize", "--ua-dark-facet-size", "number"],
    ["facetDepth", "--ua-dark-facet-depth", "number"],
    ["chromeFaces", "--ua-dark-chrome-faces", "number"],
    ["shine", "--ua-dark-shine", "number"],
    ["edgeHighlights", "--ua-dark-edge-highlights", "number"],
    ["surfaceGrain", "--ua-dark-surface-grain", "number"],
    ["lightX", "--ua-dark-light-x", "number"],
    ["lightY", "--ua-dark-light-y", "number"],
    ["lightSoftness", "--ua-dark-light-softness", "number"],
    ["lightMotion", "--ua-dark-light-motion", "boolean"],
    ["lightWander", "--ua-dark-light-wander", "boolean"],
    ["lightWanderDistance", "--ua-dark-light-wander-distance", "number"],
    ["lightSpeed", "--ua-dark-light-speed", "number"],
    ["meshMotion", "--ua-dark-mesh-motion", "boolean"],
    ["meshSpeed", "--ua-dark-mesh-speed", "number"],
    ["seed", "--ua-dark-seed", "number"],
  ];
  const DARK_ACCENTS = ["gold", "bronze"];
  // UA's defaults, used only when the vendored theme variables are missing.
  const FALLBACK_LIGHT_COLORS = ["#082f73", "#ffd60a", "#20f05a", "#ff3030"];
  const FALLBACK_METAL = "#015b1a";
  const FALLBACK_ACCENTS = { gold: { color: "#464020", share: 10 }, bronze: { color: "#00751d", share: 39 } };
  // UA's appearance.js: palette colors are lifted to read over dark panes.
  const MOTION_PALETTE_LIGHTNESS = 0.58;

  const root = document.documentElement;
  let mode = initialMode();
  let cachedDark = null;

  function cssSetting(name) {
    return window.getComputedStyle(root).getPropertyValue(name).trim();
  }

  function isHexColor(value) {
    return /^#[0-9a-f]{6}$/i.test(value);
  }

  function initialMode() {
    let saved = null;
    try {
      saved = window.localStorage.getItem(STORAGE_KEY);
    } catch (error) {
      console.warn(`${LOG_PREFIX} saved color mode is unreadable; using ${DEFAULT_MODE}`, error);
      return DEFAULT_MODE;
    }
    if (saved === null) return DEFAULT_MODE;
    if (MODES.includes(saved)) return saved;
    console.warn(`${LOG_PREFIX} ignoring unknown saved color mode; using ${DEFAULT_MODE}`, { saved });
    return DEFAULT_MODE;
  }

  // Read once: the vendored variables do not change while the window is open.
  function darkSettings() {
    if (cachedDark) return cachedDark;
    const dark = { accents: {} };
    for (const [name, variable, kind] of DARK_FIELDS) {
      const raw = cssSetting(variable);
      if (!raw) continue;
      dark[name] = kind === "number" ? Number(raw) : kind === "boolean" ? raw === "true" : raw;
    }
    for (const accent of DARK_ACCENTS) {
      const color = cssSetting(`--ua-dark-${accent}`);
      const share = Number(cssSetting(`--ua-dark-${accent}-share`));
      dark.accents[accent] = {
        color: isHexColor(color) ? color.toLowerCase() : FALLBACK_ACCENTS[accent].color,
        share: Number.isFinite(share) ? share : FALLBACK_ACCENTS[accent].share,
      };
    }
    cachedDark = dark;
    return dark;
  }

  function lightColors() {
    return ["blue", "yellow", "green", "red"].map((name, index) => {
      const color = cssSetting(`--ua-doc-light-${name}`);
      return isHexColor(color) ? color.toLowerCase() : FALLBACK_LIGHT_COLORS[index];
    });
  }

  function current() {
    return { mode, dark: JSON.parse(JSON.stringify(darkSettings())), light: { colors: lightColors() } };
  }

  function hexToHsl(hex) {
    const rgb = [1, 3, 5].map(offset => parseInt(hex.slice(offset, offset + 2), 16) / 255);
    const max = Math.max(...rgb);
    const min = Math.min(...rgb);
    const lightness = (max + min) / 2;
    const delta = max - min;
    if (!delta) return [0, 0, lightness];
    const saturation = delta / (1 - Math.abs(2 * lightness - 1));
    const hue = max === rgb[0] ? ((rgb[1] - rgb[2]) / delta) % 6
      : max === rgb[1] ? (rgb[2] - rgb[0]) / delta + 2
        : (rgb[0] - rgb[1]) / delta + 4;
    return [(hue * 60 + 360) % 360, saturation, lightness];
  }

  function hslToHex([hue, saturation, lightness]) {
    const chroma = (1 - Math.abs(2 * lightness - 1)) * saturation;
    const x = chroma * (1 - Math.abs((hue / 60) % 2 - 1));
    const m = lightness - chroma / 2;
    const sector = Math.floor(hue / 60) % 6;
    const rgb = [[chroma, x, 0], [x, chroma, 0], [0, chroma, x], [0, x, chroma], [x, 0, chroma], [chroma, 0, x]][sector];
    return `#${rgb.map(value => `0${Math.round((value + m) * 255).toString(16)}`.slice(-2)).join("")}`;
  }

  function within(value, low, high) {
    return Math.min(high, Math.max(low, value));
  }

  // The accent of corner brackets and intro glints: in dark mode the hue of
  // the gold metal tint made light enough to read as a line; in light mode
  // the hue of the first (blue) light, dark enough for light glass.
  function motionAccent() {
    if (mode === LIGHT) {
      const light = hexToHsl(lightColors()[0]);
      return hslToHex([light[0], within(light[1], 0.5, 0.9), within(light[2], 0.22, 0.34)]);
    }
    const gold = hexToHsl(darkSettings().accents.gold.color);
    return hslToHex([gold[0], Math.max(gold[1], 0.5), Math.max(gold[2], 0.62)]);
  }

  function liftForMotion(hex) {
    const hsl = hexToHsl(hex);
    return hslToHex([hsl[0], Math.min(hsl[1], 0.75), Math.max(hsl[2], MOTION_PALETTE_LIGHTNESS)]);
  }

  // The intro letters' glints and flashes: the four light colors in light
  // mode; in dark mode the metal with the gold and bronze tints at their
  // shares of the plates. Returns [{ color: "#rrggbb", weight }].
  function motionPalette() {
    if (mode === LIGHT) return lightColors().map(color => ({ color, weight: 1 }));
    const dark = darkSettings();
    const metal = isHexColor(dark.metalColor || "") ? dark.metalColor : FALLBACK_METAL;
    const accentShare = DARK_ACCENTS.reduce((sum, name) => sum + dark.accents[name].share, 0);
    const palette = [{ color: liftForMotion(metal), weight: Math.max(0, 100 - accentShare) }];
    for (const name of DARK_ACCENTS) {
      const accent = dark.accents[name];
      if (accent.share > 0) palette.push({ color: liftForMotion(accent.color), weight: accent.share });
    }
    return palette;
  }

  function apply() {
    root.setAttribute("data-color-mode", mode);
    // CSSOM custom property, which the Status window's CSP allows.
    root.style.setProperty("--ua-motion-accent", motionAccent());
    syncButtons();
  }

  function syncButtons() {
    if (!document.querySelectorAll) return;
    for (const button of document.querySelectorAll(`[${TOGGLE_ATTRIBUTE}]`)) {
      button.setAttribute("aria-label", TOGGLE_LABELS[mode]);
      button.setAttribute("title", TOGGLE_LABELS[mode]);
    }
  }

  function setMode(next) {
    if (!MODES.includes(next)) {
      console.warn(`${LOG_PREFIX} ignoring unknown color mode`, { requested: next });
      return mode;
    }
    try {
      window.localStorage.setItem(STORAGE_KEY, next);
    } catch (error) {
      console.warn(`${LOG_PREFIX} color mode could not be saved; it applies until the app restarts`, error);
    }
    if (next === mode) return mode;
    mode = next;
    apply();
    document.dispatchEvent(new CustomEvent(CHANGE_EVENT, { detail: { appearance: current() } }));
    return mode;
  }

  function connectButtons() {
    for (const button of document.querySelectorAll(`[${TOGGLE_ATTRIBUTE}]`)) {
      button.addEventListener("click", () => setMode(mode === DARK ? LIGHT : DARK));
    }
    syncButtons();
  }

  window.UAAppearance = { current, motionPalette, setMode, storageKey: STORAGE_KEY };
  apply();
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", connectButtons, { once: true });
  } else {
    connectButtons();
  }
})();
