"""Release validation, local archives and explicitly invoked date-version publication."""
import argparse
from datetime import date, datetime
import re
import shlex
import gzip
import hashlib
import io
import json
import os
import platform
from pathlib import Path
import subprocess
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]

def require(condition, message):
    if not condition:
        raise ValueError(message)

def metadata():
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
    package = manifest["package"]
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    require(any(p["name"] == package["name"] and p["version"] == package["version"] for p in lock["package"]), "Cargo version/lock mismatch")
    return package["version"]

def check(allow_dirty):
    version = metadata()
    require(all((ROOT / f).is_file() for f in ["README.md", "LICENSE", "CHANGELOG.md", "docs/providers.md", "docs/releases.md"]), "Required release documentation missing")
    binary = ROOT / "target/release/token-manager"
    require(binary.is_file() and not binary.is_symlink(), "Release binary missing")
    result = subprocess.check_output([str(binary), "--version"], text=True).strip()
    require(result == f"token-manager {version}", "Binary version mismatch")
    alias = ROOT / "target/release/tm"
    require(alias.is_file() and not alias.is_symlink(), "tm binary missing")
    require(subprocess.check_output([str(alias), "--version"], text=True).strip() == result, "tm binary version mismatch")
    state = subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True)
    require(not state or allow_dirty, "Working tree is dirty; --allow-dirty permits only a local preview")
    tag = subprocess.run(["git", "show-ref", "--verify", "--quiet", f"refs/tags/v{version}"], cwd=ROOT)
    if tag.returncode == 0:
        tagged = subprocess.check_output(["git", "rev-parse", f"v{version}^{{commit}}"], cwd=ROOT, text=True).strip()
        head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
        require(tagged == head, "Release tag belongs to another source commit")
    print("PASS local release readiness" + (" (uncommitted preview)" if state else ""))
    return binary, version

def prepare(allow_dirty):
    require(platform.system() == "Linux" and platform.machine() == "x86_64", "Only native Linux x86_64 packaging is verified")
    binary, version = check(allow_dirty)
    target = ROOT / "dist"
    require(not target.is_symlink(), "Refusing symlink output directory")
    target.mkdir(exist_ok=True)
    archive = target / f"token-manager-{version}-linux-x86_64.tar.gz"
    files = {"token-manager": binary.read_bytes(), "tm": (ROOT / "target/release/tm").read_bytes(), "README.md": (ROOT / "README.md").read_bytes(), "LICENSE": (ROOT / "LICENSE").read_bytes()}
    memory = io.BytesIO()
    with gzip.GzipFile(fileobj=memory, mode="wb", mtime=0, filename="") as zipped:
        with tarfile.open(fileobj=zipped, mode="w") as tar:
            for name, data in sorted(files.items()):
                entry = tarfile.TarInfo(name)
                entry.size = len(data)
                entry.mode = 0o755 if name in {"token-manager", "tm"} else 0o644
                tar.addfile(entry, io.BytesIO(data))
    data = memory.getvalue()
    digest = hashlib.sha256(data).hexdigest()
    for path, content in [(archive, data), (archive.with_suffix(archive.suffix + ".sha256"), f"{digest}  {archive.name}\n".encode())]:
        require(not path.is_symlink(), "Refusing output symlink")
        if path.exists():
            require(path.read_bytes() == content, "Different existing artifact retained; inspect it manually")
        else:
            with path.open("xb") as file:
                file.write(content)
    with tarfile.open(archive) as tar:
        require(set(tar.getnames()) == set(files), "Unexpected archive contents")
    print(archive)

def clean():
    target = ROOT / "target"
    require(not target.is_symlink(), "Refusing target symlink")
    require(not os.environ.get("CARGO_TARGET_DIR") or Path(os.environ["CARGO_TARGET_DIR"]).resolve() == target, "Refusing external Cargo target")
    if target.exists():
        require(set(p.name for p in target.iterdir()) <= {"debug", "release", "tmp", "package", ".rustc_info.json", "CACHEDIR.TAG"}, "Unknown files in target; inspect manually")
        subprocess.run(["cargo", "clean", "--target-dir", str(target)], cwd=ROOT, check=True)

def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()

def next_version():
    today = date.today().strftime("%Y%m%d")
    patches = [int(match.group(1)) for tag in git("tag", "--list", f"v{today}.0.*").splitlines()
               if (match := re.fullmatch(r"v" + today + r"\.0\.(\d+)", tag))]
    return f"{today}.0.{max(patches, default=-1) + 1}"

def publish(version, cargo, allow_dirty):
    require(not allow_dirty, "--allow-dirty is only for previews, never release")
    require(git("branch", "--show-current") == "main", "Release must start on main; merge reviewed changes first")
    require(not git("status", "--porcelain"), "Working tree is dirty; commit changes before release")
    git("remote", "get-url", "origin")
    subprocess.run(["git", "fetch", "--tags", "origin"], cwd=ROOT, check=True)
    require(git("rev-parse", "HEAD") == git("rev-parse", "origin/main"), "main and origin/main must match before release")
    version = version or next_version()
    require(re.fullmatch(r"\d{8}\.0\.\d+", version), "Version must use YYYYMMDD.0.PATCH")
    datetime.strptime(version[:8], "%Y%m%d")
    tag = "v" + version
    require(not git("tag", "--list", tag), "Release tag already exists")
    require(not git("ls-remote", "--tags", "origin", "refs/tags/" + tag), "Remote release tag already exists")
    initial = git("rev-parse", "HEAD")
    subprocess.run(["git", "switch", "-c", "release/" + tag], cwd=ROOT, check=True)
    manifest_path = ROOT / "Cargo.toml"
    manifest = manifest_path.read_text()
    old = tomllib.loads(manifest)["package"]["version"]
    manifest, changed = re.subn(r'(?ms)(^\[package\]\s*\n(?:(?!^\[).)*?^version\s*=\s*")[^"]+("[^\n]*)',
                                 lambda m: m[1] + version + m[2], manifest, count=1)
    require(changed == 1, "Cannot locate package version")
    lock_path = ROOT / "Cargo.lock"
    lock = lock_path.read_text()
    lock, changed = re.subn(r'(?m)(^name\s*=\s*"token-manager"\s*\nversion\s*=\s*")[^"]+("$)',
                             lambda m: m[1] + version + m[2], lock)
    require(changed == 1, "Cannot locate root lockfile version")
    changelog_path = ROOT / "CHANGELOG.md"
    changelog = changelog_path.read_text()
    require("## Unreleased\n" in changelog, "Missing Unreleased changelog section")
    manifest_path.write_text(manifest)
    lock_path.write_text(lock)
    changelog_path.write_text(changelog.replace("## Unreleased\n", f"## Unreleased\n\n## {version}\n", 1))
    subprocess.run(["git", "add", "Cargo.toml", "Cargo.lock", "CHANGELOG.md"], cwd=ROOT, check=True)
    subprocess.run(["git", "commit", "-m", f"chore: release {version} (from {old})"], cwd=ROOT, check=True)
    subprocess.run(["make", "release-check", "CARGO=" + cargo, "RELEASE_FLAGS="], cwd=ROOT, check=True)
    require(not git("status", "--porcelain"), "Release checks changed source; inspect before continuing")
    require(git("rev-parse", "main") == initial, "main changed during validation")
    subprocess.run(["git", "switch", "main"], cwd=ROOT, check=True)
    subprocess.run(["git", "merge", "--no-ff", "release/" + tag, "-m", "Merge release " + tag], cwd=ROOT, check=True)
    subprocess.run(["git", "tag", "-a", tag, "-m", "Release " + version], cwd=ROOT, check=True)
    subprocess.run(["git", "push", "--atomic", "origin", "main", "refs/tags/" + tag], cwd=ROOT, check=True)
    require(not git("status", "--porcelain"), "Source changed before publication")
    subprocess.run([*shlex.split(cargo), "publish", "--locked", "--registry", "crates-io"], cwd=ROOT, check=True)
    print("Released " + version + ": main and annotated tag pushed; crates.io publication complete")

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("action", choices=["check", "prepare", "clean", "next-version", "release"])
parser.add_argument("--allow-dirty", action="store_true")
parser.add_argument("--version", default="")
parser.add_argument("--cargo", default="cargo")
args = parser.parse_args()
try:
    if args.action == "check": check(args.allow_dirty)
    elif args.action == "prepare": prepare(args.allow_dirty)
    elif args.action == "next-version": print(next_version())
    elif args.action == "release": publish(args.version, args.cargo, args.allow_dirty)
    else: clean()
except (ValueError, OSError, subprocess.SubprocessError) as error:
    raise SystemExit(str(error))
