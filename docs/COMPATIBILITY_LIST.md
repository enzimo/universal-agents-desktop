# UA compatibility list (maintainers)

This page explains how `compatible-runtimes.json` is maintained and checked.
It is for people who maintain this repository and its CI, not for launcher users.

## What the list is

`compatible-runtimes.json` at the repository root lists every Universal Agents
(UA) release the launcher may offer. The launcher reads the copy on `main`, and
offers a newer listed UA version when that entry's `host_contract_version`
equals its own. The host contract is the set of assumptions the launcher makes
about the UA container: how `/data` is laid out, how the container starts, and
how subscription logins are imported and tested.

```json
{
  "schema_version": 1,
  "runtimes": [
    {
      "ua_version": "0.9.4",
      "host_contract_version": 3,
      "image": "ghcr.io/enzimo/universal-agents@sha256:e138...0131"
    }
  ]
}
```

Rules, all enforced by the check below:

- `schema_version` is `1`. Each entry has exactly the keys `ua_version`,
  `host_contract_version` and `image`, in that order.
- `ua_version` is a version such as `0.9.0` or `1.0.0-rc.1`, without a leading
  `v` and without build metadata (`+...`).
- `host_contract_version` is a positive whole number.
- `image` is in `ghcr.io/enzimo/universal-agents` and pinned by
  `@sha256:` and 64 lowercase hex digits. Tags such as `:0.9.0` are not allowed.
- Entries are in ascending version order, with no repeated version or image.
- The list is append-only: existing entries never change, move or disappear.
  The one exception is an entry whose image no longer exists in the registry,
  because the launcher could offer it but never download it. A person removes
  such entries in a reviewed pull request and merges it by hand, since the
  check rejects the removal.
- The file is 2-space indented JSON with one trailing newline, exactly as
  Python's `json.dumps(document, indent=2) + "\n"` writes it. One fixed layout
  keeps every change a pure append in the diff.

## How UA releases add themselves

Each stable UA release opens a pull request here from a branch named
`ua-release/v{version}`. The pull request appends one entry. UA creates it with
its `DESKTOP_COMPATIBILITY_TOKEN` secret, a personal token. UA's built-in
workflow token cannot write to this repository. GitHub also starts no workflows
for events caused by a built-in workflow token. With a personal token, the pull
request is an ordinary user's, so the check below runs.

## What the check verifies

The **Compatibility list** workflow checks new entries on native Linux AMD64
and ARM64 Docker daemons. It also tests the newest listed image when the
checker, workflow, smoke tests or `runtime-lock.json` changes. Both native
checks must pass before the merge job runs.

1. The proposed list has the schema and fixed layout described above.
2. The change is append-only; earlier entries keep their identity and order.
3. The UA release and tag exist. Each new entry's immutable image exactly
   matches `container-image.txt` from that release. A mismatched image is never
   executed.
4. `scripts/runtime_contract.py` pulls that exact image and exercises the
   current `runtime-lock.json` against it:
   - UA's own instance layout resolves the locked configuration, credential
     store and shared workspace paths.
   - The locked sign-in import works before onboarding, leaves its read-only
     source untouched, writes a private store, and preserves that store when
     malformed or incomplete credentials are rejected. The real onboarding
     provider test resolves the imported credentials with only its network
     transport replaced.
   - The image's ordinary `run` command starts browser onboarding on the
     locked web port with the locked health and setup routes.
   - A synthetic local configuration starts the configured runtime, chat and
     documentation routes, then restarts with the same data. Its configuration,
     imported credentials and shared workspace content survive.
5. Negative tests deliberately change the workspace path, import command and
   startup command. The published image must reject each for the expected reason.

One-shot test containers have no network access. The running service uses a
disposable standard Docker bridge and publishes both locked
ports only on host loopback. Health, browser routes and the broker connection
are tested from the host, as desktop uses them. They receive disposable named
volumes and synthetic credentials, never the runner's sign-in, Docker socket,
real data or an external model key. The synthetic source is written over stdin
into its own Docker volume, so a remote daemon or restricted Colima profile
needs no host folder share. Only import helpers receive a read-only view of
that source. Desktop's exact host-file bind arguments and file-sharing behavior
keep their core and platform integration checks. All containers and volumes are
removed, including on failure; job ownership labels also let the managed runner
recover after an interruption. After its unit tests, the compatibility job upgrades only its own
registered cleanup hook to this checkout's reviewed copy, keeping a previous
copy. That hook removes exact-job-label named volumes and networks as well as
containers; other runner registrations and resources are untouched. A cleanup failure fails the check.

This replaces the old git-blob comparison. Changes to build instructions,
comments or internal implementations can pass as long as the actual launcher
requirements still work. Fixture preparation uses UA's layout and onboarding
helpers, and the provider probe replaces its network transport. A rename of
those test dependencies may require a reviewed fixture update even when the
launcher still works; a failing probe is never silently skipped. Tests cannot prove every UA workflow: model quality,
real subscription sign-in and graphical desktop behavior keep their separate
runtime and desktop testing requirements.

Before reading releases, the checker verifies access to the UA repository.
Missing credentials, failed pulls, timeouts and failed runtime behavior are
errors that keep the entry open. There is no source-hash shortcut or successful
result when Docker is unavailable.

| Exit code | Meaning |
| --- | --- |
| 0 | List, release identity and runtime checks passed. |
| 1 | Invalid list, release or image, a GitHub/Docker error, or a failed runtime check. Fix it before merging. |
| 2 | The entry uses a host contract this launcher cannot test. Update the launcher and its reviewed test first. |

Reports appear in each native check's workflow summary.

## When it merges automatically

The workflow's merge job squash-merges the pull request and deletes its branch
only when all of these are true:

- the check job passed;
- the head branch starts with `ua-release/`;
- the pull request comes from this repository, not a fork;
- the pull request is open and not a draft;
- according to GitHub's list of changed files, the only file changed is
  `compatible-runtimes.json`, and nothing was renamed onto it;
- the pull request's head is still the commit that was checked. The merge uses
  `--match-head-commit`, so a later push cannot be merged unchecked.

Failed or cancelled checks and branches outside `ua-release/` skip the merge
job and leave the pull request open. Check failures explain their reasons in
the native job summaries. The merge job reports any remaining refusal, such as
additional changed files or a moved head. It does not use an unconditional job
that could hold the workflow's concurrency slot after cancellation.

The workflow performs the exact-head, list-only merge after its native checks.
It uses the workflow's own token, so the merge commit does not start other
workflows. The CI and native-build workflows ignore list-only changes anyway.
Squash merging must stay enabled in the repository settings.

## When a person must decide

- A new host contract needs a reviewed launcher change and a corresponding
  runtime test. The checker never assigns an unsupported contract automatically.
- A runtime test fails. Diagnose its report before approving the release;
  changing an internal source file alone is no longer a reason to stop.
- A pull request changes anything besides the compatibility list, or does not
  come from a `ua-release/` branch. It needs normal review even if tests pass.

A previous UA tag is not required. Each proposed image proves its own behavior,
so deleting historical tags cannot prevent a compatible new release from being
approved.

## GitHub access

`UA_READ_TOKEN` remains a fine-grained Actions secret with read-only Contents
access to the private UA repository. The checker uses it only for release and
tag reads. UA's `DESKTOP_COMPATIBILITY_TOKEN` remains the token that opens
release entries in this repository.

Image pulls use this repository's short-lived `GITHUB_TOKEN`, with
`packages: read`. In the UA container package's settings, **Manage Actions
access** must grant this desktop repository **Read** access. That package
setting is necessary for cross-repository pulls; read-only Contents access on
a fine-grained token does not grant registry access. See
[GitHub's package permissions](https://docs.github.com/en/packages/learn-github-packages/about-permissions-for-github-packages).

The managed runner gives each job a private `DOCKER_CONFIG`. The workflow
stores the registry login there without a host credential helper, and the
runner deletes it at job completion. No new long-lived registry secret is
needed. A refused package pull fails the check with the registry error.

## Running the check locally

Python's standard library, RTK, GitHub CLI, Docker and access to the private
image are required. Sign Docker in using your own approved registry access.

```sh
# Check a proposed append against main:
rtk proxy python3 scripts/check_compatibility_list.py --base-ref origin/main

# Exercise the newest listed image too, as CI does for checker-only changes:
rtk proxy python3 scripts/check_compatibility_list.py --base-ref origin/main --verify-current

# Run the same runtime checks directly for a listed version:
rtk proxy python3 tests/runtime_import_smoke.py 0.9.5

# Script unit tests without Docker or network access:
rtk proxy python3 -m unittest discover -s tests -p 'test_*.py' -v
```

`--contract` selects another reviewed runtime lock for testing;
`--ua-repository` selects the release source. The old `--contract-files` option
and `compatibility/contract-files.txt` have been removed.
