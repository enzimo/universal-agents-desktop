const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");

// A minimal element: enough of the DOM for app.js to render into.
function makeElement(document, tag = "div") {
  return {
    tagName: tag.toUpperCase(), value: "", listeners: {}, attributes: {}, children: [], dataset: {},
    addEventListener(event, callback) { this.listeners[event] = callback; },
    removeAttribute(name) { delete this[name]; },
    setAttribute(name, value) { this.attributes[name] = String(value); },
    getAttribute(name) { return this.attributes[name] ?? null; },
    append(...nodes) { this.children.push(...nodes); },
    replaceChildren(...nodes) { this.children = nodes; },
    focus() { document.activeElement = this; },
  };
}

// All text in an element and its rendered children, in order.
function textOf(node) {
  return [node.textContent ?? "", ...(node.children ?? []).map(textOf)].join("");
}

function setup(responses = {}, windowExtras = {}) {
  const elements = new Map();
  const calls = [];
  const timers = [];
  const timeouts = [];
  const documentListeners = {};
  let progressListener;
  const context = vm.createContext({
    window: { ...windowExtras, __TAURI__: { event: { listen: async (name, callback) => { progressListener = callback; return () => { progressListener = undefined; }; } }, core: { invoke: async (command, args) => {
      calls.push({ command, args });
      if (responses[command]) return responses[command](args);
      if (command === "registry_access") throw new Error("Access denied");
      // Leave initial refresh pending; each test supplies an explicit runtime state.
      return new Promise(() => {});
    } } } },
    document: {
      activeElement: null,
      body: { classList: { _classes: new Set(), toggle(name) {
        if (this._classes.has(name)) { this._classes.delete(name); return false; }
        this._classes.add(name); return true;
      }, contains(name) { return this._classes.has(name); } } },
      querySelector(id) {
        if (!elements.has(id)) elements.set(id, makeElement(context.document));
        return elements.get(id);
      },
      createElement(tag) { return makeElement(context.document, tag); },
      addEventListener(type, callback) { (documentListeners[type] ||= []).push(callback); },
    },
    setInterval(callback, delay) { timers.push({callback, delay}); },
    setTimeout(callback, delay) { const timeout = {callback, delay}; timeouts.push(timeout); return timeout; },
    clearTimeout(timeout) { const index = timeouts.indexOf(timeout); if (index >= 0) timeouts.splice(index, 1); },
  });
  // Most tests read the logs, so their section starts open; one test closes it.
  context.document.querySelector("#logs-section").open = true;
  vm.runInContext(fs.readFileSync(`${__dirname}/app.js`, "utf8"), context);
  return {
    context, elements, calls, timers, timeouts,
    // Run the pending timeouts of `delay` ms, as if that time passed.
    elapse: (delay) => timeouts.filter(timeout => timeout.delay === delay).forEach(timeout => {
      timeouts.splice(timeouts.indexOf(timeout), 1);
      timeout.callback();
    }),
    progress: payload => progressListener?.({payload}),
    run: (code) => vm.runInContext(code, context),
    key: (key) => (documentListeners.keydown || []).forEach(callback => callback({ key })),
    text: (id) => textOf(elements.get(id)),
  };
}

test("import is enabled during onboarding and ready, with actionable pending copy", () => {
  const ui = setup();
  ui.run(`renderIntegration({workspace_available: true, openai_subscription_state: "pending_import", pending_codex_auth_file: "/source/auth.json", openai_subscription_detail: "Ready to import"});`);
  for (const phase of ["not_created", "docker_unavailable", "stopped", "starting", "error"]) {
    ui.run(`renderStatus({phase: "${phase}"})`);
    assert.equal(ui.elements.get("#finish-subscription").disabled, true, phase);
    assert.match(ui.elements.get("#subscription-detail").textContent, /selection is saved/);
  }
  ui.run('renderStatus({phase: "onboarding"})');
  assert.equal(ui.elements.get("#finish-subscription").disabled, false);
  ui.run('renderStatus({phase: "ready"})');
  assert.equal(ui.elements.get("#finish-subscription").disabled, false);
  ui.run("setBusy(true)");
  assert.equal(ui.elements.get("#finish-subscription").disabled, true);
});

test("PAT clears immediately even on error and never appears in notice", async () => {
  const ui = setup();
  ui.elements.get("#registry-token").value = "private-test-token";
  ui.elements.get("#registry-user").value = "octocat";
  await ui.run('authenticateRegistry("pat")');
  assert.equal(ui.elements.get("#registry-token").value, "");
  const request = ui.calls.find(call => call.command === "registry_access");
  assert.equal(request.args.token, "private-test-token");
  assert.equal(request.args.method, "pat");
  assert.doesNotMatch(ui.elements.get("#notice").textContent, /private-test-token/);
  assert.equal(ui.elements.get("#registry-pat").disabled, false);
});

for (const phase of ["onboarding", "ready"]) {
  test(`automatic import completes during ${phase} and stops after success`, async () => {
    const ui = setup({
      finish_openai_subscription: () => ({openai_subscription_state: "imported", workspace_available: true}),
      runtime_status: () => ({phase}),
    });
    ui.run('latestIntegration = {openai_subscription_state: "pending_import"};');
    for (const blocked of ["not_created", "starting", "error", "docker_unavailable", "stopped"]) {
      ui.run(`latestStatus = {phase: "${blocked}"}`);
      await ui.run("attemptPendingImport()");
    }
    assert.equal(ui.calls.filter(call => call.command === "finish_openai_subscription").length, 0);
    ui.run(`latestStatus = {phase: "${phase}"}`);
    await ui.run("attemptPendingImport()");
    assert.equal(ui.run("latestIntegration.openai_subscription_state"), "imported");
    assert.equal(ui.run("operationInFlight"), false);
    await ui.run("attemptPendingImport()");
    assert.equal(ui.calls.filter(call => call.command === "finish_openai_subscription").length, 1);
  });
}

test("failed onboarding import remains pending and allows manual retry without polling retries", async () => {
  const ui = setup({finish_openai_subscription: () => {throw new Error("Choose a current auth.json");}});
  ui.run('latestIntegration = {openai_subscription_state: "pending_import"}; latestStatus = {phase: "onboarding"};');
  await ui.run("attemptPendingImport()");
  assert.equal(ui.run("latestIntegration.openai_subscription_state"), "pending_import");
  assert.equal(ui.elements.get("#finish-subscription").disabled, false);
  assert.match(ui.elements.get("#notice").textContent, /Download diagnostics/);
  await ui.run("attemptPendingImport()");
  assert.equal(ui.calls.filter(call => call.command === "finish_openai_subscription").length, 1);
});

test("Docker unavailable opens a blocking dialog and disables runtime controls", async () => {
  const ui = setup();
  ui.run('renderIntegration({workspace_available: true});');
  ui.run('latestDockerPrerequisite = {available: false, installed: false, setup_hint: "Install Docker Desktop"}; applyDockerState(false);');
  assert.equal(ui.elements.get("#docker-gate").hidden, false);
  assert.equal(ui.elements.get("#docker-gate-hint").textContent, "Install Docker Desktop");
  // The dialog is genuinely blocking: the launcher behind it is inert.
  assert.equal(ui.elements.get(".shell").inert, true);
  // Recovery controls stay available so the user can quit, read logs, export
  // diagnostics, and record a workspace or credential choice.
  for (const id of ["#quit-desktop-app", "#download-diagnostics",
    "#choose-workspace", "#connect-subscription", "#cancel-subscription"]) {
    assert.equal(ui.elements.get(id).disabled, false, id);
  }
  // Enlarging the log view is a pure layout action and stays reachable.
  assert.notEqual(ui.elements.get("#expand-logs").disabled, true);
  // Anything that needs the daemon is disabled.
  for (const id of ["#start-runtime", "#restart-runtime", "#stop-runtime", "#stop-runtime-and-quit",
    "#registry-existing", "#registry-gh", "#registry-login", "#download-update"]) {
    assert.equal(ui.elements.get(id).disabled, true, id);
  }
});

test("Proceed reveals the launcher with a 30-second waiting banner", async () => {
  const ui = setup();
  ui.run('renderIntegration({workspace_available: true}); applyDockerState(false);');
  ui.elements.get("#docker-proceed").listeners.click();
  assert.equal(ui.elements.get("#docker-gate").hidden, true);
  // Proceed releases the launcher so recovery actions are reachable.
  assert.equal(ui.elements.get(".shell").inert, false);
  assert.equal(ui.elements.get("#docker-banner").hidden, false);
  assert.match(ui.elements.get("#docker-banner-text").textContent,
    /still unable to detect Docker.*automatic check every 30 sec/);
  // Buttons remain disabled until Docker answers.
  assert.equal(ui.elements.get("#start-runtime").disabled, true);
  const timer = ui.timers.find(timer => timer.delay === 30000);
  assert.ok(timer, "a 30-second Docker check must be registered");
  // The periodic check only runs while Docker is unavailable.
  timer.callback();
  assert.equal(ui.calls.filter(call => call.command === "docker_prerequisite").length, 1);
  ui.run("applyDockerState(true)");
  timer.callback();
  assert.equal(ui.calls.filter(call => call.command === "docker_prerequisite").length, 1);
});

test("Docker recovery shows a green banner until the first interaction", async () => {
  const ui = setup();
  ui.run('renderIntegration({workspace_available: true}); applyDockerState(false);');
  ui.elements.get("#docker-proceed").listeners.click();
  ui.run('applyDockerState(true); renderStatus({phase: "stopped", ua_version: "0.5.0"});');
  assert.equal(ui.elements.get("#docker-banner").hidden, false);
  assert.equal(ui.elements.get("#docker-banner-text").textContent, "Docker detected, good to go.");
  assert.match(ui.elements.get("#docker-banner").className, /banner-success/);
  assert.equal(ui.elements.get("#start-runtime").disabled, false);
  // The first user interaction acknowledges recovery and clears the banner.
  ui.run("acknowledgeDockerRecovery()");
  assert.equal(ui.elements.get("#docker-banner").hidden, true);
  // A later healthy status does not re-show the confirmation.
  ui.run('renderStatus({phase: "ready", ua_version: "0.5.0"});');
  assert.equal(ui.elements.get("#docker-banner").hidden, true);
});

test("status polls do not repeat the Docker probe while the dialog already has detail", async () => {
  const ui = setup();
  ui.run('latestDockerPrerequisite = {available: false, installed: false, setup_hint: "hint"};');
  const before = ui.calls.filter(call => call.command === "docker_prerequisite").length;
  for (let i = 0; i < 3; i += 1) ui.run('renderStatus({phase: "docker_unavailable", ua_version: "0.5.0"});');
  assert.equal(ui.calls.filter(call => call.command === "docker_prerequisite").length, before);
  // With no detail yet, the first unavailable status still fetches it.
  ui.run("latestDockerPrerequisite = null;");
  ui.run('renderStatus({phase: "docker_unavailable", ua_version: "0.5.0"});');
  assert.equal(ui.calls.filter(call => call.command === "docker_prerequisite").length, before + 1);
});

test("a fresh Docker failure re-opens the dialog but a continuing one stays dismissed", async () => {
  const ui = setup();
  ui.run('renderIntegration({workspace_available: true}); applyDockerState(false);');
  ui.elements.get("#docker-proceed").listeners.click();
  assert.equal(ui.elements.get("#docker-gate").hidden, true);
  // Repeated unavailable polls must not re-block the launcher.
  ui.run("applyDockerState(false); applyDockerState(false);");
  assert.equal(ui.elements.get("#docker-gate").hidden, true);
  // Docker returns, then fails again: that is a fresh failure and re-blocks.
  ui.run("applyDockerState(true); applyDockerState(false);");
  assert.equal(ui.elements.get("#docker-gate").hidden, false);
});

test("Docker help buttons open only the fixed install and guide destinations", async () => {
  const ui = setup({open_docker_help: () => null});
  await ui.elements.get("#docker-install").listeners.click();
  await ui.elements.get("#docker-guide").listeners.click();
  const targets = ui.calls.filter(call => call.command === "open_docker_help").map(call => call.args.target);
  assert.deepEqual(targets, ["install", "guide"]);
});

test("a reviewed update downloads in the background and the badge follows it", async () => {
  let finish;
  const downloaded = {...review, image_downloaded: true};
  const ui = setup({
    check_runtime_updates: () => review,
    download_runtime_update: () => new Promise(resolve => { finish = resolve; }),
  });
  await new Promise(resolve => setImmediate(resolve));
  ui.run('renderIntegration({workspace_available: true}); applyDockerState(true);');
  ui.run(`renderUpdate(${JSON.stringify(review)})`);
  assert.equal(ui.elements.get("#update-badge").hidden, false);
  assert.equal(ui.elements.get("#update-badge-label").textContent, "UA 0.5.0 is available");
  assert.match(ui.elements.get("#update-badge").className, /is-available/);
  const pending = ui.run("downloadUpdate()");
  await new Promise(resolve => setImmediate(resolve));
  // The download state shows in the badge, the steps and the Download button.
  assert.match(ui.elements.get("#update-badge").className, /is-downloading/);
  assert.equal(ui.elements.get("#update-badge-label").textContent, "Downloading UA 0.5.0…");
  assert.equal(ui.elements.get("#download-update").disabled, true);
  ui.progress({message: "Downloading", current_bytes: 42, total_bytes: 100});
  assert.equal(ui.elements.get("#update-badge-label").textContent, "Downloading UA 0.5.0 · 42%");
  assert.match(ui.elements.get("#update-badge").className, /has-percent/);
  assert.match(ui.text("#runtime-steps"), /Next: Downloading · 42%/);
  finish(downloaded);
  await pending;
  // Once downloaded, the badge points to the review and switch.
  assert.equal(ui.elements.get("#update-badge").hidden, false);
  assert.match(ui.elements.get("#update-badge").className, /is-downloaded/);
  assert.equal(ui.elements.get("#update-badge-label").textContent, "UA 0.5.0 is downloaded");
  assert.equal(ui.elements.get("#download-update").hidden, true);
  assert.match(ui.text("#runtime-steps"), /Done: Downloaded/);
  assert.match(ui.text("#runtime-steps"), /Next: Review the notes/);
});

test("status polling pauses while a background download holds the Docker lock", async () => {
  let finish;
  const ui = setup({
    check_runtime_updates: () => review,
    download_runtime_update: () => new Promise(resolve => { finish = resolve; }),
    runtime_status: () => ({phase: "ready", ua_version: "0.5.0"}),
    recent_logs: () => "logs",
  });
  await new Promise(resolve => setImmediate(resolve));
  ui.run('renderIntegration({workspace_available: true}); applyDockerState(true);');
  ui.run(`renderUpdate(${JSON.stringify(review)})`);
  const statusBefore = ui.calls.filter(call => call.command === "runtime_status").length;
  const logsBefore = ui.calls.filter(call => call.command === "recent_logs").length;
  const pending = ui.run("downloadUpdate()");
  await new Promise(resolve => setImmediate(resolve));
  // The three-second poll must not queue work behind the in-flight download.
  ui.timers.find(timer => timer.delay === 3000).callback();
  assert.equal(ui.calls.filter(call => call.command === "runtime_status").length, statusBefore);
  assert.equal(ui.calls.filter(call => call.command === "recent_logs").length, logsBefore);
  finish({...review, image_downloaded: true});
  await pending;
  // Polling resumes once the download completes.
  ui.timers.find(timer => timer.delay === 3000).callback();
  assert.equal(ui.calls.filter(call => call.command === "runtime_status").length, statusBefore + 1);
});

test("a background download never restarts the runtime", async () => {
  const ui = setup({
    check_runtime_updates: () => review,
    download_runtime_update: () => ({...review, image_downloaded: true}),
  });
  await new Promise(resolve => setImmediate(resolve));
  ui.run('renderIntegration({workspace_available: true}); applyDockerState(true);');
  ui.run(`renderUpdate(${JSON.stringify(review)})`);
  await ui.run("downloadUpdate()");
  assert.equal(ui.calls.filter(call => call.command === "download_runtime_update").length, 1);
  assert.equal(ui.calls.filter(call => call.command === "install_runtime_update").length, 0);
  assert.equal(ui.calls.filter(call => call.command === "restart_runtime").length, 0);
});

test("the downloaded badge opens the drawer and never applies anything", async () => {
  const ui = setup();
  ui.run(`renderUpdate(${JSON.stringify({...review, image_downloaded: true})}); renderIntegration({workspace_available: true}); applyDockerState(true);`);
  assert.equal(ui.elements.get("#update-badge").hidden, false);
  const open = ui.elements.get("#update-badge-open");
  open.listeners.click({currentTarget: open});
  assert.equal(ui.elements.get("#updates-drawer").hidden, false);
  assert.equal(ui.run("document.activeElement"), ui.elements.get("#updates-title"));
  // The badge steps aside while the drawer is open, and stays away once
  // closed because the person acted on it; focus goes to the header button.
  assert.equal(ui.elements.get("#update-badge").hidden, true);
  ui.key("Escape");
  assert.equal(ui.elements.get("#update-badge").hidden, true);
  assert.equal(ui.run("document.activeElement"), ui.elements.get("#open-updates"));
  assert.equal(ui.calls.filter(call => call.command === "install_runtime_update").length, 0);
});

test("the update download button is withheld while Docker is unavailable", async () => {
  const ui = setup();
  // The launch-time check never answers in this fixture.
  ui.run("checkingUpdates = false");
  ui.run(`renderUpdate(${JSON.stringify(review)}); applyDockerState(false);`);
  assert.equal(ui.elements.get("#update-badge").hidden, true);
  assert.equal(ui.elements.get("#download-update").disabled, true);
  ui.run("applyDockerState(true)");
  assert.equal(ui.elements.get("#update-badge").hidden, false);
  assert.equal(ui.elements.get("#download-update").disabled, false);
});

test("Read The Docs invokes only the fixed documentation action", async () => {
  const ui = setup({open_docs_browser: () => null});
  ui.run('renderStatus({phase:"ready", web_url:"http://127.0.0.1:18090/webchat", docs_url:"http://127.0.0.1:18090/docs/"})');
  assert.equal(ui.elements.get("#read-docs").href, "http://127.0.0.1:18090/docs/");
  let prevented = false;
  await ui.elements.get("#read-docs").listeners.click({preventDefault() { prevented = true; }});
  assert.equal(prevented, true);
  assert.equal(ui.calls.filter(call => call.command === "open_docs_browser").length, 1);
});

const review = {desktop_version:"0.4.3",current_version:"0.4.2",latest_version:"0.5.0",review_id:"reviewed-digest",breaking_changes:true,releases:[{version:"0.5.0",url:"https://github.com/enzimo/universal-agents/releases/tag/0.5.0",notes:"<script>bad()</script>",breaking_changes:true}]};
test("release review displays plain text, breaking warning and requires acknowledgment", async () => {
  const ui = setup({check_runtime_updates: () => review});
  await ui.run("checkUpdates()");
  ui.run('checkingUpdates = false; renderIntegration({workspace_available:true});');
  ui.run(`renderUpdate(${JSON.stringify(review)})`);
  assert.equal(ui.elements.get("#desktop-version").textContent, "0.4.3");
  assert.match(ui.elements.get("#update-warning").textContent, /Breaking changes/);
  assert.match(ui.elements.get("#update-notes").textContent, /<script>/);
  assert.equal(ui.elements.get("#update-notes").innerHTML, undefined);
  assert.equal(ui.elements.get("#install-update").disabled, true);
  ui.elements.get("#update-ack").checked = true;
  ui.run("updateActionAvailability()");
  assert.equal(ui.elements.get("#install-update").disabled, false);
  ui.run("setBusy(true)");
  assert.equal(ui.elements.get("#install-update").disabled, true);
  ui.run(`renderUpdate(${JSON.stringify({...review,review_id:null,blocked_reason:"Update desktop first"})})`);
  assert.equal(ui.elements.get("#runtime-actions").hidden, true);
  assert.match(ui.elements.get("#update-status").textContent, /can't install it yet/);
  assert.equal(ui.elements.get("#update-warning").hidden, false);
  assert.match(ui.elements.get("#update-warning").textContent, /Update desktop first/);
});
test("release notes start collapsed for each new review and keep their state for the same one", async () => {
  const ui = setup({check_runtime_updates: () => review});
  await ui.run("checkUpdates()");
  const notes = ui.elements.get("#update-notes-section");
  const ack = ui.elements.get("#update-ack");
  assert.equal(notes.open, false);
  // The reader opens the notes and acknowledges; a finished download returns
  // the same review, which keeps both.
  notes.open = true;
  ack.checked = true;
  ui.run(`renderUpdate(${JSON.stringify(review)})`);
  assert.equal(notes.open, true);
  assert.equal(ack.checked, true);
  // A different review (another chosen version) starts collapsed and unacknowledged.
  ui.run(`renderUpdate(${JSON.stringify({...review, review_id: "other-digest", latest_version: "0.5.1"})})`);
  assert.equal(notes.open, false);
  assert.equal(ack.checked, false);
  // So does a new check.
  notes.open = true;
  await ui.run("checkUpdates()");
  assert.equal(notes.open, false);
});
test("update submits only cached review identity and explicit consent", async () => {
  const ui = setup({check_runtime_updates: () => review,install_runtime_update: () => ({phase:"starting"})});
  await new Promise(resolve => setImmediate(resolve));
  ui.run('renderIntegration({workspace_available:true});');
  ui.elements.get("#update-ack").checked = true;
  ui.run("updateActionAvailability()");
  await ui.run("installUpdate()");
  const args = ui.calls.find(call => call.command === "install_runtime_update").args;
  assert.deepEqual(JSON.parse(JSON.stringify(args)), {reviewId:"reviewed-digest",acknowledged:true});
  assert.equal(ui.elements.get("#update-ack").checked, false);
});

test("lifecycle controls follow runtime state and show the selected runtime version", () => {
  const ui = setup();
  ui.run('renderStatus({phase:"ready",ua_version:"0.4.2"}); renderIntegration({workspace_available:true});');
  assert.equal(ui.elements.get("#header-runtime-version").textContent, "0.4.2");
  assert.equal(ui.elements.get("#start-runtime").disabled, true);
  assert.equal(ui.elements.get("#stop-runtime").disabled, false);
  assert.equal(ui.elements.get("#restart-runtime").disabled, false);
  ui.run('renderStatus({phase:"stopped",ua_version:"0.4.2"})');
  assert.equal(ui.elements.get("#start-runtime").disabled, false);
  assert.equal(ui.elements.get("#stop-runtime").disabled, true);
  assert.equal(ui.elements.get("#restart-runtime").disabled, true);
  ui.run('renderStatus({phase:"not_created",ua_version:"0.4.2"})');
  assert.equal(ui.elements.get("#start-runtime").textContent, "Install & start");
  ui.run('setBusy(true)');
  for (const action of ["start", "stop", "restart"]) {
    assert.equal(ui.elements.get(`#${action}-runtime`).disabled, true);
  }
});

test("a refused start keeps Stop and Restart reachable and explains itself", () => {
  const ui = setup();
  ui.run('renderIntegration({workspace_available:true});');
  ui.run('renderStatus({phase:"startup_failed",ua_version:"0.8.5",detail:"Universal Agents stopped while starting (exit code 2)."})');
  assert.equal(ui.elements.get("#status-label").textContent, "Stopped while starting");
  assert.match(ui.elements.get("#status-pill").className, /status-startup_failed/);
  for (const id of ["#stop-runtime", "#restart-runtime", "#stop-runtime-and-quit"]) {
    assert.equal(ui.elements.get(id).disabled, false, id);
  }
  // The container is still running under Docker's restart policy, so Start
  // would not change anything.
  assert.equal(ui.elements.get("#start-runtime").disabled, true);
  assert.equal(ui.elements.get("#open-app").disabled, true);
  assert.equal(ui.elements.get("#open-browser").disabled, true);
});

test("the Web UI opens in the app or a browser only while UA's web server answers", () => {
  const ui = setup();
  const openButtons = () => ["#open-app", "#open-browser"].map(id => ui.elements.get(id).disabled);
  for (const phase of ["not_created", "stopped", "starting", "startup_failed", "error", "docker_unavailable"]) {
    ui.run(`renderStatus({phase:${JSON.stringify(phase)},ua_version:"0.8.5",detail:"x"})`);
    assert.deepEqual(openButtons(), [true, true], phase);
  }
  for (const phase of ["onboarding", "ready"]) {
    ui.run(`renderStatus({phase:${JSON.stringify(phase)},ua_version:"0.8.5",detail:"x"})`);
    assert.deepEqual(openButtons(), [false, false], phase);
  }
});

test("logs refresh every three seconds during startup without overlapping requests or clearing errors", async () => {
  let finish;
  const ui = setup({recent_logs: () => new Promise(resolve => { finish = resolve; })});
  const timer = ui.timers.find(timer => timer.delay === 3000);
  assert.ok(timer);
  assert.equal(ui.calls.filter(call => call.command === "recent_logs").length, 1);
  ui.run('setBusy(true); showError("Keep this error");');
  timer.callback(); timer.callback();
  assert.equal(ui.calls.filter(call => call.command === "recent_logs").length, 1);
  finish("Booting UA");
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(ui.elements.get("#logs").textContent, "Booting UA");
  timer.callback();
  assert.equal(ui.calls.filter(call => call.command === "recent_logs").length, 2);
  finish("UA ready");
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(ui.elements.get("#logs").textContent, "UA ready");
  assert.match(ui.elements.get("#notice").textContent, /Download diagnostics/);
  ui.run('document.hidden = true'); timer.callback();
  assert.equal(ui.calls.filter(call => call.command === "recent_logs").length, 2);
});

test("errors show app-written guidance but never raw command output", () => {
  const ui = setup();
  const notice = () => ui.elements.get("#notice").textContent;
  ui.run('showError("\\u001fPort 18090 on this computer is already used by another program.")');
  assert.equal(notice(), "Port 18090 on this computer is already used by another program.");
  for (const raw of [
    "Docker command failed: docker pull: error getting credentials - err: exec: \"docker-credential-desktop\"",
    "The approved update is pending: \u001fSign in.",
  ]) {
    ui.run(`showError(${JSON.stringify(raw)})`);
    assert.match(notice(), /^The operation did not complete\. Download diagnostics/);
  }
});

test("a refused install shows the credential helper fix", async () => {
  const fix = "\u001fDocker cannot download images because its settings file names the credential helper docker-credential-desktop, which is not installed.";
  const ui = setup({start_runtime: () => Promise.reject(fix)});
  ui.run('renderIntegration({workspace_available:true});');
  ui.run('renderStatus({phase:"not_created",ua_version:"0.8.5",detail:"Not installed"})');
  await ui.elements.get("#start-runtime").listeners.click();
  assert.equal(ui.elements.get("#notice").hidden, false);
  assert.equal(ui.elements.get("#notice").textContent, fix.slice(1));
});

for (const command of ["registry_access", "start_runtime", "install_runtime_update"]) {
  test(`${command} shows live progress until completion and removes its listener`, async () => {
    let complete;
    const ui = setup({[command]: () => new Promise(resolve => { complete = resolve; })});
    const pending = ui.run(`invokeWithProgress("${command}", {}, "Preparing…")`);
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(ui.elements.get("#operation-bar").hidden, false);
    ui.progress("Fetching runtime image from GHCR");
    assert.match(ui.elements.get("#operation-detail").textContent, /Fetching/);
    complete({phase: "onboarding", detail: "Setup ready"});
    await pending;
    assert.equal(ui.elements.get("#operation-bar").hidden, true);
    assert.equal(ui.elements.get("#operation-detail").textContent, "Setup ready");
    ui.progress("Stale message");
    assert.equal(ui.elements.get("#operation-detail").textContent, "Setup ready");
  });
}
test("image progress displays byte counts and makes the bar determinate", async () => {
  let complete;
  const ui = setup({start_runtime: () => new Promise(resolve => { complete = resolve; })});
  const pending = ui.run('invokeWithProgress("start_runtime", {}, "Preparing…")');
  await new Promise(resolve => setImmediate(resolve));
  ui.progress({message:"Downloading runtime image: 12.5 MB of 50.0 MB",current_bytes:12_500_000,total_bytes:50_000_000});
  assert.equal(ui.elements.get("#operation-detail").textContent, "Downloading runtime image: 12.5 MB of 50.0 MB");
  assert.equal(ui.elements.get("#operation-bar").value, 12_500_000);
  assert.equal(ui.elements.get("#operation-bar").max, 50_000_000);
  complete({phase:"ready",detail:"Ready"});
  await pending;
});

test("an unchecked offer says it is unchecked and warns before installing", () => {
  const ui = setup();
  const beta = {...review, latest_version:"0.5.1-beta.1", breaking_changes:false, listed:false, prerelease:true,
    releases:[{version:"0.5.1-beta.1", url:"https://example.invalid/beta", notes:"beta", breaking_changes:false, prerelease:true}]};
  ui.run(`renderUpdate(${JSON.stringify(beta)})`);
  assert.match(ui.elements.get("#update-status").textContent, /0\.5\.1-beta\.1 is available as an unchecked pre-release/);
  assert.equal(ui.elements.get("#update-warning").hidden, false);
  assert.match(ui.elements.get("#update-warning").textContent, /not on this launcher's compatibility list and is a pre-release/);
  assert.match(ui.elements.get("#update-notes").textContent, /UA 0\.5\.1-beta\.1 \(pre-release\)/);
  assert.equal(ui.elements.get("#runtime-actions").hidden, false);
  assert.equal(ui.elements.get("#update-badge-label").textContent, "Unchecked UA 0.5.1-beta.1 is available");
  assert.match(ui.text("#runtime-steps"), /Unchecked: not on this launcher's compatibility list/);

  // A listed offer has no unchecked warning, and names a newer unlisted release.
  ui.run(`renderUpdate(${JSON.stringify({...review, breaking_changes:false, listed:true, prerelease:false, newer_unlisted:"0.5.1"})})`);
  assert.match(ui.elements.get("#update-status").textContent, /^UA 0\.5\.0 is available\./);
  assert.equal(ui.elements.get("#update-warning").hidden, true);
  assert.equal(ui.elements.get("#update-note").hidden, false);
  assert.match(ui.elements.get("#update-note").textContent, /UA 0\.5\.1 is newer but not on this launcher's compatibility list yet/);
});

test("offering unchecked versions is saved natively and refreshes the offer", async () => {
  const ui = setup({
    set_unlisted_runtime_offers: args => ({workspace_available:true, openai_subscription_state:"disconnected", offer_unlisted_runtimes:args.enabled}),
    check_runtime_updates: () => ({...review, listed:false, prerelease:true}),
  });
  await new Promise(resolve => setImmediate(resolve));
  const toggle = ui.elements.get("#offer-unlisted-runtimes");
  toggle.checked = true;
  const before = ui.calls.filter(call => call.command === "check_runtime_updates").length;
  await toggle.listeners.change();
  assert.deepEqual(JSON.parse(JSON.stringify(ui.calls.find(call => call.command === "set_unlisted_runtime_offers").args)), {enabled:true});
  assert.equal(toggle.checked, true);
  assert.equal(ui.calls.filter(call => call.command === "check_runtime_updates").length, before + 1);
  assert.match(ui.elements.get("#update-status").textContent, /unchecked pre-release/);
});

test("external skill refresh setting persists through the native command", async () => {
  const ui = setup({
    set_external_skill_refresh: args => ({workspace_available:true,openai_subscription_state:"not_configured",refresh_external_skills_on_start:args.enabled}),
    runtime_status: () => ({phase:"stopped",ua_version:"0.4.2"}),
  });
  const toggle = ui.elements.get("#refresh-external-skills");
  toggle.checked = true;
  await toggle.listeners.change();
  assert.deepEqual(JSON.parse(JSON.stringify(ui.calls.find(call => call.command === "set_external_skill_refresh").args)), {enabled:true});
  assert.equal(toggle.checked, true);
});
test("open folder reveals the effective workspace without changing it", async () => {
  const ui = setup({open_workspace_folder: () => null});
  ui.run('renderIntegration({workspace_available:true, workspace_is_external:false});');
  await ui.elements.get("#open-workspace-folder").listeners.click();
  assert.equal(ui.calls.filter(call => call.command === "open_workspace_folder").length, 1);
  assert.equal(ui.calls.filter(call => call.command === "choose_workspace").length, 0);
  // The reveal action must re-enable so it can be used repeatedly.
  assert.equal(ui.elements.get("#open-workspace-folder").disabled, false);
});

test("an unavailable workspace keeps Open folder available for the error", async () => {
  const ui = setup({open_workspace_folder: () => { throw new Error("workspace unavailable"); }});
  ui.run('renderIntegration({workspace_available:false, workspace_issue:"Folder unavailable"});');
  await ui.elements.get("#open-workspace-folder").listeners.click();
  assert.match(ui.elements.get("#notice").textContent, /Download diagnostics/);
  assert.equal(ui.elements.get("#open-workspace-folder").disabled, false);
});

test("logs expand into a larger view and collapse again", async () => {
  const ui = setup();
  const toggle = ui.elements.get("#expand-logs");
  assert.equal(ui.run('document.body.classList.contains("logs-expanded")'), false);
  toggle.listeners.click();
  assert.equal(ui.run('document.body.classList.contains("logs-expanded")'), true);
  assert.equal(toggle.getAttribute("aria-label"), "Collapse logs");
  assert.equal(toggle.getAttribute("aria-expanded"), "true");
  toggle.listeners.click();
  assert.equal(ui.run('document.body.classList.contains("logs-expanded")'), false);
  assert.equal(toggle.getAttribute("aria-label"), "Expand logs");
  assert.equal(toggle.getAttribute("aria-expanded"), "false");
});

test("logs stay expandable during a long-running operation", () => {
  const ui = setup();
  ui.run('renderStatus({phase:"stopped"}); renderIntegration({workspace_available:true});');
  ui.run("setBusy(true)");
  // Enlarging the log view is a pure layout action, so a pull must not disable
  // it. (Refresh has its own single-flight guard and is covered elsewhere.)
  assert.notEqual(ui.elements.get("#expand-logs").disabled, true);
});

test("quitting offers both outcomes and only stops UA when asked", async () => {
  const ui = setup({
    quit_desktop_app: () => null,
    stop_runtime_and_quit: () => ({phase:"stopped", detail:"Universal Agents is stopped."}),
  });
  ui.run('renderStatus({phase:"ready"}); renderIntegration({workspace_available:true});');
  assert.equal(ui.elements.get("#stop-runtime-and-quit").disabled, false);
  assert.equal(ui.elements.get("#quit-desktop-app").disabled, false);

  await ui.elements.get("#quit-desktop-app").listeners.click();
  assert.equal(ui.calls.filter(call => call.command === "quit_desktop_app").length, 1);
  assert.equal(ui.calls.filter(call => call.command === "stop_runtime_and_quit").length, 0);

  await ui.elements.get("#stop-runtime-and-quit").listeners.click();
  assert.equal(ui.calls.filter(call => call.command === "stop_runtime_and_quit").length, 1);
});

test("Stop UA and quit is unavailable when nothing is running", () => {
  const ui = setup();
  for (const phase of ["stopped", "not_created", "docker_unavailable"]) {
    ui.run(`renderStatus({phase: "${phase}"}); renderIntegration({workspace_available:true});`);
    assert.equal(ui.elements.get("#stop-runtime-and-quit").disabled, true, phase);
    // Quitting the host never depends on runtime state.
    assert.equal(ui.elements.get("#quit-desktop-app").disabled, false, phase);
  }
});

test("failed Stop UA and quit reports the error and stays open", async () => {
  const ui = setup({stop_runtime_and_quit: () => { throw new Error("Docker unavailable"); }});
  ui.run('renderStatus({phase:"ready"}); renderIntegration({workspace_available:true});');
  await ui.elements.get("#stop-runtime-and-quit").listeners.click();
  assert.match(ui.elements.get("#notice").textContent, /Download diagnostics/);
  assert.equal(ui.run("operationInFlight"), false);
});

test("failed image access stops progress and permits a retry", async () => {
  const ui = setup();
  await ui.run('authenticateRegistry("gh")');
  assert.equal(ui.elements.get("#operation-bar").hidden, true);
  assert.match(ui.elements.get("#operation-detail").textContent, /failed/);
  assert.equal(ui.elements.get("#registry-gh").disabled, false);
});

test("terminal login automatically renders runtime result without another auth click", async () => {
  const ui = setup({begin_github_login: () => ({phase:"onboarding", detail:"Setup ready"}), integration_status: () => ({workspace_available:true})});
  await ui.elements.get("#registry-login").listeners.click();
  assert.equal(ui.calls.filter(call => call.command === "begin_github_login").length, 1);
  assert.equal(ui.calls.filter(call => call.command === "registry_access").length, 0);
  assert.equal(ui.elements.get("#status-label").textContent, "Setup ready");
  assert.equal(ui.run("operationInFlight"), false);
});
test("terminal failure stops progress and re-enables sign-in", async () => {
  const ui = setup({begin_github_login: () => {throw new Error("GitHub sign-in timed out");}});
  await ui.elements.get("#registry-login").listeners.click();
  assert.match(ui.elements.get("#notice").textContent, /Download diagnostics/);
  assert.equal(ui.elements.get("#operation-bar").hidden, true);
  assert.equal(ui.elements.get("#registry-login").disabled, false);
});

// ---------- Updates drawer ----------

const desktopCurrent = {current_version: "0.5.0", latest_version: "0.5.0", newer: false, published_at: null, error: null};
const desktopNewer = {current_version: "0.5.0", latest_version: "0.5.5", newer: true, published_at: "2026-10-13T08:00:00Z", error: null};
const catalog = [
  {version: "0.9.3", published_at: "2026-10-13T08:00:00Z", listed: true, prerelease: false, relation: "newer", recommended: true, notes_url: "https://example.invalid/0.9.3", downloaded: false},
  {version: "0.9.2", published_at: "2026-10-12T08:00:00Z", listed: true, prerelease: false, relation: "newer", recommended: false, notes_url: "https://example.invalid/0.9.2", downloaded: true},
  {version: "0.9.1", published_at: null, listed: true, prerelease: false, relation: "running", recommended: false, notes_url: null, downloaded: true},
  {version: "0.9.0", published_at: "2026-10-10T08:00:00Z", listed: true, prerelease: false, relation: "older", recommended: false, notes_url: "https://example.invalid/0.9.0", downloaded: false},
];
const offer = {desktop_version: "0.5.0", current_version: "0.9.1", latest_version: "0.9.3", update_pending: false,
  review_id: "offer-digest", image_downloaded: false, listed: true, prerelease: false, breaking_changes: false,
  blocked_reason: null, newer_unlisted: null, catalog, desktop: desktopCurrent,
  releases: [
    {version: "0.9.3", url: "https://example.invalid/0.9.3", notes: "three", breaking_changes: false, prerelease: false, published_at: "2026-10-13T08:00:00Z"},
    {version: "0.9.2", url: "https://example.invalid/0.9.2", notes: "two", breaking_changes: false, prerelease: false, published_at: null},
  ]};

function drawerSetup(responses = {}) {
  const ui = setup(responses);
  // Each test renders its own review instead of the launch-time check.
  ui.run('checkingUpdates = false; renderIntegration({workspace_available: true}); applyDockerState(true);');
  return ui;
}

test("the UA lane explains every state and shows steps only for an installable version", () => {
  const ui = drawerSetup();
  const status = () => ui.elements.get("#update-status").textContent;
  ui.run("checkingUpdates = true; renderUpdateCenter()");
  assert.equal(status(), "Checking GitHub for UA releases…");
  assert.equal(ui.elements.get("#update-checked").textContent, "Checking now…");
  ui.run("checkingUpdates = false; lastCheckFailed = true; renderUpdateCenter()");
  assert.match(status(), /Could not check for updates.*Check now/);
  assert.equal(ui.elements.get("#launcher-status").textContent, "Could not check for a newer launcher.");

  ui.run(`renderUpdate(${JSON.stringify({...offer, review_id: null, releases: [], latest_version: "0.9.1", catalog: catalog.slice(2)})})`);
  assert.equal(status(), "UA 0.9.1 is up to date.");
  assert.equal(ui.elements.get("#runtime-steps").hidden, true);
  assert.equal(ui.elements.get("#runtime-actions").hidden, true);
  assert.equal(ui.elements.get("#update-review").hidden, true);

  ui.run(`renderUpdate(${JSON.stringify({...offer, update_pending: true, review_id: null, catalog: []})})`);
  assert.equal(status(), "UA 0.9.1 update is pending. Use Start to finish or retry.");
  assert.match(ui.elements.get("#catalog-note").textContent, /once the pending update finishes/);
  assert.equal(ui.elements.get("#update-review").hidden, true);

  ui.run(`renderUpdate(${JSON.stringify({...offer, review_id: null, blocked_reason: "Install the latest Universal Agents desktop app first."})})`);
  assert.match(status(), /can't install it yet/);
  assert.match(ui.elements.get("#update-warning").textContent, /desktop app first/);
  assert.equal(ui.elements.get("#runtime-steps").hidden, true);
  // Notes stay readable, but there is nothing to acknowledge.
  assert.equal(ui.elements.get("#update-review").hidden, false);
  assert.equal(ui.elements.get("#update-ack").parentElement, undefined);

  ui.run(`renderUpdate(${JSON.stringify(offer)})`);
  assert.equal(status(), "UA 0.9.3 is available. Running: 0.9.1.");
  assert.equal(ui.elements.get("#runtime-lane-versions").textContent, "0.9.1 → 0.9.3");
  const steps = ui.elements.get("#runtime-steps").children;
  assert.deepEqual(steps.map(step => step.className), ["step is-done", "step is-now", "step is-todo", "step is-todo"]);
  assert.match(textOf(steps[0]), /^Done: Found UA 0\.9\.3Released .*\. Works with this launcher\.$/);
  assert.match(textOf(steps[2]), /2 releases since UA 0\.9\.1\./);
  assert.equal(ui.elements.get("#runtime-actions").hidden, false);
  assert.equal(ui.elements.get("#install-update").disabled, true);
  // Acknowledging marks the review step done and allows the switch.
  ui.elements.get("#update-ack").checked = true;
  ui.elements.get("#update-ack").listeners.change();
  assert.equal(steps.length, 4);
  assert.deepEqual(ui.elements.get("#runtime-steps").children.map(step => step.className),
    ["step is-done", "step is-now", "step is-done", "step is-todo"]);
  assert.equal(ui.elements.get("#install-update").disabled, false);
});

test("the header count and the badge show the most useful update first", () => {
  const ui = drawerSetup();
  const count = () => ui.elements.get("#updates-count");
  const badge = () => [ui.elements.get("#update-badge").hidden, ui.elements.get("#update-badge-label").textContent];
  ui.run(`renderUpdate(${JSON.stringify({...offer, review_id: null, releases: [], desktop: desktopCurrent})})`);
  assert.equal(count().hidden, true);
  assert.equal(ui.elements.get("#open-updates").getAttribute("aria-label"), "Updates");
  assert.equal(ui.elements.get("#update-badge").hidden, true);

  ui.run(`renderUpdate(${JSON.stringify({...offer, review_id: null, releases: [], desktop: desktopNewer})})`);
  assert.equal(count().textContent, "1");
  assert.deepEqual(badge(), [false, "Launcher 0.5.5 is available"]);

  ui.run(`renderUpdate(${JSON.stringify({...offer, desktop: desktopNewer})})`);
  assert.equal(count().textContent, "2");
  assert.equal(ui.elements.get("#open-updates").getAttribute("aria-label"), "Updates, 2 waiting for you");
  assert.deepEqual(badge(), [false, "UA 0.9.3 is available"]);

  ui.run(`renderUpdate(${JSON.stringify({...offer, image_downloaded: true, desktop: desktopNewer})})`);
  assert.deepEqual(badge(), [false, "UA 0.9.3 is downloaded"]);

  ui.run("downloadingUpdate = true; renderUpdateCenter()");
  assert.deepEqual(badge(), [false, "Downloading UA 0.9.3…"]);

  // A pending update counts until Start finishes it.
  ui.run(`downloadingUpdate = false; renderUpdate(${JSON.stringify({...offer, update_pending: true, review_id: null, catalog: [], desktop: desktopCurrent})})`);
  assert.equal(count().textContent, "1");
});

test("the launcher lane links only to the newest launcher's release page", async () => {
  const ui = drawerSetup({open_release_page: () => null});
  ui.run(`renderUpdate(${JSON.stringify({...offer, desktop: desktopNewer})})`);
  assert.equal(ui.elements.get("#launcher-status").textContent, "Launcher 0.5.5 is available. You have 0.5.0.");
  assert.equal(ui.elements.get("#launcher-lane-versions").textContent, "0.5.0 → 0.5.5");
  assert.deepEqual(ui.elements.get("#launcher-steps").children.map(step => step.className),
    ["step is-done", "step is-now", "step is-todo"]);
  assert.match(ui.text("#launcher-steps"), /Reopen the launcherUniversal Agents keeps running while you install\./);
  assert.equal(ui.elements.get("#launcher-actions").hidden, false);
  await ui.elements.get("#open-launcher-download").listeners.click();
  const request = ui.calls.find(call => call.command === "open_release_page");
  assert.deepEqual(JSON.parse(JSON.stringify(request.args)), {product: "launcher", version: "0.5.5"});

  ui.run(`renderUpdate(${JSON.stringify({...offer, desktop: desktopCurrent})})`);
  assert.equal(ui.elements.get("#launcher-status").textContent, "Launcher 0.5.0 is up to date.");
  assert.equal(ui.elements.get("#launcher-actions").hidden, true);
  ui.run(`renderUpdate(${JSON.stringify({...offer, desktop: {...desktopCurrent, error: "Could not check GitHub for a newer launcher."}})})`);
  assert.equal(ui.elements.get("#launcher-status").textContent, "Could not check GitHub for a newer launcher.");
  // The UA lane is unaffected by a failed launcher check.
  assert.equal(ui.elements.get("#update-status").textContent, "UA 0.9.3 is available. Running: 0.9.1.");
});

test("other versions: newer ones can be chosen, older ones are for reference", async () => {
  const chosen = {...offer, latest_version: "0.9.2", review_id: "chosen-digest", image_downloaded: true, releases: offer.releases.slice(1)};
  const ui = drawerSetup({review_runtime_version: () => chosen, open_release_page: () => null});
  ui.run(`renderUpdate(${JSON.stringify(offer)})`);
  const rows = () => ui.elements.get("#runtime-catalog").children;
  assert.deepEqual(rows().map(row => row.className),
    ["catalog-row is-newer is-selected", "catalog-row is-newer", "catalog-row is-running", "catalog-row is-older"]);
  assert.match(textOf(rows()[0]), /^0\.9\.3.*Works with this launcherSelected$/);
  assert.match(textOf(rows()[1]), /Downloaded/);
  assert.match(textOf(rows()[2]), /Running$/);
  assert.match(textOf(rows()[3]), /Older than the running versionNotes$/);
  const choose = rows()[1].children[2].children[0];
  assert.equal(choose.textContent, "Choose");
  assert.equal(choose.getAttribute("aria-label"), "Make UA 0.9.2 the update");
  // Older rows offer no way to install them.
  assert.ok(!textOf(rows()[3]).includes("Choose"));

  await choose.listeners.click();
  assert.deepEqual(JSON.parse(JSON.stringify(ui.calls.find(call => call.command === "review_runtime_version").args)), {version: "0.9.2"});
  assert.equal(ui.run("latestUpdate.review_id"), "chosen-digest");
  assert.equal(ui.elements.get("#update-status").textContent,
    "UA 0.9.2 is downloaded. Review the notes, then switch when your work is finished.");
  assert.match(textOf(ui.elements.get("#runtime-steps").children[0]), /Chose UA 0\.9\.2/);
  assert.match(textOf(rows()[1]), /Selected$/);
  // Focus continues at the lane's next action: the download is done, so the acknowledgment.
  assert.equal(ui.run("document.activeElement"), ui.elements.get("#update-ack"));

  await rows()[3].children[2].children[1].listeners.click();
  assert.deepEqual(JSON.parse(JSON.stringify(ui.calls.find(call => call.command === "open_release_page").args)), {product: "runtime", version: "0.9.0"});
});

test("versions cannot be chosen while a download or another operation runs", () => {
  const ui = drawerSetup();
  ui.run(`renderUpdate(${JSON.stringify(offer)})`);
  const choose = () => ui.elements.get("#runtime-catalog").children[1].children[2].children[0];
  assert.equal(choose().disabled, false);
  ui.run("downloadingUpdate = true; renderUpdateCenter()");
  assert.equal(choose().disabled, true);
  assert.equal(ui.elements.get("#check-updates").disabled, true);
  assert.equal(ui.elements.get("#offer-unlisted-runtimes").disabled, true);
  ui.run("downloadingUpdate = false; setBusy(true)");
  assert.equal(choose().disabled, true);
  // Release notes of older versions stay available.
  const notes = ui.elements.get("#runtime-catalog").children[3].children[2].children[1];
  assert.equal(notes.disabled, false);
});

test("the drawer is a modal: it locks the launcher, closes on Escape or the backdrop, and returns focus", () => {
  const ui = drawerSetup();
  ui.run(`renderUpdate(${JSON.stringify(offer)})`);
  const drawer = ui.elements.get("#updates-drawer");
  const opener = ui.elements.get("#open-updates");
  opener.focus();
  opener.listeners.click();
  assert.equal(drawer.hidden, false);
  assert.equal(ui.elements.get(".shell").inert, true);
  assert.equal(ui.run("document.activeElement"), ui.elements.get("#updates-title"));
  ui.key("Escape");
  assert.equal(drawer.hidden, true);
  assert.equal(ui.elements.get(".shell").inert, false);
  assert.equal(ui.run("document.activeElement"), opener);

  // A click returns focus to the clicked control even when it was not focused
  // (WebKit does not focus clicked buttons).
  ui.elements.get("#expand-logs").focus();
  opener.listeners.click({currentTarget: opener});
  ui.elements.get("#close-updates").listeners.click();
  assert.equal(ui.run("document.activeElement"), opener);

  opener.listeners.click();
  // A click inside the drawer keeps it open; one on the backdrop closes it.
  drawer.listeners.click({target: ui.elements.get("#update-status")});
  assert.equal(drawer.hidden, false);
  drawer.listeners.click({target: drawer});
  assert.equal(drawer.hidden, true);

  // Escape with expanded logs and no drawer still collapses the logs.
  ui.elements.get("#expand-logs").listeners.click();
  ui.elements.get("#expand-logs").click = () => ui.elements.get("#expand-logs").listeners.click();
  ui.key("Escape");
  assert.equal(ui.run('document.body.classList.contains("logs-expanded")'), false);
});

test("the Docker dialog closes the drawer and errors show inside an open drawer", async () => {
  const ui = drawerSetup({open_release_page: () => { throw "\u001fNo release page is known for UA 0.9.0. Check for updates again."; }});
  ui.run(`renderUpdate(${JSON.stringify(offer)})`);
  ui.elements.get("#open-updates").listeners.click();
  await ui.run('openReleasePage("runtime", "0.9.0")');
  assert.equal(ui.elements.get("#drawer-notice").hidden, false);
  assert.equal(ui.elements.get("#drawer-notice").textContent, "No release page is known for UA 0.9.0. Check for updates again.");
  assert.notEqual(ui.elements.get("#notice").hidden, false);
  ui.run("applyDockerState(false)");
  assert.equal(ui.elements.get("#updates-drawer").hidden, true);
  assert.equal(ui.elements.get("#docker-gate").hidden, false);
  assert.equal(ui.elements.get(".shell").inert, true);
});

test("switching closes the drawer so the restart progress under the header is visible", async () => {
  const ui = drawerSetup({install_runtime_update: () => ({phase: "starting", detail: "Starting"}), check_runtime_updates: () => offer});
  ui.run(`renderUpdate(${JSON.stringify(offer)})`);
  ui.elements.get("#open-updates").listeners.click();
  ui.elements.get("#update-ack").checked = true;
  ui.run("updateActionAvailability()");
  await ui.run("installUpdate()");
  assert.equal(ui.elements.get("#updates-drawer").hidden, true);
  assert.deepEqual(JSON.parse(JSON.stringify(ui.calls.find(call => call.command === "install_runtime_update").args)),
    {reviewId: "offer-digest", acknowledged: true});
});


test("the update badge hides after five seconds or when dismissed and returns at the next stage", () => {
  const ui = drawerSetup();
  const badge = ui.elements.get("#update-badge");
  ui.run(`renderUpdate(${JSON.stringify(offer)})`);
  assert.equal(badge.hidden, false);
  ui.elapse(5000);
  assert.equal(badge.hidden, true);
  // A recheck of the same stage does not bring it back.
  ui.run(`latestUpdate = null; renderUpdateCenter(); renderUpdate(${JSON.stringify(offer)})`);
  assert.equal(badge.hidden, true);
  // The header button still says an update is waiting.
  assert.equal(ui.elements.get("#updates-count").textContent, "1");

  // The next stage shows it again; the dismiss button hides it at once.
  ui.run(`renderUpdate(${JSON.stringify({...offer, image_downloaded: true})})`);
  assert.equal(badge.hidden, false);
  assert.equal(ui.elements.get("#update-badge-label").textContent, "UA 0.9.3 is downloaded");
  ui.elements.get("#update-badge-dismiss").listeners.click();
  assert.equal(badge.hidden, true);
  assert.equal(ui.run("document.activeElement"), ui.elements.get("#open-updates"));
  assert.equal(ui.timeouts.length, 0);

  // Pointing at it or focusing it holds it; leaving starts the five seconds again.
  ui.run(`renderUpdate(${JSON.stringify({...offer, desktop: desktopNewer, review_id: null, releases: []})})`);
  assert.equal(badge.hidden, false);
  badge.listeners.pointerenter();
  ui.elapse(5000);
  assert.equal(badge.hidden, false);
  badge.listeners.pointerleave();
  ui.elapse(5000);
  assert.equal(badge.hidden, true);
});

test("log refresh can be paused and resumed, and fetches the chosen number of lines", async () => {
  const ui = setup({recent_logs: () => "line"});
  const tails = () => ui.calls.filter(call => call.command === "recent_logs").map(call => call.args.tail);
  const timer = ui.timers.find(timer => timer.delay === 3000);
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(tails(), [250]);
  const toggle = ui.elements.get("#toggle-log-refresh");
  assert.equal(ui.elements.get("#logs-state").textContent, "Live");
  assert.equal(toggle.getAttribute("aria-label"), "Pause log refresh");
  toggle.listeners.click();
  assert.equal(ui.elements.get("#logs-state").textContent, "Paused");
  assert.equal(toggle.getAttribute("aria-label"), "Resume log refresh");
  assert.match(toggle.className, /is-paused/);
  timer.callback();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(tails().length, 1, "paused logs are not refreshed");

  // A new line count is fetched at once, even while paused, within 10 to 5000.
  const lines = ui.elements.get("#log-lines");
  for (const [typed, expected] of [["1000", 1000], ["99999", 5000], ["3", 10], ["abc", 250]]) {
    lines.value = typed;
    lines.listeners.change();
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(lines.value, String(expected), typed);
    assert.equal(tails().at(-1), expected, typed);
  }
  const fetched = tails().length;
  toggle.listeners.click();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(ui.elements.get("#logs-state").textContent, "Live");
  assert.equal(tails().length, fetched + 1, "resuming fetches at once");
  timer.callback();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(tails().length, fetched + 2);
});

test("Stop UA and quit spins while UA stops and recovers if stopping fails", async () => {
  let fail;
  const ui = setup({stop_runtime_and_quit: () => new Promise((resolve, reject) => { fail = reject; })});
  ui.run('renderStatus({phase:"ready"}); renderIntegration({workspace_available:true});');
  const button = ui.elements.get("#stop-runtime-and-quit");
  const pending = button.listeners.click();
  await new Promise(resolve => setImmediate(resolve));
  assert.match(button.className, /is-working/);
  assert.equal(button.getAttribute("aria-busy"), "true");
  assert.equal(ui.elements.get("#stop-and-quit-label").textContent, "Stopping…");
  assert.equal(button.disabled, true);
  fail(new Error("Docker unavailable"));
  await pending;
  assert.doesNotMatch(button.className, /is-working/);
  assert.equal(ui.elements.get("#stop-and-quit-label").textContent, "Quit");
  assert.equal(button.getAttribute("aria-busy"), "false");
});

test("the optional OpenAI section opens by itself only when its saved file goes missing", () => {
  const ui = setup();
  const section = ui.elements.get("#subscription-access");
  ui.run('renderIntegration({workspace_available:true, openai_subscription_state:"disconnected"})');
  assert.notEqual(section.open, true);
  ui.run('renderIntegration({workspace_available:true, openai_subscription_state:"source_unavailable"})');
  assert.equal(section.open, true);
  // Once the person closes it, later polls in the same state leave it closed.
  section.open = false;
  ui.run('renderIntegration({workspace_available:true, openai_subscription_state:"source_unavailable"})');
  assert.equal(section.open, false);
  // The state tag in its title line shows while it is collapsed.
  assert.equal(ui.elements.get("#subscription-badge").textContent, "Source unavailable");
});


test("collapsed logs are not refreshed, and opening them fetches at once", async () => {
  const ui = setup({recent_logs: () => "line"});
  await new Promise(resolve => setImmediate(resolve));
  const fetched = () => ui.calls.filter(call => call.command === "recent_logs").length;
  const before = fetched();
  const section = ui.elements.get("#logs-section");
  section.open = false;
  ui.timers.find(timer => timer.delay === 3000).callback();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(fetched(), before);
  section.open = true;
  section.listeners.toggle();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(fetched(), before + 1);
});

test("every button in the launcher has a tooltip", () => {
  const html = fs.readFileSync(`${__dirname}/index.html`, "utf8");
  const buttons = [...html.matchAll(/<button\b[^>]*>/gs)].map(match => match[0]);
  assert.ok(buttons.length > 30);
  for (const button of buttons) assert.match(button, /\btitle="[^"]{8,}"/, button);
});

test("tooltips explain why Start, Restart and Finish import are unavailable, then come back", () => {
  const ui = setup();
  const title = id => ui.elements.get(id).title;
  // The fake page has no index.html titles, so give the defaults read at load.
  ui.run(`defaultTitles.start = "Start UA"; defaultTitles.install = "Download and start UA"; defaultTitles.restart = "Restart UA"; defaultTitles.finish = "Import now";`);
  ui.run('renderIntegration({workspace_available:false, workspace_issue:"gone"}); renderStatus({phase:"stopped"})');
  assert.match(title("#start-runtime"), /Repair the saved workspace/);
  assert.match(title("#restart-runtime"), /Repair the saved workspace/);
  ui.run('renderIntegration({workspace_available:true})');
  assert.equal(title("#start-runtime"), "Start UA");
  assert.equal(title("#restart-runtime"), "Restart UA");
  ui.run('renderStatus({phase:"not_created"})');
  assert.equal(title("#start-runtime"), "Download and start UA");
  ui.run('renderIntegration({workspace_available:true, openai_subscription_state:"pending_import"}); renderStatus({phase:"stopped"})');
  assert.match(title("#finish-subscription"), /Start UA first/);
  ui.run('renderStatus({phase:"ready"})');
  assert.equal(title("#finish-subscription"), "Import now");
});

test("the mark opens only the fixed Universal Agents project page", async () => {
  const ui = setup({open_ua_repository: () => null});
  await ui.elements.get("#open-ua-repository").listeners.click();
  const calls = ui.calls.filter(call => call.command === "open_ua_repository");
  assert.equal(calls.length, 1);
  assert.equal(calls[0].args, undefined, "the window passes no address");
});

test("the runtime state is a coloured word with a class per phase", () => {
  const ui = setup();
  ui.run('renderStatus({phase:"ready"})');
  assert.equal(ui.elements.get("#status-pill").className, "status-text status-ready");
  assert.equal(ui.elements.get("#status-label").textContent, "Running");
});

// ---------- Appearance (theme.js) ----------

const THEME_SOURCE = fs.readFileSync(`${__dirname}/theme.js`, "utf8");
// The real vendored variables, so these tests notice a re-vendor that changes them.
const VENDORED_THEME_CSS = Object.fromEntries(
  [...fs.readFileSync(`${__dirname}/vendor/ua-runtime-theme.css`, "utf8").matchAll(/(--[\w-]+):\s*([^;]+);/g)]
    .map(match => [match[1], match[2].trim()]),
);
const THEME_KEY = "ua-desktop.status-window.color-mode";

function themeSetup({ stored, readError = false, writeError = false } = {}) {
  const storage = new Map(stored === undefined ? [] : [[THEME_KEY, stored]]);
  const warnings = [];
  const events = [];
  const rootAttributes = {};
  const rootStyle = new Map();
  const documentListeners = {};
  const toggle = {
    attributes: { "data-theme-toggle": "" },
    listeners: {},
    getAttribute(name) { return this.attributes[name] ?? null; },
    setAttribute(name, value) { this.attributes[name] = String(value); },
    addEventListener(type, callback) { this.listeners[type] = callback; },
  };
  const buttons = [toggle];
  const document = {
    readyState: "loading",
    documentElement: {
      setAttribute(name, value) { rootAttributes[name] = String(value); },
      style: { setProperty(name, value) { rootStyle.set(name, value); } },
    },
    addEventListener(type, callback) { documentListeners[type] = callback; },
    dispatchEvent(event) { events.push(event); },
    querySelectorAll() { return buttons; },
  };
  const window = {
    localStorage: {
      getItem(key) { if (readError) throw new Error("storage disabled"); return storage.has(key) ? storage.get(key) : null; },
      setItem(key, value) { if (writeError) throw new Error("storage full"); storage.set(key, value); },
    },
    getComputedStyle: () => ({ getPropertyValue: name => VENDORED_THEME_CSS[name] ?? "" }),
  };
  const context = vm.createContext({
    window, document,
    console: { warn: (...args) => warnings.push(args) },
    CustomEvent: class { constructor(type, init) { this.type = type; this.detail = init?.detail; } },
  });
  vm.runInContext(THEME_SOURCE, context);
  return {
    window, storage, warnings, events, rootAttributes, rootStyle, buttons,
    appearance: () => window.UAAppearance,
    ready: () => documentListeners.DOMContentLoaded(),
    label: () => toggle.attributes["aria-label"],
    title: () => toggle.attributes.title,
    click: () => toggle.listeners.click(),
  };
}

test("the Status window opens dark by default and reads the vendored background settings", () => {
  const ui = themeSetup();
  assert.equal(ui.rootAttributes["data-color-mode"], "dark");
  assert.deepEqual(ui.warnings, []);
  const current = ui.appearance().current();
  assert.equal(current.mode, "dark");
  // The dark renderer and the intro glass get the vendored values, not guesses.
  assert.equal(current.dark.seed, Number(VENDORED_THEME_CSS["--ua-dark-seed"]));
  assert.equal(current.dark.lightX, Number(VENDORED_THEME_CSS["--ua-dark-light-x"]));
  assert.equal(current.dark.metalColor, VENDORED_THEME_CSS["--ua-dark-metal"]);
  assert.deepEqual(JSON.parse(JSON.stringify(current.dark.accents.gold)), {
    color: VENDORED_THEME_CSS["--ua-dark-gold"], share: Number(VENDORED_THEME_CSS["--ua-dark-gold-share"]),
  });
  // Metal at the share the accents leave, then gold and bronze (UA appearance.js).
  const palette = ui.appearance().motionPalette();
  assert.deepEqual([...palette.map(entry => entry.weight)], [51, 10, 39]);
  for (const entry of palette) assert.match(entry.color, /^#[0-9a-f]{6}$/);
  assert.match(ui.rootStyle.get("--ua-motion-accent"), /^#[0-9a-f]{6}$/);
  ui.ready();
  // Dark shows the sun, which switches to light; the label says so.
  assert.equal(ui.label(), "Switch to light theme");
  assert.equal(ui.title(), "Switch to light theme");
});

test("a saved light choice is kept; unknown or unreadable choices fall back to dark with a warning", () => {
  const light = themeSetup({ stored: "light" });
  assert.equal(light.rootAttributes["data-color-mode"], "light");
  assert.equal(light.appearance().current().mode, "light");
  assert.deepEqual([...light.appearance().motionPalette().map(entry => entry.weight)], [1, 1, 1, 1]);
  assert.deepEqual(light.warnings, []);
  for (const options of [{ stored: "sepia" }, { readError: true }]) {
    const ui = themeSetup(options);
    assert.equal(ui.rootAttributes["data-color-mode"], "dark", JSON.stringify(options));
    assert.equal(ui.warnings.length, 1, JSON.stringify(options));
    assert.match(ui.warnings[0][0], /^ua\.desktop\.theme:/);
  }
});

test("the sun/moon switch flips the theme, remembers it and announces it to the background", () => {
  const ui = themeSetup();
  ui.ready();
  ui.click();
  assert.equal(ui.storage.get(THEME_KEY), "light");
  assert.equal(ui.rootAttributes["data-color-mode"], "light");
  // Light shows the moon, which switches back to dark.
  assert.equal(ui.label(), "Switch to dark theme");
  assert.equal(ui.events.length, 1);
  assert.equal(ui.events[0].type, "ua:appearance-change");
  assert.equal(ui.events[0].detail.appearance.mode, "light");
  ui.click();
  assert.equal(ui.storage.get(THEME_KEY), "dark");
  assert.equal(ui.events.length, 2);
  assert.equal(ui.events.at(-1).detail.appearance.mode, "dark");
  assert.equal(ui.label(), "Switch to light theme");
  // Choosing the current mode again through the API changes nothing.
  ui.appearance().setMode("dark");
  assert.equal(ui.events.length, 2);
});

test("a choice that cannot be saved still applies and says why", () => {
  const ui = themeSetup({ writeError: true });
  ui.ready();
  ui.click();
  assert.equal(ui.rootAttributes["data-color-mode"], "light");
  assert.equal(ui.warnings.length, 1);
  assert.match(ui.warnings[0][0], /could not be saved/);
});

// ---------- Intro (intro.js) ----------

const INTRO_SOURCE = fs.readFileSync(`${__dirname}/intro.js`, "utf8");

function introSetup({ reduced = false, hidden = false, wordFails = false, colorMode = "dark", metal = true } = {}) {
  const clock = { now: 0, timers: [], nextId: 1 };
  const errors = [];
  const documentListeners = new Map();
  const word = { created: 0, plays: 0, stills: 0, stopped: false, done: null };
  const rootClasses = new Set();
  const rootAttributes = new Map([["data-color-mode", colorMode]]);
  const reveals = [];
  const media = {
    matches: reduced, listeners: [],
    addEventListener(type, callback) { this.listeners.push(callback); },
    removeEventListener(type, callback) { this.listeners = this.listeners.filter(item => item !== callback); },
  };
  const all = [];
  const document = {
    readyState: "loading", hidden, activeElement: null,
    documentElement: {
      classList: { add: name => rootClasses.add(name), remove: name => rootClasses.delete(name), contains: name => rootClasses.has(name) },
      setAttribute: (name, value) => rootAttributes.set(name, String(value)),
      removeAttribute: name => rootAttributes.delete(name),
      hasAttribute: name => rootAttributes.has(name),
      getAttribute: name => rootAttributes.get(name) ?? null,
    },
    addEventListener(type, callback) { if (!documentListeners.has(type)) documentListeners.set(type, []); documentListeners.get(type).push(callback); },
    removeEventListener(type, callback) { documentListeners.set(type, (documentListeners.get(type) || []).filter(item => item !== callback)); },
    getElementById: id => all.find(node => node.id === id) ?? null,
    querySelector: selector => (selector === ".shell" ? shell : selector === "[data-intro-focus]" ? heading : null),
  };
  function element(tag) {
    const classes = new Set();
    const node = {
      tagName: tag.toUpperCase(), id: "", textContent: "", type: "", inert: false,
      attributes: {}, children: [], listeners: {}, parentNode: null,
      classList: {
        add: (...names) => names.forEach(name => classes.add(name)),
        remove: (...names) => names.forEach(name => classes.delete(name)),
        contains: name => classes.has(name),
      },
      setAttribute(name, value) { this.attributes[name] = String(value); if (name === "id") this.id = String(value); },
      getAttribute(name) { return name in this.attributes ? this.attributes[name] : null; },
      appendChild(child) { child.parentNode = this; this.children.push(child); return child; },
      remove() { if (this.parentNode) this.parentNode.children = this.parentNode.children.filter(child => child !== this); this.parentNode = null; },
      addEventListener(type, callback) { (this.listeners[type] ||= []).push(callback); },
      focus() { document.activeElement = this; },
    };
    all.push(node);
    return node;
  }
  document.createElement = element;
  document.createElementNS = (namespace, tag) => element(tag);
  document.body = element("body");
  // The page as intro.js finds it at the top of <body>.
  const canvas = document.body.appendChild(element("canvas"));
  document.body.appendChild(element("script"));
  const shell = element("main");
  const heading = element("h1");
  const fab = element("button");
  const window = {
    matchMedia: () => media,
    setTimeout(callback, delay) { const id = clock.nextId++; clock.timers.push({ id, due: clock.now + delay, callback }); return id; },
    clearTimeout(id) { clock.timers = clock.timers.filter(timer => timer.id !== id); },
    UAMetallicMesh: metal ? { reveal: duration => reveals.push(duration) } : undefined,
    UAIntroWord: {
      create() {
        if (wordFails) throw new Error("no canvas");
        word.created += 1;
        return { play(done) { word.plays += 1; word.done = done; }, showStill() { word.stills += 1; }, stop() { word.stopped = true; } };
      },
    },
  };
  vm.runInContext(INTRO_SOURCE, vm.createContext({ window, document, console: { error: (...args) => errors.push(args), warn() {} } }));
  const intro = window.UADesktopIntro;
  const dismissed = [];
  intro.onDismiss(() => dismissed.push(true));
  return {
    intro, document, shell, heading, fab, canvas, word, errors, dismissed, media, rootClasses, reveals,
    held: () => rootAttributes.has("data-ua-background-hold"),
    overlay: () => all.find(node => node.classList.contains("ua-intro")),
    begin: () => all.find(node => node.classList.contains("ua-intro-begin")),
    // The parser reaches the end of the page after the deferred scripts ran.
    loaded() {
      for (const node of [shell, fab]) document.body.appendChild(node);
      document.readyState = "interactive";
      for (const callback of documentListeners.get("DOMContentLoaded") || []) callback();
    },
    advance(ms) {
      const end = clock.now + ms;
      for (;;) {
        const next = clock.timers.filter(timer => timer.due <= end).sort((a, b) => a.due - b.due)[0];
        if (!next) break;
        clock.timers = clock.timers.filter(timer => timer !== next);
        clock.now = next.due;
        next.callback();
      }
      clock.now = end;
    },
    key(key) {
      let prevented = false;
      for (const callback of [...(documentListeners.get("keydown") || [])]) callback({ key, preventDefault() { prevented = true; } });
      return prevented;
    },
    fire(type) { for (const callback of [...(documentListeners.get(type) || [])]) callback({ type }); },
  };
}

test("the intro covers the launcher, forms the title, then Escape hands focus to the launcher", () => {
  const ui = introSetup();
  const overlay = ui.overlay();
  // It is in place before the launcher is parsed, so the launcher never flashes.
  assert.equal(overlay.parentNode, ui.document.body);
  assert.equal(overlay.getAttribute("role"), "dialog");
  assert.equal(overlay.getAttribute("aria-modal"), "true");
  assert.ok(ui.rootClasses.has("ua-intro-open"));
  assert.equal(ui.intro.isOpen(), true);
  ui.loaded();
  // Everything behind the intro is out of reach until it is dismissed.
  for (const node of [ui.shell, ui.fab, ui.canvas]) assert.equal(node.inert, true);
  assert.equal(overlay.inert, false);
  assert.ok(overlay.classList.contains("is-animated"));
  assert.equal(ui.document.activeElement, overlay);
  ui.advance(500);
  assert.ok(overlay.classList.contains("show-title"));
  assert.equal(ui.word.plays, 1);
  ui.word.done();
  ui.advance(250);
  assert.ok(overlay.classList.contains("show-begin"));
  assert.equal(ui.document.activeElement, ui.begin());
  assert.equal(ui.key("Escape"), true);
  assert.equal(ui.intro.isOpen(), false);
  assert.equal(ui.dismissed.length, 1);
  // The overlay leaves the accessibility tree at once and the page after its fade.
  assert.equal(overlay.getAttribute("aria-hidden"), "true");
  assert.equal(overlay.inert, true);
  assert.ok(overlay.classList.contains("is-leaving"));
  assert.ok(!ui.rootClasses.has("ua-intro-open"));
  for (const node of [ui.shell, ui.fab, ui.canvas]) assert.equal(node.inert, false);
  assert.equal(ui.document.activeElement, ui.heading);
  assert.ok(ui.shell.classList.contains("ua-intro-enter"));
  ui.advance(750);
  assert.equal(overlay.parentNode, null);
  assert.equal(ui.word.stopped, true);
  assert.ok(!ui.shell.classList.contains("ua-intro-enter"));
  // Later keys belong to the launcher.
  assert.equal(ui.key("Escape"), false);
  assert.equal(ui.dismissed.length, 1);
});

test("Enter, Space and a click each dismiss the intro; other keys do not", () => {
  for (const key of ["Enter", " "]) {
    const ui = introSetup();
    ui.loaded();
    assert.equal(ui.key("a"), false);
    assert.equal(ui.intro.isOpen(), true);
    // Dismissing works before "Click to Begin" has appeared.
    assert.equal(ui.key(key), true, JSON.stringify(key));
    assert.equal(ui.intro.isOpen(), false, JSON.stringify(key));
  }
  const ui = introSetup();
  ui.loaded();
  ui.overlay().listeners.click[0]();
  assert.equal(ui.intro.isOpen(), false);
  assert.equal(ui.document.activeElement, ui.heading);
});

test("with reduced motion the finished title and button show at once", () => {
  const ui = introSetup({ reduced: true });
  ui.loaded();
  const overlay = ui.overlay();
  assert.ok(overlay.classList.contains("show-title"));
  assert.ok(overlay.classList.contains("show-begin"));
  assert.ok(!overlay.classList.contains("is-animated"));
  assert.equal(ui.word.stills, 1);
  assert.equal(ui.word.plays, 0);
  assert.equal(ui.document.activeElement, ui.begin());
  ui.key("Enter");
  // No fade: the launcher shows without its entrance and the overlay goes now.
  assert.ok(!ui.shell.classList.contains("ua-intro-enter"));
  ui.advance(0);
  assert.equal(overlay.parentNode, null);
});

test("turning on reduced motion while the title forms shows it still", () => {
  const ui = introSetup();
  ui.loaded();
  ui.advance(500);
  ui.media.matches = true;
  for (const callback of ui.media.listeners) callback();
  const overlay = ui.overlay();
  assert.ok(overlay.classList.contains("show-begin"));
  assert.ok(!overlay.classList.contains("is-animated"));
  assert.equal(ui.word.stills, 1);
});

test("a window that starts hidden waits to play the intro until it is shown", () => {
  const ui = introSetup({ hidden: true });
  ui.loaded();
  assert.equal(ui.shell.inert, true);
  assert.equal(ui.word.created, 0);
  ui.document.hidden = false;
  ui.fire("visibilitychange");
  assert.ok(ui.overlay().classList.contains("is-animated"));
  assert.equal(ui.word.created, 1);
});

test("an unattended intro settles into a still title instead of drawing frames all day", () => {
  const ui = introSetup();
  ui.loaded();
  ui.advance(500);
  ui.word.done();
  ui.advance(250);
  assert.ok(!ui.overlay().classList.contains("is-still"));
  ui.advance(30000);
  assert.ok(ui.overlay().classList.contains("is-still"));
  assert.equal(ui.word.stills, 1);
  assert.equal(ui.intro.isOpen(), true);
});

test("an intro that cannot start steps aside, logs why and releases the launcher", () => {
  const ui = introSetup({ wordFails: true });
  ui.loaded();
  assert.equal(ui.intro.isOpen(), false);
  assert.equal(ui.overlay().parentNode, null);
  assert.ok(!ui.rootClasses.has("ua-intro-open"));
  assert.equal(ui.shell.inert, false);
  assert.equal(ui.dismissed.length, 1);
  assert.match(ui.errors[0][0], /^ua\.desktop\.intro:/);
});

// ---------- Launcher and intro together ----------

function fakeIntro() {
  return {
    open: true, listeners: [],
    isOpen() { return this.open; },
    onDismiss(callback) { this.listeners.push(callback); },
    finish() { this.open = false; this.listeners.forEach(callback => callback()); },
  };
}

test("the Docker prerequisite dialog waits until the intro is dismissed", () => {
  const intro = fakeIntro();
  const ui = setup({}, { UADesktopIntro: intro });
  let focused = false;
  ui.elements.get("#docker-install").focus = () => { focused = true; };
  ui.run('renderIntegration({workspace_available: true}); latestDockerPrerequisite = {available: false, installed: false, setup_hint: "Install Docker Desktop"}; applyDockerState(false);');
  assert.notEqual(ui.elements.get("#docker-gate").hidden, false);
  // The launcher stays inert under the intro even though no dialog shows.
  assert.equal(ui.elements.get(".shell").inert, true);
  assert.equal(focused, false);
  // Runtime controls already reflect the missing daemon.
  assert.equal(ui.elements.get("#start-runtime").disabled, true);
  intro.finish();
  assert.equal(ui.elements.get("#docker-gate").hidden, false);
  assert.equal(ui.elements.get("#docker-gate-hint").textContent, "Install Docker Desktop");
  assert.equal(ui.elements.get(".shell").inert, true);
  assert.equal(focused, true);
  ui.elements.get("#docker-proceed").listeners.click();
  assert.equal(ui.elements.get(".shell").inert, false);
});

test("dismissing the intro does not count as seeing the Docker recovery banner", () => {
  const intro = fakeIntro();
  const ui = setup({}, { UADesktopIntro: intro });
  ui.run("renderIntegration({workspace_available: true}); applyDockerState(false); applyDockerState(true);");
  assert.equal(ui.elements.get("#docker-banner-text").textContent, "Docker detected, good to go.");
  // The click or key that dismisses the intro arrives while it is still open.
  ui.run("acknowledgeDockerRecovery()");
  intro.finish();
  assert.equal(ui.elements.get("#docker-banner").hidden, false);
  assert.equal(ui.elements.get(".shell").inert, false);
  ui.run("acknowledgeDockerRecovery()");
  assert.equal(ui.elements.get("#docker-banner").hidden, true);
});

// ---------- Still background (vendor/metallic_mesh.js) ----------

test("the dark background draws once, without a frame loop, even without WebGL2", () => {
  const frames = [];
  const warnings = [];
  const windowListeners = {};
  const documentListeners = {};
  const drawn = { fills: 0 };
  const body = { removeChild(node) { node.removed = true; } };
  function canvas() {
    return {
      style: {}, attributes: {}, parentNode: null,
      setAttribute(name, value) { this.attributes[name] = value; },
      addEventListener() {},
      getContext(type) {
        if (type !== "2d") return null;
        return {
          setTransform() {}, fillRect() {}, beginPath() {}, moveTo() {}, lineTo() {}, closePath() {}, stroke() {},
          fill() { drawn.fills += 1; },
          createLinearGradient() { return { addColorStop() {} }; },
        };
      },
    };
  }
  const host = {
    classList: { contains: name => name === "ua-doc-static-mesh" },
    insertAdjacentElement(position, node) { node.parentNode = body; },
  };
  const document = {
    querySelector: selector => (selector === "[data-ua-metallic-mesh]" ? host : null),
    getElementById: () => null,
    createElement: canvas,
    documentElement: { getAttribute: () => null, hasAttribute: () => false },
    addEventListener(type, callback) { documentListeners[type] = callback; },
  };
  const window = {
    innerWidth: 1180, innerHeight: 920, devicePixelRatio: 2,
    performance: { now: () => 1000 },
    matchMedia: () => ({ matches: false, addEventListener() {} }),
    getComputedStyle: () => ({ position: "fixed", top: "0px", right: "0px", bottom: "0px", left: "0px", zIndex: "-1" }),
    requestAnimationFrame(callback) { frames.push(callback); return frames.length; },
    cancelAnimationFrame() {},
    addEventListener(type, callback) { windowListeners[type] = callback; },
    UAAppearance: { current: () => ({ mode: "dark", dark: {} }) },
  };
  vm.runInContext(fs.readFileSync(`${__dirname}/vendor/metallic_mesh.js`, "utf8"),
    vm.createContext({ window, document, console: { warn: (...args) => warnings.push(args) } }));
  assert.equal(window.UAMetallicMesh.lab.mode(), "basic");
  assert.match(warnings[0][0], /WebGL2 is unavailable/);
  assert.ok(drawn.fills > 20, "the still drawing paints the faces");
  assert.equal(frames.length, 0, "a still surface must not schedule animation frames");
  // Resizing and switching to light and back each redraw once, still without a loop.
  windowListeners.resize();
  documentListeners["ua:appearance-change"]({ detail: { appearance: { mode: "light", dark: {} } } });
  assert.equal(window.UAMetallicMesh.canvas.style.display, "none");
  documentListeners["ua:appearance-change"]({ detail: { appearance: { mode: "dark", dark: {} } } });
  assert.equal(window.UAMetallicMesh.canvas.style.display, "");
  assert.equal(frames.length, 0);
});

test("the dark background fills in once from blank as the intro starts, then stays still", () => {
  const ui = introSetup();
  // Held blank before the deferred renderers run, so no full frame shows first.
  assert.equal(ui.held(), true);
  ui.loaded();
  assert.deepEqual(ui.reveals, [1800]);
  assert.equal(ui.held(), false);
  assert.equal(ui.rootClasses.has("ua-intro-filling"), false);
  ui.advance(5000);
  ui.key("Escape");
  assert.deepEqual(ui.reveals, [1800], "dismissing never fills again");
});

test("the light background fades in through the fill class for 1.8 seconds", () => {
  const ui = introSetup({ colorMode: "light" });
  assert.equal(ui.held(), true);
  ui.loaded();
  assert.deepEqual(ui.reveals, []);
  assert.equal(ui.held(), false);
  assert.ok(ui.rootClasses.has("ua-intro-filling"));
  ui.advance(1799);
  assert.ok(ui.rootClasses.has("ua-intro-filling"));
  ui.advance(1);
  assert.equal(ui.rootClasses.has("ua-intro-filling"), false);
});

test("the dark fill falls back to the fade when the metal renderer is missing", () => {
  const ui = introSetup({ metal: false });
  ui.loaded();
  assert.ok(ui.rootClasses.has("ua-intro-filling"));
  assert.equal(ui.held(), false);
});

test("reduced motion shows the background at once, without a hold or a fill", () => {
  const ui = introSetup({ reduced: true });
  assert.equal(ui.held(), false);
  ui.loaded();
  assert.deepEqual(ui.reveals, []);
  assert.equal(ui.rootClasses.has("ua-intro-filling"), false);
});

test("a window that starts hidden keeps the background held until it is shown", () => {
  const ui = introSetup({ hidden: true });
  ui.loaded();
  assert.equal(ui.held(), true);
  assert.deepEqual(ui.reveals, []);
  ui.document.hidden = false;
  ui.fire("visibilitychange");
  assert.deepEqual(ui.reveals, [1800]);
  assert.equal(ui.held(), false);
});

test("an intro that fails or is dismissed early never leaves the background blank", () => {
  const failed = introSetup({ wordFails: true });
  failed.loaded();
  assert.equal(failed.held(), false);
  const early = introSetup({ hidden: true });
  early.loaded();
  early.intro.dismiss();
  assert.equal(early.held(), false);
  assert.deepEqual(early.reveals, []);
});
