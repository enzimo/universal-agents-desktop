"""Publish the existing repository guides without maintaining duplicate sources."""

from pathlib import Path, PurePosixPath

from mkdocs.config.defaults import MkDocsConfig
from mkdocs.structure.files import File, Files

ROOT_FILES = (
    "README.md",
    "QUICKSTART.md",
    "CONTRIBUTING.md",
    "SECURITY.md",
    "runtime-lock.json",
    "compatible-runtimes.json",
    "favicon.svg",
    "tests/ui/README.md",
)


def repository_file(config: MkDocsConfig, root: Path, source: str) -> File:
    """Keep source paths for MkDocs link resolution and use lowercase page URLs."""
    file = File.generated(config, source, abs_src_path=str(root / source))
    file.edit_uri = source
    if file.is_documentation_page():
        path = PurePosixPath(source)
        destination = path.parent if path.name == "README.md" else path.with_suffix("")
        file.dest_uri = str(destination / "index.html").lower()
    return file


def on_files(files: Files, *, config: MkDocsConfig) -> Files:
    root = Path(config.config_file_path).parent
    # Only remap repository documents. Theme assets keep their own paths.
    for file in list(files):
        if file.src_dir == config.docs_dir:
            files.remove(file)
            files.append(repository_file(config, root, f"docs/{file.src_uri}"))
    for source in ROOT_FILES:
        files.append(repository_file(config, root, source))
    return files


def on_post_build(*, config: MkDocsConfig) -> None:
    # Pages must serve the generated site directly, without a Jekyll pass.
    (Path(config.site_dir) / ".nojekyll").write_text("", encoding="utf-8")
