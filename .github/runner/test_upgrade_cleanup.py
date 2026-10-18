"""The runtime check may upgrade only its own registered managed hook."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import upgrade_cleanup


class UpgradeCleanupTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.cache = self.root / "state" / "cache"
        self.cache.mkdir(parents=True)
        self.hook = self.root / "ci_hook.py"
        self.hook.write_text("# previous hook\n")
        self.source = self.root / "candidate.py"
        self.source.write_text("# reviewed hook\n")
        self.config = self.root / "config.json"
        self.config.write_text(json.dumps({"state": str(self.cache.parent), "repositories": ["example-owner/desktop"]}))
        self.env = {"UA_CI_HOOK_VERSION": "1", "UA_CI_CACHE": str(self.cache),
                    "GITHUB_REPOSITORY": "example-owner/desktop", "GITHUB_ENV": str(self.root / "env")}
        self.enterContext(patch.object(upgrade_cleanup, "SOURCE", self.source))

    def test_upgrades_only_registered_hook_and_keeps_previous_copy(self):
        upgrade_cleanup.upgrade(self.env)
        upgrade_cleanup.upgrade(self.env)
        self.assertEqual(self.hook.read_bytes(), self.source.read_bytes())
        self.assertEqual((self.root / "ci_hook.previous.py").read_text(), "# previous hook\n")
        self.assertIn("UA_CI_DOCKER_RESOURCES_VERSION=1", (self.root / "env").read_text())

    def test_refuses_an_unregistered_repository(self):
        self.env["GITHUB_REPOSITORY"] = "example-owner/other"
        with self.assertRaisesRegex(ValueError, "registration"):
            upgrade_cleanup.upgrade(self.env)
        self.assertEqual(self.hook.read_text(), "# previous hook\n")

    def test_refuses_state_outside_the_registered_runner(self):
        self.config.write_text(json.dumps({"state": str(self.root / "other"), "repositories": ["example-owner/desktop"]}))
        with self.assertRaisesRegex(ValueError, "registration"):
            upgrade_cleanup.upgrade(self.env)

    def test_refuses_linked_hook(self):
        self.hook.unlink()
        self.hook.symlink_to(self.source)
        with self.assertRaisesRegex(ValueError, "linked"):
            upgrade_cleanup.upgrade(self.env)


if __name__ == "__main__":
    unittest.main()
