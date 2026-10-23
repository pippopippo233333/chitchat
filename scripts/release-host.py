"""Builds, signs and publishes a host release to GitHub Releases, where running hosts find it
(Resources → Update, or `chitchat-host update`).

    python scripts/release-host.py --linux-ssh user@buildbox --notes "What changed"
    python scripts/release-host.py --linux-ssh user@buildbox --notes "..." --dry-run

Before running: raise "version" in crates/chitchat-host/Cargo.toml (hosts only update to a higher
version) and commit: the release is built from the last commit, not from uncommitted edits.

Windows is built here; Linux is built over SSH on an Ubuntu machine with Rust (--linux-ssh, or
$CHITCHAT_LINUX_BUILD), in a fresh ~/chitchat-release-build folder that is not touched by anything
else. Signing uses the desktop updater key, ~/.tauri/chitchat-updater.key (never commit or share
it). Host releases are never marked "latest": that release is the desktop app's.
"""

import argparse
import io
import os
import re
import shlex
import shutil
import subprocess
import sys
import tarfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REPO = "pippopippo233333/chitchat"
KEY = Path.home() / ".tauri" / "chitchat-updater.key"
OUT = ROOT / "target" / "host-release"
REMOTE_DIR = "chitchat-release-build"


def run(cmd, **kw):
    print("+", " ".join(str(c) for c in cmd), flush=True)
    return subprocess.run(cmd, check=True, **kw)


def version() -> str:
    text = (ROOT / "crates" / "chitchat-host" / "Cargo.toml").read_text(encoding="utf-8")
    return re.search(r'^version\s*=\s*"([^"]+)"', text, re.M).group(1)


def package(name: str, binary: Path, bin_name: str, web: Path, ver: str) -> Path:
    """A tar.gz with only plain files: the binary, web/ and VERSION."""
    path = OUT / name

    def add(tar, arcname, data, mode):
        info = tarfile.TarInfo(arcname)
        info.size, info.mode = len(data), mode
        tar.addfile(info, io.BytesIO(data))

    with tarfile.open(path, "w:gz") as tar:
        add(tar, bin_name, binary.read_bytes(), 0o755)
        add(tar, "VERSION", ver.encode(), 0o644)
        for f in sorted(web.rglob("*")):
            if f.is_file():
                add(tar, "web/" + f.relative_to(web).as_posix(), f.read_bytes(), 0o644)
    return path


def sign(path: Path):
    env = dict(os.environ, TAURI_SIGNING_PRIVATE_KEY=KEY.read_text(encoding="utf-8").strip(), TAURI_SIGNING_PRIVATE_KEY_PASSWORD="")
    npx = "npx.cmd" if os.name == "nt" else "npx"
    run([npx, "tauri", "signer", "sign", str(path)], cwd=ROOT / "desktop", env=env, stdout=subprocess.DEVNULL)
    sig = path.with_name(path.name + ".sig")
    if not sig.is_file():
        sys.exit(f"signing made no {sig.name}")
    return sig


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--notes", required=True, help="what's new (shown on the host's update card)")
    ap.add_argument("--linux-ssh", default=os.environ.get("CHITCHAT_LINUX_BUILD"), help="user@host of an Ubuntu build machine")
    ap.add_argument("--dry-run", action="store_true", help="build, sign and check, but don't publish")
    args = ap.parse_args()

    ver = version()
    tag = f"host-v{ver}"
    if not KEY.is_file():
        sys.exit(f"signing key missing: {KEY}")
    if not args.linux_ssh:
        sys.exit("give --linux-ssh user@host (or set CHITCHAT_LINUX_BUILD) for the Linux build")
    if subprocess.run(["git", "status", "--porcelain", "--", "crates", "web/src", "web/public", "Cargo.lock"], cwd=ROOT, capture_output=True, text=True).stdout.strip():
        print("note: uncommitted changes in crates/ or web/ are NOT in this release (it builds the last commit)")
    if not args.dry_run and subprocess.run(["gh", "release", "view", tag, "-R", REPO], capture_output=True).returncode == 0:
        sys.exit(f"{tag} is already released: raise the version in crates/chitchat-host/Cargo.toml first")

    if OUT.exists():
        shutil.rmtree(OUT)
    OUT.mkdir(parents=True)
    # Build from a clean export of the last commit, so local edits can't slip in.
    src = OUT / "src"
    src.mkdir()
    archive = subprocess.run(["git", "archive", "--format=tar", "HEAD"], cwd=ROOT, capture_output=True, check=True).stdout
    tarfile.open(fileobj=io.BytesIO(archive)).extractall(src, filter="data")

    npm = "npm.cmd" if os.name == "nt" else "npm"
    run([npm, "ci", "--prefix", "web"], cwd=src)
    run([npm, "--prefix", "web", "run", "build"], cwd=src)
    web = src / "web" / "dist"

    run(["cargo", "build", "--release", "-p", "chitchat-host", "--target-dir", str(ROOT / "target" / "host-release-build")], cwd=src)
    win_bin = ROOT / "target" / "host-release-build" / "release" / "chitchat-host.exe"

    remote = shlex.quote(REMOTE_DIR)
    run(["ssh", args.linux_ssh, f"rm -rf ~/{remote} && mkdir ~/{remote}"])
    subprocess.run(["ssh", args.linux_ssh, f"tar -x -C ~/{remote}"], input=archive, check=True)
    run(["ssh", args.linux_ssh, f"cd ~/{remote} && . ~/.cargo/env 2>/dev/null; cargo build --release -p chitchat-host"])
    linux_bin = OUT / "chitchat-host-linux"
    run(["scp", f"{args.linux_ssh}:{REMOTE_DIR}/target/release/chitchat-host", str(linux_bin)])

    files = []
    for platform, binary, bin_name in (("windows-x86_64", win_bin, "chitchat-host.exe"), ("linux-x86_64", linux_bin, "chitchat-host")):
        path = package(f"chitchat-host-{ver}-{platform}.tar.gz", binary, bin_name, web, ver)
        sig = sign(path)
        run([str(win_bin), "update", "--verify", str(path)])  # the new host accepts its own release
        files += [path, sig]
    print("built:", ", ".join(f.name for f in files))
    if args.dry_run:
        return

    body = (
        f"{args.notes}\n\n**Updating:** hosts show this under Resources → Update, or run `chitchat-host update` with the host stopped. "
        "Databases are backed up first, and if the new version fails to start the old one comes back by itself.\n\n"
        "**New install:** unpack the archive for your system; it has the host binary and the web client (`web/`). See docs/hosting.md."
    )
    run(["gh", "release", "create", tag, *map(str, files), "-R", REPO, "--title", f"ChitChat host {ver}", "--notes", body, "--latest=false"])
    print(f"published {tag}: hosts will offer the update")


if __name__ == "__main__":
    main()
