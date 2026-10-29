# Local CI

Native tests, audits, development installers, and release workflows select
self-hosted runners. Documentation builds and deployment use disposable
GitHub-hosted runners so public contributions cannot execute on personal hosts.
Release triggers are unchanged. Running CI does not authorize a release.

| Runner labels, in addition to `self-hosted, ua-build` | Work |
| --- | --- |
| `Windows, X64, ua-windows` | Native Windows tests and NSIS/MSI packages |
| `macOS, ARM64, ua-macos` | Native Apple Silicon tests and universal macOS app/DMG |
| `Linux, X64, ua-linux` | Core checks, native Linux tests and DEB/AppImage packages |

The sibling Universal Agents runtime tests on Linux and builds container images
natively on WSL for AMD64 and the Mac's dedicated Colima Docker daemon for ARM64. Its
Linux amd64/arm64 images run under Docker on Windows, macOS, and Linux. This does
not establish native Windows support for every Python dependency or shell tool.
Container publishing uses a disposable Buildx builder on each native host.
Preflight builds and runs both architectures; jobs check the existing binfmt
registration instead of installing global emulation handlers each time.

## Host requirements

The public documentation site is the exception to these local-runner requirements.
See [Documentation publishing](#documentation-publishing) below.

Use an organization runner group restricted to approved repositories, with public
access enabled only after installing the reviewed host policy. Fork PRs cannot
use local hosts. Assign the custom labels only after installing
the cleanup hooks. Each runner process needs its own application, work, hook,
and state directories. Do not share a work directory or tool cache between
runner processes. Keep the native Windows runner in the signed-in account so
the existing interactive WebView probes can execute.

Provision RTK 0.48.0, system Python 3.12 or newer, Node.js 22 or newer, Git,
Rust 1.95.0 with rustfmt and Clippy, and the platform's Tauri prerequisites.
Jobs select the toolchain through `RUSTUP_TOOLCHAIN` and install components with
`--no-self-update`, leaving the account's default toolchain unchanged.
Runtime sidecar jobs install Node.js 24.20.0. Runtime jobs install Python 3.12
or 3.13 in disposable environments. GitHub CLI 2.100.0 is provisioned for release jobs.
The Actions runner must support the Node runtime used by the pinned actions;
leave runner auto-update enabled.

On macOS install both `aarch64-apple-darwin` and `x86_64-apple-darwin` under
Rust 1.95.0. Set `RUSTUP_TOOLCHAIN=1.95.0` for direct builds. Put Apple's system
tools before Homebrew on PATH, then include Cargo and Homebrew explicitly.
In particular Tauri must find `/usr/bin/xattr`. Universal packaging verifies
both architectures compile; execution tests on this host cover ARM64 only.

On Linux provision a native Linux Docker daemon, Buildx, QEMU/binfmt, compiler
tools, OpenSSL development headers, WebKitGTK 4.1 development headers,
libxdo, Ayatana AppIndicator, librsvg, patchelf, FUSE 2, Xvfb, dbus-x11, and
xdg-utils. Native Linux packages built on Ubuntu 24.04 have that userspace
baseline; do not claim Ubuntu 22.04 compatibility without a separate build.

### Windows and WSL

The Windows runner cannot execute GitHub Linux container actions directly.
Register a separate Linux runner inside WSL 2. It can build both the runtime
containers and the Linux desktop app. Use its ext4 filesystem, not `/mnt/c`,
for the runner checkout and build caches. GUI tests use Xvfb and D-Bus.

The September 2026 setup creates a dedicated Ubuntu 24.04 WSL 2 distribution
named `ua-ci`. The existing `Ubuntu` WSL 1 distribution is preserved. The
native Windows and WSL runners share CPU, RAM, and physical disk, so each job
limits Cargo to two build processes. WSL needs a persistent launcher at Windows
logon; Linux systemd services do not start a stopped WSL distribution themselves.
Use the signed-in account for the Windows runner and a dedicated Linux account
for the WSL runner.

Turn Windows **Smart App Control** off on the Windows runner host. It blocks
any program that is neither signed nor widely known, which includes every
build script and test binary Cargo compiles and then runs, and it cannot exempt
a folder. A fresh Windows installation starts it in evaluation mode and may
switch it on by itself later. When that happened on a runner host, every
Windows job failed with `An Application Control policy has blocked this file.
(os error 4551)` while compiling `proc-macro2`. The CodeIntegrity event log
records the blocks as events 3033 and 3077. Turn it off with
`VerifiedAndReputablePolicyState = 0` under
`HKLM\SYSTEM\CurrentControlSet\Control\CI\Policy` and `CiTool.exe --refresh`.
Microsoft Defender antivirus stays on.

The Windows runner must run as the signed-in desktop user. The native popup
test runs as the runner's account on the interactive desktop. A runner
installed as a Windows service runs as `NETWORK SERVICE`; there the probe
printed PASS but WebView2 could not shut down and the step timed out ("Popup
probe did not finish in the interactive desktop within 120 seconds"). The same
code passed and exited when started as the desktop user. Leave the runner
service stopped and disabled (its registration can stay), and start the
runner's `run.cmd` from a Windows logon task that runs as the desktop user with
limited rights, starts at that user's sign-in and restarts if it exits. A
disconnected session is enough for the probes; after a reboot that user must
sign in (or use automatic sign-in) before the runner comes back.

The Linux desktop jobs run the native popup test under a virtual display, so
every Linux runner distro needs the `xvfb` package (`xvfb-run`) as well as
`dbus` (`dbus-run-session`). A WSL distro without `xvfb` fails that step with
`xvfb-run: command not found` (exit code 127).

WSL filesystem deletion makes space reusable inside Linux. Returning that space
to Windows also requires discard/trim and sparse VHD support. Enable the distro's
`fstrim.timer`, verify sparse VHD support with the installed WSL version, and
check Windows free space as well as `df`. Do not shut down all WSL distributions
or compact Docker Desktop's VHD during CI. See [Microsoft's WSL disk guidance](https://learn.microsoft.com/en-us/windows/wsl/disk-space).

## Local CI for public repositories

The runner group selects approved repositories, retaining any already approved
private repositories. Each installed hook separately lists approved public
repositories in `public_repositories` in its `config.json`, outside the checkout.
Only add Universal Agents and Desktop to that public list for this rollout.

After an API update, read back both repository and workflow access. Include
`selected_workflows` in each update while workflow restrictions are enabled;
omitting it clears the list and leaves jobs queued even when runners are online.
Group policy updates can also clear selected repositories. Apply the repository
list after the final group update, then verify both lists again.

For public work, the hook accepts pushes, schedules, manual runs, and PRs whose head
and base both belong to the configured repository. Fork PRs, missing or mismatched
repository identity, `pull_request_target`, `workflow_run`, and other events are
refused before job state or commands are created. Same-repository branches are
trusted team code: only people and automation trusted with the machines may push
them. Public documentation validation stays on disposable hosted runners.
Private repositories sharing the hosts keep their existing non-PR trigger policy.

Every local job requires `UA_CI_PUBLIC_JOB_POLICY_VERSION=1` beside the existing
hook/cache markers. Install the reviewed hook and public allowlist before enabling
group access. Preserve host paths, disk reserves, caches and private-repository
entries. Jobs no longer replace installed hooks with a checkout's copy.

The standard-library runner tests exercise accepted public work and rejected
fork/unsupported events. Documentation tests live under `tests/docs/` in their
own locked environment, so native compatibility checks do not need MkDocs. After
installation, dispatch **Local runner preflight**, inspect cleanup, and confirm
all four runners are online. This validation publishes no releases or images.

## Documentation publishing

The site is published at [Universal Agents Desktop documentation](https://enzimo.github.io/universal-agents-desktop/),
with the quickstart under `/universal-agents-desktop/quickstart/`.
`publish-docs.yml` tests and builds it on GitHub-hosted `ubuntu-24.04` runners.
It publishes automatically after a push to `main` or a stable `vX.Y.Z` tag, or a
manual run on either ref. Pull requests validate without uploading or deploying.
Feature branches and pre-release tags cannot deploy. Native build workflows and
their installed public-job policy remain separate.

In **Settings > Pages > Build and deployment**, select **GitHub Actions** as the
source. The `github-pages` environment permits branch `main` and tag pattern
`v*.*.*`; the workflow excludes pre-release tags and checks tag syntax. To retry a
main deployment, open **Actions > Publish Documentation > Run workflow** and
select `main`. The build token has only `contents: read` and `pages: read`; only
the guarded deployment job receives `pages: write` and `id-token: write`.

The workflow verifies the SHA-256 of RTK 0.51.0 before installing it through a
JavaScript action. Subsequent shell steps run through RTK. Python 3.13 and Node.js
24.20.0 come from pinned setup actions. No local hook, `UA_CI_PYTHON`, Docker,
Rust compiler, native desktop session, or secret is needed to build the site.

`mkdocs.yml` uses the Material theme and the existing Markdown guides as its
source. `scripts/docs_site.py` maps repository paths into MkDocs without copying
the guides. MkDocs resolves links against their original paths and writes
lowercase page URLs. The hook includes only the documentation folder and its
listed root files, so local configuration, credentials and build output are not
published. Theme files keep their own paths. This website changes no launcher UI,
so the desktop-only `ui/DESIGN.md` needs no change.

Use an isolated environment for documentation tools:

```sh
rtk proxy python3 -m venv .venv
PATH="$PWD/.venv/bin:$PATH" rtk proxy python -m pip install --require-hashes -r requirements-docs.lock.txt
PATH="$PWD/.venv/bin:$PATH" rtk proxy python -m unittest discover -s tests/docs -v
PATH="$PWD/.venv/bin:$PATH" rtk proxy python -m mkdocs build --strict
PATH="$PWD/.venv/bin:$PATH" rtk proxy python -m mkdocs serve
```

The docs tests build the real site, serve it under the project URL path, check
every generated page's local links and assets, and execute the publication
conditions against pull requests, public/private repositories, main, stable and
pre-release tags. They also execute tag validation against malformed versions.
The real hosted build and a successful HTTP request to the public quickstart
complete deployment verification. Native Rust and GUI checks remain required
for launcher changes; a documentation build does not replace them.

`requirements-docs.txt` owns the documentation dependencies; CI installs its
hash-checked lock. After changing that input, regenerate the lock with uv 0.13.6:

```sh
rtk proxy uv pip compile requirements-docs.txt --generate-hashes --universal --output-file requirements-docs.lock.txt
```

## Install cleanup hooks

On Apple Silicon Macs, Homebrew's Buildx plugin may be registered only in the
account's Docker configuration. Jobs use a disposable `DOCKER_CONFIG`, so that
registration does not apply. Provision the plugin in Docker's standard system
plugin directory. If Homebrew provides the executable and the destination does
not already exist, a Mac administrator can create the link after checking the directory.

Check the directory itself for a dangling link first. A removed Docker Desktop
installation can leave `/usr/local/lib/docker/cli-plugins` pointing at its missing
application folder. This makes `install -d` fail with a `stat` error. Verify that
the link is broken and points at the removed application, then move only that
link to an unused backup name before creating the real directory and plugin link:

```sh
rtk proxy sudo install -d -m 0755 /usr/local/lib/docker/cli-plugins
rtk proxy sudo ln -s /opt/homebrew/opt/docker-buildx/bin/docker-buildx /usr/local/lib/docker/cli-plugins/docker-buildx
```

Verify `docker buildx version` with an empty `DOCKER_CONFIG` before accepting
build jobs. This preserves the job's Docker credential isolation.

Install a reviewed copy of `.github/runner/ci_hook.py` outside both the runner
application directory and its work directory. Run the installer from system
Python, with the runner idle. The following paths are examples, not host config.

```sh
rtk proxy python3 .github/runner/provision_sccache.py /opt/ua-ci-tools
rtk proxy python3 .github/runner/install_hooks.py \
  --sccache /opt/ua-ci-tools/sccache \
  --runner /opt/actions-runner \
  --destination /opt/ua-ci-hooks \
  --repository enzimo/universal-agents-desktop \
  --repository enzimo/universal-agents \
  --public-repository enzimo/universal-agents-desktop \
  --public-repository enzimo/universal-agents
```

On Windows use `python3` from the signed-in account and Windows paths. The
installer writes platform-specific wrappers and updates the runner's `.env`.
Restart the idle runner afterward. Do not set `UA_CI_HOOK_VERSION` manually.
Every local-runner job checks that the started hook actually exported it.

The hook implementation is mirrored in both repositories so either checkout can
provision a host. Keep the files identical when changing their contract. Their
tests need only standard-library Python:

```sh
rtk proxy python3 -m unittest discover -s .github/runner -v
```

## Resource ownership and retention

Before a job starts, the hook retries cleanup of any recorded interrupted job,
checks repository trust, and requires at least 30 GiB free on the state and work
filesystems. A failed cleanup or low disk reserve blocks the next job.

Each job receives a random identity and private Cargo, XDG, Docker CLI, and
temporary directories. Compiler and npm/pip download caches are retained under
the installed hooks directory, separately from job state. Runtime Python packages go in the checkout's `.venv`.
Builds retain the usual `target` layout because runtime verification and desktop
UI tests locate their executables there. They do not use the developer's UA data.

After all steps and action post steps finish, the completed hook removes:

- Containers and anonymous volumes bearing the exact job ownership label.
- Images produced by the job and its uniquely named Buildx builder and cache.
- Interactive Windows test tasks bearing the job's prefix.
- The job checkout, build output, environments, disposable Cargo files, and temporary files.
- That runner's downloaded actions and setup-action tool cache.

The standard runner tracks and terminates descendant job processes. Interactive
Windows tasks also have explicit stop/unregister paths. A hard host failure
cannot execute a hook; its manifest survives so the next job retries cleanup.
If a daemon is unavailable, restore it and run the installed cleanup wrapper
before accepting more jobs. Never erase the manifest to bypass an error.

Uploaded evidence and development bundles expire after three days, because
GitHub counts artifact storage over the whole month (see `GOTCHAS.md`). Local copies
are deleted after upload. Runner diagnostic logs older than seven days are
removed at job start. Fixed system toolchains, the runner installation, and
shared upstream Docker images are retained. Hooks do not prune shared Docker
images, stop Docker Desktop, or delete unrelated caches or volumes. Base images
for live tests and BuildKit are a small provisioned working set; audit their
size when upgrading those versions. The 30 GiB gate prevents further admission
when unrelated host usage consumes the reserve; it is not a per-job disk quota.

GitHub's hook ordering and recovery behavior are documented in
[Running scripts before or after a job](https://docs.github.com/en/actions/how-tos/manage-runners/self-hosted-runners/run-scripts).

## Activate and verify

1. Give the provisioning login organization runner administration permission,
   for example through a delegated runner-administrator role. Token scopes and
   the account role must both permit runner administration.
2. Verify that the Windows runner can serve both repositories. Register the Mac
   and WSL runners in the restricted runner group.
3. Install hooks, restart the idle runners, and assign the reviewed labels.
4. Merge the reviewed workflow changes, then dispatch **Local runner preflight**.
   Inspect both the started and completed hook logs. Confirm the disposable
   Docker builder, image, checkout, and state cache disappeared afterward.
5. Run the ordinary CI and native builds. Build success does not replace the
   interactive Docker lifecycle checks in `PLATFORM_TESTING.md` or release signing.

For container readiness, dispatch `publish-container.yml` with `build_only=true`.
It builds both architectures without image or registry-cache publication.
Documentation deployment and release workflows have external publishing effects.

On WSL the hook installer requires `--reserve-path /mnt/c`, or the mount for
the Windows drive actually holding the distro VHD. This checks physical Windows
free space as well as Linux filesystem space before admitting a job.

### Runner recovery

If the dedicated CI distribution reports I/O errors before it can start a
command, first check that its runners are offline and no job is active. Restarting
the distribution stops every process inside it. With the operator's approval,
terminate only that distribution, then start it again and verify command execution
and both runner services. Keep Docker Desktop and other distributions untouched.
If the error remains, investigate it before accepting more jobs.

Keep the Mac awake while it serves CI. For a temporary verification session,
run `rtk proxy caffeinate -dimsu` in a terminal and leave it running until the
jobs finish. A launch agent cannot accept work while its host is asleep.

If an assigned Windows job never starts a workflow step, inspect the listener's
worker-startup logs. A 30-second worker-channel timeout recovered after restarting
the idle runner's existing interactive task during this rollout; its initial cause
remains unconfirmed. End the stalled job and verify no worker is active before
restarting that task. Keep the logs if it recurs.

OpenSSH can report `WinError 448` when traversing a runner's versioned junctions
even though the physical binaries start. An SSH-only error does not establish why
an interactive runner failed; inspect that runner's logs before changing its files.

### WSL disk maintenance

The inspected WSL 2.7.13 installation rejects sparse VHD activation because of
potential data corruption. Do not use its `--allow-unsafe` override. Job cleanup
still frees blocks for reuse inside the distro. A VHD can retain its previous
peak size on Windows until offline compaction.

Keep `fstrim.timer` enabled. During a maintenance window, drain and stop the Linux
runner, trim its filesystem, then stop only the dedicated distro. Wait for
Windows to release the VHD. From an elevated Windows session, run:

```powershell
rtk proxy powershell -NoProfile -ExecutionPolicy Bypass -File .github/runner/compact_wsl.ps1 -Distribution ua-ci
```

The helper resolves that distro's VHD from its registration, refuses a running
distro, and invokes DiskPart only for that file. It neither stops other distros
nor forces sparse mode. If Windows still has the file open, it fails. Retry in
the next idle maintenance window after the file is released. Schedule this
maintenance together with the WSL runner launcher when activating the runner;
keep the Windows disk reserve check enabled between maintenance windows.

## Activated runners

The organization runner group `Universal Agents` selects approved repositories,
and installed hooks refuse fork PRs even if a workflow requests a local runner.
Four runners, one each for Windows and macOS plus two Linux runners in the
dedicated WSL distribution, use
the reviewed labels and installed cleanup hooks. A maintainer administers them
through a delegated runner role while their base organization membership
remains `member`. Keep each host's runner names, accounts and folders in that
host's own notes, outside this repository.

The Mac uses a launch agent. Windows has `UA CI Windows runner` and
`UA CI Linux runner` logon tasks; the latter keeps the dedicated WSL distro
running alongside its systemd runner service. Its startup checks for overdue
weekly compaction when the distro is stopped. A continuously running distro
still needs an idle maintenance window for physical VHD compaction.

Windows hooks inherit `PSExecutionPolicyPreference=Bypass` from the runner's
`.env`. This changes the runner process and its children, not account-wide
PowerShell policy. GitHub's hook invocation does not supply an execution-policy
flag, so direct SSH tests must exercise the same environment.

Windows workflow shells execute GitHub's extensionless generated script with
`ScriptBlock::Create`; invoking its filename does not execute PowerShell code.
Interactive probe polling waits for both a completed task state and its final
result, since Task Scheduler updates those observations separately.

The Mac container builder uses the dedicated Colima profile `ua-ci`, with 4 CPUs,
8 GiB RAM, and a 100 GiB data disk. The runner launcher starts it before accepting
jobs. Install hooks with `--docker-host` set to that profile's socket and
`--colima-profile ua-ci` so cleanup trims reclaimed Docker blocks. The account's
default Colima profile and Docker context remain separate.

## Bounded CI monitoring and release builds

Native Windows jobs allow 90 minutes for debug tests, lint, popup checks and
the separate optimized application build. Those checks consumed the previous
45-minute budget before compilation of the optimized application could finish
on the shared host. Linux jobs keep their 45-minute limit. These are job
deadlines; every test and build remains required.

Use `.github/scripts/watch_ci.py` to monitor a commit without an interactive
assistant loop. It polls GitHub every 30 seconds, saves job logs once, writes
`summary.json`, and exits on success, failure, or its default 45-minute deadline.
Exit codes are 0 (success), 1 (failed job), 2 (deadline), and 3 (API error).
It neither retries jobs nor publishes releases. A timeout leaves running jobs
under their workflow deadlines; inspect the summary before deciding to cancel.

```bash
rtk proxy python3 .github/scripts/watch_ci.py --repo OWNER/REPOSITORY \
  --sha FULL_COMMIT_SHA --workflow "WORKFLOW NAME" --output /tmp/ua-ci-result
```

Repeat `--workflow` for every required workflow. Pass `--run RUN_ID` instead to
track a known release run. Keep this process outside the runner work directory;
job cleanup removes that directory. Review failures or the final summary instead
of repeatedly fetching logs. Remove downloaded evidence when it is no longer needed.

Documentation-only commits skip runtime test profiles and native desktop builds;
the documentation workflows still validate the guides. Dependency audits run for
dependency changes and weekly. Native AMD64/ARM64 runtime images build in parallel
on WSL 2 and the Mac. Desktop release tags promote five installers from successful
main-branch builds of the exact commit, verifying GitHub artifact digests. Tag only
after those builds succeed and before their three-day artifact retention expires;
the release fails immediately if any build is missing, failed, or unfinished.

The disposable Linux `setup-python` executable gets a relative library search
path through `patchelf` before creating virtual environments. This lets restricted
child processes load Python without `LD_LIBRARY_PATH`, preserving their environment
sanitization. Provision `patchelf` on the Linux runner; system Python runs the helper
and is never modified. The adjusted toolchain is reclaimed after the job.

## Bounded build caches and faster feedback

The installed hooks use pinned sccache 0.17.0 for Rust compiler output and retain
npm/pip download caches. npx execution environments and npm logs are discarded
after each job, including their generated executable symlinks. The provisioner verifies a pinned upstream archive
SHA-256 before installing the binary. Cargo target output, Python environments,
Docker configuration and credentials remain disposable. No remote cache service
or GitHub cache storage is used. Rust incremental compilation remains disabled.

`--cache-gib 20` limits the combined retained caches for both repositories on
one runner. `--cache-days 7` expires scopes unused for seven days. The hooks
check these limits at job admission and completion, evicting whole scopes in
least-recently-used order. They also evict caches when a filesystem falls below
the existing 30 GiB reserve, including the WSL backing drive. These are retention
limits, not a filesystem quota during an active job. Each compiler cache also
has sccache's 3 GiB limit. Expiry is enforced when the runner next handles work.

Repositories and refs use separate hashed cache directories. Only main pushes
and manual main builds share the main cache. Pull requests, feature branches,
and release tags have separate scopes and never populate main's cache. There
is one cache root per runner, with an ownership marker; unknown directories,
symlinks and junctions are rejected before eviction. Never share this directory
between runner processes or remove its ownership marker.

Each job starts a dedicated compiler-cache server and stops it before cleanup
or eviction. Unix hosts use a socket inside job state. Windows reserves TCP
port 48227 for this runner; change `--cache-server-port` if that port is already
assigned. An occupied endpoint fails admission. No personal sccache server is
used or stopped. The completed hook prints cache statistics into the job log.
Local runner preflight proves an actual cache hit after deleting compiler output.

Desktop pull requests still test and compile all platforms, including both
macOS architectures, but omit installer packaging and bundle upload. Pushes
retain installer production for exact-commit release promotion. Manual desktop
workflows expose a `package` boolean, default true; select false for compilation
and tests without installers. Cold and warm runs should be compared separately;
compiler caches do not cache final linking or guarantee a particular duration.
