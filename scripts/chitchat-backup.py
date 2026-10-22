#!/usr/bin/env python3
"""Backups of a ChitChat host's data directory, safe while the host is running.

    chitchat-backup.py backup  [--data DIR] [--dest DIR] [--keep N]
    chitchat-backup.py verify  SNAPSHOT
    chitchat-backup.py restore SNAPSHOT --to NEW_DIR

backup: copies every server database with SQLite's online backup (a consistent copy even
while people chat; the WAL is included), then every stored file and the host's settings and
certificates, into DEST/<date-time>/. Stored files (named by their content hash, never
changed) already in the last snapshot are hard links to it, so each snapshot only costs the
new files; settings are always copied. The snapshot is checked
(database integrity, every attachment's file present) before it is published, and only then
are the oldest snapshots beyond --keep deleted.

restore: copies a snapshot into a NEW directory and checks it; it never touches the live data.
To switch to it: stop the host, move the live data aside, move the restored copy in its place,
start the host (see docs/hosting.md).

Python 3 standard library only. Snapshots are private (mode 700): they hold password hashes,
messages and private keys.
"""

import argparse
import fcntl
import json
import os
import re
import shutil
import sqlite3
import sys
import time
from pathlib import Path

DB_NAME = "chitchat.db"
SNAPSHOT_NAME = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{6}$")
REPORT = "backup-report.json"
ATTEMPTS = 3
SKIP_NAMES = {f"{DB_NAME}-wal", f"{DB_NAME}-shm", f"{DB_NAME}-journal"}


def log(msg):
    print(msg, flush=True)


def backup_db(src: Path, dst: Path):
    """Consistent copy of a live SQLite database (WAL mode included)."""
    # Read-only URI: the backup never writes to the live database.
    source = sqlite3.connect(f"file:{src}?mode=ro", uri=True, timeout=30)
    target = sqlite3.connect(dst)
    try:
        source.backup(target)
    finally:
        target.close()
        source.close()


def is_blob(rel: Path) -> bool:
    """Stored files (`servers/<slug>/files/ab/abcd…`) are named by their content hash and never
    change, so the same name means the same content. Everything else may change in place."""
    parts = rel.parts
    return len(parts) == 5 and parts[0] == "servers" and parts[2] == "files" and parts[4].startswith(parts[3])


def copy_tree(src: Path, dst: Path, prev: Path | None, stats: dict):
    """Copies src into dst, skipping databases (copied separately) and temp folders.
    Stored files already in the previous snapshot are hard-linked to it; settings and other
    files are always copied."""
    for root, dirs, files in os.walk(src):
        rel = Path(root).relative_to(src)
        dirs[:] = [d for d in dirs if d != "tmp"]
        (dst / rel).mkdir(parents=True, exist_ok=True)
        for name in files:
            if name == DB_NAME or name in SKIP_NAMES or name.endswith(".tmp"):
                continue
            s, d = Path(root) / name, dst / rel / name
            st = s.stat()
            p = prev / rel / name if prev else None
            if p and is_blob(rel / name) and p.is_file() and p.stat().st_size == st.st_size:
                os.link(p, d)
                stats["linked"] += 1
                continue
            shutil.copy2(s, d)
            stats["copied"] += 1
            stats["copied_bytes"] += st.st_size


def check_snapshot(snap: Path) -> dict:
    """Integrity of every database and presence of every attachment's file."""
    report = {"servers": {}, "ok": True}
    servers = snap / "servers"
    for sdir in sorted(servers.iterdir()) if servers.is_dir() else []:
        db = sdir / DB_NAME
        if not db.is_file():
            continue
        con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
        try:
            integrity = con.execute("PRAGMA integrity_check").fetchone()[0]
            version = con.execute("PRAGMA user_version").fetchone()[0]
            hashes = set()
            for blob, thumb in con.execute("SELECT blob_hash, thumb_hash FROM attachments"):
                hashes.add(blob)
                if thumb:
                    hashes.add(thumb)
            messages = con.execute("SELECT COUNT(*) FROM messages").fetchone()[0]
            users = con.execute("SELECT COUNT(*) FROM users").fetchone()[0]
        finally:
            con.close()
        missing = [h for h in hashes if not (sdir / "files" / h[:2] / h).is_file()]
        ok = integrity == "ok" and not missing
        report["ok"] &= ok
        report["servers"][sdir.name] = {
            "integrity": integrity, "schema": version, "messages": messages, "users": users,
            "files": len(hashes), "missing_files": len(missing), "ok": ok,
        }
    if not report["servers"]:
        report["ok"] = False
        report["error"] = "no server databases in the snapshot"
    return report


def snapshots(dest: Path) -> list[Path]:
    """Only folders this script made: a date-time name and a report inside. Nothing else in the
    backup folder is ever pruned."""
    return sorted(p for p in dest.iterdir() if p.is_dir() and SNAPSHOT_NAME.match(p.name) and (p / REPORT).is_file())


def inside(a: Path, b: Path) -> bool:
    """a is b or somewhere inside it."""
    return a.resolve().is_relative_to(b.resolve())


def cmd_backup(args) -> int:
    data, dest = Path(args.data).expanduser(), Path(args.dest).expanduser()
    if not (data / "servers").is_dir():
        log(f"error: {data} doesn't look like a ChitChat data directory (no servers/)")
        return 2
    if inside(dest, data) or inside(data, dest):
        log("error: the backup folder and the data directory must be separate (neither inside the other)")
        return 2
    dest.mkdir(parents=True, exist_ok=True)
    os.chmod(dest, 0o700)
    with open(dest / ".lock", "w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            log("another backup is running; skipping")
            return 0
        return snapshot(data, dest, args.keep)


def snapshot(data: Path, dest: Path, keep: int) -> int:
    started = time.time()
    for old in dest.glob(".partial-*"):
        if old.is_dir():
            shutil.rmtree(old, ignore_errors=True)
    prev_list = snapshots(dest)
    prev = prev_list[-1] if prev_list else None
    # A file can be deleted (a message pruned) between copying a database and copying the
    # files; the check catches it and the snapshot is simply made again.
    for attempt in range(1, ATTEMPTS + 1):
        name = time.strftime("%Y-%m-%d_%H%M%S")
        partial = dest / f".partial-{name}"
        stats = {"copied": 0, "linked": 0, "copied_bytes": 0}
        try:
            old_umask = os.umask(0o077)
            try:
                # Databases first, then files: files added meanwhile are harmless extras.
                for db in sorted((data / "servers").glob(f"*/{DB_NAME}")):
                    out = partial / "servers" / db.parent.name / DB_NAME
                    out.parent.mkdir(parents=True, exist_ok=True)
                    backup_db(db, out)
                copy_tree(data, partial, prev, stats)
            finally:
                os.umask(old_umask)
            report = check_snapshot(partial)
            report.update(created=name, source=str(data), seconds=round(time.time() - started, 1), attempt=attempt, **stats)
            (partial / REPORT).write_text(json.dumps(report, indent=2))
        except Exception as e:
            log(f"error: backup failed: {e}")
            shutil.rmtree(partial, ignore_errors=True)
            return 1
        if report["ok"]:
            partial.rename(dest / name)
            break
        if attempt < ATTEMPTS:
            log(f"snapshot check failed (attempt {attempt}), trying again")
            shutil.rmtree(partial, ignore_errors=True)
            time.sleep(5)
        else:
            log(json.dumps(report, indent=2))
            log(f"error: the snapshot failed its checks; kept for inspection in {partial}")
            return 1

    # Prune only after a good snapshot exists.
    all_snaps = snapshots(dest)
    for old in all_snaps[: max(0, len(all_snaps) - keep)]:
        shutil.rmtree(old)
        log(f"deleted old snapshot {old.name}")
    servers = ", ".join(f"{k} ({v['messages']} messages, {v['files']} files)" for k, v in report["servers"].items())
    log(f"snapshot {name} ok in {report['seconds']} s: {servers}; {stats['copied']} files copied "
        f"({stats['copied_bytes'] // 1024} KB), {stats['linked']} unchanged; keeping {min(len(all_snaps), keep)}")
    return 0


def cmd_verify(args) -> int:
    report = check_snapshot(Path(args.snapshot).expanduser())
    log(json.dumps(report, indent=2))
    return 0 if report["ok"] else 1


def cmd_restore(args) -> int:
    snap, to = Path(args.snapshot).expanduser(), Path(args.to).expanduser()
    if not (snap / "servers").is_dir():
        log(f"error: {snap} is not a snapshot")
        return 2
    if inside(to, snap) or inside(snap, to):
        log("error: restore into a folder outside the snapshot")
        return 2
    if to.exists() and any(to.iterdir()):
        log(f"error: {to} exists and isn't empty; restore only into a new folder")
        return 2
    old_umask = os.umask(0o077)
    try:
        shutil.copytree(snap, to, dirs_exist_ok=True, ignore=shutil.ignore_patterns(REPORT))
    finally:
        os.umask(old_umask)
    report = check_snapshot(to)
    log(json.dumps(report, indent=2))
    if not report["ok"]:
        log("error: the restored copy failed its checks")
        return 1
    log(f"restored into {to}. To use it: stop the host, move the live data folder aside, "
        f"move {to} in its place, start the host.")
    return 0


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("backup")
    b.add_argument("--data", default="~/chitchat/data")
    b.add_argument("--dest", default="~/chitchat-backups")
    b.add_argument("--keep", type=int, default=7, help="snapshots to keep (default 7)")
    v = sub.add_parser("verify")
    v.add_argument("snapshot")
    r = sub.add_parser("restore")
    r.add_argument("snapshot")
    r.add_argument("--to", required=True)
    args = p.parse_args()
    if getattr(args, "keep", 1) < 1:
        p.error("--keep must be at least 1")
    return {"backup": cmd_backup, "verify": cmd_verify, "restore": cmd_restore}[args.cmd](args)


if __name__ == "__main__":
    sys.exit(main())
