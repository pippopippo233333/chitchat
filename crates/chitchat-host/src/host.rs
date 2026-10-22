//! The host process: several isolated servers, each in `data/servers/<slug>/`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::sync::{RwLock, broadcast};

use crate::events::Event;
use crate::ipfilter::IpFilter;
use crate::models::{AttachmentOut, AttachmentRow};
use crate::spam::SpamGuard;
use crate::{db, media};

#[derive(Serialize, Deserialize)]
pub struct HostConfig {
    pub name: String,
}

impl HostConfig {
    fn load_or_init(data_dir: &Path) -> Result<HostConfig> {
        let path = data_dir.join("host.toml");
        if path.exists() {
            return Ok(toml::from_str(&std::fs::read_to_string(&path)?)?);
        }
        let cfg = HostConfig { name: "ChitChat Host".into() };
        std::fs::write(&path, toml::to_string(&cfg)?)?;
        Ok(cfg)
    }
}

/// One member's voice connection (docs/api.md 4e, VoiceState).
#[derive(Clone)]
pub struct VoiceState {
    pub channel_id: i64,
    /// The WebSocket connection that owns it; also the stream id of their audio.
    pub session: u64,
    pub self_mute: bool,
    pub self_deaf: bool,
    /// No SPEAK permission, or timed out.
    pub suppressed: bool,
    /// Sharing their screen (needs STREAM).
    pub streaming: bool,
    /// The client's screen-share transceiver mids (`voice_state.screen_mids`).
    pub screen_mids: Vec<String>,
}

impl VoiceState {
    pub fn json(&self, user_id: i64) -> serde_json::Value {
        serde_json::json!({
            "user_id": user_id.to_string(),
            "channel_id": self.channel_id.to_string(),
            "session_id": self.session.to_string(),
            "self_mute": self.self_mute,
            "self_deaf": self.self_deaf,
            "suppressed": self.suppressed,
            "streaming": self.streaming,
        })
    }
}

pub struct ServerCtx {
    pub slug: String,
    pub db: SqlitePool,
    pub events: broadcast::Sender<Arc<Event>>,
    /// Open WebSocket connections per user.
    presence: Mutex<HashMap<i64, usize>>,
    pub spam: Mutex<SpamGuard>,
    /// The owner's IP rules, checked on every request.
    pub ip: std::sync::RwLock<IpFilter>,
    /// Uploaded files: `data/servers/<slug>/files/`.
    pub files_dir: PathBuf,
    file_key: [u8; 32],
    /// Who is in which voice channel, per user (one voice connection per user).
    pub voice: Mutex<HashMap<i64, VoiceState>>,
    /// Owner-only AI moderation (settings, queue, stats).
    pub ai: crate::ai::Ai,
    /// Storage used (files + message text) and when it was measured; see `api::storage`.
    pub storage_used: tokio::sync::Mutex<Option<(std::time::Instant, i64)>>,
}

impl ServerCtx {
    async fn open(slug: String, dir: &Path, db: SqlitePool) -> Result<ServerCtx> {
        let ip = db::load_ip_filter(&db).await?;
        let file_key = db::file_key(&db).await?;
        let files_dir = dir.join("files");
        std::fs::create_dir_all(files_dir.join("tmp"))?;
        let ai = crate::ai::Ai::new(crate::ai::load_config(&db).await);
        let ctx = ServerCtx::new(slug, db, ip, files_dir, file_key, ai);
        ctx.apply_spam_settings(&db::server_row(&ctx.db).await?);
        Ok(ctx)
    }

    pub fn apply_spam_settings(&self, row: &crate::models::ServerRow) {
        self.spam.lock().unwrap().set_limits(crate::spam::Limits::from_settings(row));
    }

    fn new(slug: String, db: SqlitePool, ip: IpFilter, files_dir: PathBuf, file_key: [u8; 32], ai: crate::ai::Ai) -> ServerCtx {
        ServerCtx {
            files_dir,
            file_key,
            slug,
            db,
            events: broadcast::channel(1024).0,
            presence: Mutex::new(HashMap::new()),
            spam: Mutex::new(SpamGuard::default()),
            ip: std::sync::RwLock::new(ip),
            voice: Mutex::new(HashMap::new()),
            ai,
            storage_used: tokio::sync::Mutex::new(None),
        }
    }

    /// A signed link to an attachment (`variant` is "file" or "thumb").
    pub fn file_url(&self, id: i64, variant: &str, filename: &str) -> String {
        let exp = media::link_expiry(crate::models::now_ms());
        let sig = media::sign(&self.file_key, id, variant, exp);
        format!("/s/{}/api/files/{id}/{variant}/{filename}?exp={exp}&sig={sig}", self.slug)
    }

    pub fn verify_file_link(&self, id: i64, variant: &str, exp: i64, sig: &str) -> bool {
        media::verify(&self.file_key, id, variant, exp, sig, crate::models::now_ms())
    }

    pub fn attachment_out(&self, a: &AttachmentRow) -> AttachmentOut {
        AttachmentOut {
            id: a.id.to_string(),
            filename: a.filename.clone(),
            kind: a.kind.clone(),
            mime: a.mime.clone(),
            size: a.size,
            width: a.width,
            height: a.height,
            duration_ms: a.duration_ms,
            voice: a.voice,
            url: self.file_url(a.id, "file", &a.filename),
            thumbnail_url: a.thumb_hash.as_ref().map(|_| self.file_url(a.id, "thumb", "thumb.webp")),
        }
    }

    pub fn ip_allowed(&self, ip: std::net::IpAddr) -> bool {
        self.ip.read().unwrap().allows(ip)
    }

    /// Re-reads the IP rules after a change and tells open connections to re-check themselves.
    pub async fn reload_ip_rules(&self) -> sqlx::Result<()> {
        let filter = db::load_ip_filter(&self.db).await?;
        *self.ip.write().unwrap() = filter;
        self.emit(Event::ip_recheck());
        Ok(())
    }

    pub fn emit(&self, ev: Event) {
        let _ = self.events.send(Arc::new(ev));
    }

    pub fn voice_states(&self) -> Vec<serde_json::Value> {
        self.voice.lock().unwrap().iter().map(|(u, s)| s.json(*u)).collect()
    }

    /// Removes the user's voice state if `session` still owns it, and tells everyone.
    pub fn voice_left(&self, user_id: i64, session: u64) {
        let removed = {
            let mut v = self.voice.lock().unwrap();
            match v.get(&user_id) {
                Some(s) if s.session == session => v.remove(&user_id),
                _ => None,
            }
        };
        if let Some(s) = removed {
            self.emit(Event::new("voice_state_update", serde_json::json!({ "user_id": user_id.to_string(), "channel_id": null, "previous_channel_id": s.channel_id.to_string() })));
        }
    }

    pub fn is_online(&self, user_id: i64) -> bool {
        self.presence.lock().unwrap().get(&user_id).is_some_and(|n| *n > 0)
    }

    /// Returns true when this is the user's first open connection.
    pub fn connect(&self, user_id: i64) -> bool {
        let mut p = self.presence.lock().unwrap();
        let n = p.entry(user_id).or_insert(0);
        *n += 1;
        *n == 1
    }

    /// Returns true when the user's last connection closed.
    pub fn disconnect(&self, user_id: i64) -> bool {
        let mut p = self.presence.lock().unwrap();
        match p.get_mut(&user_id) {
            Some(n) if *n > 1 => {
                *n -= 1;
                false
            }
            Some(_) => {
                p.remove(&user_id);
                true
            }
            None => false,
        }
    }
}

pub struct Host {
    pub name: String,
    pub data_dir: PathBuf,
    pub servers: RwLock<HashMap<String, Arc<ServerCtx>>>,
}

pub fn valid_slug(s: &str) -> bool {
    (2..=32).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl Host {
    pub async fn load(data_dir: &Path) -> Result<Arc<Host>> {
        let servers_dir = data_dir.join("servers");
        std::fs::create_dir_all(&servers_dir)?;
        let cfg = HostConfig::load_or_init(data_dir)?;
        let mut servers = HashMap::new();
        for entry in std::fs::read_dir(&servers_dir)? {
            let entry = entry?;
            let slug = entry.file_name().to_string_lossy().to_string();
            let db_path = entry.path().join("chitchat.db");
            if !entry.file_type()?.is_dir() || !valid_slug(&slug) || !db_path.exists() {
                continue;
            }
            let pool = db::open(&db_path, false).await?;
            let ctx = Arc::new(ServerCtx::open(slug.clone(), &entry.path(), pool).await?);
            crate::ai::spawn(ctx.clone());
            servers.insert(slug, ctx);
        }
        Ok(Arc::new(Host { name: cfg.name, data_dir: data_dir.to_path_buf(), servers: RwLock::new(servers) }))
    }

    pub async fn get(&self, slug: &str) -> Option<Arc<ServerCtx>> {
        self.servers.read().await.get(slug).cloned()
    }

    /// Creates a new server and returns its owner setup code.
    pub async fn create_server(&self, slug: &str, name: &str) -> Result<String> {
        if !valid_slug(slug) {
            bail!("invalid slug '{slug}': use 2-32 characters a-z, 0-9 and '-'");
        }
        let mut servers = self.servers.write().await;
        if servers.contains_key(slug) {
            bail!("server '{slug}' already exists");
        }
        let dir = self.data_dir.join("servers").join(slug);
        let (pool, code) = db::create_server(&dir, name).await?;
        let ctx = Arc::new(ServerCtx::open(slug.to_string(), &dir, pool).await?);
        crate::ai::spawn(ctx.clone());
        servers.insert(slug.to_string(), ctx);
        Ok(code)
    }
}
