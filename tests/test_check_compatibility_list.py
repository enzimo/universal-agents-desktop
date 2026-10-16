"""Unit tests for scripts/check_compatibility_list.py with a fake GitHub CLI.

Run: python3 -m unittest discover -s tests -p 'test_check_compatibility_list.py' -v
"""
import copy
import importlib.util
import io
import json
import subprocess
import sys
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import urllib.parse

SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "check_compatibility_list.py"
spec = importlib.util.spec_from_file_location("check_compatibility_list", SCRIPT)
check = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = check  # dataclasses look the module up by name
spec.loader.exec_module(check)

REPO = check.UA_REPOSITORY
CONTRACT_FILES = ["runtime_settings/instance_layout.py", "docker/entrypoint.sh"]


def image(char: str) -> str:
    return f"{check.IMAGE_REPOSITORY}@sha256:{char * 64}"


def entry(version: str, digest_char: str, contract: int = 3) -> dict:
    return {"ua_version": version, "host_contract_version": contract, "image": image(digest_char)}


def text(*entries: dict) -> str:
    return check.canonical_text({"schema_version": 1, "runtimes": list(entries)})


class FakeGh:
    """Answers the few gh commands the script uses from in-memory UA data."""

    def __init__(self):
        self.accessible = True
        self.releases: dict[str, str] = {}  # tag -> container-image.txt
        self.files: dict[tuple[str, str], str] = {}  # (tag, path) -> blob sha
        self.calls: list[list[str]] = []

    def publish(self, version: str, published_image: str, shas: dict[str, str] | None = None):
        tag = f"v{version}"
        self.releases[tag] = (f"Release: {tag}\nImage: {check.IMAGE_REPOSITORY}:{version}\n"
                              f"Immutable image: {published_image}\n")
        for path in CONTRACT_FILES:
            self.files[(tag, path)] = (shas or {}).get(path, f"sha-of-{path}")

    def __call__(self, args: list[str]) -> check.GhResult:
        self.calls.append(args)
        not_found = check.GhResult(1, '{"message":"Not Found"}', "gh: Not Found (HTTP 404)")
        if not self.accessible:
            return not_found
        if args[:2] == ["release", "download"]:
            body = self.releases.get(args[2])
            return check.GhResult(0, body, "") if body else check.GhResult(1, "", "release not found")
        route = args[1]
        if route == f"repos/{REPO}":
            return check.GhResult(0, REPO + "\n", "")
        if route.startswith(f"repos/{REPO}/git/ref/tags/"):
            tag = urllib.parse.unquote(route.rsplit("/", 1)[1])
            return check.GhResult(0, f"refs/tags/{tag}\n", "") if tag in self.releases else not_found
        if route.startswith(f"repos/{REPO}/contents/"):
            path, query = route[len(f"repos/{REPO}/contents/"):].split("?ref=")
            sha = self.files.get((urllib.parse.unquote(query), urllib.parse.unquote(path)))
            if sha is None:
                return not_found
            return check.GhResult(0, json.dumps({"type": "file", "sha": sha, "path": path}), "")
        raise AssertionError(f"unexpected gh call: {args}")


class CheckTests(unittest.TestCase):
    def setUp(self):
        self.gh = FakeGh()
        self.gh.publish("0.8.5", image("a"))
        self.gh.publish("0.9.0", image("b"))
        self.base = text(entry("0.8.5", "a"))
        self.head = text(entry("0.8.5", "a"), entry("0.9.0", "b"))

    def run_check(self, base, head):
        report = check.check_change(base, head, CONTRACT_FILES, self.gh)
        return report, report.render()

    def assert_outcome(self, report, rendered, code, *fragments):
        self.assertEqual(report.exit_code, code, rendered)
        for fragment in fragments:
            self.assertIn(fragment, rendered)

    def test_valid_append_passes(self):
        report, rendered = self.run_check(self.base, self.head)
        self.assert_outcome(report, rendered, check.EXIT_PASSED,
                            "0.9.0 image: matches", "all 2 files are identical between UA v0.8.5 and v0.9.0")
        self.assertNotIn(check.FAIL, [status for status, _ in report.lines])

    def test_unchanged_list_passes_without_contacting_github(self):
        report, rendered = self.run_check(self.head, self.head)
        self.assert_outcome(report, rendered, check.EXIT_PASSED, "0 new entries")
        self.assertEqual(self.gh.calls, [])

    def test_digest_mismatch_fails(self):
        self.gh.publish("0.9.0", image("c"))
        report, rendered = self.run_check(self.base, self.head)
        self.assert_outcome(report, rendered, check.EXIT_REJECTED,
                            f"the list says {image('b')} but UA release v0.9.0 published {image('c')}")

    def test_missing_release_fails(self):
        del self.gh.releases["v0.9.0"]
        report, rendered = self.run_check(self.base, self.head)
        self.assert_outcome(report, rendered, check.EXIT_REJECTED, "release v0.9.0", "release not found")

    def test_changed_contract_file_needs_review(self):
        self.gh.publish("0.9.0", image("b"), {"docker/entrypoint.sh": "new-sha"})
        report, rendered = self.run_check(self.base, self.head)
        self.assert_outcome(report, rendered, check.EXIT_NEEDS_REVIEW,
                            "differ between UA v0.8.5 and v0.9.0: docker/entrypoint.sh (changed)",
                            "a person must decide")
        self.assertNotIn("instance_layout.py (changed)", rendered)

    def test_contract_file_missing_at_new_tag_needs_review(self):
        del self.gh.files[("v0.9.0", "runtime_settings/instance_layout.py")]
        report, rendered = self.run_check(self.base, self.head)
        self.assert_outcome(report, rendered, check.EXIT_NEEDS_REVIEW,
                            "runtime_settings/instance_layout.py (missing at v0.9.0)")

    def test_contract_file_missing_at_both_tags_needs_review(self):
        for tag in ("v0.8.5", "v0.9.0"):
            del self.gh.files[(tag, "docker/entrypoint.sh")]
        report, rendered = self.run_check(self.base, self.head)
        self.assert_outcome(report, rendered, check.EXIT_NEEDS_REVIEW,
                            "docker/entrypoint.sh (missing at v0.8.5 and v0.9.0)")

    def test_digest_mismatch_and_contract_change_reports_both_and_rejects(self):
        self.gh.publish("0.9.0", image("c"), {"docker/entrypoint.sh": "new-sha"})
        report, rendered = self.run_check(self.base, self.head)
        self.assert_outcome(report, rendered, check.EXIT_REJECTED, "published", "(changed)")

    def test_compares_with_newest_listed_version_of_same_contract(self):
        self.gh.publish("0.8.6", image("d"), {"docker/entrypoint.sh": "reviewed-sha"})
        self.gh.publish("0.9.0", image("b"), {"docker/entrypoint.sh": "reviewed-sha"})
        base = text(entry("0.8.5", "a"), entry("0.8.6", "d"))
        head = text(entry("0.8.5", "a"), entry("0.8.6", "d"), entry("0.9.0", "b"))
        report, rendered = self.run_check(base, head)
        self.assert_outcome(report, rendered, check.EXIT_PASSED, "between UA v0.8.6 and v0.9.0")

    def test_first_entry_of_new_contract_needs_review(self):
        head = text(entry("0.8.5", "a"), entry("0.9.0", "b", contract=4))
        report, rendered = self.run_check(self.base, head)
        self.assert_outcome(report, rendered, check.EXIT_NEEDS_REVIEW,
                            "no earlier listed version uses host contract 4")

    def test_missing_base_list_treats_every_entry_as_new(self):
        report, rendered = self.run_check(None, self.head)
        self.assert_outcome(report, rendered, check.EXIT_NEEDS_REVIEW,
                            "has no compatible-runtimes.json", "0.8.5 contract files: no earlier")

    def test_modified_entry_fails(self):
        changed = text(entry("0.8.5", "a", contract=4), entry("0.9.0", "b"))
        report, rendered = self.run_check(self.base, changed)
        self.assert_outcome(report, rendered, check.EXIT_REJECTED, "Append-only: entry 1 was 0.8.5")
        self.assertEqual(self.gh.calls, [])

    def test_removed_entry_fails(self):
        base = self.head
        head = text(entry("0.8.5", "a"))
        report, rendered = self.run_check(base, head)
        self.assert_outcome(report, rendered, check.EXIT_REJECTED, "entry 2, 0.9.0", "was removed")

    def test_new_entry_inserted_before_existing_fails(self):
        self.gh.publish("0.8.0", image("e"))
        base = text(entry("0.8.5", "a"))
        head = text(entry("0.8.0", "e"), entry("0.8.5", "a"))
        report, rendered = self.run_check(base, head)
        self.assert_outcome(report, rendered, check.EXIT_REJECTED, "Append-only: entry 1")

    def test_unsorted_entries_fail(self):
        head = text(entry("0.9.0", "b"), entry("0.8.5", "a"))
        report, rendered = self.run_check(None, head)
        self.assert_outcome(report, rendered, check.EXIT_REJECTED, "must sort after 0.9.0")

    def test_reordered_existing_entries_fail(self):
        base = self.head
        head = text(entry("0.9.0", "b"), entry("0.8.5", "a"))
        report, rendered = self.run_check(base, head)
        self.assert_outcome(report, rendered, check.EXIT_REJECTED, "ascending version order")

    def test_duplicate_version_fails(self):
        head = text(entry("0.8.5", "a"), entry("0.8.5", "b"))
        report, rendered = self.run_check(self.base, head)
        self.assert_outcome(report, rendered, check.EXIT_REJECTED, "version 0.8.5 is already listed")

    def test_duplicate_image_fails(self):
        head = text(entry("0.8.5", "a"), entry("0.9.0", "a"))
        report, rendered = self.run_check(self.base, head)
        self.assert_outcome(report, rendered, check.EXIT_REJECTED, "is already listed as entry 1")

    def test_bad_images_fail(self):
        digest = "b" * 64
        for bad in (f"ghcr.io/someone/else@sha256:{digest}",
                    f"{check.IMAGE_REPOSITORY}:0.9.0",
                    f"{check.IMAGE_REPOSITORY}@sha256:{'b' * 63}",
                    f"{check.IMAGE_REPOSITORY}@sha256:{'B' * 64}",
                    f"{check.IMAGE_REPOSITORY}:0.9.0@sha256:{digest}"):
            with self.subTest(image=bad):
                bad_entry = entry("0.9.0", "b") | {"image": bad}
                report, rendered = self.run_check(self.base, text(entry("0.8.5", "a"), bad_entry))
                self.assert_outcome(report, rendered, check.EXIT_REJECTED, "entry 2: image must be")

    def test_bad_entry_fields_fail(self):
        cases = {
            "leading v": entry("v0.9.0", "b"),
            "build metadata": entry("0.9.0+build.1", "b"),
            "partial version": entry("0.9", "b"),
            "zero contract": entry("0.9.0", "b", contract=0),
            "boolean contract": entry("0.9.0", "b") | {"host_contract_version": True},
            "string contract": entry("0.9.0", "b") | {"host_contract_version": "3"},
            "extra key": entry("0.9.0", "b") | {"notes": "x"},
        }
        missing = entry("0.9.0", "b")
        del missing["image"]
        cases["missing key"] = missing
        for name, bad_entry in cases.items():
            with self.subTest(name):
                report, rendered = self.run_check(self.base, text(entry("0.8.5", "a"), bad_entry))
                self.assert_outcome(report, rendered, check.EXIT_REJECTED, "entry 2:")

    def test_bad_documents_fail(self):
        cases = {
            "schema version": check.canonical_text({"schema_version": 2, "runtimes": []}),
            "boolean schema": check.canonical_text({"schema_version": True, "runtimes": []}),
            "extra top-level key": check.canonical_text({"schema_version": 1, "runtimes": [], "x": 1}),
            "not json": "{",
            "duplicate keys": '{"schema_version": 1, "schema_version": 1, "runtimes": []}\n',
            "compact layout": json.dumps({"schema_version": 1, "runtimes": [entry("0.8.5", "a")]}) + "\n",
            "no trailing newline": self.head.rstrip("\n"),
        }
        for name, head in cases.items():
            with self.subTest(name):
                report, rendered = self.run_check(self.base, head)
                self.assert_outcome(report, rendered, check.EXIT_REJECTED, "Proposed list:")

    def test_prerelease_ordering(self):
        self.gh.publish("1.0.0-rc.1", image("c"))
        self.gh.publish("1.0.0", image("d"))
        ordered = [entry("0.9.0", "b"), entry("1.0.0-alpha", "e"), entry("1.0.0-alpha.1", "f"),
                   entry("1.0.0-rc.1", "c"), entry("1.0.0-rc.2", "1"), entry("1.0.0", "d")]
        self.assertEqual(check.validate_document({"schema_version": 1, "runtimes": ordered}), [])
        reversed_pair = [entry("1.0.0", "d"), entry("1.0.0-rc.1", "c")]
        self.assertTrue(check.validate_document({"schema_version": 1, "runtimes": reversed_pair}))
        self.assertLess(check.semver_key("1.0.0-rc.9"), check.semver_key("1.0.0-rc.10"))
        self.assertLess(check.semver_key("1.0.0-9"), check.semver_key("1.0.0-alpha"))

    def test_invalid_base_list_fails(self):
        report, rendered = self.run_check("not json", self.head)
        self.assert_outcome(report, rendered, check.EXIT_REJECTED, "Base-branch list: is not valid JSON")

    def test_unreadable_ua_repository_fails_without_blaming_files(self):
        self.gh.accessible = False
        report, rendered = self.run_check(self.base, self.head)
        self.assert_outcome(report, rendered, check.EXIT_REJECTED,
                            "cannot read the UA repository", "UA_READ_TOKEN")
        self.assertNotIn("(missing at", rendered)

    def test_missing_previous_tag_needs_review(self):
        del self.gh.releases["v0.8.5"]
        report, rendered = self.run_check(self.base, self.head)
        self.assert_outcome(report, rendered, check.EXIT_NEEDS_REVIEW,
                            "0.9.0 image: matches",
                            "UA tag v0.8.5 of the previous listed version 0.8.5 no longer exists",
                            "compare the contract files with a saved copy")
        self.assertNotIn(check.FAIL, [status for status, _ in report.lines])
        self.assertFalse(any(args[1].startswith(f"repos/{REPO}/contents/") for args in self.gh.calls
                             if len(args) > 1))

    def test_missing_new_tag_fails(self):
        real = self.gh.__call__

        def without_new_tag(args):
            if args[1:2] == [f"repos/{REPO}/git/ref/tags/v0.9.0"]:
                return check.GhResult(1, '{"message":"Not Found"}', "gh: Not Found (HTTP 404)")
            return real(args)

        report = check.check_change(self.base, self.head, CONTRACT_FILES, without_new_tag)
        self.assertEqual(report.exit_code, check.EXIT_REJECTED)
        self.assertIn("UA tag v0.9.0 does not exist", report.render())

    def test_tag_lookup_errors_fail_instead_of_counting_as_missing(self):
        real = self.gh.__call__

        def flaky(args):
            if args[1:2] == [f"repos/{REPO}/git/ref/tags/v0.8.5"]:
                return check.GhResult(1, "", "gh: Server Error (HTTP 502)")
            return real(args)

        report = check.check_change(self.base, self.head, CONTRACT_FILES, flaky)
        self.assertEqual(report.exit_code, check.EXIT_REJECTED)
        self.assertIn("cannot look up UA tag v0.8.5", report.render())

    def test_other_api_errors_fail_instead_of_counting_as_changed(self):
        real = self.gh.__call__

        def flaky(args):
            if args[1].startswith(f"repos/{REPO}/contents/docker/"):
                return check.GhResult(1, "", "gh: Server Error (HTTP 502)")
            return real(args)

        report = check.check_change(self.base, self.head, CONTRACT_FILES, flaky)
        self.assertEqual(report.exit_code, check.EXIT_REJECTED)
        self.assertIn("HTTP 502", report.render())


class ContractFileListTests(unittest.TestCase):
    def write(self, content):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        path = Path(directory.name) / "contract-files.txt"
        path.write_text(content, encoding="utf-8")
        return path

    def test_repository_list_is_valid(self):
        paths = check.load_contract_files(check.DEFAULT_CONTRACT_FILES)
        self.assertIn("docker/entrypoint.sh", paths)
        self.assertEqual(len(paths), 6)

    def test_comments_and_blank_lines_are_ignored(self):
        self.assertEqual(check.load_contract_files(self.write("# note\n\n a/b.py \n")), ["a/b.py"])

    def test_bad_lists_are_rejected(self):
        for content in ("# nothing\n", "/abs.py\n", "a/../b.py\n", "a.py\na.py\n", "a\\b.py\n"):
            with self.subTest(content=content):
                with self.assertRaises(ValueError):
                    check.load_contract_files(self.write(content))


class CommandLineTests(unittest.TestCase):
    def test_file_interface_and_exit_code(self):
        gh = FakeGh()
        gh.publish("0.8.5", image("a"))
        gh.publish("0.9.0", image("c"))
        with tempfile.TemporaryDirectory() as directory:
            base, head = Path(directory, "base.json"), Path(directory, "head.json")
            base.write_text(text(entry("0.8.5", "a")), encoding="utf-8")
            head.write_text(text(entry("0.8.5", "a"), entry("0.9.0", "b")), encoding="utf-8")
            with patch("sys.stdout", new_callable=io.StringIO) as out:
                code = check.main(["--base", str(base), "--head", str(head)], gh=gh)
        self.assertEqual(code, check.EXIT_REJECTED)
        self.assertIn("the change is rejected", out.getvalue())

    def test_missing_base_file_is_an_error(self):
        with patch("sys.stdout", new_callable=io.StringIO) as out:
            code = check.main(["--base", "/nonexistent/base.json"], gh=FakeGh())
        self.assertEqual(code, check.EXIT_REJECTED)
        self.assertIn("Cannot start the check", out.getvalue())

    def git_repo(self, directory: str, base_content: str | None) -> Path:
        def git(*args):
            subprocess.run(["git", "-C", directory, *args], check=True, capture_output=True)
        git("init", "--quiet", "--initial-branch=main")
        git("config", "user.email", "test@example.invalid")
        git("config", "user.name", "Test")
        head = Path(directory, check.LIST_FILE)
        Path(directory, "README").write_text("x\n", encoding="utf-8")
        if base_content is not None:
            head.write_text(base_content, encoding="utf-8")
        git("add", ".")
        git("commit", "--quiet", "-m", "base")
        return head

    def test_base_ref_reads_the_list_from_git(self):
        gh = FakeGh()
        gh.publish("0.8.5", image("a"))
        gh.publish("0.9.0", image("b"))
        with tempfile.TemporaryDirectory() as directory:
            head = self.git_repo(directory, text(entry("0.8.5", "a")))
            head.write_text(text(entry("0.8.5", "a"), entry("0.9.0", "b")), encoding="utf-8")
            contract_list = Path(directory, "contract-files.txt")
            contract_list.write_text("\n".join(CONTRACT_FILES) + "\n", encoding="utf-8")
            with patch("sys.stdout", new_callable=io.StringIO) as out:
                code = check.main(["--base-ref", "main", "--head", str(head),
                                   "--contract-files", str(contract_list)], gh=gh)
        self.assertEqual(code, check.EXIT_PASSED, out.getvalue())
        self.assertIn("1 new entry added", out.getvalue())

    def test_base_ref_without_list_treats_entries_as_new(self):
        with tempfile.TemporaryDirectory() as directory:
            head = self.git_repo(directory, None)
            head.write_text(text(entry("0.8.5", "a")), encoding="utf-8")
            gh = FakeGh()
            gh.publish("0.8.5", image("a"))
            with patch("sys.stdout", new_callable=io.StringIO) as out:
                code = check.main(["--base-ref", "main", "--head", str(head)], gh=gh)
        self.assertEqual(code, check.EXIT_NEEDS_REVIEW, out.getvalue())
        self.assertIn("has no compatible-runtimes.json", out.getvalue())

    def test_unknown_base_ref_is_an_error(self):
        with tempfile.TemporaryDirectory() as directory:
            head = self.git_repo(directory, text(entry("0.8.5", "a")))
            with patch("sys.stdout", new_callable=io.StringIO) as out:
                code = check.main(["--base-ref", "origin/nope", "--head", str(head)], gh=FakeGh())
        self.assertEqual(code, check.EXIT_REJECTED)
        self.assertIn("git ref origin/nope does not exist", out.getvalue())

    def test_report_never_contains_token(self):
        gh = FakeGh()
        gh.publish("0.8.5", image("a"))
        gh.publish("0.9.0", image("b"))
        with patch.dict("os.environ", {"GH_TOKEN": "secret-token-value"}):
            rendered = check.check_change(text(entry("0.8.5", "a")),
                                          text(entry("0.8.5", "a"), entry("0.9.0", "b")),
                                          CONTRACT_FILES, gh).render()
        self.assertNotIn("secret-token-value", rendered)


if __name__ == "__main__":
    unittest.main()
