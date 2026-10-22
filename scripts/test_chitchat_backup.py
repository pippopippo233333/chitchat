"""Filesystem/SQLite backup regression checks; no live host or data is used.
On Windows only, flock is stubbed: concurrency locking is tested on Linux.
Run: python -m unittest discover -s scripts -p 'test_*.py' -v
"""
import argparse
from contextlib import closing
import importlib.util
import json
import os
from pathlib import Path
import sqlite3
import sys
import tempfile
import types
import unittest
from unittest.mock import patch
if os.name == 'nt':
    sys.modules.setdefault('fcntl', types.SimpleNamespace(LOCK_EX=2, LOCK_NB=4, flock=lambda *args: None))
spec = importlib.util.spec_from_file_location('backup', Path(__file__).with_name('chitchat-backup.py'))
backup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(backup)


def fixture(data):
    folder = data / 'servers' / 'main'
    folder.mkdir(parents=True)
    db = sqlite3.connect(folder / 'chitchat.db')
    db.executescript('CREATE TABLE attachments(blob_hash TEXT,thumb_hash TEXT); CREATE TABLE messages(id INTEGER); CREATE TABLE users(id INTEGER); INSERT INTO messages VALUES(1); INSERT INTO users VALUES(1);')
    db.commit()
    db.close()
    return folder


class BackupRegression(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
    def tearDown(self):
        self.temp.cleanup()
    def test_same_size_and_timestamp_change_is_preserved(self):
        source, previous, target = [self.root / n for n in ['source', 'previous', 'target']]
        source.mkdir(); previous.mkdir()
        (source / 'host.toml').write_text('BBBB')
        (previous / 'host.toml').write_text('AAAA')
        os.utime(source / 'host.toml', ns=(100000000000,100000000000))
        os.utime(previous / 'host.toml', ns=(100000000000,100000000000))
        backup.copy_tree(source,target,previous,{'copied':0,'linked':0,'copied_bytes':0})
        self.assertEqual((target / 'host.toml').read_text(),'BBBB')
        self.assertEqual((previous / 'host.toml').read_text(),'AAAA')
    def test_retention_preserves_unrelated_folders(self):
        data, dest = self.root / 'data', self.root / 'backups'
        fixture(data);dest.mkdir()
        for name in ['notes','2010-01-01_000000']:
            folder=dest / name;folder.mkdir();(folder / 'keep.txt').write_text('unrelated')
        args=argparse.Namespace(data=str(data),dest=str(dest),keep=1)
        self.assertEqual(backup.cmd_backup(args),0)
        for name in ['notes','2010-01-01_000000']:
            self.assertEqual((dest / name / 'keep.txt').read_text(),'unrelated')
    def test_backup_rejects_destination_containing_live_data(self):
        data=self.root / 'data';fixture(data)
        args=argparse.Namespace(data=str(data),dest=str(self.root),keep=100)
        self.assertEqual(backup.cmd_backup(args),2)
        self.assertTrue((data / 'servers/main/chitchat.db').is_file())
    def test_restore_rejects_target_inside_snapshot_before_copying(self):
        snap=self.root / 'snapshot';fixture(snap)
        args=argparse.Namespace(snapshot=str(snap),to=str(snap / 'nested'))
        with patch.object(backup.shutil,'copytree') as copy:
            self.assertEqual(backup.cmd_restore(args),2)
            copy.assert_not_called()
    def test_online_sqlite_backup_includes_committed_wal(self):
        folder=fixture(self.root / 'data');db=folder / 'chitchat.db'
        writer=sqlite3.connect(db)
        try:
            writer.execute('PRAGMA journal_mode=WAL')
            writer.execute('INSERT INTO messages VALUES(2)');writer.commit()
            writer.execute('INSERT INTO messages VALUES(3)')
            output=self.root / 'copy.db';backup.backup_db(db,output)
            with closing(sqlite3.connect(output)) as copy:
                self.assertEqual(copy.execute('SELECT COUNT(*) FROM messages').fetchone()[0],2)
                self.assertEqual(copy.execute('PRAGMA integrity_check').fetchone()[0],'ok')
        finally:
            writer.rollback();writer.close()
    def test_failed_snapshot_keeps_previous_good_backup(self):
        data,dest=self.root / 'data',self.root / 'backups'
        folder=fixture(data);previous=dest / '2020-01-01_000000';fixture(previous)
        (previous / 'backup-report.json').write_text(json.dumps({'ok':True}))
        with closing(sqlite3.connect(folder / 'chitchat.db')) as db:
            db.execute('INSERT INTO attachments VALUES(?,NULL)',('abc123',));db.commit()
        args=argparse.Namespace(data=str(data),dest=str(dest),keep=1)
        with patch.object(backup.time,'sleep'):
            self.assertEqual(backup.cmd_backup(args),1)
        self.assertTrue((previous / 'servers/main/chitchat.db').is_file())
        self.assertEqual(backup.snapshots(dest),[previous])
    def test_missing_file_during_live_copy_retries_before_publishing(self):
        data,dest=self.root / 'data',self.root / 'backups';folder=fixture(data)
        media=folder / 'files/ab/abc123';media.parent.mkdir(parents=True);media.write_bytes(b'audio')
        with closing(sqlite3.connect(folder / 'chitchat.db')) as db:
            db.execute('INSERT INTO attachments VALUES(?,NULL)',('abc123',));db.commit()
        copy=backup.copy_tree;calls=0
        def racing_copy(source,target,previous,stats):
            nonlocal calls
            copy(source,target,previous,stats);calls+=1
            if calls==1:(target / 'servers/main/files/ab/abc123').unlink()
        args=argparse.Namespace(data=str(data),dest=str(dest),keep=1)
        with patch.object(backup,'copy_tree',racing_copy),patch.object(backup.time,'sleep'):
            self.assertEqual(backup.cmd_backup(args),0)
        self.assertEqual(calls,2)
        snapshots=backup.snapshots(dest);self.assertEqual(len(snapshots),1)
        self.assertTrue(backup.check_snapshot(snapshots[0])['ok'])
    @unittest.skipIf(os.name=='nt','real flock requires Linux')
    def test_overlapping_backup_is_skipped(self):
        data,dest=self.root / 'data',self.root / 'backups';fixture(data);dest.mkdir()
        with open(dest / '.lock','w') as lock:
            backup.fcntl.flock(lock,backup.fcntl.LOCK_EX|backup.fcntl.LOCK_NB)
            args=argparse.Namespace(data=str(data),dest=str(dest),keep=1)
            self.assertEqual(backup.cmd_backup(args),0)
            self.assertEqual(backup.snapshots(dest),[])
    def test_missing_attachment_fails_verification(self):
        snap=self.root / 'snapshot';folder=fixture(snap)
        with closing(sqlite3.connect(folder / 'chitchat.db')) as db:
            db.execute('INSERT INTO attachments VALUES(?,NULL)',('abc123',));db.commit()
        self.assertFalse(backup.check_snapshot(snap)['ok'])

if __name__ == '__main__':
    unittest.main()
