"""Exercise Make release ordering with a fake Cargo; never contact a registry."""
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]

class ReleaseWorkflowTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.home = Path(temporary.name)
        self.root = self.home / "project"
        self.root.mkdir()
        (self.root / "scripts").mkdir()
        (self.root / "tests").mkdir()
        (self.root / "tests/release_workflow.py").write_text(
            "import unittest, pathlib\n"
            "class FixtureChecks(unittest.TestCase):\n"
            " def test_manifest_present(self): self.assertTrue(pathlib.Path('Cargo.toml').is_file())\n"
        )
        (self.root / "docs").mkdir()
        shutil.copyfile(ROOT / "Makefile", self.root / "Makefile")
        shutil.copyfile(ROOT / "scripts/release.py", self.root / "scripts/release.py")
        for name in ["README.md", "LICENSE", "CHANGELOG.md", "docs/providers.md", "docs/releases.md"]:
            (self.root / name).write_text("## Unreleased\n\n- Release fixture\n")
        (self.root / "Cargo.toml").write_text('[package]\nname="token-manager"\nversion="0.1.0"\n')
        (self.root / "Cargo.lock").write_text('[[package]]\nname="token-manager"\nversion="0.1.0"\n')
        (self.root / ".gitignore").write_text('/target/\n/dist/\n__pycache__/\n')
        for args in [["init", "-qb", "main"], ["add", "."], ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "core.hooksPath=/dev/null", "commit", "-qm", "fixture"]]:
            subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True)
        for key, value in [("user.name", "Fixture"), ("user.email", "fixture@example.invalid"), ("core.hooksPath", "/dev/null")]:
            subprocess.run(["git", "config", key, value], cwd=self.root, check=True)
        remote = self.home / "remote.git"
        subprocess.run(["git", "init", "--bare", "-q", str(remote)], check=True)
        subprocess.run(["git", "remote", "add", "origin", str(remote)], cwd=self.root, check=True)
        subprocess.run(["git", "push", "-qu", "origin", "main"], cwd=self.root, check=True)
        self.log = self.home / "cargo-calls.jsonl"
        self.cargo = self.home / "cargo-fixture"
        self.cargo.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys, tomllib
args=sys.argv[1:]
with open(os.environ['CARGO_FIXTURE_LOG'],'a') as log: log.write(json.dumps(args)+'\\n')
if args[0]=='build':
 p=pathlib.Path('target/release/token-manager');p.parent.mkdir(parents=True,exist_ok=True)
 version=tomllib.loads(pathlib.Path('Cargo.toml').read_text())['package']['version']
 p.write_text('#!/bin/sh\\necho token-manager '+version+'\\n');p.chmod(0o755)
if args[0]=='publish' and '--dry-run' in args and os.environ.get('FAIL_DRY_RUN'): sys.exit(23)
if args[0]=='publish' and '--dry-run' not in args and os.environ.get('FAIL_PUBLISH'): sys.exit(24)
''')
        self.cargo.chmod(0o755)

    def run_make(self, target, *options, **environment):
        env = dict(os.environ, CARGO_FIXTURE_LOG=str(self.log), **environment)
        for name in ["MAKEFLAGS", "MFLAGS", "MAKEOVERRIDES", "RELEASE_FLAGS"]:
            env.pop(name, None)
        return subprocess.run(["make", target, f"CARGO={self.cargo}", *options], cwd=self.root,
                              env=env, text=True, capture_output=True, timeout=30)

    def publishes(self):
        calls = [json.loads(line) for line in self.log.read_text().splitlines()]
        return [call for call in calls if call[0] == "publish"]

    def test_release_check_only_dry_runs(self):
        result = self.run_make("release-check")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.publishes(), [["publish", "--locked", "--registry", "crates-io", "--dry-run"]])

    def test_release_uploads_only_after_dry_run(self):
        result = self.run_make("release", "VERSION=20261122.0.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.publishes(), [
            ["publish", "--locked", "--registry", "crates-io", "--dry-run"],
            ["publish", "--locked", "--registry", "crates-io"]])
        self.assertEqual(subprocess.check_output(["git", "tag"], cwd=self.root), b"v20261122.0.0\n")
        self.assertEqual(subprocess.check_output(["git", "cat-file", "-t", "v20261122.0.0"], cwd=self.root).strip(), b"tag")
        remote_tag = subprocess.check_output(["git", "ls-remote", "--tags", "origin", "refs/tags/v20261122.0.0"], cwd=self.root)
        self.assertTrue(remote_tag.strip())
        self.assertIn("## 20261122.0.0", (self.root / "CHANGELOG.md").read_text())

    def test_failed_dry_run_prevents_upload(self):
        result = self.run_make("release", FAIL_DRY_RUN="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(all("--dry-run" in call for call in self.publishes()))

    def test_dirty_preview_cannot_publish(self):
        (self.root / "README.md").write_text("Uncommitted change\n")
        result = self.run_make("release", "RELEASE_FLAGS=--allow-dirty")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("only for previews", result.stderr)
        self.assertFalse(self.log.exists())

    def test_date_version_increments_same_day_tags(self):
        import datetime
        today = datetime.date.today().strftime("%Y%m%d")
        subprocess.run(["git", "tag", f"v{today}.0.2"], cwd=self.root, check=True)
        result = subprocess.check_output(["python3", "scripts/release.py", "next-version"], cwd=self.root, text=True).strip()
        self.assertEqual(result, f"{today}.0.3")

    def test_upload_failure_is_not_reported_as_success(self):
        result = self.run_make("release", FAIL_PUBLISH="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(self.publishes()), 2)

    @unittest.skipUnless(platform.system() == "Linux" and platform.machine() == "x86_64", "Native archive supports Linux x86_64")
    def test_local_archive_never_invokes_publish(self):
        result = self.run_make("release-archive")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.publishes(), [])
        self.assertTrue((self.root / "dist/token-manager-0.1.0-linux-x86_64.tar.gz").exists())
