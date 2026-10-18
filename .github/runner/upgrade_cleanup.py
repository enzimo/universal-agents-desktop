"""Install this checkout's reviewed cleanup hook for the current managed job.

The runner already owns and records this job's private state. Upgrade that one
installed hook before creating named volumes or networks so recovery after a
crash can reclaim them too. Existing runner registration and cache settings stay
in place; this is not a replacement for initial hook installation.
"""
import json
import logging
import os
from pathlib import Path
import shutil
import stat
import tempfile

SOURCE = Path(__file__).with_name("ci_hook.py")


def upgrade(environment=None):
    env = os.environ if environment is None else environment
    if env.get("UA_CI_HOOK_VERSION") != "1" or not env.get("UA_CI_CACHE"):
        raise ValueError("An active managed runner job is required")
    cache = Path(env["UA_CI_CACHE"])
    if cache != cache.resolve() or cache.name != "cache" or cache.parent.name != "state":
        raise ValueError("The job cache must be the runner's private state/cache directory")
    destination = cache.parent.parent
    hook, config = destination / "ci_hook.py", destination / "config.json"
    if not cache.is_dir() or hook.is_symlink() or not hook.is_file() or config.is_symlink():
        raise ValueError("The installed managed cleanup hook is missing or linked")
    settings = json.loads(config.read_text())
    if Path(settings["state"]).resolve() != cache.parent or env.get("GITHUB_REPOSITORY") not in settings["repositories"]:
        raise ValueError("The runner registration does not own this job's state and repository")
    source = SOURCE.read_bytes()
    compile(source, str(SOURCE), "exec")
    if hook.read_bytes() != source:
        backup = destination / "ci_hook.previous.py"
        if backup.is_symlink():
            raise ValueError("The cleanup backup must not be a symbolic link")
        shutil.copyfile(hook, backup)
        with tempfile.NamedTemporaryFile(dir=destination, delete=False) as temporary:
            temporary.write(source)
            replacement = Path(temporary.name)
        try:
            replacement.chmod(stat.S_IMODE(hook.stat().st_mode))
            replacement.replace(hook)
        finally:
            replacement.unlink(missing_ok=True)
    with Path(env["GITHUB_ENV"]).open("a") as stream:
        stream.write("UA_CI_DOCKER_RESOURCES_VERSION=1\n")
    logging.getLogger(__name__).info("The managed job can recover named Docker volumes and networks")


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO)
    upgrade()
