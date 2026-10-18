"""Failure containment tests for the real-image contract runner."""
import json
import os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import subprocess
import sys
import socket
import threading
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
        self.probe.volumes = ["owned-data", "owned-workspace", "owned-codex-source"]
        self.probe.network = "owned-network"
        with patch.object(self.probe, "docker", return_value=subprocess.CompletedProcess([], 0, "", "")) as docker:
            self.probe.cleanup()
        self.assertEqual([call.args for call in docker.call_args_list], [
            ("rm", "--force", "owned-runtime"), ("rm", "--force", "owned-helper"),
            ("volume", "rm", "owned-codex-source"),
            ("volume", "rm", "owned-workspace"), ("volume", "rm", "owned-data"),
            ("network", "rm", "owned-network")])

    def test_cleanup_failure_prevents_approval(self):
        self.probe.volumes = ["owned-data", "owned-workspace"]
        with patch.object(self.probe, "docker", return_value=subprocess.CompletedProcess([], 1, "", "in use")) as docker:
            with self.assertRaisesRegex(runtime.ContractError, "Test resource cleanup failed"):
                self.probe.cleanup()
        self.assertEqual(docker.call_count, 2)

    def test_ci_ownership_label_allows_interrupted_job_cleanup(self):
        with patch.dict(os.environ, {"UA_CI_ID": "example-job"}):
            self.assertIn("com.universalagents.ci.job=example-job", self.probe.labels())

    def endpoints(self, *, onboarding=False, broker_ready=True, broker_http=True):
        contract = self.contract

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.send_header("Content-Type", "application/json" if self.path == contract["health_path"] else "text/html")
                self.end_headers()
                if self.path == contract["health_path"]:
                    body = {"status": "ok"}
                    if onboarding:
                        body.update(service="onboard-web", instance_root=contract["data_mount"])
                    self.wfile.write(json.dumps(body).encode())
                else:
                    self.wfile.write(b"<html>Runtime</html>")

            def log_message(self, *_args):
                pass

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        if broker_ready and broker_http:
            broker = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
            threading.Thread(target=broker.serve_forever, daemon=True).start()
            self.addCleanup(broker.server_close)
            self.addCleanup(broker.shutdown)
            broker_port = broker.server_port
        else:
            broker = socket.socket()
            broker.bind(("127.0.0.1", 0))
            if broker_ready:
                broker.listen()
            self.addCleanup(broker.close)
            broker_port = broker.getsockname()[1]
        ports = {f"{self.contract['container_ports'][role]}/tcp": [
            {"HostIp": "127.0.0.1", "HostPort": str(port)}]
            for role, port in (("web", server.server_port), ("broker", broker_port))}
        return subprocess.CompletedProcess([], 0, json.dumps({"State": {"Running": True}, "NetworkSettings": {"Ports": ports}}), "")

    def test_checks_browser_and_broker_from_the_host(self):
        with patch.object(self.probe, "docker", return_value=self.endpoints()):
            self.probe.health("test-runtime", "ready")

    def test_checks_onboarding_from_the_host(self):
        with patch.object(self.probe, "docker", return_value=self.endpoints(onboarding=True)):
            self.probe.health("test-runtime", "onboarding")

    def test_missing_broker_listener_prevents_approval(self):
        inspection = self.endpoints(broker_ready=False)
        running = subprocess.CompletedProcess([], 0, "true\n", "")
        with patch.object(self.probe, "docker", side_effect=[inspection, running]):
            with patch.object(runtime.time, "monotonic", side_effect=[0, 0, 181]):
                with patch.object(runtime.time, "sleep"):
                    with self.assertRaisesRegex(runtime.ContractError, "broker ports did not become ready"):
                        self.probe.health("test-runtime", "ready")

    def test_exited_container_fails_without_waiting_for_health_timeout(self):
        result = subprocess.CompletedProcess([], 0, '{"State":{"Running":false}}', "")
        with patch.object(self.probe, "docker", return_value=result):
            with self.assertRaisesRegex(runtime.ContractError, "Runtime exited before"):
                self.probe.health("test-runtime", "ready")

    def test_tcp_acceptance_without_a_broker_response_prevents_approval(self):
        inspection = self.endpoints(broker_http=False)
        running = subprocess.CompletedProcess([], 0, "true\n", "")
        with patch.object(self.probe, "docker", side_effect=[inspection, running]):
            with patch.object(runtime.time, "monotonic", side_effect=[0, 0, 181]):
                with patch.object(runtime.time, "sleep"):
                    with self.assertRaisesRegex(runtime.ContractError, "broker ports did not become ready"):
                        self.probe.health("test-runtime", "ready")


if __name__ == "__main__":
    unittest.main()
