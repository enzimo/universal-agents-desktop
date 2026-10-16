// Mockup-only behavior: the toolbar switches the launcher self-update badge
// between its states, and [data-open] / [data-close] buttons show and hide a
// mock dialog or drawer. Nothing here talks to the real app.
(function () {
  const root = document.documentElement;
  const params = new URLSearchParams(location.search);
  function setLauncherState(state) {
    root.dataset.launcherState = state;
    document.querySelectorAll("[data-set-launcher]").forEach((button) => {
      button.setAttribute("aria-pressed", String(button.dataset.setLauncher === state));
    });
  }
  document.addEventListener("click", (event) => {
    const set = event.target.closest("[data-set-launcher]");
    if (set) setLauncherState(set.dataset.setLauncher);
    const open = event.target.closest("[data-open]");
    if (open) document.getElementById(open.dataset.open).hidden = false;
    const close = event.target.closest("[data-close]");
    if (close) document.getElementById(close.dataset.close).hidden = true;
  });
  document.addEventListener("keydown", (event) => {
    if (event.key !== "Escape") return;
    document.querySelectorAll("[data-dismissable]").forEach((element) => { element.hidden = true; });
  });
  setLauncherState(params.get("launcher") || "available");
  if (params.get("open")) {
    const target = document.getElementById(params.get("open"));
    if (target) target.hidden = false;
  }
})();
