"""Prove the real image check rejects intentionally incompatible contracts."""
import copy
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from runtime_contract import ContractError, load_contract, verify_image


def main():
    contract = load_contract(ROOT / "runtime-lock.json")
    entries = json.loads((ROOT / "compatible-runtimes.json").read_text())["runtimes"]
    image = [entry["image"] for entry in entries
             if entry["host_contract_version"] == contract["host_contract_version"]][-1]
    cases = [
        ("workspace layout", "workspace_mount", "/data/workspaces/incorrect", "Shared workspace path changed"),
        ("sign-in import", "codex_import_command",
         ["python", "-c", "raise RuntimeError('Intentional broken desktop import')"],
         "Intentional broken desktop import"),
        ("startup command", "command", ["intentional-unsupported-desktop-command"],
         "Runtime exited before"),
    ]
    for label, field, value, message in cases:
        broken = copy.deepcopy(contract)
        broken[field] = value
        try:
            verify_image(broken, image)
        except ContractError as error:
            if message not in str(error):
                raise AssertionError(f"{label} failed for an unexpected reason: {error}") from error
        else:
            raise AssertionError(f"The check approved an incompatible {label}")
        print(f"PASS: incompatible {label} rejected by the published image")


if __name__ == "__main__":
    main()
