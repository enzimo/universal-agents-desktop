"""Real static-site and publication-boundary checks, without Docker or a GUI."""

import json
import os
import subprocess
import sys
import tempfile
import threading
import unittest
from fnmatch import fnmatchcase
from functools import partial
from html.parser import HTMLParser
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urljoin, urlparse
from urllib.request import urlopen

import yaml

ROOT = Path(__file__).resolve().parents[2]
SITE_PATH = "/universal-agents-desktop/"


class Links(HTMLParser):
    def __init__(self):
        super().__init__()
        self.targets = set()

    def handle_starttag(self, tag, attrs):
        for name, value in attrs:
            if name in {"href", "src"} and value and not value.startswith("#"):
                self.targets.add(value)


class QuietHandler(SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass


class DocumentationSiteTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="desktop-docs-")
        cls.addClassCleanup(cls.temporary.cleanup)
        directory = Path(cls.temporary.name)
        cls.site = directory / SITE_PATH.strip("/")
        completed = subprocess.run(
            [sys.executable, "-m", "mkdocs", "build", "--strict", "--site-dir", str(cls.site)],
            cwd=ROOT, capture_output=True, text=True, timeout=120, check=False,
            env={**os.environ, "UA_DOCS_SITE_URL": f"https://example.com{SITE_PATH}"},
        )
        if completed.returncode:
            raise RuntimeError(f"Documentation build failed:\n{completed.stdout}\n{completed.stderr}")
        cls.server = ThreadingHTTPServer(
            ("127.0.0.1", 0), partial(QuietHandler, directory=str(directory)),
        )
        cls.addClassCleanup(cls.server.server_close)
        thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        thread.start()
        cls.addClassCleanup(thread.join)
        cls.addClassCleanup(cls.server.shutdown)
        cls.origin = f"http://127.0.0.1:{cls.server.server_port}"

    def test_key_routes_and_pages_artifact(self):
        for path in ("", "quickstart/", "docs/usage/", "docs/troubleshooting/", "search/search_index.json"):
            with self.subTest(path=path), urlopen(self.origin + SITE_PATH + path, timeout=5) as response:
                self.assertEqual(response.status, 200)
                self.assertTrue(response.read())
        self.assertTrue((self.site / ".nojekyll").is_file())
        for name in ("runtime-lock.json", "compatible-runtimes.json"):
            self.assertEqual((self.site / name).read_bytes(), (ROOT / name).read_bytes())

    def test_every_generated_page_keeps_local_links_under_the_project_path(self):
        targets = set()
        pages = list(self.site.rglob("index.html"))
        self.assertTrue(pages)
        for page in pages:
            url = self.origin + SITE_PATH + page.relative_to(self.site).as_posix()
            links = Links()
            links.feed(page.read_text(encoding="utf-8"))
            for value in links.targets:
                target = urljoin(url, value)
                parsed = urlparse(target)
                if parsed.netloc == urlparse(self.origin).netloc:
                    self.assertTrue(parsed.path.startswith(SITE_PATH), (page, value))
                    targets.add(target.split("#", 1)[0])
        self.assertIn(self.origin + SITE_PATH + "quickstart/", targets)
        self.assertTrue(any(urlparse(target).path.endswith(".css") for target in targets))
        self.assertTrue(any(urlparse(target).path.endswith(".js") for target in targets))
        for target in targets:
            with self.subTest(target=target), urlopen(target, timeout=5) as response:
                self.assertEqual(response.status, 200)
                self.assertTrue(response.read())


class DocumentationWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = yaml.load(
            (ROOT / ".github/workflows/publish-docs.yml").read_text(), Loader=yaml.BaseLoader,
        )

    def test_public_contributions_use_read_only_hosted_builds(self):
        workflow = self.workflow
        self.assertEqual(workflow["permissions"], {"contents": "read"})
        build = workflow["jobs"]["build"]
        self.assertNotIn("if", build)
        self.assertEqual(build["runs-on"], "ubuntu-24.04")
        self.assertEqual(build["permissions"], {"contents": "read", "pages": "read"})
        deploy = workflow["jobs"]["deploy"]
        self.assertEqual(deploy["runs-on"], "ubuntu-24.04")
        self.assertEqual(deploy["needs"], "build")
        self.assertEqual(deploy["permissions"], {"pages": "write", "id-token": "write"})
        self.assertEqual(deploy["environment"]["name"], "github-pages")
        steps = build["steps"]
        bootstrap = next(step for step in steps if step.get("name") == "Install pinned RTK")
        first_shell = next(step for step in steps if "run" in step)
        self.assertLess(steps.index(bootstrap), steps.index(first_shell))
        self.assertIn("RTK archive checksum mismatch", bootstrap["with"]["script"])
        self.assertTrue(workflow["defaults"]["run"]["shell"].startswith("rtk proxy bash "))
        upload = next(step for step in steps if step.get("name") == "Upload GitHub Pages artifact")
        self.assertEqual(upload["with"]["path"], "site")
        self.assertEqual(upload["with"]["retention-days"], "7")
        self.assertNotIn("continue-on-error", upload)

    def test_local_jobs_require_the_public_job_policy_and_exclude_forks(self):
        for path in (ROOT / ".github/workflows").glob("*.yml"):
            if path.name == "publish-docs.yml":
                continue
            workflow = yaml.load(path.read_text(), Loader=yaml.BaseLoader)
            for name, job in workflow["jobs"].items():
                with self.subTest(workflow=path.name, job=name):
                    self.assertNotIn("repository.private", job["if"])
                    self.assertIn("pull_request", job["if"])
                    self.assertIn("UA_CI_PUBLIC_JOB_POLICY_VERSION", job["steps"][0]["run"])

    def test_publish_conditions_allow_main_and_stable_tags_only(self):
        workflow = self.workflow
        steps = workflow["jobs"]["build"]["steps"]
        condition = workflow["jobs"]["deploy"]["if"]
        for name in ("Configure GitHub Pages", "Upload GitHub Pages artifact"):
            self.assertEqual(next(step["if"] for step in steps if step.get("name") == name), condition)
        cases = [
            ("push", "refs/heads/main", True),
            ("push", "refs/tags/v1.2.3", True),
            ("workflow_dispatch", "refs/heads/main", True),
            ("workflow_dispatch", "refs/tags/v1.2.3", True),
            ("pull_request", "refs/pull/123/merge", False),
            ("pull_request", "refs/heads/main", False),
            ("workflow_dispatch", "refs/heads/feature/docs", False),
            ("workflow_dispatch", "refs/tags/v1.2.3-beta.1", False),
            ("workflow_dispatch", "refs/tags/v1.2.3-rc.1", False),
            ("workflow_dispatch", "refs/tags/v1.2.3-nightly.202610280317", False),
        ]
        contexts = [
            {"event_name": event, "ref": ref, "event": {"repository": {"private": private}}}
            for private in (False, True) for event, ref, _ in cases
        ]
        # These conditions use a JavaScript-compatible subset of GitHub expressions.
        completed = subprocess.run(
            ["node", "--eval",
             ("const {condition, contexts} = JSON.parse(process.argv[1]);"
             "const decide = new Function('github', 'startsWith', 'contains', 'return (' + condition + ')');"
             "process.stdout.write(JSON.stringify(contexts.map(github => "
             "decide(github, (s,p) => s.startsWith(p), (s,p) => s.includes(p)))));"),
             json.dumps({"condition": condition, "contexts": contexts})],
            check=True, capture_output=True, text=True, timeout=10,
        )
        self.assertEqual(json.loads(completed.stdout), [allowed for _, _, allowed in cases] * 2)

    def test_triggers_exclude_prerelease_tags_and_manual_tags_are_validated(self):
        triggers = self.workflow["on"]
        self.assertIn("pull_request", triggers)
        self.assertIn("workflow_dispatch", triggers)
        self.assertEqual(triggers["push"]["branches"], ["main"])
        patterns = triggers["push"]["tags"]
        validate = next(step for step in self.workflow["jobs"]["build"]["steps"]
                        if step.get("name") == "Validate stable documentation tag")
        self.assertEqual(validate["if"], "github.ref_type == 'tag'")
        for tag, expected in (("v1.2.3", True), ("v1.2.3-beta.1", False), ("v01.2.3", False)):
            with self.subTest(tag=tag):
                selected = False
                for pattern in patterns:
                    if fnmatchcase(tag, pattern.lstrip("!")):
                        selected = not pattern.startswith("!")
                if "-" in tag:
                    self.assertFalse(selected)
                result = subprocess.run(
                    ["rtk", "proxy", "bash", "-c", validate["run"]],
                    env={**os.environ, "GITHUB_REF_NAME": tag}, capture_output=True, text=True,
                    timeout=10, check=False,
                )
                self.assertEqual(result.returncode == 0, expected)


if __name__ == "__main__":
    unittest.main()
