"""Local Linux CLI packaging only. Never commit, tag, push or publish."""
import argparse
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
    state = subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True)
    require(not state or allow_dirty, "Working tree is dirty; --allow-dirty permits only a local preview")
    tag = subprocess.run(["git", "show-ref", "--verify", "--quiet", f"refs/tags/v{version}"], cwd=ROOT)
    require(tag.returncode != 0, "Release tag already exists")
    print("PASS local release readiness" + (" (uncommitted preview)" if state else ""))
    return binary, version

def prepare(allow_dirty):
    require(platform.system() == "Linux" and platform.machine() == "x86_64", "Only native Linux x86_64 packaging is verified")
    binary, version = check(allow_dirty)
    target = ROOT / "dist"
    require(not target.is_symlink(), "Refusing symlink output directory")
    target.mkdir(exist_ok=True)
    archive = target / f"token-manager-{version}-linux-x86_64.tar.gz"
    files = {"token-manager": binary.read_bytes(), "README.md": (ROOT / "README.md").read_bytes(), "LICENSE": (ROOT / "LICENSE").read_bytes()}
    memory = io.BytesIO()
    with gzip.GzipFile(fileobj=memory, mode="wb", mtime=0, filename="") as zipped:
        with tarfile.open(fileobj=zipped, mode="w") as tar:
            for name, data in sorted(files.items()):
                entry = tarfile.TarInfo(name)
                entry.size = len(data)
                entry.mode = 0o755 if name == "token-manager" else 0o644
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
        require(set(p.name for p in target.iterdir()) <= {"debug", "release", "tmp", ".rustc_info.json", "CACHEDIR.TAG"}, "Unknown files in target; inspect manually")
        subprocess.run(["cargo", "clean", "--target-dir", str(target)], cwd=ROOT, check=True)

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("action", choices=["check", "prepare", "clean"])
parser.add_argument("--allow-dirty", action="store_true")
args = parser.parse_args()
try:
    if args.action == "check": check(args.allow_dirty)
    elif args.action == "prepare": prepare(args.allow_dirty)
    else: clean()
except (ValueError, OSError, subprocess.SubprocessError) as error:
    raise SystemExit(str(error))
