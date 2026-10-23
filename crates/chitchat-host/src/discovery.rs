//! Server discovery with no central server. Hosts that turn it on publish their discoverable
//! servers and the hosts they know at `GET /api/discovery`, and fetch the same from those hosts
//! every half hour (gossip), so the list spreads host to host. A new host joins the network by
//! knowing one other host: a seed URL, a seed list file (e.g. a text file on GitHub), or an
//! announce from a host that already knows it. Browsers ask their own host for the merged list
//! (`GET /api/discovery/browse`), so they never contact strangers' hosts themselves.
//!
//! Everything fetched is untrusted: https only, public IPs only (no fetching into the LAN), no
//! redirects, small size and count caps, text trimmed. A host must answer with its own URL to
//! be listed under it.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::host::{Host, valid_slug};
use crate::models::now_ms;

const ROUND_EVERY: Duration = Duration::from_secs(30 * 60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BODY: usize = 512 * 1024;
const MAX_PEERS: usize = 500;
const MAX_SERVERS_PER_HOST: usize = 50;
const MAX_NEW_PEERS_PER_HOST: usize = 50;
const MAX_SEEDS: usize = 200;
const PARALLEL: usize = 8;
/// A peer that hasn't answered for this long is forgotten.
const FORGET_AFTER_MS: i64 = 7 * 24 * 3600 * 1000;
/// Peers we pass on to others must have answered this recently.
const SHARE_IF_SEEN_MS: i64 = 3 * 24 * 3600 * 1000;
const ANNOUNCE_GAP: Duration = Duration::from_secs(60);

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Config {
    #[serde(default)]
    pub enabled: bool,
    /// How other hosts reach this one, e.g. `https://chat.example.org` (no `/s/...`).
    #[serde(default)]
    pub public_url: Option<String>,
    /// Other hosts' URLs to start from.
    #[serde(default)]
    pub seeds: Vec<String>,
    /// URLs of plain files listing host URLs (one per line, or a JSON array).
    #[serde(default)]
    pub seed_lists: Vec<String>,
    /// Hosts never fetched, listed or passed on.
    #[serde(default)]
    pub blocked: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RemoteServer {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub requires_password: bool,
    pub invite_only: bool,
    pub registration_open: bool,
    pub member_count: i64,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Peer {
    pub url: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub servers: Vec<RemoteServer>,
    pub last_ok: Option<i64>,
    #[serde(default)]
    pub fails: u32,
    pub added_at: i64,
}

struct State {
    host: Arc<Host>,
    config_file: PathBuf,
    peers_file: PathBuf,
    config: Mutex<Config>,
    peers: Mutex<HashMap<String, Peer>>,
    last_round: Mutex<Option<i64>>,
    announces: Mutex<HashMap<IpAddr, Instant>>,
    wake: tokio::sync::Notify,
}

static STATE: OnceLock<State> = OnceLock::new();

fn allow_private() -> bool {
    std::env::var("CHITCHAT_DISCOVERY_ALLOW_PRIVATE").is_ok_and(|v| v == "1")
}

fn load<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save<T: Serialize>(path: &Path, value: &T) {
    let tmp = path.with_extension("tmp");
    let ok = serde_json::to_vec_pretty(value).ok().is_some_and(|b| std::fs::write(&tmp, b).is_ok()) && std::fs::rename(&tmp, path).is_ok();
    if !ok {
        tracing::warn!(file = %path.display(), "could not save discovery data");
    }
}

/// Loads the saved settings and peers and starts the gossip loop.
pub fn start(host: Arc<Host>) {
    let data_dir = host.data_dir.clone();
    let config_file = data_dir.join("discovery.json");
    let peers_file = data_dir.join("discovery-peers.json");
    let config: Config = load(&config_file);
    let peers: Vec<Peer> = load(&peers_file);
    let state = STATE.get_or_init(|| State {
        host,
        config: Mutex::new(config),
        peers: Mutex::new(peers.into_iter().map(|p| (p.url.clone(), p)).collect()),
        config_file,
        peers_file,
        last_round: Mutex::new(None),
        announces: Mutex::new(HashMap::new()),
        wake: tokio::sync::Notify::new(),
    });
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(10)).await;
        loop {
            if state.config.lock().unwrap().enabled {
                round(state).await;
            }
            tokio::select! {
                _ = tokio::time::sleep(ROUND_EVERY) => {}
                _ = state.wake.notified() => {}
            }
        }
    });
}

// ---------------------------------------------------------------- URLs and fetching

/// `https://Host:port/anything` → `https://host:port`, or an error for anything else.
pub fn normalize_url(raw: &str) -> Result<String> {
    let u = Url::parse(raw.trim()).context("not a URL")?;
    let http_ok = u.scheme() == "http" && allow_private();
    if u.scheme() != "https" && !http_ok {
        bail!("must start with https://");
    }
    if !u.username().is_empty() || u.password().is_some() || u.query().is_some() || u.fragment().is_some() {
        bail!("no user, query or # part");
    }
    if !matches!(u.path(), "" | "/") {
        bail!("use the host's address only, without a path");
    }
    let host = u.host_str().context("no host name")?.to_lowercase();
    Ok(match u.port() {
        Some(p) => format!("{}://{host}:{p}", u.scheme()),
        None => format!("{}://{host}", u.scheme()),
    })
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || o[0] == 0
                || (o[0] == 100 && (64..128).contains(&o[1])))
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            if let Some(v4) = v6.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(v4));
            }
            !(v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() || (s[0] & 0xfe00) == 0xfc00 || (s[0] & 0xffc0) == 0xfe80)
        }
    }
}

/// GETs (or POSTs `body` to) `url`, refusing private addresses, redirects and big answers.
async fn fetch(url: &str, body: Option<&Value>) -> Result<Vec<u8>> {
    let u = Url::parse(url)?;
    let host = u.host_str().context("no host")?.to_string();
    let port = u.port_or_known_default().context("no port")?;
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port)).await?.collect();
    let addr = addrs.into_iter().find(|a| allow_private() || public_ip(a.ip())).context("no public address")?;
    // Pin the checked address, so a second DNS answer can't point the request into the LAN.
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .resolve(&host, addr)
        .user_agent(concat!("chitchat-host/", env!("CARGO_PKG_VERSION"), " discovery"))
        .build()?;
    let req = match body {
        Some(b) => client.post(url).json(b),
        None => client.get(url),
    };
    let mut resp = req.send().await?;
    if !resp.status().is_success() {
        bail!("status {}", resp.status());
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        out.extend_from_slice(&chunk);
        if out.len() > MAX_BODY {
            bail!("answer too big");
        }
    }
    Ok(out)
}

fn clip(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control() || *c == '\n').take(max).collect::<String>().trim().to_string()
}
/// Another host's discovery answer: its name, its servers and the hosts it knows.
type Doc = (String, Vec<RemoteServer>, Vec<String>);

/// Reads another host's `/api/discovery` answer, keeping only well-formed, trimmed fields.
/// Reads another host's `/api/discovery` answer, keeping only well-formed, trimmed fields.
fn parse_doc(url: &str, raw: &[u8]) -> Result<Doc> {
    let v: Value = serde_json::from_slice(raw).context("not JSON")?;
    let claimed = v["url"].as_str().map(normalize_url).transpose().ok().flatten();
    if claimed.as_deref() != Some(url) {
        bail!("the host doesn't say it is {url}");
    }
    let name = clip(v["name"].as_str().unwrap_or(""), 64);
    let mut servers = Vec::new();
    for s in v["servers"].as_array().into_iter().flatten().take(MAX_SERVERS_PER_HOST) {
        let slug = s["slug"].as_str().unwrap_or("");
        let sname = clip(s["name"].as_str().unwrap_or(""), 64);
        if !valid_slug(slug) || sname.is_empty() {
            continue;
        }
        servers.push(RemoteServer {
            slug: slug.to_string(),
            name: sname,
            description: clip(s["description"].as_str().unwrap_or(""), 300),
            requires_password: s["requires_password"].as_bool().unwrap_or(false),
            invite_only: s["invite_only"].as_bool().unwrap_or(false),
            registration_open: s["registration_open"].as_bool().unwrap_or(true),
            member_count: s["member_count"].as_i64().unwrap_or(0).clamp(0, 10_000_000),
        });
    }
    let peers = v["peers"].as_array().into_iter().flatten().filter_map(|p| p.as_str().and_then(|p| normalize_url(p).ok())).take(MAX_NEW_PEERS_PER_HOST).collect();
    Ok((name, servers, peers))
}

async fn seed_list(url: &str) -> Vec<String> {
    let Ok(raw) = fetch(url, None).await else {
        tracing::warn!(%url, "discovery: could not read the seed list");
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&raw);
    let items: Vec<String> = match serde_json::from_str::<Vec<String>>(&text) {
        Ok(list) => list,
        Err(_) => text.lines().map(|l| l.split('#').next().unwrap_or("").trim().to_string()).filter(|l| !l.is_empty()).collect(),
    };
    items.iter().filter_map(|u| normalize_url(u).ok()).take(MAX_SEEDS).collect()
}

// ---------------------------------------------------------------- gossip

async fn round(state: &'static State) {
    let cfg = state.config.lock().unwrap().clone();
    let me = cfg.public_url.as_deref().and_then(|u| normalize_url(u).ok());
    let blocked: HashSet<String> = cfg.blocked.iter().filter_map(|u| normalize_url(u).ok()).collect();
    let mut candidates: HashSet<String> = cfg.seeds.iter().filter_map(|u| normalize_url(u).ok()).collect();
    for list in &cfg.seed_lists {
        candidates.extend(seed_list(list).await);
    }
    candidates.extend(state.peers.lock().unwrap().keys().cloned());

    // Fetch everyone known, then (once) the new hosts they told us about.
    let mut seen = HashSet::new();
    for _pass in 0..2 {
        let todo: Vec<String> = candidates.drain().filter(|u| !blocked.contains(u) && Some(u) != me.as_ref() && seen.insert(u.clone())).collect();
        let results: Vec<(String, Result<Doc>)> = futures_util::stream::iter(todo)
            .map(|url| async move {
                let r = match fetch(&format!("{url}/api/discovery"), None).await {
                    Ok(raw) => parse_doc(&url, &raw),
                    Err(e) => Err(e),
                };
                (url, r)
            })
            .buffer_unordered(PARALLEL)
            .collect()
            .await;
        let mut ok_urls = Vec::new();
        {
            let mut peers = state.peers.lock().unwrap();
            for (url, r) in results {
                match r {
                    Ok((name, servers, more)) => {
                        let p = peers.entry(url.clone()).or_insert_with(|| Peer { url: url.clone(), added_at: now_ms(), ..Default::default() });
                        p.name = name;
                        p.servers = servers;
                        p.last_ok = Some(now_ms());
                        p.fails = 0;
                        candidates.extend(more);
                        ok_urls.push(url);
                    }
                    Err(e) => {
                        tracing::debug!(%url, error = %e, "discovery: host didn't answer");
                        if let Some(p) = peers.get_mut(&url) {
                            p.fails += 1;
                        }
                    }
                }
            }
        }
        // Tell the hosts that answered about us, so they list us even if nobody links to us.
        if let Some(me) = &me {
            let body = json!({ "url": me });
            futures_util::stream::iter(ok_urls)
                .for_each_concurrent(PARALLEL, |url| {
                    let body = body.clone();
                    async move {
                        let _ = fetch(&format!("{url}/api/discovery/announce"), Some(&body)).await;
                    }
                })
                .await;
        }
    }
    forget_old(state);
    *state.last_round.lock().unwrap() = Some(now_ms());
    let peers: Vec<Peer> = state.peers.lock().unwrap().values().cloned().collect();
    save(&state.peers_file, &peers);
    tracing::info!(hosts = peers.iter().filter(|p| p.last_ok.is_some()).count(), "discovery round done");
}

fn forget_old(state: &State) {
    let now = now_ms();
    let mut peers = state.peers.lock().unwrap();
    peers.retain(|_, p| now - p.last_ok.unwrap_or(p.added_at) < FORGET_AFTER_MS);
    if peers.len() > MAX_PEERS {
        let mut by_age: Vec<(i64, String)> = peers.values().map(|p| (p.last_ok.unwrap_or(0), p.url.clone())).collect();
        by_age.sort();
        for (_, url) in by_age.into_iter().take(peers.len() - MAX_PEERS) {
            peers.remove(&url);
        }
    }
}

// ---------------------------------------------------------------- what the API needs

pub fn config() -> Config {
    STATE.get().map(|s| s.config.lock().unwrap().clone()).unwrap_or_default()
}

pub fn set_config(cfg: Config) {
    let Some(state) = STATE.get() else { return };
    let enabled = cfg.enabled;
    save(&state.config_file, &cfg);
    *state.config.lock().unwrap() = cfg;
    if enabled {
        state.wake.notify_one();
    }
}

pub fn peers() -> (Vec<Peer>, Option<i64>) {
    let Some(state) = STATE.get() else { return (Vec::new(), None) };
    let mut peers: Vec<Peer> = state.peers.lock().unwrap().values().cloned().collect();
    peers.sort_by(|a, b| a.url.cmp(&b.url));
    (peers, *state.last_round.lock().unwrap())
}

async fn local_servers(host: &Host) -> Vec<RemoteServer> {
    let servers: Vec<_> = host.servers.read().await.values().cloned().collect();
    let mut out = Vec::new();
    for s in servers {
        let Ok(row) = crate::db::server_row(&s.db).await else { continue };
        if !row.discoverable {
            continue;
        }
        let members: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE kicked = 0").fetch_one(&s.db).await.unwrap_or(0);
        out.push(RemoteServer {
            slug: s.slug.clone(),
            name: row.name,
            description: clip(&row.description, 300),
            requires_password: row.password_hash.is_some(),
            invite_only: row.invite_only,
            registration_open: row.registration_open,
            member_count: members,
        });
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// Our own `/api/discovery` answer, or None when discovery is off.
pub async fn document() -> Option<Value> {
    let state = STATE.get()?;
    let cfg = state.config.lock().unwrap().clone();
    if !cfg.enabled {
        return None;
    }
    let now = now_ms();
    let blocked: HashSet<String> = cfg.blocked.iter().filter_map(|u| normalize_url(u).ok()).collect();
    let shared: Vec<String> = state
        .peers
        .lock()
        .unwrap()
        .values()
        .filter(|p| p.last_ok.is_some_and(|t| now - t < SHARE_IF_SEEN_MS) && !blocked.contains(&p.url))
        .map(|p| p.url.clone())
        .take(100)
        .collect();
    Some(json!({
        "version": 1,
        "name": state.host.name,
        "url": cfg.public_url.as_deref().and_then(|u| normalize_url(u).ok()),
        "servers": local_servers(&state.host).await,
        "peers": shared,
    }))
}

/// Every discoverable server we know: ours and the ones other hosts published.
pub async fn browse() -> Option<Value> {
    let state = STATE.get()?;
    let cfg = state.config.lock().unwrap().clone();
    if !cfg.enabled {
        return None;
    }
    let me = cfg.public_url.as_deref().and_then(|u| normalize_url(u).ok());
    let blocked: HashSet<String> = cfg.blocked.iter().filter_map(|u| normalize_url(u).ok()).collect();
    let entry = |host_url: &str, host_name: &str, s: &RemoteServer, last_seen: Option<i64>, local: bool| {
        json!({
            "url": format!("{host_url}/s/{}", s.slug),
            "host_url": host_url, "host_name": host_name, "local": local, "last_seen": last_seen,
            "slug": s.slug, "name": s.name, "description": s.description,
            "requires_password": s.requires_password, "invite_only": s.invite_only,
            "registration_open": s.registration_open, "member_count": s.member_count,
        })
    };
    let mut out: Vec<Value> = local_servers(&state.host).await.iter().map(|s| entry(me.as_deref().unwrap_or(""), &state.host.name, s, Some(now_ms()), true)).collect();
    let peers: Vec<Peer> = state.peers.lock().unwrap().values().filter(|p| p.last_ok.is_some() && !blocked.contains(&p.url)).cloned().collect();
    let hosts = peers.len();
    for p in &peers {
        out.extend(p.servers.iter().map(|s| entry(&p.url, &p.name, s, p.last_ok, false)));
    }
    out.sort_by_key(|v| std::cmp::Reverse(v["member_count"].as_i64().unwrap_or(0)));
    Some(json!({ "servers": out, "hosts": hosts + 1, "updated_at": *state.last_round.lock().unwrap() }))
}

/// Another host says it exists: check it really answers as that URL, then remember it.
pub fn announce(from: IpAddr, url: &str) -> Result<()> {
    let state = STATE.get().context("discovery not started")?;
    let cfg = state.config.lock().unwrap().clone();
    if !cfg.enabled {
        bail!("discovery is off");
    }
    let url = normalize_url(url)?;
    if cfg.blocked.iter().filter_map(|u| normalize_url(u).ok()).any(|b| b == url) || state.peers.lock().unwrap().contains_key(&url) {
        return Ok(());
    }
    {
        let mut a = state.announces.lock().unwrap();
        if a.get(&from).is_some_and(|t| t.elapsed() < ANNOUNCE_GAP) {
            bail!("too many announces");
        }
        if a.len() > 10_000 {
            a.clear();
        }
        a.insert(from, Instant::now());
    }
    tokio::spawn(async move {
        let r = match fetch(&format!("{url}/api/discovery"), None).await {
            Ok(raw) => parse_doc(&url, &raw),
            Err(e) => Err(e),
        };
        if let Ok((name, servers, _)) = r {
            let mut peers = state.peers.lock().unwrap();
            peers.insert(url.clone(), Peer { url: url.clone(), name, servers, last_ok: Some(now_ms()), fails: 0, added_at: now_ms() });
            tracing::info!(%url, "discovery: new host announced itself");
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(normalize_url("https://Chat.Example.org/").unwrap(), "https://chat.example.org");
        assert_eq!(normalize_url("https://a.b:8090").unwrap(), "https://a.b:8090");
        assert!(normalize_url("ftp://a.b").is_err());
        assert!(normalize_url("https://a.b/s/dev").is_err());
        assert!(normalize_url("https://u:p@a.b").is_err());
        assert!(!public_ip("192.168.1.5".parse().unwrap()));
        assert!(!public_ip("100.70.0.1".parse().unwrap()));
        assert!(!public_ip("::ffff:127.0.0.1".parse().unwrap()));
        assert!(!public_ip("fd00::1".parse().unwrap()));
        assert!(public_ip("79.55.64.11".parse().unwrap()));
    }

    #[test]
    fn documents_are_checked() {
        let doc = br#"{"url":"https://a.b","name":"A","servers":[{"slug":"ok","name":"Fine"},{"slug":"BAD SLUG","name":"x"}],"peers":["https://c.d/","nope"]}"#;
        let (name, servers, peers) = parse_doc("https://a.b", doc).unwrap();
        assert_eq!(name, "A");
        assert_eq!(servers.len(), 1);
        assert_eq!(peers, vec!["https://c.d".to_string()]);
        assert!(parse_doc("https://other.host", doc).is_err());
    }
}
