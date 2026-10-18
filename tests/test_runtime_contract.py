"""Failure containment tests for the real-image contract runner."""
import json
import os
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import runtime_contract as runtime


class RuntimeContractTests(unittest.TestCase):
    def setUp(self):
        self.contract = runtime.load_contract(ROOT / "runtime-lock.json")
        self.probe = runtime.ImageProbe(self.contract, "ghcr.io/example/image@sha256:" + "a" * 64)

    def test_missing_docker_is_a_failed_check(self):
        with patch.object(runtime.subprocess, "run", side_effect=FileNotFoundError("docker")):
            with self.assertRaisesRegex(runtime.ContractError, "Docker info could not finish"):
                self.probe.run()

    def test_docker_timeout_is_a_failed_check(self):
        with patch.object(runtime.subprocess, "run", side_effect=subprocess.TimeoutExpired("docker", 60)):
            with self.assertRaisesRegex(runtime.ContractError, "could not finish"):
                self.probe.run()

    def test_wrong_architecture_never_pulls_or_runs_the_image(self):
        result = subprocess.CompletedProcess([], 0, json.dumps({"OSType": "linux", "Architecture": "aarch64"}), "")
        with patch.dict(os.environ, {"UA_COMPAT_PLATFORM": "linux/amd64"}):
            with patch.object(self.probe, "docker", return_value=result) as docker:
                with self.assertRaisesRegex(runtime.ContractError, "architecture does not match"):
                    self.probe.run()
        self.assertEqual(docker.call_count, 1)

    def test_failed_probe_cleans_up_every_owned_resource(self):
        self.probe.containers = ["owned-helper", "owned-runtime"]
        self.probe.volumes = ["owned-data", "owned-workspace"]
        with patch.object(self.probe, "docker", return_value=subprocess.CompletedProcess([], 0, "", "")) as docker:
            self.probe.cleanup()
        self.assertEqual([call.args for call in docker.call_args_list], [
            ("rm", "--force", "owned-runtime"), ("rm", "--force", "owned-helper"),
            ("volume", "rm", "owned-workspace"), ("volume", "rm", "owned-data")])

    def test_cleanup_failure_prevents_approval(self):
        self.probe.volumes = ["owned-data", "owned-workspace"]
        with patch.object(self.probe, "docker", return_value=subprocess.CompletedProcess([], 1, "", "in use")) as docker:
            with self.assertRaisesRegex(runtime.ContractError, "Test resource cleanup failed"):
                self.probe.cleanup()
        self.assertEqual(docker.call_count, 2)

    def test_ci_ownership_label_allows_interrupted_job_cleanup(self):
        with patch.dict(os.environ, {"UA_CI_ID": "example-job"}):
            self.assertIn("com.universalagents.ci.job=example-job", self.probe.labels())


if __name__ == "__main__":
    unittest.main()
