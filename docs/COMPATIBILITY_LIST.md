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
      "ua_version": "0.8.5",
      "host_contract_version": 3,
      "image": "ghcr.io/enzimo/universal-agents@sha256:d5d4...0197"
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

The **Compatibility list** workflow (`.github/workflows/compatibility-list.yml`)
runs on pull requests that change the list, the contract file list, the
checker, or the workflow itself. It runs on the managed self-hosted Linux runner
and skips forks. It first runs the checker's unit tests. Then it runs
`scripts/check_compatibility_list.py`, which compares the list on the pull
request's exact head commit with the list on the base branch:

1. **The proposed list is valid**, following the rules above.
2. **The change is append-only.** Every base entry is unchanged and in place.
3. **Each new entry's image is the one UA published.** The checker downloads
   `container-image.txt` from UA release `v{ua_version}`. That file's
   `Immutable image:` line must equal the entry's `image` exactly.
4. **The UA files the launcher depends on did not change.** The checker finds
   the newest entry on the base branch with the same `host_contract_version`.
   It then compares every path in `compatibility/contract-files.txt` at UA tag
   `v{previous}` and tag `v{new}`, using git blob SHAs. A file missing on
   either side counts as changed. If tag `v{previous}` no longer exists, a
   person must compare the files instead.

Before it reads any UA data, the checker confirms the token can read the UA
repository. GitHub answers "not found" both for a missing file and for a private
repository the token cannot see. The early check keeps a token problem from
being reported as a changed contract file.

The checker prints one line per check and exits with:

| Exit code | Meaning |
| --- | --- |
| 0 | Every check passed. |
| 1 | Rejected: invalid list, a non-append edit, an image mismatch, a missing release or new-version tag, or a GitHub error. Fix the change; do not merge it. |
| 2 | The list is valid, but a person must decide (see below). |

Both non-zero results fail the workflow's check job. The report appears in the
workflow run's summary.

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

Otherwise the merge job leaves the pull request open and writes the reasons in
the run summary. GitHub's built-in auto-merge is not used because it needs branch
protection, which this private repository's plan does not offer. The merge
uses the workflow's own token, so the merge commit does not start other
workflows. The CI and native-build workflows ignore list-only changes anyway.
Squash merging must stay enabled in the repository settings.

## When a person must decide

- **Contract files changed.** The report names each file that changed between
  the two UA tags. Read UA's diff for those files.
  - If the launcher still works with the new release, test it as described in
    [Runtime integration](RUNTIME_INTEGRATION.md) and
    [`tests/runtime_import_smoke.py`](../tests/runtime_import_smoke.py). Then
    merge the pull request by hand. Later releases are compared against this
    newly listed version.
  - If UA changed the contract, do not merge the entry under the old contract
    number. The launcher needs an update that supports the new behavior and a
    new `host_contract_version`. Close the automated pull request, and add the
    entry with the new contract number in the launcher update's pull request.
- **No earlier entry uses that contract.** The first release under a new
  contract cannot be compared with anything. A person adds it, normally in the
  same change that teaches the launcher the new contract.
- **The previous version's UA tag no longer exists.** UA's repository was
  recreated in October 2026 without its old tags, so UA 0.9.3 and earlier have
  no tag to compare against. For the first release listed after that (UA
  0.9.4), compare the files in `compatibility/contract-files.txt` by hand with
  a saved copy of UA 0.9.3, then merge by hand. Later releases are compared
  against the newly listed version again.
- **The pull request is not from UA's release automation.** For example, a
  person edited the list or the contract file list. The check still runs and
  reports, but only a person merges.

If the launcher starts depending on another UA file, add it to
`compatibility/contract-files.txt` in a reviewed pull request.

## Secrets

| Secret | Repository | Purpose and permissions |
| --- | --- | --- |
| `UA_READ_TOKEN` | this repository | Lets the check read UA. Use a fine-grained personal access token whose resource owner is `enzimo`, limited to `enzimo/universal-agents`, with repository permission **Contents: Read-only**. GitHub adds **Metadata: Read-only** automatically. Contents covers file contents, tags, releases and release asset downloads, so no other permission is needed. |
| `DESKTOP_COMPATIBILITY_TOKEN` | `enzimo/universal-agents` | Lets UA's release workflow push a `ua-release/` branch here and open the pull request. It needs **Contents: Read and write** and **Pull requests: Read and write** on `enzimo/universal-agents-desktop`. It is maintained with UA's release workflow. |

If `UA_READ_TOKEN` is missing, the check job fails with a message naming it.
Fine-grained tokens expire. When one does, the check fails at the UA access step
until the token is replaced.

## Running the check locally

The checker uses only Python's standard library and calls GitHub through
`rtk proxy gh`, so it uses your GitHub CLI login. It only reads from GitHub.

```sh
# Compare the working copy with main, as CI does:
rtk proxy python3 scripts/check_compatibility_list.py --base-ref origin/main

# Or compare two files:
rtk proxy python3 scripts/check_compatibility_list.py --base /tmp/base.json --head /tmp/head.json

# Unit tests (fake GitHub layer, no network):
rtk proxy python3 -m unittest discover -s tests -p 'test_check_compatibility_list.py' -v
```

`--contract-files` and `--ua-repository` override the defaults for testing. If
the base ref has no list yet, every entry counts as new and needs a person's
review. This applies to the change that first adds the list to `main`.
