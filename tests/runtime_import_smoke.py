"""Exercise the reviewed import command against a locally available listed image.

Uses only synthetic credentials and temporary mounts. No network or live UA data.
The image is the newest entry for this launcher's contract in
compatible-runtimes.json, or the listed UA version given as the argument.
Run explicitly with: rtk proxy python3 tests/runtime_import_smoke.py [UA_VERSION]
"""

import base64
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def listed_lock(ua_version=None):
    """The launcher contract combined with one compatibility-list entry."""
    lock = json.loads((ROOT / "runtime-lock.json").read_text())
    runtimes = json.loads((ROOT / "compatible-runtimes.json").read_text())["runtimes"]
    candidates = [
        entry for entry in runtimes
        if entry["host_contract_version"] == lock["host_contract_version"]
        and (ua_version is None or entry["ua_version"] == ua_version)
    ]
    if not candidates:
        raise SystemExit(f"UA {ua_version or '(newest)'} is not listed for host contract {lock['host_contract_version']}.")
    entry = candidates[-1]
    print(f"Smoke-testing UA {entry['ua_version']} ({entry['image']})")
    return {**lock, "ua_version": entry["ua_version"], "image": entry["image"]}


def main():
    lock = listed_lock(sys.argv[1] if len(sys.argv) > 1 else None)
    command = lock["codex_import_command"]
    with tempfile.TemporaryDirectory(prefix=".ua-import-smoke-", dir=Path.home()) as temporary:
        root = Path(temporary)
        data = root / "data"
        data.mkdir()
        source = root / "auth.json"
        payload = base64.urlsafe_b64encode(json.dumps({"exp": int(time.time()) + 3600}).encode()).decode().rstrip("=")
        token = f"synthetic.{payload}.signature"
        source.write_text(json.dumps({"tokens": {"access_token": token, "refresh_token": "synthetic-refresh"}}))
        original = source.read_bytes()
        args = [
            "docker", "run", "--rm", "--pull", "never", "--network", "none",
            "--volume", f"{data}:{lock['data_mount']}",
            "--volume", f"{source}:{lock['codex_auth_mount']}:ro",
            "--entrypoint", command[0], lock["image"], *command[1:],
            lock["codex_home"], lock["codex_auth_store"],
        ]
        result = subprocess.run(args, capture_output=True, text=True, timeout=60)
        assert result.returncode == 0, result.stderr
        assert source.read_bytes() == original, "Importer modified its source"
        assert not (data / Path(lock["instance_config"]).relative_to(lock["data_mount"])).exists()
        # Exercise the actual onboarding provider-test path. Replace only its
        # network transport, so synthetic credentials never leave the container.
        probe = """import os, sys
from unittest.mock import patch
from assistant_gateway.onboarding_web import provider_test
calls = []
def transport(url, headers, payload, *, provider_label):
    assert headers['Authorization'].startswith('Bearer synthetic.')
    assert provider_label == 'Codex'
    calls.append(url)
    return {'output': []}
def test():
    return provider_test.test_provider_connection(provider='openai-codex', endpoint='https://chatgpt.com/backend-api/codex', token='', model='test-model', instance_root='/data')
# UA 0.6+ sends the Codex request through the shared Responses stream helper.
with patch.object(provider_test, 'post_responses_stream', transport):
    failed = test()
    assert not failed.ok and 'credentials are not available' in failed.message, failed.message
    assert not calls
    os.environ['OPENAI_CODEX_AUTH_STORE_PATH'] = sys.argv[1]
    passed = test()
    assert passed.ok, passed.message
    assert len(calls) == 1
print('PASS: reproduced onboarding lookup failure, then actual provider test resolved imported credentials')
"""
        result = subprocess.run([
            "docker", "run", "--rm", "--pull", "never", "--network", "none",
            "--volume", f"{data}:{lock['data_mount']}:ro",
            "--entrypoint", "python", lock["image"], "-c", probe, lock["codex_auth_store"],
        ], capture_output=True, text=True, timeout=60)
        assert result.returncode == 0, result.stderr
        print(result.stdout.strip())
        store = data / Path(lock["codex_auth_store"]).relative_to(lock["data_mount"])
        imported = store.read_bytes()
        assert store.stat().st_mode & 0o777 == 0o600
        for invalid in ["malformed-synthetic-secret", json.dumps({"tokens": {"access_token": token}})]:
            source.write_text(invalid)
            result = subprocess.run(args, capture_output=True, text=True, timeout=60)
            assert result.returncode != 0, "Invalid credentials accepted"
            assert "Choose a current file-backed Codex auth.json" in result.stderr
            assert token not in result.stdout + result.stderr
            assert invalid not in result.stdout + result.stderr
            assert store.read_bytes() == imported, "Failed import damaged the existing store"
        print("PASS: pre-onboarding import, source preservation, UA resolution, private permissions, and failed-import preservation")


if __name__ == "__main__":
    main()
