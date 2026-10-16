// WKWebView can suppress target=_blank anchors before its new-window delegate.
// Route those clicks through window.open so the native URL policy handles them.
// This adapter exposes no IPC or desktop capability to the hosted page.
(() => {
  if (window !== window.top || window.__uaDesktopPopupLinks) return;
  window.__uaDesktopPopupLinks = true;
  document.addEventListener("click", (event) => {
    const link = event.target.closest?.("a[target='_blank']");
    if (!link || event.defaultPrevented || event.button !== 0 || link.hasAttribute("download")) return;
    const url = new URL(link.href, location.href);
    if (!['http:', 'https:'].includes(url.protocol)) return;
    event.preventDefault();
    const child = window.open("about:blank", "_blank");
    if (!child) return;
    // Match the implicit noopener behavior of target=_blank. Opening external
    // URLs is subsequently handled by native policy in the system browser.
    child.opener = null;
    if (link.relList.contains("noreferrer")) {
      const policy = child.document.createElement("meta");
      policy.name = "referrer";
      policy.content = "no-referrer";
      child.document.head.append(policy);
    }
    child.location.replace(url.href);
  });
})();
