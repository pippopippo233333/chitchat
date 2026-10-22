//! Backups as the host admin sees them in Resources: the snapshots made by
//! `scripts/chitchat-backup.py`, and a button to make one now.
//!
//! The backup folder is `$CHITCHAT_BACKUP_DIR`, or `~/chitchat-backups` when it exists (the documented
//! setup). "Back up now" starts the `chitchat-backup.service` systemd user unit when it's
//! installed for this host's data and backup folder (same settings as the daily
//! timer), otherwise runs the script itself.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, Auth};
use crate::host::Host;

const UNIT: &str = "chitchat-backup.service";
const TIMER: &str = "chitchat-backup.timer";
const REPORT: &str = "backup-report.json";
const SCRIPT: &str = "scripts/chitchat-backup.py";
const KEEP: &str = "7";

/// A run started by this process when there's no systemd unit: (running, last run failed).
static OWN_RUN: Mutex<(bool, bool)> = Mutex::new((false, false));

fn backup_dir(data_dir: &Path) -> Option<PathBuf> {
    let dir =
        std::env::var_os("CHITCHAT_BACKUP_DIR").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("chitchat-backups")))?.canonicalize().ok()?;
    let data_dir = data_dir.canonicalize().ok()?;
    // A backup folder inside the data directory would back itself up. One holding another
    // host's snapshots (a second host as the same user) isn't ours to show or add to.
    let other_host = snapshot_dirs(&dir).iter().any(|p| {
        let report: Value = std::fs::read(p.join(REPORT)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        report["source"].as_str().and_then(|s| Path::new(s).canonicalize().ok()).is_none_or(|s| s != data_dir)
    });
    (dir.is_dir() && !dir.starts_with(&data_dir) && !other_host).then_some(dir)
}

/// Folders the backup script made, newest first.
fn snapshot_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.join(REPORT).is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(is_snapshot)).collect())
        .unwrap_or_default();
    found.sort();
    found.reverse();
    found
}

async fn systemctl(args: &[&str]) -> Option<String> {
    let out = tokio::process::Command::new("systemctl").arg("--user").args(args).output().await.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

async fn show(unit: &str, prop: &str) -> Option<String> {
    systemctl(&["show", "--timestamp=unix", "-p", prop, "--value", unit]).await.filter(|v| !v.is_empty())
}

/// The backup unit is used only when it backs up this host's data into this folder, so another
/// host on the same machine (a test copy) never starts or reports the real backups.
async fn uses_unit(data_dir: &Path, dir: &Path) -> bool {
    let Some(exec) = show(UNIT, "ExecStart").await else { return false };
    let arg = |flag: &str| exec.split_whitespace().skip_while(|w| *w != flag).nth(1).map(PathBuf::from).and_then(|p| p.canonicalize().ok());
    arg("--data") == data_dir.canonicalize().ok() && arg("--dest") == dir.canonicalize().ok()
}

fn is_snapshot(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 17
        && b[10] == b'_'
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => true,
            _ => c.is_ascii_digit(),
        })
}

/// Bytes the snapshots really take: hard-linked files are counted once.
fn used_bytes(dirs: &[PathBuf]) -> u64 {
    let mut seen = std::collections::HashSet::<(u64, u64)>::new();
    let mut total = 0;
    let mut stack = dirs.to_vec();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                stack.push(e.path());
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if meta.nlink() > 1 && !seen.insert((meta.dev(), meta.ino())) {
                    continue;
                }
            }
            #[cfg(not(unix))]
            let _ = &mut seen;
            total += meta.len();
        }
    }
    total
}

fn snapshots(dir: &Path) -> (Vec<Value>, u64) {
    let found = snapshot_dirs(dir);
    let list = found
        .iter()
        .map(|p| {
            let file = p.join(REPORT);
            let report: Value = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
            let created = std::fs::metadata(&file).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64);
            let servers = report["servers"].as_object();
            let sum = |key: &str| servers.map_or(0, |s| s.values().filter_map(|v| v[key].as_u64()).sum::<u64>());
            json!({
                "name": p.file_name().and_then(|n| n.to_str()),
                "created": created,
                "ok": report["ok"].as_bool().unwrap_or(false),
                "seconds": report["seconds"].as_f64(),
                "servers": servers.map_or(0, |s| s.len()),
                "messages": sum("messages"),
                "files": sum("files"),
            })
        })
        .collect();
    (list, used_bytes(&found))
}

async fn out(data_dir: &Path) -> Value {
    let Some(dir) = backup_dir(data_dir) else {
        return json!({ "configured": false, "running": false, "scheduled": false, "next_run": null, "last_failed": false, "count": 0, "total_bytes": 0, "snapshots": [] });
    };
    let unit = uses_unit(data_dir, &dir).await;
    let (running, last_failed) = if unit {
        let state = show(UNIT, "ActiveState").await;
        let result = show(UNIT, "Result").await;
        (matches!(state.as_deref(), Some("activating" | "active" | "reloading")), state.as_deref() == Some("failed") || result.as_deref().is_some_and(|r| r != "success"))
    } else {
        *OWN_RUN.lock().unwrap()
    };
    let scheduled = unit && show(TIMER, "ActiveState").await.as_deref() == Some("active");
    let next_run = if scheduled {
        // "@1761192000" with --timestamp=unix.
        show(TIMER, "NextElapseUSecRealtime").await.and_then(|v| v.strip_prefix('@')?.parse::<u64>().ok()).map(|s| s * 1000)
    } else {
        None
    };
    let (list, total) = tokio::task::spawn_blocking(move || snapshots(&dir)).await.unwrap_or_default();
    json!({
        "configured": true,
        "running": running,
        "scheduled": scheduled,
        "next_run": next_run,
        "last_failed": last_failed,
        "count": list.len(),
        "total_bytes": total,
        "snapshots": list,
    })
}

/// `GET /host/backups`
pub async fn get(State(host): State<Arc<Host>>, auth: Auth) -> ApiResult<Json<Value>> {
    super::resources::require_host_admin(&auth)?;
    Ok(Json(out(&host.data_dir).await))
}

/// `POST /host/backups`: start a backup now.
pub async fn run(State(host): State<Arc<Host>>, auth: Auth) -> ApiResult<(StatusCode, Json<Value>)> {
    super::resources::require_host_admin(&auth)?;
    let data_dir = host.data_dir.clone();
    let Some(dir) = backup_dir(&data_dir) else {
        return Err(ApiError::not_found("backups_not_configured", "Backups aren't set up on this host."));
    };
    let current = out(&data_dir).await;
    if current["running"].as_bool() == Some(true) {
        return Err(ApiError::new(StatusCode::CONFLICT, "backup_running", "A backup is already running."));
    }
    if uses_unit(&data_dir, &dir).await {
        systemctl(&["start", "--no-block", UNIT]).await.ok_or_else(ApiError::internal)?;
    } else {
        *OWN_RUN.lock().unwrap() = (true, false);
        let child = tokio::process::Command::new("python3")
            .arg(SCRIPT)
            .args(["backup", "--keep", KEEP, "--data"])
            .arg(&data_dir)
            .arg("--dest")
            .arg(&dir)
            .stdout(std::process::Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                *OWN_RUN.lock().unwrap() = (false, true);
                tracing::warn!("couldn't start {SCRIPT}: {e}");
                return Err(ApiError::internal());
            }
        };
        tokio::spawn(async move {
            let ok = child.wait().await.is_ok_and(|s| s.success());
            *OWN_RUN.lock().unwrap() = (false, !ok);
        });
    }
    super::moderation::audit(&auth.srv, Some(auth.user.id), "backup_run", None, json!({})).await;
    let mut body = out(&data_dir).await;
    body["running"] = json!(true);
    Ok((StatusCode::ACCEPTED, Json(body)))
}
