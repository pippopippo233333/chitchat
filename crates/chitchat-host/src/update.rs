//! Updating the host from its GitHub releases (`host-vX.Y.Z`), from the dashboard or with
//! `chitchat-host update`.
//!
//! A release has one archive per platform, `chitchat-host-<version>-<platform>.tar.gz`, holding
//! the binary, the built web client (`web/`) and `VERSION`, plus a minisign signature (`.sig`)
//! made with the same key as the desktop app's updates. Nothing is unpacked before the signature
//! checks out.
//!
//! Applying: back up every server's database, unpack into a staging folder, put the new binary and
//! web client in place (the old ones stay next to them as `.old`), then restart. The new version
//! is on probation until it has run for a minute: if it fails to start three times, the old
//! binary, web client and databases are put back and the old version starts again.

use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::host::Host;

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const REPO: &str = "pippopippo233333/chitchat";
/// The updater key (`desktop/src-tauri/tauri.conf.json`), base64 of the minisign public key file.
const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDNEQjhEQzkwMjY0OERFODUKUldTRjNrZ21rTnk0UFRNNVlmSEFDSjc2MHBrcHRNZTBKRXV1WGlaUC92OW9jZ3JTaE5RLzQrOHMK";
const MAX_ARCHIVE: u64 = 300 * 1024 * 1024;
/// A new version that fails to start this many times is rolled back.
const MAX_ATTEMPTS: u32 = 3;
/// Exit code asking the service manager to start the host again (systemd: `Restart=on-failure`).
pub const RESTART_CODE: i32 = 75;

/// What an update would install: the newest `host-v*` release for this platform.
#[derive(Clone, Serialize)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub published_at: String,
    #[serde(skip)]
    archive_url: String,
    #[serde(skip)]
    sig_url: String,
    #[serde(skip)]
    archive_name: String,
}

/// Kept in `<data>/update/pending.json` while a new version is on probation.
#[derive(Serialize, Deserialize)]
struct Pending {
    from: String,
    to: String,
    attempts: u32,
    exe: PathBuf,
    web: PathBuf,
    backup: PathBuf,
    /// Server slugs whose databases are in `backup`.
    servers: Vec<String>,
}

/// The web client folder the host serves (`run --web-dir`), which an update replaces.
pub static WEB_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Running state, for the dashboard.
static STATE: Mutex<State> = Mutex::new(State { applying: false, error: None });
struct State {
    applying: bool,
    error: Option<String>,
}

pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// `a > b` for `X.Y.Z` versions (anything else never counts as newer).
fn newer(a: &str, b: &str) -> bool {
    let parse = |v: &str| -> Option<(u64, u64, u64)> {
        let mut it = v.split('.').map(|p| p.parse::<u64>().ok());
        let r = (it.next()??, it.next()??, it.next()??);
        it.next().is_none().then_some(r)
    };
    matches!((parse(a), parse(b)), (Some(a), Some(b)) if a > b)
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("chitchat-host/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .unwrap_or_default()
}

/// The newest release for this platform, if it's newer than this host.
pub async fn check() -> Result<Option<Release>> {
    // Not releases/latest: that one is the desktop app's (its updater reads latest.json there).
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=50");
    let list: Vec<Value> = http().get(&url).header("Accept", "application/vnd.github+json").send().await?.error_for_status()?.json().await?;
    let want = platform();
    let mut best: Option<Release> = None;
    for r in list {
        if r["draft"].as_bool() != Some(false) || r["prerelease"].as_bool() != Some(false) {
            continue;
        }
        let Some(version) = r["tag_name"].as_str().and_then(|t| t.strip_prefix("host-v")) else { continue };
        if !newer(version, CURRENT) || best.as_ref().is_some_and(|b| !newer(version, &b.version)) {
            continue;
        }
        let name = format!("chitchat-host-{version}-{want}.tar.gz");
        let asset = |n: &str| r["assets"].as_array()?.iter().find(|a| a["name"].as_str() == Some(n))?["browser_download_url"].as_str().map(str::to_string);
        let (Some(archive_url), Some(sig_url)) = (asset(&name), asset(&format!("{name}.sig"))) else { continue };
        best = Some(Release {
            version: version.to_string(),
            notes: r["body"].as_str().unwrap_or_default().chars().take(4000).collect(),
            published_at: r["published_at"].as_str().unwrap_or_default().to_string(),
            archive_url,
            sig_url,
            archive_name: name,
        });
    }
    Ok(best)
}

async fn download(url: &str, max: u64) -> Result<Vec<u8>> {
    let parsed = reqwest::Url::parse(url)?;
    if parsed.scheme() != "https" || parsed.host_str() != Some("github.com") {
        bail!("unexpected download address {url}");
    }
    let mut res = http().get(url).send().await?.error_for_status()?;
    if res.content_length().is_some_and(|n| n > max) {
        bail!("the download is too large");
    }
    let mut out = Vec::new();
    while let Some(chunk) = res.chunk().await? {
        out.extend_from_slice(&chunk);
        if out.len() as u64 > max {
            bail!("the download is too large");
        }
    }
    Ok(out)
}

/// Checks `sig` (a `.sig` file as `tauri signer sign` writes it) for `data` named `name`.
fn verify(data: &[u8], sig: &[u8], name: &str) -> Result<()> {
    let text = |b64: &[u8]| -> Result<String> { Ok(String::from_utf8(STANDARD.decode(String::from_utf8_lossy(b64).trim())?)?) };
    let key = minisign_verify::PublicKey::decode(&text(PUBLIC_KEY.as_bytes())?).map_err(|e| anyhow!("public key: {e}"))?;
    let sig = minisign_verify::Signature::decode(&text(sig)?).map_err(|e| anyhow!("signature: {e}"))?;
    key.verify(data, &sig, false).map_err(|e| anyhow!("the signature doesn't match ({e})"))?;
    // The (signed) trusted comment names the file, so another signed file can't stand in for it.
    if !sig.trusted_comment().split_whitespace().any(|w| w == format!("file:{name}")) {
        bail!("the signature is for a different file");
    }
    Ok(())
}

/// `update --verify <file>`: checks a release archive against `<file>.sig`.
pub fn verify_file(file: &Path) -> Result<()> {
    let name = file.file_name().and_then(|n| n.to_str()).context("no file name")?;
    verify(&std::fs::read(file)?, &std::fs::read(with_suffix(file, ".sig"))?, name)?;
    println!("{name}: signature OK");
    Ok(())
}

/// Unpacks only plain files and folders with plain relative paths.
fn unpack(archive: &[u8], dest: &Path) -> Result<()> {
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for entry in tar.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        if path.as_os_str().is_empty() || !path.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir)) {
            bail!("unsafe path in the archive: {}", path.display());
        }
        let kind = entry.header().entry_type();
        let target = dest.join(&path);
        if kind.is_dir() {
            std::fs::create_dir_all(&target)?;
        } else if kind.is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            entry.unpack(&target)?;
        } else {
            bail!("unexpected entry in the archive: {}", path.display());
        }
    }
    Ok(())
}

fn update_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("update")
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Moves `new` into `place`, keeping what was there as `<place>.old`.
fn swap_in(new: &Path, place: &Path) -> Result<()> {
    let old = with_suffix(place, ".old");
    if old.is_dir() {
        std::fs::remove_dir_all(&old)?;
    } else if old.exists() {
        std::fs::remove_file(&old)?;
    }
    if place.exists() {
        std::fs::rename(place, &old).with_context(|| format!("moving {} aside", place.display()))?;
    }
    std::fs::rename(new, place).with_context(|| format!("putting the new {} in place", place.display()))
}

/// Puts `<place>.old` back.
fn swap_back(place: &Path) -> Result<()> {
    let old = with_suffix(place, ".old");
    if !old.exists() {
        bail!("{} is missing", old.display());
    }
    if place.is_dir() {
        std::fs::remove_dir_all(place)?;
    } else if place.exists() {
        std::fs::remove_file(place)?;
    }
    std::fs::rename(&old, place)?;
    Ok(())
}

/// Copies `from` (a folder) to `to` next to the destination, so the final step is a rename on
/// the same disk.
fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Downloads, checks and installs `release`. The caller then restarts the host.
async fn install(host: &Host, web_dir: &Path, release: &Release) -> Result<()> {
    let archive = download(&release.archive_url, MAX_ARCHIVE).await.context("downloading the update")?;
    let sig = download(&release.sig_url, 64 * 1024).await.context("downloading its signature")?;
    verify(&archive, &sig, &release.archive_name)?;

    let dir = update_dir(&host.data_dir);
    let staging = dir.join("staging");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir_all(&staging)?;
    unpack(&archive, &staging)?;
    let version = std::fs::read_to_string(staging.join("VERSION")).context("the update has no VERSION")?;
    if version.trim() != release.version {
        bail!("the update says it is version {}, not {}", version.trim(), release.version);
    }
    let bin_name = if cfg!(windows) { "chitchat-host.exe" } else { "chitchat-host" };
    let new_bin = staging.join(bin_name);
    if !new_bin.is_file() || !staging.join("web/index.html").is_file() {
        bail!("the update is incomplete");
    }

    // Back up every database before the new version (which may migrate them) runs.
    let stamp = chrono_stamp();
    let backup = dir.join("backups").join(format!("{CURRENT}-to-{}-{stamp}", release.version));
    std::fs::create_dir_all(&backup)?;
    let mut servers = Vec::new();
    for (slug, srv) in host.servers.read().await.iter() {
        let file = backup.join(format!("{slug}.db"));
        sqlx::query("VACUUM INTO ?").bind(file.to_string_lossy().to_string()).execute(&srv.db).await.with_context(|| format!("backing up {slug}"))?;
        servers.push(slug.clone());
    }

    let exe = std::env::current_exe()?.canonicalize()?;
    let web = web_dir.canonicalize().with_context(|| format!("finding the web client folder {}", web_dir.display()))?;
    // Stage next to the destinations, so the swaps are renames on the same disk.
    let exe_new = with_suffix(&exe, ".new");
    std::fs::copy(&new_bin, &exe_new)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe_new, std::fs::Permissions::from_mode(0o755))?;
    }
    let web_new = with_suffix(&web, ".new");
    if web_new.exists() {
        std::fs::remove_dir_all(&web_new)?;
    }
    copy_dir(&staging.join("web"), &web_new)?;

    let pending = Pending { from: CURRENT.into(), to: release.version.clone(), attempts: 0, exe: exe.clone(), web: web.clone(), backup, servers };
    std::fs::write(dir.join("pending.json"), serde_json::to_vec_pretty(&pending)?)?;
    swap_in(&exe_new, &exe)?;
    if let Err(e) = swap_in(&web_new, &web) {
        let _ = swap_back(&exe);
        let _ = std::fs::remove_file(dir.join("pending.json"));
        return Err(e);
    }
    let _ = std::fs::remove_dir_all(&staging);
    tracing::info!("update to {} installed; restarting", release.version);
    Ok(())
}

fn chrono_stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    secs.to_string()
}

fn write_result(data_dir: &Path, ok: bool, message: &str) {
    let _ = std::fs::write(update_dir(data_dir).join("last-result.json"), json!({ "ok": ok, "message": message, "at": chrono_stamp() }).to_string());
}

/// Starts the host again with the same arguments: the service manager does it when there is one,
/// otherwise this process replaces itself (Unix) or starts the new one (Windows).
pub fn restart() -> ! {
    if std::env::var_os("INVOCATION_ID").is_some() {
        std::process::exit(RESTART_CODE); // systemd
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("chitchat-host"));
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let e = std::process::Command::new(&exe).args(&args).exec();
        tracing::error!("couldn't restart: {e}");
    }
    #[cfg(not(unix))]
    {
        // The new process waits for the port: see `wait_for_port`.
        if let Err(e) = std::process::Command::new(&exe).args(&args).env("CHITCHAT_RESTARTED", "1").spawn() {
            tracing::error!("couldn't restart: {e}");
        }
    }
    std::process::exit(RESTART_CODE)
}

/// Whether this process was started by `restart` (and the old one may still hold the port).
pub fn wait_for_port() -> bool {
    std::env::var_os("CHITCHAT_RESTARTED").is_some()
}

/// Before the databases are opened: counts a start of a new version on probation, and rolls it
/// back after too many failed starts.
pub fn on_start(data_dir: &Path) -> Result<()> {
    let file = update_dir(data_dir).join("pending.json");
    let Ok(bytes) = std::fs::read(&file) else { return Ok(()) };
    let mut pending: Pending = serde_json::from_slice(&bytes).context("reading update/pending.json")?;
    if pending.to != CURRENT {
        return Ok(()); // An older binary is running (rolled back by hand): nothing to judge.
    }
    pending.attempts += 1;
    if pending.attempts <= MAX_ATTEMPTS {
        std::fs::write(&file, serde_json::to_vec_pretty(&pending)?)?;
        tracing::info!("running the update to {CURRENT} (start {} of {MAX_ATTEMPTS} before it counts as failed)", pending.attempts);
        return Ok(());
    }
    tracing::error!("version {CURRENT} failed to start {MAX_ATTEMPTS} times: going back to {}", pending.from);
    for slug in &pending.servers {
        let db = data_dir.join("servers").join(slug).join("chitchat.db");
        std::fs::copy(pending.backup.join(format!("{slug}.db")), &db).with_context(|| format!("restoring {slug}"))?;
        for extra in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(with_suffix(&db, extra));
        }
    }
    swap_back(&pending.exe)?;
    swap_back(&pending.web)?;
    std::fs::remove_file(&file)?;
    write_result(data_dir, false, &format!("Version {CURRENT} didn't start, so {} was restored (with the databases as they were before the update).", pending.from));
    restart()
}

/// Once the host is serving: a new version that runs for a minute is kept.
pub fn on_running(data_dir: PathBuf) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        let file = update_dir(&data_dir).join("pending.json");
        let Ok(pending) = std::fs::read(&file).map_err(anyhow::Error::from).and_then(|b| Ok(serde_json::from_slice::<Pending>(&b)?)) else { return };
        if pending.to == CURRENT {
            let _ = std::fs::remove_file(&file);
            write_result(&data_dir, true, &format!("Updated from {} to {CURRENT}.", pending.from));
            tracing::info!("update to {CURRENT} kept");
        }
    });
}

/// For the dashboard: this version, the newest release, and the last update's outcome.
pub async fn status(data_dir: &Path) -> Value {
    let (latest, error) = match check().await {
        Ok(r) => (r, None),
        Err(e) => (None, Some(format!("Couldn't check for updates: {e}"))),
    };
    let last: Value = std::fs::read(update_dir(data_dir).join("last-result.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    let state = STATE.lock().unwrap();
    json!({
        "current": CURRENT,
        "platform": platform(),
        "latest": latest,
        "check_error": error,
        "applying": state.applying,
        "apply_error": state.error,
        "last_result": last,
    })
}

/// Installs the newest release in the background, then restarts. Errors if one is already running
/// or there's nothing to install.
pub async fn start_apply(host: std::sync::Arc<Host>, web_dir: PathBuf) -> Result<String> {
    {
        let mut s = STATE.lock().unwrap();
        if s.applying {
            bail!("an update is already being installed");
        }
        s.applying = true;
        s.error = None;
    }
    let release = match check().await {
        Ok(Some(r)) => r,
        other => {
            STATE.lock().unwrap().applying = false;
            return match other {
                Ok(_) => Err(anyhow!("this host is already up to date")),
                Err(e) => Err(e.context("checking for updates")),
            };
        }
    };
    let version = release.version.clone();
    tokio::spawn(async move {
        match install(&host, &web_dir, &release).await {
            Ok(()) => {
                // Let the response and events go out first.
                tokio::time::sleep(Duration::from_millis(800)).await;
                restart();
            }
            Err(e) => {
                tracing::warn!("update failed: {e:#}");
                let mut s = STATE.lock().unwrap();
                s.applying = false;
                s.error = Some(format!("{e:#}"));
            }
        }
    });
    Ok(version)
}

/// `chitchat-host update [--check]` (with the host stopped).
pub async fn cli(host: &Host, web_dir: &Path, only_check: bool) -> Result<()> {
    match check().await? {
        None => println!("ChitChat host {CURRENT} is up to date."),
        Some(r) if only_check => println!("Version {} is available (you have {CURRENT}). Install it with: chitchat-host update", r.version),
        Some(r) => {
            println!("Installing version {} (you have {CURRENT})…", r.version);
            install(host, web_dir, &r).await?;
            println!("Done. Start the host as usual; if the new version fails to start three times, the old one comes back by itself.");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(newer("0.2.0", "0.1.9"));
        assert!(newer("1.0.0", "0.99.99"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1", "0.0.1"));
        assert!(!newer("0.2.0-beta", "0.1.0"));
    }

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut b = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for (path, data) in entries {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, path, *data).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn unpacks_plain_files_only() {
        let root = std::env::temp_dir().join(format!("chitchat-unpack-{}", std::process::id()));
        let dir = root.join("dest");
        std::fs::create_dir_all(&dir).unwrap();
        unpack(&archive(&[("VERSION", b"1.0.0"), ("web/index.html", b"<html>")]), &dir).unwrap();
        assert_eq!(std::fs::read(dir.join("web/index.html")).unwrap(), b"<html>");

        // A raw header with a path that escapes the destination.
        let mut b = tar::Builder::new(Vec::new());
        let mut h = tar::Header::new_gnu();
        h.as_gnu_mut().unwrap().name[..9].copy_from_slice(b"../escape");
        h.set_size(1);
        h.set_cksum();
        b.append(&h, &b"x"[..]).unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &b.into_inner().unwrap()).unwrap();
        assert!(unpack(&gz.finish().unwrap(), &dir).is_err());
        assert!(!root.join("escape").exists());

        let mut b = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Symlink);
        h.set_size(0);
        b.append_link(&mut h, "link", "/etc/passwd").unwrap();
        assert!(unpack(&b.into_inner().unwrap().finish().unwrap(), &dir).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_bad_signatures() {
        assert!(verify(b"data", b"not base64!", "x.tar.gz").is_err());
        // A real signature from the updater key is checked in scripts/release-host.py's dry run.
    }
}
