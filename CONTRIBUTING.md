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
- Commit with your GitHub noreply address
  (`git config user.email <id>+<login>@users.noreply.github.com`) and turn on
  GitHub's "Keep my email addresses private" and "Block command line pushes
  that expose my email" settings. Check with
  `git log --format='%an <%ae> | %cn <%ce>'` before pushing.
- Organization and repository names used in real addresses (download links,
  image names, the license holder) are public by design and may stay.

`tests/test_no_personal_details.py` fails when a tracked file contains an email
address outside the reserved example domains, a phone number outside the
fictional 555 ranges, or a home folder path with a real account name. The
**Personal details** workflow runs it on every push and pull request, including
documentation-only changes. It cannot recognize names, handles, machine names or
copied chats without listing them, so those still depend on review.
