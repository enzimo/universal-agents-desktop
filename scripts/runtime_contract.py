"""Exercise the desktop's reviewed contract against an immutable UA image.

Only synthetic credentials and disposable Docker volumes are used. The image
has no network access, host credentials, Docker socket, or user data. These are
behavior checks, so internal UA refactors do not require another approval.
"""
from __future__ import annotations

import base64
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import uuid

SUPPORTED_CONTRACT = 3
MARKER = "desktop-contract-workspace-marker"


class ContractError(Exception):
    """The published image could not satisfy the reviewed desktop contract."""


def load_contract(path: Path) -> dict:
    contract = json.loads(path.read_text(encoding="utf-8"))
    if type(contract.get("host_contract_version")) is not int:
        raise ValueError("runtime-lock.json needs a whole-number host_contract_version")
    return contract


LAYOUT_PROBE = r'''
import json, sys
from pathlib import Path
from runtime_settings.instance_layout import InstanceLayout
lock = json.loads(sys.argv[1])
layout = InstanceLayout.at(lock['data_mount'])
assert str(layout.instance_config_path) == lock['instance_config'], 'Instance config path changed'
assert str(layout.openai_codex_auth_path) == lock['codex_auth_store'], 'Sign-in store path changed'
assert str(layout.team_shared_dir) == lock['workspace_mount'], 'Shared workspace path changed'
layout.ensure_directories()
marker = layout.team_shared_dir / sys.argv[2]
marker.write_text('workspace preserved\n')
assert not layout.instance_config_path.exists(), 'Fresh data already configured'
'''

STORE_PROBE = r'''
import hashlib, json, stat, sys
from pathlib import Path
lock = json.loads(sys.argv[1])
path = Path(lock['codex_auth_store'])
assert stat.S_IMODE(path.stat().st_mode) == 0o600, 'Sign-in store is not private'
assert not Path(lock['instance_config']).exists(), 'Import requires onboarding'
print(hashlib.sha256(path.read_bytes()).hexdigest())
'''

PROVIDER_PROBE = r'''
import json, os, sys
from unittest.mock import patch
from assistant_gateway.onboarding_web import provider_test
lock = json.loads(sys.argv[1])
os.environ['OPENAI_CODEX_AUTH_STORE_PATH'] = lock['codex_auth_store']
calls = []
def transport(url, headers, payload, *, provider_label):
    assert headers['Authorization'].startswith('Bearer synthetic.')
    assert provider_label == 'Codex'
    calls.append(url)
    return {'output': []}
with patch.object(provider_test, 'post_responses_stream', transport):
    result = provider_test.test_provider_connection(
        provider='openai-codex', endpoint='https://chatgpt.com/backend-api/codex',
        token='', model='test-model', instance_root=lock['data_mount'])
assert result.ok, result.message
assert len(calls) == 1, 'Provider did not resolve the imported sign-in'
'''

HEALTH_PROBE = r'''
import json, sys, time
from urllib.request import urlopen
lock, expected = json.loads(sys.argv[1]), sys.argv[2]
base = 'http://127.0.0.1:' + str(lock['container_ports']['web'])
deadline = time.monotonic() + 180
last = 'no response'
while time.monotonic() < deadline:
    try:
        with urlopen(base + lock['health_path'], timeout=3) as response:
            health = json.load(response)
        assert health.get('status') == 'ok', health
        if expected == 'onboarding':
            assert health.get('service') == 'onboard-web', health
            assert health.get('instance_root') == lock['data_mount'], health
            paths = [lock['onboarding_ui_path']]
        else:
            assert health.get('service') != 'onboard-web', health
            paths = [lock['web_ui_path'], lock['docs_path']]
        for path in paths:
            with urlopen(base + path, timeout=3) as response:
                assert response.status == 200, path
                assert 'text/html' in response.headers.get('Content-Type', ''), path
        print('PASS: ' + expected + ' health and browser routes')
        break
    except Exception as error:
        last = str(error)
        time.sleep(1)
else:
    raise RuntimeError('Locked health/browser routes did not become ready: ' + last)
'''

CONFIGURE_PROBE = r'''
import json, sys
from pathlib import Path
from assistant_gateway.onboarding_engine import (
    OnboardingTemplate, normalize_onboarding_answers, write_onboarded_instance)
lock = json.loads(sys.argv[1])
template = OnboardingTemplate(team_id='desktop-smoke')
answers = normalize_onboarding_answers({
    'instance_root': lock['data_mount'], 'assistant_name': 'Nova',
    'model_provider': 'openai-codex', 'model_id': 'gpt-5.1-codex',
    'storage_backend': 'sqlite', 'refresh_external_skills_on_start': False,
    'skills_repositories': [],
}, template)
result = write_onboarded_instance(answers=answers, template=template)
assert str(result.config.config_path) == lock['instance_config']
assert Path(lock['instance_config']).is_file()
'''

PRESERVATION_PROBE = r'''
import hashlib, json, sys
from pathlib import Path
lock = json.loads(sys.argv[1])
assert (Path(lock['workspace_mount']) / sys.argv[2]).read_text() == 'workspace preserved\n'
assert Path(lock['instance_config']).is_file(), 'Restart lost the instance config'
assert hashlib.sha256(Path(lock['codex_auth_store']).read_bytes()).hexdigest() == sys.argv[3], 'Restart changed imported credentials'
assert not Path(lock['codex_auth_mount']).exists(), 'Runtime can read the source credential file'
'''


class ImageProbe:
    def __init__(self, contract: dict, image: str):
        self.contract = contract
        self.image = image
        self.identity = "ua-contract-" + uuid.uuid4().hex
        self.volumes: list[str] = []
        self.containers: list[str] = []

    def docker(self, *args: str, timeout: int = 60, checked: bool = True):
        try:
            result = subprocess.run(["rtk", "proxy", "docker", *args],
                                    capture_output=True, text=True, timeout=timeout, check=False)
        except (OSError, subprocess.TimeoutExpired) as error:
            raise ContractError(f"Docker {args[0]} could not finish: {error}") from error
        if checked and result.returncode:
            raise ContractError(f"Docker {args[0]} failed: {result.stderr[-2000:]}")
        return result

    def volume(self, suffix: str) -> str:
        name = f"{self.identity}-{suffix}"
        self.docker("volume", "create", *self.labels(), name)
        self.volumes.append(name)
        return name

    def labels(self) -> list[str]:
        labels = ["--label", f"ua.contract-check={self.identity}"]
        if os.environ.get("UA_CI_ID"):
            labels += ["--label", f"com.universalagents.ci.job={os.environ['UA_CI_ID']}"]
        return labels

    def mounts(self) -> list[str]:
        return ["--volume", f"{self.volumes[0]}:{self.contract['data_mount']}",
                "--volume", f"{self.volumes[1]}:{self.contract['workspace_mount']}"]

    def helper(self, command: list[str], *, extra: list[str] | None = None,
               checked: bool = True):
        name = f"{self.identity}-{uuid.uuid4().hex[:8]}"
        self.containers.append(name)
        result = self.docker(
            "run", "--rm", "--pull", "never", "--network", "none", "--name", name,
            *self.labels(), *self.mounts(), "--env", "HOME=/tmp/ua-contract-home",
            "--env", f"CODEX_HOME={self.contract['codex_home']}", *(extra or []),
            "--entrypoint", command[0], self.image, *command[1:], checked=checked, timeout=120)
        return result

    def python(self, program: str, *args: str) -> str:
        return self.helper(["python", "-c", program, json.dumps(self.contract), *args]).stdout.strip()

    def start(self) -> str:
        name = f"{self.identity}-runtime"
        self.containers.append(name)
        environment = {
            "NETWORK_PUBLIC_ORIGIN": "http://127.0.0.1",
            "OPENAI_CODEX_AUTH_STORE_PATH": self.contract["codex_auth_store"],
            "TEAM_REFRESH_EXTERNAL_SKILLS": "false",
            "INTERNAL_WEBSITE_PUBLIC_PORT": str(self.contract["container_ports"]["web"]),
            "SECURE_TOOL_BROKER_PUBLIC_PORT": str(self.contract["container_ports"]["broker"]),
        }
        env_args = [arg for key, value in environment.items() for arg in ("--env", f"{key}={value}")]
        self.docker("run", "--detach", "--pull", "never", "--network", "none",
                    "--name", name, *self.labels(), *self.mounts(), *env_args,
                    self.image, *self.contract["command"])
        return name

    def health(self, name: str, expected: str) -> None:
        self.docker("exec", name, "python", "-c", HEALTH_PROBE,
                    json.dumps(self.contract), expected, timeout=200)

    def cleanup(self) -> None:
        errors = []
        for name in reversed(self.containers):
            result = self.docker("rm", "--force", name, checked=False)
            if result.returncode and "No such container" not in result.stderr:
                errors.append(f"container {name}: {result.stderr[-300:]}")
        for name in reversed(self.volumes):
            result = self.docker("volume", "rm", name, checked=False)
            if result.returncode:
                errors.append(f"volume {name}: {result.stderr[-300:]}")
        if errors:
            raise ContractError("Test resource cleanup failed: " + "; ".join(errors))

    def run(self) -> None:
        if self.contract["host_contract_version"] != SUPPORTED_CONTRACT:
            raise ContractError(f"No runtime probe implements host contract {self.contract['host_contract_version']}")
        try:
            info = json.loads(self.docker("info", "--format", "{{json .}}").stdout)
            aliases = {"amd64": "amd64", "x86_64": "amd64", "arm64": "arm64", "aarch64": "arm64"}
            platform = "linux/" + aliases.get(info.get("Architecture"), "unsupported")
            if info.get("OSType") != "linux" or platform.endswith("unsupported"):
                raise ContractError("A native Linux AMD64 or ARM64 Docker daemon is required")
            if os.environ.get("UA_COMPAT_PLATFORM", platform) != platform:
                raise ContractError("Docker daemon architecture does not match this check's platform")
            self.docker("pull", self.image, timeout=600)
            self.volume("data")
            self.volume("workspace")
            self.python(LAYOUT_PROBE, MARKER)
            self.check_import()
            runtime = self.start()
            self.health(runtime, "onboarding")
            self.docker("stop", "--time", "10", runtime)
            self.python(CONFIGURE_PROBE)
            self.docker("start", runtime)
            self.health(runtime, "ready")
            self.docker("restart", "--time", "10", runtime)
            self.health(runtime, "ready")
            self.python(PRESERVATION_PROBE, MARKER, self.store_digest)
        finally:
            self.cleanup()

    def check_import(self) -> None:
        with tempfile.TemporaryDirectory(prefix=".ua-contract-", dir=Path.home()) as temporary:
            source = Path(temporary) / "auth.json"
            payload = base64.urlsafe_b64encode(json.dumps({"exp": int(time.time()) + 3600}).encode()).decode().rstrip("=")
            token = f"synthetic.{payload}.signature"
            source.write_text(json.dumps({"tokens": {"access_token": token, "refresh_token": "synthetic-refresh"}}))
            original = source.read_bytes()
            command = [*self.contract["codex_import_command"],
                       self.contract["codex_home"], self.contract["codex_auth_store"]]
            mounts = ["--volume", f"{source}:{self.contract['codex_auth_mount']}:ro"]
            self.helper(command, extra=mounts)
            if source.read_bytes() != original:
                raise ContractError("Import changed the read-only source credential file")
            self.store_digest = self.python(STORE_PROBE)
            self.python(PROVIDER_PROBE)
            for invalid in ("invalid-synthetic-auth", json.dumps({"tokens": {"access_token": token}})):
                source.write_text(invalid)
                result = self.helper(command, extra=mounts, checked=False)
                if result.returncode == 0:
                    raise ContractError("Import accepted an invalid credential file")
                if token in result.stdout + result.stderr or invalid in result.stdout + result.stderr:
                    raise ContractError("Import printed synthetic credential contents")
                if self.python(STORE_PROBE) != self.store_digest:
                    raise ContractError("Failed import changed the existing sign-in store")


def verify_image(contract: dict, image: str) -> None:
    # A cancelled CI run must clean up before the job's registry login is removed.
    def interrupted(_signum, _frame):
        raise ContractError("Runtime check was interrupted")

    previous = signal.signal(signal.SIGTERM, interrupted)
    try:
        ImageProbe(contract, image).run()
    finally:
        signal.signal(signal.SIGTERM, previous)
