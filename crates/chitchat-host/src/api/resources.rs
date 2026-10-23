//! Host resources: CPU cores and RAM for the host and for the AI classifier (Laya), with
//! limits the host admin sets. Limits are host-wide (one process serves every server), so only
//! the owner of the host's first server may see or change them.
//!
//! Enforcement: on Linux under a systemd *user* service (the documented setup), limits become
//! `CPUQuota=` and `MemoryHigh=`/`MemoryMax=` on the units via `systemctl --user set-property`
//! (they persist across restarts). Anywhere else they're reported but not enforced.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{ApiError, ApiResult, Auth, Body};

const MB: u64 = 1024 * 1024;
const MIN_RAM_MB: u64 = 256;
/// MemoryMax (the hard limit, where the kernel kills the process) sits this far above MemoryHigh.
const HARD_HEADROOM: f64 = 1.25;
const SAMPLE_EVERY: Duration = Duration::from_secs(2);

#[derive(Serialize, Deserialize, Clone, Default)]
struct Limits {
    server_cores: Option<f64>,
    server_ram_mb: Option<u64>,
    ai_cores: Option<f64>,
    ai_ram_mb: Option<u64>,
    /// The systemd user unit that runs the AI classifier.
    #[serde(default = "default_ai_unit")]
    ai_unit: String,
    /// Where the AI's model files are (only measured, for the dashboard).
    ai_model_dir: Option<PathBuf>,
    /// Private calls (voice in direct messages), for every server on this host.
    #[serde(default)]
    calls: CallLimits,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct CallLimits {
    pub enabled: bool,
    /// Calls going on at once, per server.
    pub max_calls: u32,
    /// People in one call, including the caller.
    pub max_people: u32,
}

impl Default for CallLimits {
    fn default() -> Self {
        CallLimits { enabled: true, max_calls: 5, max_people: 4 }
    }
}

/// The host admin's private-call settings (defaults until the host has started measuring).
pub fn call_limits() -> CallLimits {
    STATE.get().map(|s| s.limits.lock().unwrap().calls).unwrap_or_default()
}

fn default_ai_unit() -> String {
    "laya.service".into()
}

/// One process tree (a systemd unit's cgroup, or this process) as last measured.
#[derive(Default, Clone, Copy)]
struct Usage {
    cores: Option<f64>,
    ram_bytes: Option<u64>,
}

#[derive(Default)]
struct Snapshot {
    server: Usage,
    ai: Usage,
    ai_running: bool,
    ai_disk_bytes: Option<u64>,
}

struct State {
    admin_slug: String,
    file: PathBuf,
    data_dir: PathBuf,
    limits: Mutex<Limits>,
    snapshot: Mutex<Snapshot>,
}

static STATE: OnceLock<State> = OnceLock::new();

/// Loads the saved limits, applies them, and starts measuring. `admin_slug` is the server whose
/// owner is the host admin.
pub fn start(data_dir: &Path, admin_slug: String) {
    let file = data_dir.join("host-limits.json");
    let limits: Limits = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| Limits { ai_unit: default_ai_unit(), ..Default::default() });
    let state = STATE.get_or_init(|| State { admin_slug, file, data_dir: data_dir.to_path_buf(), limits: Mutex::new(limits.clone()), snapshot: Mutex::new(Snapshot::default()) });
    tokio::spawn(async move {
        apply(&limits).await;
        sample_forever(state).await;
    });
}

// ---------------------------------------------------------------- measuring (Linux cgroup v2)

/// This process's cgroup, e.g. `/user.slice/user-1000.slice/user@1000.service/app.slice/chitchat.service`.
fn own_cgroup() -> Option<String> {
    let s = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let path = s.lines().find_map(|l| l.strip_prefix("0::"))?.trim().to_string();
    (path != "/").then_some(path)
}

/// Our own systemd user unit, when we run as one (that's when limits can be enforced).
fn own_user_unit() -> Option<String> {
    let cg = own_cgroup()?;
    let unit = cg.rsplit('/').next()?;
    (cg.contains("/user@") && unit.ends_with(".service")).then(|| unit.to_string())
}

async fn systemctl(args: &[&str]) -> Option<String> {
    let out = tokio::process::Command::new("systemctl").arg("--user").args(args).output().await.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

async fn unit_cgroup(unit: &str) -> Option<String> {
    systemctl(&["show", "-p", "ControlGroup", "--value", unit]).await.filter(|p| !p.is_empty())
}

async fn unit_loaded(unit: &str) -> bool {
    systemctl(&["show", "-p", "LoadState", "--value", unit]).await.as_deref() == Some("loaded")
}

/// (CPU microseconds used so far, RAM bytes) of a cgroup.
fn read_cgroup(path: &str) -> Option<(u64, u64)> {
    let dir = Path::new("/sys/fs/cgroup").join(path.trim_start_matches('/'));
    let cpu = std::fs::read_to_string(dir.join("cpu.stat")).ok()?;
    let usec = cpu.lines().find_map(|l| l.strip_prefix("usage_usec "))?.trim().parse().ok()?;
    let ram = std::fs::read_to_string(dir.join("memory.current")).ok()?.trim().parse().ok()?;
    Some((usec, ram))
}

/// (CPU microseconds, RAM bytes) of this process when it isn't in its own cgroup.
fn read_self() -> Option<(u64, u64)> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    // utime and stime (fields 14 and 15), in clock ticks (100 per second on Linux).
    let ticks: u64 = fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?;
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let rss_kb: u64 = status.lines().find_map(|l| l.strip_prefix("VmRSS:"))?.split_whitespace().next()?.parse().ok()?;
    Some((ticks * 10_000, rss_kb * 1024))
}

fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

fn ai_model_dir(limits: &Limits) -> Option<PathBuf> {
    limits.ai_model_dir.clone().or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/huggingface"))).filter(|d| d.is_dir())
}

async fn sample_forever(state: &'static State) {
    let mut last: [Option<(Instant, u64)>; 2] = [None, None];
    let mut last_disk: Option<Instant> = None;
    loop {
        let unit = state.limits.lock().unwrap().ai_unit.clone();
        let server_raw = match own_cgroup() {
            Some(cg) => read_cgroup(&cg).or_else(read_self),
            None => read_self(),
        };
        let ai_cg = unit_cgroup(&unit).await;
        let ai_raw = ai_cg.as_deref().and_then(read_cgroup);
        let now = Instant::now();
        let mut usage = [Usage::default(); 2];
        for (i, raw) in [server_raw, ai_raw].into_iter().enumerate() {
            if let Some((usec, ram)) = raw {
                usage[i].ram_bytes = Some(ram);
                if let Some((t, prev)) = last[i] {
                    let secs = now.duration_since(t).as_secs_f64();
                    usage[i].cores = Some((usec.saturating_sub(prev) as f64 / 1e6 / secs.max(0.001) * 100.0).round() / 100.0);
                }
                last[i] = Some((now, usec));
            } else {
                last[i] = None;
            }
        }
        let disk_dir = if last_disk.is_none_or(|t| t.elapsed() > Duration::from_secs(60)) {
            last_disk = Some(now);
            ai_model_dir(&state.limits.lock().unwrap())
        } else {
            None
        };
        let disk = match disk_dir {
            Some(d) => tokio::task::spawn_blocking(move || dir_size(&d)).await.ok(),
            None => None,
        };
        {
            let mut s = state.snapshot.lock().unwrap();
            s.server = usage[0];
            s.ai = usage[1];
            s.ai_running = ai_cg.is_some();
            if disk.is_some() {
                s.ai_disk_bytes = disk;
            }
        }
        tokio::time::sleep(SAMPLE_EVERY).await;
    }
}

// ---------------------------------------------------------------- the machine

fn cores_total() -> u32 {
    // `available_parallelism` already counts a CPU quota, so read the online CPUs directly.
    let online = std::fs::read_to_string("/sys/devices/system/cpu/online").ok().and_then(|s| {
        s.trim()
            .split(',')
            .map(|r| match r.split_once('-') {
                Some((a, b)) => Some(b.parse::<u32>().ok()? - a.parse::<u32>().ok()? + 1),
                None => r.parse::<u32>().ok().map(|_| 1),
            })
            .sum::<Option<u32>>()
    });
    online.unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get() as u32))
}

/// (total, available) RAM in bytes.
fn memory() -> Option<(u64, u64)> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb = |key: &str| -> Option<u64> { info.lines().find_map(|l| l.strip_prefix(key))?.split_whitespace().next()?.parse().ok() };
    Some((kb("MemTotal:")? * 1024, kb("MemAvailable:")? * 1024))
}

/// (total, free) bytes of the disk holding `dir`.
async fn disk(dir: &Path) -> Option<(u64, u64)> {
    let out = tokio::process::Command::new("df").args(["-B1", "--output=size,avail"]).arg(dir).output().await.ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut nums = text.lines().nth(1)?.split_whitespace().map(|n| n.parse::<u64>().ok());
    Some((nums.next()??, nums.next()??))
}

// ---------------------------------------------------------------- enforcing

fn properties(cores: Option<f64>, ram_mb: Option<u64>) -> Vec<String> {
    vec![
        match cores {
            Some(c) => format!("CPUQuota={}%", (c * 100.0).round() as u64),
            None => "CPUQuota=".into(),
        },
        match ram_mb {
            Some(m) => format!("MemoryHigh={}", m * MB),
            None => "MemoryHigh=infinity".into(),
        },
        match ram_mb {
            Some(m) => format!("MemoryMax={}", (m as f64 * HARD_HEADROOM) as u64 * MB),
            None => "MemoryMax=infinity".into(),
        },
    ]
}

/// (server enforced, AI enforced)
async fn apply(limits: &Limits) -> (bool, bool) {
    let mut done = (false, false);
    if let Some(unit) = own_user_unit() {
        let props = properties(limits.server_cores, limits.server_ram_mb);
        let args: Vec<&str> = ["set-property", unit.as_str()].into_iter().chain(props.iter().map(String::as_str)).collect();
        done.0 = systemctl(&args).await.is_some();
        if !done.0 {
            tracing::warn!("couldn't apply the host's CPU/RAM limits to {unit}");
        }
    }
    if own_user_unit().is_some() && unit_loaded(&limits.ai_unit).await {
        let props = properties(limits.ai_cores, limits.ai_ram_mb);
        let args: Vec<&str> = ["set-property", limits.ai_unit.as_str()].into_iter().chain(props.iter().map(String::as_str)).collect();
        done.1 = systemctl(&args).await.is_some();
        if !done.1 {
            tracing::warn!("couldn't apply the AI's CPU/RAM limits to {}", limits.ai_unit);
        }
    }
    done
}

// ---------------------------------------------------------------- API

/// Only the host admin may do host-wide things (`403 host_admin_only` for everyone else).
pub(super) fn require_host_admin(auth: &Auth) -> ApiResult<()> {
    admin(auth).map(|_| ())
}

fn admin(auth: &Auth) -> ApiResult<&'static State> {
    let state = STATE.get().ok_or_else(ApiError::internal)?;
    if !auth.is_owner || auth.srv.slug != state.admin_slug {
        return Err(ApiError::forbidden("host_admin_only", "Only the host admin (the owner of this host's first server) can do that."));
    }
    Ok(state)
}

async fn out(state: &State) -> Value {
    let limits = state.limits.lock().unwrap().clone();
    let (server, ai, ai_running, ai_disk) = {
        let s = state.snapshot.lock().unwrap();
        (s.server, s.ai, s.ai_running, s.ai_disk_bytes)
    };
    let managed = own_user_unit().is_some();
    let ai_loaded = managed && unit_loaded(&limits.ai_unit).await;
    let (ram_total, ram_available) = memory().map_or((None, None), |(t, a)| (Some(t), Some(a)));
    let (disk_total, disk_free) = disk(&state.data_dir).await.map_or((None, None), |(t, f)| (Some(t), Some(f)));
    json!({
        "cpu": {
            "cores_total": cores_total(),
            "server": { "limit_cores": limits.server_cores, "usage_cores": server.cores, "enforced": managed },
            "ai": { "unit": ai_loaded.then_some(&limits.ai_unit), "running": ai_running, "limit_cores": limits.ai_cores, "usage_cores": ai.cores, "enforced": ai_loaded },
        },
        "ram": {
            "total_bytes": ram_total,
            "available_bytes": ram_available,
            "server": { "used_bytes": server.ram_bytes, "limit_bytes": limits.server_ram_mb.map(|m| m * MB), "enforced": managed },
            "ai": { "used_bytes": ai.ram_bytes, "limit_bytes": limits.ai_ram_mb.map(|m| m * MB), "enforced": ai_loaded },
        },
        "disk": { "total_bytes": disk_total, "free_bytes": disk_free },
        "ai_disk_bytes": ai_disk,
        "calls": limits.calls,
    })
}

/// `GET /host/resources`
pub async fn get(auth: Auth) -> ApiResult<Json<Value>> {
    let state = admin(&auth)?;
    Ok(Json(out(state).await))
}

/// Each field: absent = keep, `null` = unlimited, a number = the limit.
#[derive(Deserialize)]
pub struct PatchReq {
    #[serde(default, deserialize_with = "super::ai::present")]
    server_cores: Option<Option<f64>>,
    #[serde(default, deserialize_with = "super::ai::present")]
    server_ram_mb: Option<Option<u64>>,
    #[serde(default, deserialize_with = "super::ai::present")]
    ai_cores: Option<Option<f64>>,
    #[serde(default, deserialize_with = "super::ai::present")]
    ai_ram_mb: Option<Option<u64>>,
    calls: Option<CallLimits>,
}

/// `PATCH /host/resources`
pub async fn patch(auth: Auth, Body(req): Body<PatchReq>) -> ApiResult<Json<Value>> {
    let state = admin(&auth)?;
    let total = cores_total() as f64;
    let bad = |field: &'static str| ApiError::validation(std::collections::BTreeMap::from([(field, "out_of_range")]));
    let cores_ok = |c: Option<f64>| c.is_none_or(|c| c.is_finite() && (0.5..=total).contains(&c) && (c * 2.0).fract() == 0.0);
    let ram_ok = |m: Option<u64>| m.is_none_or(|m| m >= MIN_RAM_MB && memory().is_none_or(|(t, _)| m * MB <= t));
    let limits = {
        let mut l = state.limits.lock().unwrap();
        if let Some(c) = req.server_cores {
            if !cores_ok(c) {
                return Err(bad("server_cores"));
            }
            l.server_cores = c;
        }
        if let Some(c) = req.ai_cores {
            if !cores_ok(c) {
                return Err(bad("ai_cores"));
            }
            l.ai_cores = c;
        }
        if let Some(m) = req.server_ram_mb {
            if !ram_ok(m) {
                return Err(bad("server_ram_mb"));
            }
            l.server_ram_mb = m;
        }
        if let Some(m) = req.ai_ram_mb {
            if !ram_ok(m) {
                return Err(bad("ai_ram_mb"));
            }
            l.ai_ram_mb = m;
        }
        if let Some(c) = req.calls {
            if !(1..=100).contains(&c.max_calls) {
                return Err(bad("max_calls"));
            }
            if !(2..=25).contains(&c.max_people) {
                return Err(bad("max_people"));
            }
            l.calls = c;
        }
        l.clone()
    };
    std::fs::write(&state.file, serde_json::to_vec_pretty(&limits).unwrap_or_default()).map_err(|e| anyhow::anyhow!(e))?;
    apply(&limits).await;
    super::moderation::audit(&auth.srv, Some(auth.user.id), "host_limits_update", None, serde_json::to_value(&limits).unwrap_or_default()).await;
    Ok(Json(out(state).await))
}
