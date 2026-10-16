import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location("release", Path(__file__).parents[1] / "scripts/prepare_release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class PrepareReleaseTests(unittest.TestCase):
    def test_only_successful_main_push_at_exact_sha(self):
        sha = "a" * 40
        runs = [dict(id=n, head_sha=sha, path=path, event="push", head_branch="main",
                     status="completed", conclusion="success")
                for n, path in enumerate(release.WORKFLOWS)]
        self.assertEqual(len(release.select_runs(runs, sha)), 2)
        for field, value in (("head_sha", "b" * 40), ("event", "pull_request"),
                             ("status", "queued"), ("conclusion", "failure")):
            changed = [dict(run) for run in runs]
            changed[0][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                release.select_runs(changed, sha)

    def test_flat_output_and_checksums_ignore_unrelated_zip_members(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "build.zip"
            with zipfile.ZipFile(archive, "w") as bundle:
                for suffix in release.FORMATS:
                    bundle.writestr("../../installer" + suffix, b"installer")
                bundle.writestr("../../unrelated.txt", "ignored")
            output = root / "assets"
            result = release.assemble([archive], output, "0.4.5")
            self.assertEqual(len(list(output.iterdir())), 10)
            self.assertEqual(len(result), 5)
            self.assertFalse((root / "unrelated.txt").exists())
            for name, digest in result.items():
                self.assertEqual(digest, hashlib.sha256(b"installer").hexdigest())
                self.assertEqual((output / (name + ".sha256")).read_text(), f"{digest}  {name}\n")

    def test_missing_or_duplicate_installers_fail_before_writing(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "build.zip"
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr("one.dmg", b"one")
                bundle.writestr("two.dmg", b"two")
            with self.assertRaises(ValueError):
                release.assemble([archive], root / "assets", "0.4.5")
            self.assertFalse((root / "assets").exists())
