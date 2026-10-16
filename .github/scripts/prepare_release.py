"""Promote only successful main-branch builds of the exact release commit."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib
import zipfile

WORKFLOWS = {
    ".github/workflows/ci.yml": {"universal-agents-macos-universal-development"},
    ".github/workflows/native-builds.yml": {
        "universal-agents-windows-x64-development", "universal-agents-linux-x64-development"},
}
FORMATS = {".dmg": "macOS-universal", ".exe": "Windows-x64-setup",
           ".msi": "Windows-x64", ".deb": "Linux-x64", ".AppImage": "Linux-x64"}


def api(route: str) -> dict:
    return json.loads(subprocess.check_output(["rtk", "proxy", "gh", "api", route], text=True, timeout=60))


def select_runs(runs: list[dict], sha: str) -> dict:
    selected = {}
    for workflow in WORKFLOWS:
        matches = [run for run in runs if run["head_sha"] == sha and run["path"] == workflow
                   and run["event"] == "push" and run["head_branch"] == "main"]
        if not matches:
            raise ValueError(f"No main-branch build for {workflow} at {sha}; run CI before tagging")
        run = max(matches, key=lambda value: value["id"])
        if run["status"] != "completed" or run["conclusion"] != "success":
            raise ValueError(f"Build {run['id']} is not successful; finish CI before tagging")
        selected[workflow] = run
    return selected


def assemble(archives: list[Path], output: Path, version: str) -> dict:
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("Invalid release version")
    if output.exists() and any(output.iterdir()):
        raise ValueError("Release output must be empty")
    found = {suffix: [] for suffix in FORMATS}
    for archive in archives:
        with zipfile.ZipFile(archive) as bundle:
            for item in bundle.infolist():
                for suffix in FORMATS:
                    if not item.is_dir() and item.filename.endswith(suffix):
                        if item.file_size > 256 * 1024**2:
                            raise ValueError("Installer exceeds the 256 MiB release limit")
                        found[suffix].append(bundle.read(item))
    if any(len(values) != 1 for values in found.values()):
        raise ValueError(f"Expected exactly one installer per format: { {key: len(value) for key, value in found.items()} }")
    output.mkdir(parents=True, exist_ok=True)
    checksums = {}
    for suffix, platform in FORMATS.items():
        name = f"Universal-Agents-Desktop-{version}-{platform}{suffix}"
        data = found[suffix][0]
        digest = hashlib.sha256(data).hexdigest()
        (output / name).write_bytes(data)
        (output / (name + ".sha256")).write_text(f"{digest}  {name}\n")
        checksums[name] = digest
    return checksums


def prepare(repo: str, sha: str, version: str, output: Path) -> dict:
    if not re.fullmatch(r"[\w.-]+/[\w.-]+", repo) or not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise ValueError("Expected an owner/repository and full source SHA")
    runs = select_runs(api(f"repos/{repo}/actions/runs?head_sha={sha}&per_page=100")["workflow_runs"], sha)
    output.parent.mkdir(parents=True, exist_ok=True)
    evidence = {"source_sha": sha, "builds": [], "artifacts": []}
    with tempfile.TemporaryDirectory(prefix="ua-release-download-", dir=output.parent) as temporary:
        archives = []
        for workflow, run in runs.items():
            evidence["builds"].append({"id": run["id"], "url": run["html_url"], "workflow": workflow})
            available = api(f"repos/{repo}/actions/runs/{run['id']}/artifacts?per_page=100")["artifacts"]
            for name in sorted(WORKFLOWS[workflow]):
                matches = [artifact for artifact in available if artifact["name"] == name and not artifact["expired"]]
                if len(matches) != 1:
                    raise ValueError(f"Expected one unexpired {name} artifact from build {run['id']}")
                artifact = matches[0]
                if not re.fullmatch(r"sha256:[0-9a-f]{64}", artifact.get("digest", "")):
                    raise ValueError("GitHub artifact has no verifiable SHA-256 digest")
                archive = Path(temporary) / f"{artifact['id']}.zip"
                with archive.open("wb") as stream:
                    subprocess.run(["rtk", "proxy", "gh", "api", f"repos/{repo}/actions/artifacts/{artifact['id']}/zip"],
                                   stdout=stream, check=True, timeout=300)
                with archive.open("rb") as stream:
                    digest = "sha256:" + hashlib.file_digest(stream, "sha256").hexdigest()
                if digest != artifact["digest"]:
                    raise ValueError(f"Artifact checksum mismatch: {name}")
                evidence["artifacts"].append({"id": artifact["id"], "name": name, "digest": digest})
                archives.append(archive)
        evidence["installers"] = assemble(archives, output, version)
    (output.parent / "release-provenance.json").write_text(json.dumps(evidence, indent=2) + "\n")
    return evidence


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    version = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    if args.tag != "v" + version or json.loads((root / "src-tauri/tauri.conf.json").read_text())["version"] != version:
        raise ValueError("Tag and desktop package versions disagree")
    prepare(args.repo, args.sha, version, args.output)
    print(f"Verified all five installers for {args.sha}")
