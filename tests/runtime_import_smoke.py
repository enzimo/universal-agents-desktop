"""Run the complete desktop contract check for a listed runtime image.

Only synthetic credentials and disposable state are used. This entry point is
also useful for maintainers investigating an image rejected by release CI.
"""
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from runtime_contract import load_contract, verify_image


def main():
    contract = load_contract(ROOT / "runtime-lock.json")
    runtimes = json.loads((ROOT / "compatible-runtimes.json").read_text())["runtimes"]
    version = sys.argv[1] if len(sys.argv) > 1 else None
    candidates = [entry for entry in runtimes
                  if entry["host_contract_version"] == contract["host_contract_version"]
                  and (version is None or entry["ua_version"] == version)]
    if not candidates:
        raise SystemExit("No listed image matches the requested version and launcher contract")
    entry = candidates[-1]
    verify_image(contract, entry["image"])
    print(f"PASS: UA {entry['ua_version']} implements the desktop runtime contract")


if __name__ == "__main__":
    main()
