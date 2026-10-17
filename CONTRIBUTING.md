# Contributing

Keep changes within the desktop-host boundary described in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md). Runtime behavior belongs in Universal Agents unless it is strictly host lifecycle, native integration, or presentation of host state.

Before opening a change:

1. Change `runtime-lock.json` only as a reviewed, intentional host-contract change. UA versions are added to `compatible-runtimes.json` by UA's release pull requests; see [Compatibility list](docs/COMPATIBILITY_LIST.md).
2. Add or update `ua-desktop-core` tests for lifecycle and ownership behavior.
3. Run formatting, Clippy with warnings denied, and the core test suite.
4. Let the macOS workflow compile and bundle the real app.
5. Synchronize affected docs and `ui/DESIGN.md`.

Prefer small functions with one owner. Keep command construction deterministic and observable. Any operation that could affect another Docker container must fail closed.

## Personal details

Everything tracked here is shared with everyone who can read the repository,
and Git history keeps old versions of every file and every commit's author.
Removing a personal detail later means recreating the repository. Keep these
out of code, tests, documentation, release notes, examples and commit metadata:

- Real names, nicknames, handles, email addresses, phone numbers and chat or
  account IDs of maintainers, users or their assistants. This includes the
  names people give their own assistant or bot, team, instance or company.
- Real domain names, server addresses, machine names, runner names, host
  accounts, hardware details and host folder paths from anyone's own setup.
- Links to individual GitHub Actions runs: their logs name the runner hosts.
- Copied chat messages, emails, reports or logs from a person's real work.

What to do instead:

- Use made-up placeholders: `exampleuser`, `example-owner`, `alice`, `bob`,
  `jordan`, the assistant name `Nova`, the company `Acme`; domains under
  `example.com`, `example.net` or `.test`; phone numbers in the `+1555` range;
  Telegram and Discord IDs such as `123456789` or `100200300`; sample paths
  such as `/Users/me/...` or `C:\Users\Jane Doe\...`.
- Describe your own machines generically (for example "one Windows host runs
  a Windows runner and a WSL runner") and keep their exact names, accounts and
  paths in the host's own notes outside this repository.
- Summarize a validation run in words instead of linking to it.
- Commit only with an identity listed in `.personal-details-allowlist`: your
  GitHub noreply address (`<id>+<login>@users.noreply.github.com`) with the
  matching login as the name. Turn on GitHub's "Keep my email addresses
  private" and "Block command line pushes that expose my email" settings.
- Organization and repository names used in real addresses (download links,
  image names, the license holder) are public by design and may stay.

### Automated checks

`scripts/check_personal_details.py` reports email addresses outside the
reserved example domains, phone numbers outside the fictional 555 ranges, home
folder paths with a real account name, and commit identities not listed in
`.personal-details-allowlist`. It checks tracked files and every commit: the
identity, the message and every added line, including lines a later commit
deleted. It needs only Python 3.9+ and Git. It cannot recognize names, handles,
machine names or copied chats without listing them, so those still depend on
review.

- Turn on the Git hooks once in every clone:
  `git config core.hooksPath .githooks`. They check staged files and the
  commit identity (pre-commit), the commit message (commit-msg), and every
  commit a push would upload (pre-push).
- The **Personal details** workflow runs the checker's tests and the full
  check on every push and pull request. Its log shows where a finding is but
  not the value.
- `.personal-details-allowlist` records this project's decisions: `identity:`
  lines for allowed commit identities, `text:` lines for real values that may
  appear, and `account:` lines for made-up account names in example paths.
  Fix a finding instead of adding it there.
- A detail that was already pushed stays in history after a new commit deletes
  it. Follow `docs/personal-details-cleanup.md`.
