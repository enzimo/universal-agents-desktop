#!/usr/bin/env python3
"""Verify a change to compatible-runtimes.json before it may be merged.

The launcher offers users any listed UA version whose host contract matches its
own, so an entry here is a promise that the launcher can run that image. This
script compares the list on the base branch with the proposed list and:

1. Validates the proposed file (format, fields, image pinning, order, duplicates).
2. Requires the change to be append-only: existing entries stay unchanged and in
   place, and new entries are only added at the end.
3. For each new entry, checks the image digest against the UA release's
   container-image.txt, then exercises the published image with runtime-lock.json.
   Internal source changes do not require approval when those behaviors pass.

Exit codes: 0 = every check passed; 1 = invalid list, release, image, or failed
runtime test; 2 = the entry names a host contract this launcher cannot test.

Every GitHub call goes through run_gh() so tests can replace it. gh reads its
token from GH_TOKEN or its stored login; this script never prints a token.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass, field
import json
from pathlib import Path
import re
import subprocess
import sys
from typing import Callable
import urllib.parse

from runtime_contract import ContractError, SUPPORTED_CONTRACT, load_contract, verify_image

REPO_ROOT = Path(__file__).resolve().parents[1]
LIST_FILE = "compatible-runtimes.json"
DEFAULT_CONTRACT = REPO_ROOT / "runtime-lock.json"
UA_REPOSITORY = "enzimo/universal-agents"
IMAGE_REPOSITORY = "ghcr.io/enzimo/universal-agents"
RELEASE_ASSET = "container-image.txt"
IMMUTABLE_IMAGE_PREFIX = "Immutable image:"
SCHEMA_VERSION = 1
TOP_LEVEL_KEYS = ("schema_version", "runtimes")
ENTRY_KEYS = ("ua_version", "host_contract_version", "image")

EXIT_PASSED = 0
EXIT_REJECTED = 1
EXIT_NEEDS_REVIEW = 2

PASS = "PASS"
FAIL = "FAIL"
REVIEW = "NEEDS REVIEW"
INFO = "INFO"

_IDENT = r"(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)"
# Semantic version without a leading "v" or build metadata (build metadata does
# not affect ordering, so it would allow two entries that sort as equal).
SEMVER = re.compile(
    rf"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-({_IDENT}(?:\.{_IDENT})*))?$")
PINNED_IMAGE = re.compile(rf"^{re.escape(IMAGE_REPOSITORY)}@sha256:[0-9a-f]{{64}}$")


@dataclass
class GhResult:
    returncode: int
    stdout: str
    stderr: str


GhRunner = Callable[[list[str]], GhResult]


def run_gh(args: list[str]) -> GhResult:
    """Run one GitHub CLI command. The only place this script talks to GitHub."""
    try:
        done = subprocess.run(["rtk", "proxy", "gh", *args], capture_output=True,
                              text=True, timeout=60, check=False)
    except FileNotFoundError as error:
        return GhResult(127, "", f"cannot run {error.filename}; install RTK and the GitHub CLI")
    except subprocess.TimeoutExpired:
        return GhResult(124, "", "GitHub CLI timed out after 60 seconds")
    return GhResult(done.returncode, done.stdout, done.stderr)


class GhError(Exception):
    """A GitHub request failed for a reason other than 'not found'."""


def _error_summary(text: str) -> str:
    lines = [line.strip() for line in text.strip().splitlines() if line.strip()]
    return lines[-1][:300] if lines else "no error output"


def _not_found(result: GhResult) -> bool:
    return result.returncode != 0 and "HTTP 404" in result.stderr


class UaSource:
    """Read-only questions about published UA releases and tags."""

    def __init__(self, gh: GhRunner, repository: str = UA_REPOSITORY):
        self.gh = gh
        self.repository = repository

    def check_access(self) -> None:
        # A private repository answers 404 to a token without access, which would
        # otherwise look like "file missing". Probe access once, up front.
        result = self.gh(["api", f"repos/{self.repository}", "--jq", ".full_name"])
        if result.returncode != 0:
            raise GhError(f"cannot read the UA repository {self.repository} "
                          f"({_error_summary(result.stderr)}). The GitHub token needs read "
                          "access to its contents and releases (UA_READ_TOKEN in CI).")

    def release_images(self, version: str) -> list[str]:
        tag = f"v{version}"
        result = self.gh(["release", "download", tag, "--repo", self.repository,
                          "--pattern", RELEASE_ASSET, "--output", "-"])
        if result.returncode != 0:
            raise GhError(f"cannot download {RELEASE_ASSET} from UA release {tag} "
                          f"({_error_summary(result.stderr)})")
        return [line.split(":", 1)[1].strip() for line in result.stdout.splitlines()
                if line.startswith(IMMUTABLE_IMAGE_PREFIX)]

    def tag_exists(self, tag: str) -> bool:
        result = self.gh(["api", f"repos/{self.repository}/git/ref/tags/{urllib.parse.quote(tag)}",
                          "--jq", ".ref"])
        if _not_found(result):
            return False
        if result.returncode != 0:
            raise GhError(f"cannot look up UA tag {tag} ({_error_summary(result.stderr)})")
        return True

    def require_tag(self, tag: str) -> None:
        if not self.tag_exists(tag):
            raise GhError(f"UA tag {tag} does not exist")



@dataclass
class Report:
    lines: list[tuple[str, str]] = field(default_factory=list)

    def add(self, status: str, message: str) -> None:
        self.lines.append((status, message))

    def has(self, status: str) -> bool:
        return any(entry_status == status for entry_status, _ in self.lines)

    @property
    def exit_code(self) -> int:
        if self.has(FAIL):
            return EXIT_REJECTED
        if self.has(REVIEW):
            return EXIT_NEEDS_REVIEW
        return EXIT_PASSED

    def render(self) -> str:
        width = max(len(status) for status in (PASS, FAIL, REVIEW, INFO))
        body = [f"{status:<{width}}  {message}" for status, message in self.lines]
        body.append("")
        body.append(VERDICTS[self.exit_code])
        return "\n".join(body) + "\n"


VERDICTS = {
    EXIT_PASSED: "Result: every check passed. The change is safe to merge.",
    EXIT_REJECTED: ("Result: the change is rejected. Fix the problems marked FAIL; "
                    "do not merge it as it is."),
    EXIT_NEEDS_REVIEW: (
        "Result: the list is valid, but a person must decide. If the launcher still "
        "works with this UA version, review and merge the entry by hand. If UA changed "
        "the host contract, update the launcher and its runtime test; do not approve "
        "an entry under an unsupported contract."),
}


def semver_key(version: str) -> tuple:
    match = SEMVER.match(version)
    if not match:
        raise ValueError(f"not a semantic version: {version}")
    major, minor, patch, prerelease = match.groups()
    if prerelease is None:
        # A stable release sorts after every prerelease of the same version.
        return (int(major), int(minor), int(patch), 1, ())
    parts = tuple((0, int(part), "") if part.isdigit() else (1, 0, part)
                  for part in prerelease.split("."))
    return (int(major), int(minor), int(patch), 0, parts)


def _reject_duplicate_keys(pairs: list[tuple[str, object]]) -> dict:
    keys = [key for key, _ in pairs]
    duplicates = sorted({key for key in keys if keys.count(key) > 1})
    if duplicates:
        raise ValueError(f"duplicate JSON keys: {', '.join(duplicates)}")
    return dict(pairs)


def canonical_text(document: dict) -> str:
    return json.dumps(document, indent=2) + "\n"


def parse_list(text: str, label: str) -> tuple[dict | None, list[str]]:
    """Parse and validate a list document. Returns (document, problems)."""
    try:
        document = json.loads(text, object_pairs_hook=_reject_duplicate_keys)
    except (json.JSONDecodeError, ValueError) as error:
        return None, [f"{label}: is not valid JSON: {error}"]
    problems = validate_document(document)
    if not problems and text != canonical_text(document):
        # One fixed layout keeps diffs to a pure append and lets UA's release
        # automation and people produce identical files.
        problems.append("is not in the standard layout: 2-space indented JSON, keys in the "
                        "documented order, one trailing newline")
    return document, [f"{label}: {problem}" for problem in problems]


def validate_document(document: object) -> list[str]:
    if not isinstance(document, dict):
        return ["the top level must be a JSON object"]
    problems = []
    if tuple(document) != TOP_LEVEL_KEYS:
        problems.append(f"the top level must have exactly the keys {', '.join(TOP_LEVEL_KEYS)} "
                        f"in that order (found: {', '.join(document) or 'none'})")
    schema = document.get("schema_version")
    if type(schema) is not int or schema != SCHEMA_VERSION:
        problems.append(f"schema_version must be {SCHEMA_VERSION} (found: {schema!r})")
    runtimes = document.get("runtimes")
    if not isinstance(runtimes, list):
        problems.append("runtimes must be a list")
        return problems
    for index, entry in enumerate(runtimes, start=1):
        problems.extend(f"entry {index}: {problem}" for problem in validate_entry(entry))
    if problems:
        return problems
    seen_versions: dict[str, int] = {}
    seen_images: dict[str, int] = {}
    for index, entry in enumerate(runtimes, start=1):
        version, image = entry["ua_version"], entry["image"]
        if version in seen_versions:
            problems.append(f"entry {index}: version {version} is already listed "
                            f"as entry {seen_versions[version]}")
        if image in seen_images:
            problems.append(f"entry {index}: image {image} is already listed "
                            f"as entry {seen_images[image]}")
        seen_versions.setdefault(version, index)
        seen_images.setdefault(image, index)
        if index > 1:
            previous = runtimes[index - 2]["ua_version"]
            if version != previous and semver_key(version) <= semver_key(previous):
                problems.append(f"entry {index}: version {version} must sort after "
                                f"{previous}; entries must be in ascending version order")
    return problems


def validate_entry(entry: object) -> list[str]:
    if not isinstance(entry, dict):
        return ["must be a JSON object"]
    problems = []
    if tuple(entry) != ENTRY_KEYS:
        problems.append(f"must have exactly the keys {', '.join(ENTRY_KEYS)} in that order "
                        f"(found: {', '.join(entry) or 'none'})")
    version = entry.get("ua_version")
    if not isinstance(version, str) or not SEMVER.match(version):
        problems.append(f"ua_version must be a version such as 1.2.3 or 1.2.3-rc.1, "
                        f"without a leading v (found: {version!r})")
    contract = entry.get("host_contract_version")
    if type(contract) is not int or contract < 1:
        problems.append(f"host_contract_version must be a positive whole number "
                        f"(found: {contract!r})")
    image = entry.get("image")
    if not isinstance(image, str) or not PINNED_IMAGE.match(image):
        problems.append(f"image must be {IMAGE_REPOSITORY}@sha256: followed by 64 lowercase "
                        f"hex digits (found: {image!r})")
    return problems


def _describe(entry: dict) -> str:
    return (f"{entry['ua_version']} (contract {entry['host_contract_version']}, "
            f"{entry['image'].rsplit('@', 1)[1][:19]}...)")


def check_append_only(base: list[dict], head: list[dict], report: Report) -> list[dict] | None:
    """Return the appended entries, or None after reporting a non-append edit."""
    problems = []
    for index, old in enumerate(base, start=1):
        if index > len(head):
            problems.append(f"entry {index}, {_describe(old)}, was removed")
        elif head[index - 1] != old:
            problems.append(f"entry {index} was {_describe(old)} on the base branch but is now "
                            f"{_describe(head[index - 1])}")
    if problems:
        for problem in problems:
            report.add(FAIL, f"Append-only: {problem}. Existing entries must stay unchanged "
                             "and in the same order; add new versions only at the end.")
        return None
    added = head[len(base):]
    report.add(PASS, f"Append-only: all {len(base)} existing entries are unchanged; "
                     f"{len(added)} new entr{'y' if len(added) == 1 else 'ies'} added at the end.")
    return added


def check_image(entry: dict, ua: UaSource, report: Report) -> bool:
    version, image = entry["ua_version"], entry["image"]
    try:
        published = ua.release_images(version)
    except GhError as error:
        report.add(FAIL, f"{version} image: {error}")
        return False
    if len(published) != 1:
        report.add(FAIL, f"{version} image: UA release v{version}'s {RELEASE_ASSET} has "
                         f"{len(published)} '{IMMUTABLE_IMAGE_PREFIX}' lines; expected exactly one")
    elif published[0] != image:
        report.add(FAIL, f"{version} image: the list says {image} but UA release v{version} "
                         f"published {published[0]}")
    else:
        report.add(PASS, f"{version} image: matches the immutable image published with "
                         f"UA release v{version}.")
        return True
    return False


def check_runtime(entry: dict, contract: dict, report: Report,
                  probe: Callable[[dict, str], None]) -> None:
    version, wanted = entry["ua_version"], entry["host_contract_version"]
    if wanted != contract["host_contract_version"] or wanted != SUPPORTED_CONTRACT:
        report.add(REVIEW, f"{version} runtime: no reviewed test implements host contract "
                           f"{wanted}. Update the launcher and its runtime test before approval.")
        return
    try:
        probe(contract, entry["image"])
    except ContractError as error:
        report.add(FAIL, f"{version} runtime: {error}")
    else:
        report.add(PASS, f"{version} runtime: published image passed host contract {wanted}: "
                         "data/workspace paths, sign-in import, onboarding, configured startup, "
                         "browser routes, restart and preservation.")


def check_change(base_text: str | None, head_text: str, contract: dict,
                 gh: GhRunner = run_gh, repository: str = UA_REPOSITORY,
                 probe: Callable[[dict, str], None] | None = None,
                 verify_current: bool = False) -> Report:
    report = Report()
    head, problems = parse_list(head_text, "Proposed list")
    if problems:
        for problem in problems:
            report.add(FAIL, problem)
        return report
    report.add(PASS, f"Proposed list: valid, {len(head['runtimes'])} entries in ascending "
                     "version order with pinned images.")
    if base_text is None:
        base_entries: list[dict] = []
        report.add(INFO, f"The base branch has no {LIST_FILE}; every entry is treated as new.")
    else:
        base, problems = parse_list(base_text, "Base-branch list")
        if problems:
            for problem in problems:
                report.add(FAIL, f"{problem}. Fix the list on the base branch by hand first.")
            return report
        base_entries = base["runtimes"]
    added = check_append_only(base_entries, head["runtimes"], report)
    if added is None:
        return report
    candidates = list(added)
    if verify_current:
        current = [entry for entry in head["runtimes"]
                   if entry["host_contract_version"] == contract["host_contract_version"]]
        if not current:
            report.add(FAIL, "No listed image exercises this launcher's runtime contract.")
            return report
        if current[-1] not in candidates:
            candidates.append(current[-1])
    if not candidates:
        return report
    ua = UaSource(gh, repository)
    try:
        ua.check_access()
    except GhError as error:
        report.add(FAIL, f"UA access: {error}")
        return report
    for entry in candidates:
        try:
            ua.require_tag(f"v{entry['ua_version']}")
        except GhError as error:
            report.add(FAIL, str(error))
            continue
        # Never execute an image whose identity failed release verification.
        if check_image(entry, ua, report):
            check_runtime(entry, contract, report, probe or verify_image)
    return report


def read_base_from_git(ref: str, head_path: Path) -> str | None:
    """Read the list at a git ref; None if the ref exists but has no list file."""
    repo = head_path.resolve().parent
    verify = subprocess.run(["git", "-C", str(repo), "rev-parse", "--verify", "--quiet",
                             f"{ref}^{{commit}}"], capture_output=True, text=True, check=False)
    if verify.returncode != 0:
        raise ValueError(f"git ref {ref} does not exist in {repo}")
    shown = subprocess.run(["git", "-C", str(repo), "show", f"{ref}:./{head_path.name}"],
                           capture_output=True, text=True, check=False)
    if shown.returncode != 0:
        if "does not exist" in shown.stderr or "exists on disk, but not in" in shown.stderr:
            return None
        raise ValueError(f"cannot read {head_path.name} at {ref}: {_error_summary(shown.stderr)}")
    return shown.stdout


def main(argv: list[str] | None = None, gh: GhRunner = run_gh) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    base_group = parser.add_mutually_exclusive_group(required=True)
    base_group.add_argument("--base", type=Path, help="the list as it is on the base branch")
    base_group.add_argument("--base-ref", help="read the base list from this git ref, "
                            "for example origin/main (a missing file counts as an empty list)")
    parser.add_argument("--head", type=Path, default=REPO_ROOT / LIST_FILE,
                        help="the proposed list (default: the repository's list file)")
    parser.add_argument("--contract", type=Path, default=DEFAULT_CONTRACT,
                        help="the reviewed launcher runtime-lock.json")
    parser.add_argument("--verify-current", action="store_true",
                        help="also exercise the newest listed image, including checker-only changes")
    parser.add_argument("--ua-repository", default=UA_REPOSITORY)
    args = parser.parse_args(argv)

    try:
        contract = load_contract(args.contract)
        head_text = args.head.read_text(encoding="utf-8")
        if args.base is not None:
            base_text = args.base.read_text(encoding="utf-8")
        else:
            base_text = read_base_from_git(args.base_ref, args.head)
    except (OSError, ValueError) as error:
        print(f"{FAIL}  Cannot start the check: {error}")
        print()
        print(VERDICTS[EXIT_REJECTED])
        return EXIT_REJECTED

    report = check_change(base_text, head_text, contract, gh, args.ua_repository,
                          verify_current=args.verify_current)
    sys.stdout.write(report.render())
    return report.exit_code


if __name__ == "__main__":
    sys.exit(main())
