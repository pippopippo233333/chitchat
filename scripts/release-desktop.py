"""Builds, signs and publishes the desktop app to GitHub Releases, where installed apps find it.

    python scripts/release-desktop.py --notes "What changed"          # build + publish
    python scripts/release-desktop.py --notes "..." --dry-run         # build + latest.json only

Before running: raise "version" in desktop/src-tauri/tauri.conf.json (apps only update to a
higher version). Signing uses the private key in ~/.tauri/chitchat-updater.key (never commit or
share it; without it no further updates can be published for installed apps).
"""

import argparse
import datetime
import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DESKTOP = ROOT / "desktop"
CONF = DESKTOP / "src-tauri" / "tauri.conf.json"
REPO = "pippopippo233333/chitchat"
KEY = Path.home() / ".tauri" / "chitchat-updater.key"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--notes", required=True, help="what's new (shown in the update dialog)")
    ap.add_argument("--dry-run", action="store_true", help="build and write latest.json, but don't publish")
    args = ap.parse_args()

    version = json.loads(CONF.read_text(encoding="utf-8"))["version"]
    tag = f"desktop-v{version}"
    if not KEY.is_file():
        sys.exit(f"signing key missing: {KEY}")
    if not args.dry_run and subprocess.run(["gh", "release", "view", tag, "-R", REPO], capture_output=True).returncode == 0:
        sys.exit(f"{tag} is already released: raise the version in {CONF.relative_to(ROOT)} first")

    env = dict(os.environ, TAURI_SIGNING_PRIVATE_KEY=KEY.read_text(encoding="utf-8").strip(), TAURI_SIGNING_PRIVATE_KEY_PASSWORD="")
    npx = "npx.cmd" if os.name == "nt" else "npx"
    subprocess.run([npx, "tauri", "build", "--bundles", "nsis"], cwd=DESKTOP, env=env, check=True)

    bundle = DESKTOP / "src-tauri" / "target" / "release" / "bundle" / "nsis"
    setup = bundle / f"ChitChat_{version}_x64-setup.exe"
    sig = setup.with_name(setup.name + ".sig")
    for f in (setup, sig):
        if not f.is_file():
            sys.exit(f"missing build output: {f}")
    latest = {
        "version": version,
        "notes": args.notes,
        "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": {
            "windows-x86_64": {
                "signature": sig.read_text(encoding="utf-8").strip(),
                "url": f"https://github.com/{REPO}/releases/download/{tag}/{setup.name}",
            }
        },
    }
    latest_file = bundle / "latest.json"
    latest_file.write_text(json.dumps(latest, indent=2) + "\n", encoding="utf-8")
    print(f"built {setup.name} and {latest_file}")
    if args.dry_run:
        return

    body = f"{args.notes}\n\n**Windows:** download `{setup.name}` and run it. Windows may warn that the app is unrecognised (it isn't code-signed yet): choose *More info* → *Run anyway*. Installed apps update themselves."
    subprocess.run(["gh", "release", "create", tag, str(setup), str(sig), str(latest_file), "-R", REPO, "--title", f"ChitChat desktop {version}", "--notes", body, "--latest"], check=True)
    print(f"published {tag}: installed apps will offer the update")


if __name__ == "__main__":
    main()
