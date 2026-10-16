(function () {
  "use strict";

  // The Status window's intro: the glass "Universal Agents" title forms over
  // the still background, then "Click to Begin" appears. A click, Enter,
  // Space or Escape fades the intro out and the launcher in. Without motion
  // the finished title and the button show at once.
  //
  // Adapted from UA's onboarding intro (ui_motion.js playIntro); the polygon
  // title is UA's vendor/intro_word.js. Differences from UA, all listed in
  // vendor/README.md:
  // - The background fills in once as the intro starts, as in UA
  //   (fillBackground), and then stays still. Until then it is held blank
  //   with UA's data-ua-background-hold mark. Without motion it shows at once.
  // - The outline filter is built without style attributes, which the
  //   Status window's Content Security Policy blocks; styles.css colors it.
  // - This script runs at the top of <body>, so the overlay exists before the
  //   launcher is drawn. The title starts once the page has loaded and the
  //   window is visible (the app can start hidden at login).
  // - Enter and Space dismiss at any time, not only once the button shows.
  // - After the intro has waited IDLE_STILL_MS the title settles into its
  //   still form, so an unattended window does not keep drawing frames.
  // - The launcher and its siblings are inert while the intro shows, and the
  //   overlay leaves the accessibility tree as soon as it is dismissed.
  //
  // `window.UADesktopIntro` lets app.js hold the Docker prerequisite dialog
  // until the intro is dismissed: isOpen(), onDismiss(callback), dismiss().
  // Listeners also run if the intro fails after it has covered the launcher.

  if (window.UADesktopIntro) return;

  var INTRO_TITLE = "Universal Agents";
  var BEGIN_LABEL = "Click to Begin";
  // Timing from UA's ui_motion.js.
  var INTRO_WORD_AT_MS = 500;
  var INTRO_TITLE_AT_MS = 1400;
  var INTRO_BEGIN_AT_MS = 2100;
  var INTRO_BEGIN_AFTER_WORD_MS = 250;
  var INTRO_LEAVE_MS = 450;
  var INTRO_ENTER_MS = 600;
  // Desktop only: how long the finished title keeps flashing before it
  // settles. The launcher may stay open all day.
  var IDLE_STILL_MS = 30000;
  var DISMISS_KEYS = ["Enter", " ", "Escape"];
  // UA's ui_motion.js INTRO_FILL_MS, the mark metallic_mesh.js watches, and
  // the class styles.css uses to fade in the light background.
  var INTRO_FILL_MS = 1800;
  var BACKGROUND_HOLD_ATTRIBUTE = "data-ua-background-hold";
  var FILL_CLASS = "ua-intro-filling";

  var OPEN_CLASS = "ua-intro-open";
  var ENTER_CLASS = "ua-intro-enter";
  var FOCUS_SELECTOR = "[data-intro-focus]";
  var SHELL_SELECTOR = ".shell";
  var OUTLINE_FILTER_ID = "ua-intro-outline";
  var OUTLINE_WIDTH = 1;
  var SVG_NS = "http://www.w3.org/2000/svg";
  var LOG_PREFIX = "ua.desktop.intro:";

  var reducedMotion = window.matchMedia
    ? window.matchMedia("(prefers-reduced-motion: reduce)")
    : { matches: false, addEventListener: function () {}, removeEventListener: function () {} };

  var state = "idle";
  var overlay = null;
  var title = null;
  var begin = null;
  var outline = null;
  var word = null;
  var animated = false;
  var timers = [];
  var inerted = [];
  var dismissListeners = [];

  function later(callback, delay) {
    timers.push(window.setTimeout(callback, delay));
  }

  function clearTimers() {
    timers.forEach(window.clearTimeout);
    timers = [];
  }

  function svgElement(name, attributes, children) {
    var element = document.createElementNS(SVG_NS, name);
    Object.keys(attributes).forEach(function (key) { element.setAttribute(key, attributes[key]); });
    (children || []).forEach(function (child) { element.appendChild(child); });
    return element;
  }

  // UA's outline filter: a ring around each letter's outer shape, drawn
  // under the letters. UA sets the flood color and the hidden SVG's position
  // with style attributes; here styles.css sets both through the classes.
  function ensureOutline() {
    if (document.getElementById(OUTLINE_FILTER_ID)) return;
    var filter = svgElement("filter", { id: OUTLINE_FILTER_ID, x: "-5%", y: "-10%", width: "110%", height: "120%" }, [
      svgElement("feComponentTransfer", { in: "SourceAlpha", result: "solid" }, [
        svgElement("feFuncA", { type: "linear", slope: "4", intercept: "0" }),
      ]),
      svgElement("feMorphology", { in: "solid", operator: "dilate", radius: String(OUTLINE_WIDTH), result: "grown" }),
      svgElement("feComposite", { in: "grown", in2: "solid", operator: "out", result: "ring" }),
      svgElement("feFlood", { class: "ua-intro-outline-color", result: "color" }),
      svgElement("feComposite", { in: "color", in2: "ring", operator: "in", result: "outline" }),
      svgElement("feMerge", {}, [
        svgElement("feMergeNode", { in: "outline" }),
        svgElement("feMergeNode", { in: "SourceGraphic" }),
      ]),
    ]);
    outline = svgElement("svg", { class: "ua-intro-outline-defs", width: "0", height: "0", "aria-hidden": "true", focusable: "false" }, [
      svgElement("defs", {}, [filter]),
    ]);
    document.body.appendChild(outline);
  }

  // Keep the background blank until the intro fills it in. Set before the
  // deferred renderers run, so the dark metal never draws a full frame first.
  function holdBackground() {
    if (reducedMotion.matches) return;
    document.documentElement.setAttribute(BACKGROUND_HOLD_ATTRIBUTE, "");
  }

  // Shows a held background at once (no fill), for example when the intro
  // ends before it played or could not start.
  function releaseBackground() {
    document.documentElement.removeAttribute(BACKGROUND_HOLD_ATTRIBUTE);
  }

  // UA's fillBackground: the dark plates fade in from the middle outward;
  // the light polygons fade in as a whole. Both stay still afterwards.
  function fillBackground() {
    var root = document.documentElement;
    if (!root.hasAttribute(BACKGROUND_HOLD_ATTRIBUTE)) return;
    var dark = root.getAttribute("data-color-mode") !== "light";
    var metal = window.UAMetallicMesh;
    if (dark && metal && typeof metal.reveal === "function") {
      metal.reveal(INTRO_FILL_MS);
    } else {
      root.classList.add(FILL_CLASS);
      window.setTimeout(function () { root.classList.remove(FILL_CLASS); }, INTRO_FILL_MS);
    }
    releaseBackground();
  }

  function mount() {
    holdBackground();
    ensureOutline();
    overlay = document.createElement("section");
    overlay.classList.add("ua-intro");
    overlay.setAttribute("role", "dialog");
    overlay.setAttribute("aria-modal", "true");
    overlay.setAttribute("aria-labelledby", "ua-intro-title");
    overlay.setAttribute("tabindex", "-1");
    title = document.createElement("h1");
    title.id = "ua-intro-title";
    title.classList.add("ua-intro-title");
    title.textContent = INTRO_TITLE;
    begin = document.createElement("button");
    begin.type = "button";
    begin.classList.add("ua-intro-begin");
    begin.textContent = BEGIN_LABEL;
    overlay.appendChild(title);
    overlay.appendChild(begin);
    // Dismissal works before anything else, so the overlay can never trap
    // the launcher behind it.
    overlay.addEventListener("click", dismiss);
    document.addEventListener("keydown", onKey);
    document.body.appendChild(overlay);
    document.documentElement.classList.add(OPEN_CLASS);
    state = "open";
  }

  function onKey(event) {
    if (state !== "open" || DISMISS_KEYS.indexOf(event.key) < 0) return;
    // Keep Enter and Space from also activating whatever has focus.
    event.preventDefault();
    dismiss();
  }

  // Everything else on the page stays out of reach while the intro shows.
  function inertSiblings() {
    if (state !== "open") return;
    Array.prototype.forEach.call(document.body.children, function (element) {
      if (element === overlay || element === outline || element.tagName === "SCRIPT") return;
      element.inert = true;
      inerted.push(element);
    });
  }

  function whenReady(callback) {
    if (document.readyState === "loading") {
      document.addEventListener("DOMContentLoaded", callback, { once: true });
    } else {
      callback();
    }
  }

  function whenVisible(callback) {
    if (!document.hidden) {
      callback();
      return;
    }
    document.addEventListener("visibilitychange", function onVisibility() {
      if (document.hidden) return;
      document.removeEventListener("visibilitychange", onVisibility);
      callback();
    });
  }

  function createWord() {
    if (window.UAIntroWord) return window.UAIntroWord.create(title, { text: INTRO_TITLE });
    console.warn(LOG_PREFIX + " the polygon title is unavailable; the title shows as text.");
    return null;
  }

  function play() {
    if (state !== "open") return;
    animated = !reducedMotion.matches;
    word = createWord();
    reducedMotion.addEventListener("change", motionChanged);
    if (!animated) {
      releaseBackground();
      showStill();
      return;
    }
    fillBackground();
    overlay.classList.add("is-animated");
    // Screen readers announce the dialog; the button takes focus once shown.
    overlay.focus();
    if (word) {
      later(function () {
        overlay.classList.add("show-title");
        word.play(function () { later(showBegin, INTRO_BEGIN_AFTER_WORD_MS); });
      }, INTRO_WORD_AT_MS);
    } else {
      later(function () { overlay.classList.add("show-title"); }, INTRO_TITLE_AT_MS);
      later(showBegin, INTRO_BEGIN_AT_MS);
    }
  }

  function showBegin() {
    if (state !== "open") return;
    overlay.classList.add("show-begin");
    begin.focus();
    later(settle, IDLE_STILL_MS);
  }

  // The finished title and button at once, without motion.
  function showStill() {
    clearTimers();
    overlay.classList.remove("is-animated");
    overlay.classList.add("show-title", "show-begin");
    if (word) word.showStill();
    begin.focus();
  }

  // The unattended intro stops drawing frames; the title stays formed.
  function settle() {
    if (state !== "open") return;
    overlay.classList.add("is-still");
    if (word) word.showStill();
  }

  function motionChanged() {
    if (state !== "open" || !animated || !reducedMotion.matches) return;
    animated = false;
    showStill();
  }

  function dismiss() {
    if (state !== "open") return;
    state = "dismissed";
    // Dismissed before the intro played (for example from app.js).
    releaseBackground();
    clearTimers();
    document.removeEventListener("keydown", onKey);
    reducedMotion.removeEventListener("change", motionChanged);
    var fade = !reducedMotion.matches;
    overlay.classList.add("is-leaving");
    overlay.setAttribute("aria-hidden", "true");
    overlay.inert = true;
    document.documentElement.classList.remove(OPEN_CLASS);
    inerted.forEach(function (element) { element.inert = false; });
    inerted = [];
    var shell = document.querySelector(SHELL_SELECTOR);
    if (shell && fade) {
      shell.classList.add(ENTER_CLASS);
      window.setTimeout(function () { shell.classList.remove(ENTER_CLASS); }, INTRO_ENTER_MS);
    }
    var target = document.querySelector(FOCUS_SELECTOR);
    if (target && typeof target.focus === "function") target.focus();
    window.setTimeout(function () {
      if (word) word.stop();
      overlay.remove();
    }, fade ? INTRO_LEAVE_MS + 300 : 0);
    notifyDismissed();
  }

  // Lets the launcher show what waited for the intro (app.js: the Docker
  // prerequisite dialog).
  function notifyDismissed() {
    dismissListeners.forEach(function (listener) {
      try {
        listener();
      } catch (error) {
        console.error(LOG_PREFIX + " a dismiss listener failed", error);
      }
    });
  }

  // A failed start must never leave the launcher hidden behind the overlay.
  function abandon(error) {
    console.error(LOG_PREFIX + " the intro could not start; showing the launcher", error);
    var wasOpen = state === "open";
    state = "failed";
    releaseBackground();
    clearTimers();
    document.removeEventListener("keydown", onKey);
    reducedMotion.removeEventListener("change", motionChanged);
    document.documentElement.classList.remove(OPEN_CLASS);
    inerted.forEach(function (element) { element.inert = false; });
    inerted = [];
    if (word) word.stop();
    if (overlay) overlay.remove();
    if (wasOpen) notifyDismissed();
  }

  function guarded(callback) {
    return function () {
      try {
        callback();
      } catch (error) {
        abandon(error);
      }
    };
  }

  window.UADesktopIntro = {
    isOpen: function () { return state === "open"; },
    onDismiss: function (listener) { dismissListeners.push(listener); },
    dismiss: dismiss,
  };

  guarded(function () {
    mount();
    whenReady(guarded(function () {
      inertSiblings();
      whenVisible(guarded(play));
    }));
  })();
}());
