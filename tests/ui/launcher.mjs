import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir, open, readFile, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { remote } from 'webdriverio';

// The executable has the real bundled UI and a simulated command boundary.
// It never instantiates the production controller or opens user credentials.
const artifacts = resolve('target/ui-test-results', process.platform);
await mkdir(artifacts, { recursive: true });
for (const name of ['results.json', 'failure.png', 'intro.png', 'always-on-notice.png', 'launcher.png', 'launcher-initial.png', 'launcher-light.png', 'updates-drawer.png']) {
  await rm(resolve(artifacts, name), { force: true });
}
const reservation = createServer();
await new Promise((yes, no) => reservation.once('error', no).listen(0, '127.0.0.1', yes));
const port = reservation.address().port;
await new Promise(yes => reservation.close(yes));
const log = await open(resolve(artifacts, 'driver.log'), 'w');
const exe = resolve('target/debug/examples', `ui_driver${process.platform === 'win32' ? '.exe' : ''}`);
const child = spawn(exe, [], {
  env: { ...process.env, TAURI_WEBDRIVER_PORT: String(port) },
  stdio: ['ignore', log.fd, log.fd],
});
let spawnError;
child.on('error', error => { spawnError = error; });
let browser;
const passed = [];
async function check(name, test) {
  await test();
  passed.push(name);
  console.log(`PASS: ${name}`);
}
try {
  let ready = false;
  for (let attempt = 0; attempt < 150; attempt++) {
    if (spawnError) throw spawnError;
    if (child.exitCode !== null) throw new Error(`UI fixture exited with ${child.exitCode}; inspect ${artifacts}/driver.log`);
    try {
      const response = await fetch(`http://127.0.0.1:${port}/status`, { signal: AbortSignal.timeout(500) });
      if (response.ok) { ready = true; break; }
    } catch {}
    await delay(200);
  }
  assert.ok(ready, 'Embedded driver must start within 30 seconds');
  browser = await remote({
    hostname: '127.0.0.1', port, path: '/', logLevel: 'warn',
    connectionRetryCount: 0, connectionRetryTimeout: 15000,
    capabilities: { browserName: 'wry', 'wdio:windowLabel': 'control' },
  });
  const element = selector => browser.$(selector);
  const waitText = async (selector, expected) => browser.waitUntil(
    async () => (await element(selector).getText()).includes(expected),
    { timeout: 10000, timeoutMsg: `${selector} must contain ${expected}` },
  );
  const ALWAYS_ON_SEEN_KEY = 'ua-desktop.status-window.always-on-notice-seen';
  const shellInert = () => browser.execute(() => document.querySelector('.shell').inert);

  // The intro covers the launcher on every page load and blocks clicks until it
  // is dismissed, so it goes first.
  await check('the intro covers the launcher until Click to Begin reveals it', async () => {
    const intro = element('.ua-intro');
    await intro.waitForExist({ timeout: 10000, timeoutMsg: 'The intro must cover the launcher on load' });
    assert.equal(await intro.getAttribute('role'), 'dialog');
    assert.equal(await element('#ua-intro-title').getProperty('textContent'), 'Universal Agents');
    assert.equal(await shellInert(), true);
    const begin = element('.ua-intro-begin');
    await begin.waitForDisplayed({ timeout: 15000, timeoutMsg: 'Click to Begin must appear once the title has formed' });
    await browser.saveScreenshot(resolve(artifacts, 'intro.png'));
    // The fixture keeps its saved choices between runs; forget Got it so the
    // first-run notice shows after Click to Begin.
    await browser.execute(key => localStorage.removeItem(key), ALWAYS_ON_SEEN_KEY);
    await begin.click();
    await intro.waitForExist({ reverse: true, timeout: 5000, timeoutMsg: 'The intro must leave after Click to Begin' });
  });
  await check('after Click to Begin, a one-time notice says agents need a computer that stays on', async () => {
    const notice = element('#always-on-notice');
    await notice.waitForDisplayed({ timeout: 5000, timeoutMsg: 'The always-on notice must follow the intro on first run' });
    assert.equal(await shellInert(), true);
    assert.equal(await browser.execute(() => document.activeElement?.id), 'always-on-got-it');
    assert.match(await element('#always-on-detail').getText(), /24\/7/);
    assert.equal(await element('#always-on-sleep-hint').getText(), 'UI fixture sleep setting hint');
    await browser.saveScreenshot(resolve(artifacts, 'always-on-notice.png'));
    await element('#always-on-got-it').click();
    await notice.waitForDisplayed({ reverse: true, timeout: 5000, timeoutMsg: 'Got it must close the notice' });
    assert.equal(await shellInert(), false);
    assert.equal(await browser.execute(() => document.activeElement?.id), 'launcher-title');
    assert.notEqual(await browser.execute(key => localStorage.getItem(key), ALWAYS_ON_SEEN_KEY), null);
  });
  await waitText('#status-label', 'Stopped');

  await check('real launcher loads both version labels and collapsed private access', async () => {
    // The fixture runs the version built into the launcher: the newest list entry.
    const { runtimes } = JSON.parse(await readFile('compatible-runtimes.json', 'utf8'));
    assert.equal(await element('#header-runtime-version').getText(), runtimes.at(-1).ua_version);
    assert.match(await element('#desktop-version').getText(), /^\d+\.\d+\.\d+$/);
    assert.equal(await element('#registry-access').getAttribute('open'), null);
    assert.equal(await element('#open-app').isEnabled(), false);
    assert.equal(await element('#open-browser').isEnabled(), false);
    await element('#registry-access > summary').click();
    assert.notEqual(await element('#registry-access').getAttribute('open'), null);
    await element('#registry-access > summary').click();
  });
  await check('the sun/moon switch flips between dark and light and remembers the choice', async () => {
    const colorMode = () => browser.execute(() => document.documentElement.getAttribute('data-color-mode'));
    const saved = () => browser.execute(() => localStorage.getItem('ua-desktop.status-window.color-mode'));
    // The intro's one-time fill has released the background.
    assert.equal(await browser.execute(() => document.documentElement.hasAttribute('data-ua-background-hold')), false);
    assert.equal(await element('#theme-toggle').getAttribute('aria-label'), 'Switch to light theme');
    await element('#theme-toggle').click();
    assert.equal(await colorMode(), 'light');
    assert.equal(await element('#theme-toggle').getAttribute('aria-label'), 'Switch to dark theme');
    assert.equal(await saved(), 'light');
    await browser.pause(400);
    await browser.saveScreenshot(resolve(artifacts, 'launcher-light.png'));
    // Leave the fixture dark, the default, for the screenshots and later runs.
    await element('#theme-toggle').click();
    assert.equal(await colorMode(), 'dark');
    assert.equal(await saved(), 'dark');
    // Let the controls' 120 ms color transitions finish before the screenshot.
    await browser.pause(400);
  });
  await browser.execute(() => window.scrollTo(0, 0));
  await browser.saveScreenshot(resolve(artifacts, 'launcher-initial.png'));
  await check('the header controls share one height and one centre line', async () => {
    const boxes = await browser.execute(() => ['#open-updates', '#start-runtime', '#restart-runtime',
      '#stop-runtime', '#stop-runtime-and-quit', '#quit-desktop-app', '#theme-toggle'].map((selector) => {
      const box = document.querySelector(selector).getBoundingClientRect();
      return { selector, height: Math.round(box.height), middle: Math.round(box.top + box.height / 2) };
    }));
    for (const box of boxes) {
      assert.equal(box.height, 38, `${box.selector} height`);
      assert.ok(Math.abs(box.middle - boxes[0].middle) <= 1, `${box.selector} is centred with the others`);
    }
  });
  await check('header lifecycle buttons send native IPC and update availability', async () => {
    await element('#start-runtime').click();
    await waitText('#status-label', 'Running');
    assert.equal(await element('#open-app').isEnabled(), true);
    assert.equal(await element('#open-browser').isEnabled(), true);
    await element('#restart-runtime').click();
    await waitText('#status-label', 'Running');
    await element('#stop-runtime').click();
    await waitText('#status-label', 'Stopped');
    assert.equal(await element('#open-app').isEnabled(), false);
    assert.equal(await element('#open-browser').isEnabled(), false);
  });
  await check('visible logs refresh automatically without clicking Refresh', async () => {
    // Logs live in a collapsed Advanced Settings section and refresh only while it is open.
    assert.equal(await element('#logs-section').getAttribute('open'), null);
    await element('#logs-section > summary').click();
    const before = await element('#logs').getText();
    await browser.waitUntil(async () => (await element('#logs').getText()) !== before,
      { timeout: 7000, timeoutMsg: 'Expected an automatic log refresh' });
  });
  await check('workspace, logs, and quit affordances are present and wired', async () => {
    // The Controls pane is open initially.
    assert.notEqual(await element('#controls-section').getAttribute('open'), null);
    // Refresh external skills ships on by default.
    assert.equal(await element('#refresh-external-skills').isSelected(), true);
    // Open folder sits beside the folder picker and does not replace it.
    assert.equal(await element('#open-workspace-folder').isExisting(), true);
    assert.equal(await element('#choose-workspace').isExisting(), true);
    // The logs pane can be enlarged and returned to its normal layout.
    await element('#expand-logs').click();
    assert.equal(await element('#expand-logs').getAttribute('aria-expanded'), 'true');
    assert.equal(await element('#expand-logs').getAttribute('aria-label'), 'Collapse logs');
    await element('#expand-logs').click();
    assert.equal(await element('#expand-logs').getAttribute('aria-expanded'), 'false');
    // Pausing stops the automatic refresh; resuming fetches again.
    await element('#toggle-log-refresh').click();
    await waitText('#logs-state', 'Paused');
    const paused = await element('#logs').getText();
    await browser.pause(4000);
    assert.equal(await element('#logs').getText(), paused);
    await element('#toggle-log-refresh').click();
    await waitText('#logs-state', 'Live');
    await browser.waitUntil(async () => (await element('#logs').getText()) !== paused,
      { timeout: 5000, timeoutMsg: 'Resuming must refresh the logs' });
    // Quit (stop UA first) and Close (launcher only) are in the header; Quit needs a running runtime.
    assert.equal(await element('#stop-and-quit-label').getText(), 'Quit');
    // Close comes first, then Quit.
    const closeBox = await element('#quit-desktop-app').getLocation();
    const quitBox = await element('#stop-runtime-and-quit').getLocation();
    assert.ok(closeBox.x < quitBox.x, 'Close sits left of Quit');
    // The log tools share the open logs section's title row.
    const title = await element('#logs-section > summary').getLocation();
    const tools = await element('#log-lines').getLocation();
    assert.ok(Math.abs(tools.y - title.y) < 24, `log tools (${tools.y}) are on the title row (${title.y})`);
    // The mark opens the UA project page; the fixture reports it instead of opening a browser.
    await element('#open-ua-repository').click();
    await waitText('#notice', 'would open the Universal Agents project page');
    assert.equal(await element('#quit-desktop-app').getText(), 'Close');
    assert.equal(await element('#quit-desktop-app').isExisting(), true);
    assert.equal(await element('#quit-desktop-app').isEnabled(), true);
    assert.equal(await element('#stop-runtime-and-quit').isEnabled(), false);
  });
  const drawerOpen = () => element('#updates-drawer').isDisplayed();
  const openDrawer = async () => {
    await element('#open-updates').click();
    await browser.waitUntil(drawerOpen, { timeout: 5000, timeoutMsg: 'The Updates drawer must open' });
  };
  await check('the header count opens the Updates drawer, and Escape closes it', async () => {
    // The fixture offers a newer UA and a newer launcher.
    await waitText('#updates-count', '2');
    await openDrawer();
    assert.equal(await shellInert(), true);
    assert.equal(await browser.execute(() => document.activeElement?.id), 'updates-title');
    await browser.keys('Escape');
    await browser.waitUntil(async () => !(await drawerOpen()), { timeout: 5000, timeoutMsg: 'Escape must close the drawer' });
    assert.equal(await shellInert(), false);
    assert.equal(await browser.execute(() => document.activeElement?.id), 'open-updates');
  });
  await check('the launcher lane opens only the newest launcher release page', async () => {
    await openDrawer();
    await waitText('#launcher-status', 'Launcher 99.9.9 is available');
    await element('#open-launcher-download').click();
    // The fixture reports the page it would open instead of opening a browser.
    await waitText('#drawer-notice', 'would open the launcher 99.9.9 release page');
  });
  await check('a reviewed update downloads in the background without restarting', async () => {
    await element('#download-update').click();
    await waitText('#update-status', 'UA 99.0.0-test is downloaded');
    await waitText('#runtime-steps', 'Downloaded');
    assert.equal(await element('#download-update').isDisplayed(), false);
    await element('#close-updates').click();
    // Finishing the download is a new stage, so the badge shows it.
    await browser.waitUntil(async () => element('#update-badge').isDisplayed(),
      { timeout: 3000, timeoutMsg: 'The badge must show the finished download' });
    await waitText('#update-badge-label', 'UA 99.0.0-test is downloaded');
    // Focus returns to the header button that opened the drawer.
    assert.equal(await browser.execute(() => document.activeElement?.id), 'open-updates');
    await element('#update-badge-dismiss').click();
    assert.equal(await element('#update-badge').isDisplayed(), false);
  });
  await check('Docker availability is probed without blocking the launcher', async () => {
    // The fixture reports Docker as available, so no prerequisite dialog or
    // waiting banner is shown and runtime controls are usable.
    assert.equal(await element('#docker-gate').isExisting(), true);
    assert.equal(await element('#docker-gate').isDisplayed(), false);
    assert.equal(await element('#docker-banner').isDisplayed(), false);
  });
  await check('a newer catalog version can be chosen; older ones cannot', async () => {
    await openDrawer();
    await element('#check-updates').click();
    await waitText('#update-status', 'UA 99.0.0-test is available');
    const older = element('#runtime-catalog .catalog-row.is-older');
    assert.match(await older.getText(), /Older than the running version/);
    assert.equal(await older.$('button*=Choose').isExisting(), false);
    await element('button[aria-label="Make UA 98.0.0-test the update"]').click();
    await waitText('#update-status', 'UA 98.0.0-test is available');
    await waitText('#runtime-steps', 'Chose UA 98.0.0-test');
    assert.match(await element('#runtime-catalog .catalog-row.is-selected').getText(), /98\.0\.0-test/);
  });
  await check('update review starts with notes collapsed, renders them as text and gates the switch on acknowledgment', async () => {
    await element('button[aria-label="Make UA 99.0.0-test the update"]').click();
    await waitText('#update-status', 'UA 99.0.0-test is available');
    assert.equal(await element('#install-update').isEnabled(), false);
    // The notes start collapsed; opening them shows the text.
    assert.equal(await element('#update-notes').isDisplayed(), false);
    await element('#update-notes-section summary').click();
    assert.match(await element('#update-notes').getText(), /<b>must remain plain text<\/b>/);
    assert.equal(await element('#update-notes b').isExisting(), false);
    await element('#update-ack').click();
    assert.equal(await element('#install-update').isEnabled(), true);
    await element('#install-update').click();
    // Switching closes the drawer so the restart shows under the header.
    await browser.waitUntil(async () => !(await drawerOpen()), { timeout: 5000, timeoutMsg: 'Switch must close the drawer' });
    await waitText('#status-detail', 'acknowledged update reached the backend');
  });
  await check('offering unchecked versions switches the offer to a pre-release and back', async () => {
    await openDrawer();
    await waitText('#update-note', '99.1.0-beta.1 is newer but not on this launcher');
    const toggle = element('#offer-unlisted-runtimes');
    assert.equal(await toggle.isSelected(), false);
    await toggle.click();
    await waitText('#update-status', '99.1.0-beta.1 is available as an unchecked pre-release');
    assert.match(await element('#update-warning').getText(), /not on this launcher's compatibility list/);
    assert.equal(await element('#update-note').isDisplayed(), false);
    await toggle.click();
    await waitText('#update-status', 'UA 99.0.0-test is available.');
  });
  await browser.saveScreenshot(resolve(artifacts, 'updates-drawer.png'));
  await browser.keys('Escape');
  await browser.saveScreenshot(resolve(artifacts, 'launcher.png'));
  await writeFile(resolve(artifacts, 'results.json'), JSON.stringify({ platform: process.platform, passed }, null, 2));
} catch (error) {
  if (browser) await browser.saveScreenshot(resolve(artifacts, 'failure.png')).catch(() => {});
  throw error;
} finally {
  if (browser) await browser.deleteSession().catch(() => {});
  child.kill();
  await log.close();
}
