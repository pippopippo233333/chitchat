//! One SQLite database per server: schema, creation and shared queries.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use rand::Rng;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{QueryBuilder, SqlitePool};

use crate::host::ServerCtx;
use crate::ids::next_id;
use crate::ipfilter::{Action, Cidr, IpFilter, Rule};
use crate::models::*;
use crate::perms;

const SCHEMA_V1: &str = r#"
CREATE TABLE server (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    icon_url TEXT,
    password_hash TEXT,
    invite_only INTEGER NOT NULL DEFAULT 0,
    registration_open INTEGER NOT NULL DEFAULT 1,
    listed INTEGER NOT NULL DEFAULT 1,
    owner_id INTEGER,
    setup_code TEXT,
    created_at INTEGER NOT NULL
);
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    avatar_url TEXT,
    created_at INTEGER NOT NULL
);
CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_id TEXT NOT NULL,
    ip TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER NOT NULL
);
CREATE INDEX sessions_user ON sessions(user_id);
CREATE TABLE roles (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    color TEXT,
    position INTEGER NOT NULL,
    permissions INTEGER NOT NULL,
    is_default INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE member_roles (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id INTEGER NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, role_id)
);
CREATE TABLE categories (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    position INTEGER NOT NULL
);
CREATE TABLE channels (
    id INTEGER PRIMARY KEY,
    type TEXT NOT NULL,
    name TEXT NOT NULL,
    topic TEXT NOT NULL DEFAULT '',
    category_id INTEGER REFERENCES categories(id) ON DELETE SET NULL,
    position INTEGER NOT NULL,
    media_allowed INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE messages (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    author_id INTEGER NOT NULL REFERENCES users(id),
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    edited_at INTEGER,
    reply_to INTEGER
);
CREATE INDEX messages_channel ON messages(channel_id, id);
CREATE TABLE reactions (
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    emoji TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (message_id, emoji, user_id)
);
"#;

const SCHEMA_V2: &str = r#"
ALTER TABLE server ADD COLUMN ip_mode TEXT NOT NULL DEFAULT 'off';
CREATE TABLE invites (
    code TEXT PRIMARY KEY,
    creator_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    max_uses INTEGER,
    uses INTEGER NOT NULL DEFAULT 0,
    expires_at INTEGER,
    created_at INTEGER NOT NULL
);
CREATE TABLE ip_rules (
    id INTEGER PRIMARY KEY,
    cidr TEXT NOT NULL,
    action TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL
);
"#;

const SCHEMA_V3: &str = r#"
ALTER TABLE server ADD COLUMN max_file_mb INTEGER NOT NULL DEFAULT 25;
ALTER TABLE server ADD COLUMN user_daily_mb INTEGER NOT NULL DEFAULT 100;
ALTER TABLE server ADD COLUMN user_total_mb INTEGER NOT NULL DEFAULT 1000;
ALTER TABLE server ADD COLUMN server_total_mb INTEGER NOT NULL DEFAULT 10240;
ALTER TABLE server ADD COLUMN media_retention_days INTEGER NOT NULL DEFAULT 0;
ALTER TABLE server ADD COLUMN file_secret TEXT;
CREATE TABLE blobs (
    hash TEXT PRIMARY KEY,
    size INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE attachments (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    message_id INTEGER REFERENCES messages(id) ON DELETE CASCADE,
    uploader_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    blob_hash TEXT NOT NULL,
    thumb_hash TEXT,
    filename TEXT NOT NULL,
    kind TEXT NOT NULL,
    mime TEXT NOT NULL,
    size INTEGER NOT NULL,
    width INTEGER,
    height INTEGER,
    created_at INTEGER NOT NULL
);
CREATE INDEX attachments_message ON attachments(message_id);
CREATE INDEX attachments_uploader ON attachments(uploader_id, created_at);
"#;

const SCHEMA_V4: &str = r#"
ALTER TABLE users ADD COLUMN timed_out_until INTEGER;
ALTER TABLE users ADD COLUMN signup_ip TEXT;
ALTER TABLE users ADD COLUMN signup_device TEXT;
CREATE INDEX users_signup_ip ON users(signup_ip);
ALTER TABLE server ADD COLUMN new_account_restrict_hours INTEGER NOT NULL DEFAULT 0;
ALTER TABLE server ADD COLUMN max_accounts_per_ip INTEGER NOT NULL DEFAULT 0;
ALTER TABLE server ADD COLUMN spam_messages_per_5s INTEGER NOT NULL DEFAULT 5;
ALTER TABLE server ADD COLUMN spam_chars_per_minute INTEGER NOT NULL DEFAULT 12000;
ALTER TABLE server ADD COLUMN spam_max_repeats INTEGER NOT NULL DEFAULT 2;
ALTER TABLE server ADD COLUMN spam_timeout_minutes INTEGER NOT NULL DEFAULT 10;
CREATE TABLE bans (
    id INTEGER PRIMARY KEY,
    user_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
    reason TEXT NOT NULL DEFAULT '',
    created_by INTEGER,
    created_at INTEGER NOT NULL,
    expires_at INTEGER
);
CREATE INDEX bans_user ON bans(user_id);
CREATE TABLE ban_marks (
    ban_id INTEGER NOT NULL REFERENCES bans(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    value TEXT NOT NULL
);
CREATE INDEX ban_marks_value ON ban_marks(kind, value);
CREATE TABLE audit_log (
    id INTEGER PRIMARY KEY,
    actor_id INTEGER,
    action TEXT NOT NULL,
    target_id INTEGER,
    details TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL
);
"#;

/// Voice: a per-channel user limit, and voice for `@everyone` (it had no use before).
const SCHEMA_V5: &str = r#"
ALTER TABLE channels ADD COLUMN user_limit INTEGER NOT NULL DEFAULT 10;
UPDATE roles SET permissions = permissions | 12288 WHERE is_default = 1;
"#;

/// Audio attachments: their length, recorded voice messages, and the owner's switch for them.
const SCHEMA_V9: &str = r#"
ALTER TABLE attachments ADD COLUMN duration_ms INTEGER;
ALTER TABLE attachments ADD COLUMN voice INTEGER NOT NULL DEFAULT 0;
ALTER TABLE server ADD COLUMN voice_messages INTEGER NOT NULL DEFAULT 1;
"#;

/// Search, mentions, direct messages, unread tracking and notification preferences.
const SCHEMA_V10: &str = r#"
-- 0 = none, 1 = @everyone, 2 = @here (only set when the author may ping everyone).
ALTER TABLE messages ADD COLUMN mention_everyone INTEGER NOT NULL DEFAULT 0;
CREATE TABLE message_mentions (
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    target_id INTEGER NOT NULL,
    PRIMARY KEY (message_id, kind, target_id)
) WITHOUT ROWID;
CREATE INDEX message_mentions_target ON message_mentions(kind, target_id, message_id);

-- A direct message is a channel of type 'dm' between dm_a < dm_b.
ALTER TABLE channels ADD COLUMN dm_a INTEGER REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE channels ADD COLUMN dm_b INTEGER REFERENCES users(id) ON DELETE CASCADE;
CREATE UNIQUE INDEX channels_dm ON channels(dm_a, dm_b) WHERE type = 'dm';

CREATE TABLE read_states (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    channel_id INTEGER NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    last_read_id INTEGER NOT NULL,
    PRIMARY KEY (user_id, channel_id)
) WITHOUT ROWID;
-- Everything sent before this upgrade counts as read.
INSERT INTO read_states (user_id, channel_id, last_read_id)
    SELECT u.id, m.channel_id, MAX(m.id) FROM users u, messages m GROUP BY u.id, m.channel_id;

ALTER TABLE users ADD COLUMN notify TEXT NOT NULL DEFAULT 'all';
ALTER TABLE users ADD COLUMN allow_dms INTEGER NOT NULL DEFAULT 1;
CREATE TABLE channel_notify (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    channel_id INTEGER NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    level TEXT NOT NULL,
    PRIMARY KEY (user_id, channel_id)
) WITHOUT ROWID;

-- SEND_DMS for @everyone (owners can take it away).
UPDATE roles SET permissions = permissions | 65536 WHERE is_default = 1;

CREATE VIRTUAL TABLE messages_fts USING fts5(content, content = 'messages', content_rowid = 'id', tokenize = 'unicode61 remove_diacritics 2');
CREATE TRIGGER messages_fts_insert AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts (rowid, content) VALUES (new.id, new.content);
END;
CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts (messages_fts, rowid, content) VALUES ('delete', old.id, old.content);
END;
CREATE TRIGGER messages_fts_update AFTER UPDATE OF content ON messages BEGIN
    INSERT INTO messages_fts (messages_fts, rowid, content) VALUES ('delete', old.id, old.content);
    INSERT INTO messages_fts (rowid, content) VALUES (new.id, new.content);
END;
INSERT INTO messages_fts (messages_fts) VALUES ('rebuild');
"#;

/// Web Push subscriptions: one per browser, tied to the login it was made with (logging out,
/// a kick or a ban removes it).
const SCHEMA_V11: &str = r#"
CREATE TABLE push_subscriptions (
    endpoint TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL REFERENCES sessions(token_hash) ON DELETE CASCADE,
    p256dh TEXT NOT NULL,
    auth TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX push_subscriptions_user ON push_subscriptions(user_id);
CREATE INDEX push_subscriptions_session ON push_subscriptions(token_hash);
"#;

/// Shown in the network-wide server browser (discovery.rs), opt-in.
const SCHEMA_V8: &str = r#"
ALTER TABLE server ADD COLUMN discoverable INTEGER NOT NULL DEFAULT 0;
"#;

/// What happens when the server's storage is full.
const SCHEMA_V7: &str = r#"
ALTER TABLE server ADD COLUMN storage_full_action TEXT NOT NULL DEFAULT 'delete_oldest';
"#;

/// AI moderation: the owner's settings (one JSON row) and the review queue.
const SCHEMA_V6: &str = r#"
CREATE TABLE ai_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    config TEXT NOT NULL
);
CREATE TABLE ai_flags (
    id INTEGER PRIMARY KEY,
    message_id INTEGER,
    channel_id INTEGER NOT NULL,
    author_id INTEGER NOT NULL,
    content TEXT NOT NULL,
    categories TEXT NOT NULL,
    scores TEXT NOT NULL,
    provider TEXT NOT NULL,
    reason TEXT NOT NULL DEFAULT '',
    action TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',
    created_at INTEGER NOT NULL,
    resolved_by INTEGER,
    resolved_at INTEGER
);
CREATE INDEX ai_flags_status ON ai_flags(status, id);
"#;

pub async fn open(path: &Path, create: bool) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(create)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new().max_connections(8).connect_with(opts).await.with_context(|| format!("opening {}", path.display()))?;
    migrate(&pool).await?;
    Ok(pool)
}

async fn migrate(pool: &SqlitePool) -> Result<()> {
    let version: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(pool).await?;
    if version < 1 {
        let mut tx = pool.begin().await?;
        sqlx::raw_sql(SCHEMA_V1).execute(&mut *tx).await?;
        sqlx::raw_sql("PRAGMA user_version = 1").execute(&mut *tx).await?;
        tx.commit().await?;
    }
    if version < 2 {
        let mut tx = pool.begin().await?;
        sqlx::raw_sql(SCHEMA_V2).execute(&mut *tx).await?;
        sqlx::raw_sql("PRAGMA user_version = 2").execute(&mut *tx).await?;
        tx.commit().await?;
    }
    if version < 3 {
        let mut tx = pool.begin().await?;
        sqlx::raw_sql(SCHEMA_V3).execute(&mut *tx).await?;
        sqlx::raw_sql("PRAGMA user_version = 3").execute(&mut *tx).await?;
        tx.commit().await?;
    }
    if version < 4 {
        let mut tx = pool.begin().await?;
        sqlx::raw_sql(SCHEMA_V4).execute(&mut *tx).await?;
        sqlx::raw_sql("PRAGMA user_version = 4").execute(&mut *tx).await?;
        tx.commit().await?;
    }
    if version < 5 {
        let mut tx = pool.begin().await?;
        sqlx::raw_sql(SCHEMA_V5).execute(&mut *tx).await?;
        sqlx::raw_sql("PRAGMA user_version = 5").execute(&mut *tx).await?;
        tx.commit().await?;
    }
    if version < 6 {
        let mut tx = pool.begin().await?;
        sqlx::raw_sql(SCHEMA_V6).execute(&mut *tx).await?;
        sqlx::raw_sql("PRAGMA user_version = 6").execute(&mut *tx).await?;
        tx.commit().await?;
    }
    if version < 7 {
        let mut tx = pool.begin().await?;
        sqlx::raw_sql(SCHEMA_V7).execute(&mut *tx).await?;
        sqlx::raw_sql("PRAGMA user_version = 7").execute(&mut *tx).await?;
        tx.commit().await?;
    }
    if version < 8 {
        let mut tx = pool.begin().await?;
        sqlx::raw_sql(SCHEMA_V8).execute(&mut *tx).await?;
        sqlx::raw_sql("PRAGMA user_version = 8").execute(&mut *tx).await?;
        tx.commit().await?;
    }
    if version < 9 {
        let mut tx = pool.begin().await?;
        sqlx::raw_sql(SCHEMA_V9).execute(&mut *tx).await?;
        sqlx::raw_sql("PRAGMA user_version = 9").execute(&mut *tx).await?;
        tx.commit().await?;
    }
    if version < 10 {
        let mut tx = pool.begin().await?;
        sqlx::raw_sql(SCHEMA_V10).execute(&mut *tx).await?;
        sqlx::raw_sql("PRAGMA user_version = 10").execute(&mut *tx).await?;
        tx.commit().await?;
    }
    if version < 11 {
        let mut tx = pool.begin().await?;
        sqlx::raw_sql(SCHEMA_V11).execute(&mut *tx).await?;
        sqlx::raw_sql("PRAGMA user_version = 11").execute(&mut *tx).await?;
        tx.commit().await?;
    }
    Ok(())
}

/// The key that signs file links, created on first use.
pub async fn file_key(pool: &SqlitePool) -> sqlx::Result<[u8; 32]> {
    let existing: Option<String> = sqlx::query_scalar("SELECT file_secret FROM server LIMIT 1").fetch_one(pool).await?;
    let hex = match existing {
        Some(h) if h.len() == 64 => h,
        _ => {
            let mut bytes = [0u8; 32];
            rand::thread_rng().fill(&mut bytes);
            let h: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            sqlx::query("UPDATE server SET file_secret = ?").bind(&h).execute(pool).await?;
            h
        }
    };
    let mut key = [0u8; 32];
    for (i, k) in key.iter_mut().enumerate() {
        *k = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap_or(0);
    }
    Ok(key)
}

/// The owner's IP rules, as checked on every request.
pub async fn load_ip_filter(pool: &SqlitePool) -> sqlx::Result<IpFilter> {
    let mode: String = sqlx::query_scalar("SELECT ip_mode FROM server LIMIT 1").fetch_one(pool).await?;
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT cidr, action FROM ip_rules").fetch_all(pool).await?;
    let mut rules: Vec<Rule> = rows
        .into_iter()
        .filter_map(|(c, a)| {
            let action = if a == "allow" { Action::Allow } else { Action::Block };
            Cidr::parse(&c).map(|cidr| Rule { cidr, action })
        })
        .collect();
    // Addresses of banned accounts (when the ban included their IP) are blocked like block rules.
    let banned: Vec<String> = sqlx::query_scalar(
        "SELECT m.value FROM ban_marks m JOIN bans b ON b.id = m.ban_id \
         WHERE m.kind = 'ip' AND (b.expires_at IS NULL OR b.expires_at > ?)",
    )
    .bind(now_ms())
    .fetch_all(pool)
    .await?;
    rules.extend(banned.iter().filter_map(|ip| Cidr::parse(ip)).map(|cidr| Rule { cidr, action: Action::Block }));
    Ok(IpFilter { allowlist: mode == "allowlist", rules })
}

/// A code like `K7QF-2MXD`, shown on the host console, that turns the first account into the owner.
pub fn new_setup_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    let mut code: String = (0..8).map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char).collect();
    code.insert(4, '-');
    code
}

/// Creates `dir/chitchat.db` with the default role and channels. Returns the pool and the owner setup code.
pub async fn create_server(dir: &Path, name: &str) -> Result<(SqlitePool, String)> {
    let db_path = dir.join("chitchat.db");
    if db_path.exists() {
        bail!("a server already exists in {}", dir.display());
    }
    std::fs::create_dir_all(dir)?;
    let pool = open(&db_path, true).await?;
    let code = new_setup_code();
    let now = now_ms();
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO server (id, name, setup_code, created_at) VALUES (?, ?, ?, ?)").bind(next_id()).bind(name).bind(&code).bind(now).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO roles (id, name, position, permissions, is_default) VALUES (?, '@everyone', 0, ?, 1)")
        .bind(next_id())
        .bind(perms::DEFAULT_EVERYONE as i64)
        .execute(&mut *tx)
        .await?;
    for (pos, (cat_name, kind, chan_name)) in [("Text Channels", "text", "general"), ("Voice Channels", "voice", "General")].into_iter().enumerate() {
        let cat_id = next_id();
        sqlx::query("INSERT INTO categories (id, name, position) VALUES (?, ?, ?)").bind(cat_id).bind(cat_name).bind(pos as i64).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO channels (id, type, name, category_id, position) VALUES (?, ?, ?, ?, 0)")
            .bind(next_id())
            .bind(kind)
            .bind(chan_name)
            .bind(cat_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok((pool, code))
}

// ---------------------------------------------------------------- queries

pub async fn server_row(pool: &SqlitePool) -> sqlx::Result<ServerRow> {
    sqlx::query_as("SELECT * FROM server LIMIT 1").fetch_one(pool).await
}

/// Effective server-wide permissions of a user, and whether they own the server.
pub async fn user_perms(pool: &SqlitePool, user_id: i64) -> sqlx::Result<(u64, bool)> {
    let owner: Option<i64> = sqlx::query_scalar("SELECT owner_id FROM server LIMIT 1").fetch_one(pool).await?;
    let is_owner = owner == Some(user_id);
    let rows: Vec<i64> = sqlx::query_scalar(
        "SELECT permissions FROM roles WHERE is_default = 1 \
         OR id IN (SELECT role_id FROM member_roles WHERE user_id = ?)",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    let base = rows.into_iter().fold(0u64, |acc, p| acc | p as u64);
    Ok((perms::effective(base, is_owner), is_owner))
}

pub async fn user_by_id(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<UserRow>> {
    sqlx::query_as("SELECT * FROM users WHERE id = ?").bind(id).fetch_optional(pool).await
}

pub async fn list_roles(pool: &SqlitePool) -> sqlx::Result<Vec<RoleOut>> {
    let rows: Vec<RoleRow> = sqlx::query_as("SELECT * FROM roles ORDER BY position DESC, id").fetch_all(pool).await?;
    Ok(rows.iter().map(RoleOut::from).collect())
}

pub async fn list_categories(pool: &SqlitePool) -> sqlx::Result<Vec<CategoryOut>> {
    let rows: Vec<CategoryRow> = sqlx::query_as("SELECT * FROM categories ORDER BY position, id").fetch_all(pool).await?;
    Ok(rows.iter().map(CategoryOut::from).collect())
}

/// A server channel (text or voice). Direct messages are never returned here, so channel
/// management can't reach them; see `any_channel_row`.
pub async fn channel_row(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<ChannelRow>> {
    sqlx::query_as("SELECT * FROM channels WHERE id = ? AND type != 'dm'").bind(id).fetch_optional(pool).await
}

/// Any channel, direct messages included.
pub async fn any_channel_row(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<ChannelRow>> {
    sqlx::query_as("SELECT * FROM channels WHERE id = ?").bind(id).fetch_optional(pool).await
}

/// Channels the user can see, ordered by category position, then channel position.
pub async fn list_channels(pool: &SqlitePool, user_perms: u64) -> sqlx::Result<Vec<ChannelOut>> {
    if !perms::has(user_perms, perms::VIEW_CHANNEL) {
        return Ok(Vec::new());
    }
    let rows: Vec<ChannelRow> = sqlx::query_as(
        "SELECT c.* FROM channels c LEFT JOIN categories k ON k.id = c.category_id WHERE c.type != 'dm' \
         ORDER BY COALESCE(k.position, -1), k.id, c.position, c.id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(|c| ChannelOut::new(c, user_perms)).collect())
}

async fn member_role_map(pool: &SqlitePool, user_id: Option<i64>) -> sqlx::Result<HashMap<i64, Vec<String>>> {
    let pairs: Vec<(i64, i64)> = match user_id {
        Some(id) => sqlx::query_as("SELECT user_id, role_id FROM member_roles WHERE user_id = ?").bind(id).fetch_all(pool).await?,
        None => sqlx::query_as("SELECT user_id, role_id FROM member_roles").fetch_all(pool).await?,
    };
    let mut map: HashMap<i64, Vec<String>> = HashMap::new();
    for (uid, rid) in pairs {
        map.entry(uid).or_default().push(rid.to_string());
    }
    Ok(map)
}

fn member_out(srv: &ServerCtx, u: &UserRow, roles: &mut HashMap<i64, Vec<String>>, owner: Option<i64>) -> MemberOut {
    MemberOut {
        user: UserOut::from(u),
        role_ids: roles.remove(&u.id).unwrap_or_default(),
        joined_at: ts(u.created_at),
        online: srv.is_online(u.id),
        is_owner: owner == Some(u.id),
        timed_out_until: u.timed_out_until.filter(|t| *t > now_ms()).map(ts),
    }
}

pub async fn list_members(srv: &ServerCtx) -> sqlx::Result<Vec<MemberOut>> {
    let owner = server_row(&srv.db).await?.owner_id;
    // Banned accounts aren't members while the ban lasts.
    let users: Vec<UserRow> = sqlx::query_as(
        "SELECT * FROM users WHERE id NOT IN (SELECT user_id FROM bans WHERE user_id IS NOT NULL \
         AND (expires_at IS NULL OR expires_at > ?)) ORDER BY id",
    )
    .bind(now_ms())
    .fetch_all(&srv.db)
    .await?;
    let mut roles = member_role_map(&srv.db, None).await?;
    Ok(users.iter().map(|u| member_out(srv, u, &mut roles, owner)).collect())
}

pub async fn member(srv: &ServerCtx, user_id: i64) -> sqlx::Result<Option<MemberOut>> {
    let Some(u) = user_by_id(&srv.db, user_id).await? else { return Ok(None) };
    let owner = server_row(&srv.db).await?.owner_id;
    let mut roles = member_role_map(&srv.db, Some(user_id)).await?;
    Ok(Some(member_out(srv, &u, &mut roles, owner)))
}

pub const MESSAGE_SELECT: &str = "SELECT m.id, m.channel_id, m.content, m.created_at, m.edited_at, m.reply_to, m.mention_everyone, \
     u.id AS author_id, u.username, u.display_name, u.avatar_url \
     FROM messages m JOIN users u ON u.id = m.author_id";

pub async fn message_row(pool: &SqlitePool, channel_id: i64, id: i64) -> sqlx::Result<Option<MessageRow>> {
    sqlx::query_as(&format!("{MESSAGE_SELECT} WHERE m.id = ? AND m.channel_id = ?")).bind(id).bind(channel_id).fetch_optional(pool).await
}

pub enum Page {
    Latest,
    Before(i64),
    After(i64),
    /// Up to half the page before this message and the rest from it on (jumping to a message).
    Around(i64),
}

/// A page of messages, always oldest → newest.
pub async fn message_page(pool: &SqlitePool, channel_id: i64, page: Page, limit: i64) -> sqlx::Result<Vec<MessageRow>> {
    let mut rows: Vec<MessageRow> = match page {
        Page::Latest => sqlx::query_as(&format!("{MESSAGE_SELECT} WHERE m.channel_id = ? ORDER BY m.id DESC LIMIT ?")).bind(channel_id).bind(limit).fetch_all(pool).await?,
        Page::Before(id) => {
            sqlx::query_as(&format!("{MESSAGE_SELECT} WHERE m.channel_id = ? AND m.id < ? ORDER BY m.id DESC LIMIT ?"))
                .bind(channel_id)
                .bind(id)
                .bind(limit)
                .fetch_all(pool)
                .await?
        }
        Page::Around(id) => {
            let mut before: Vec<MessageRow> = sqlx::query_as(&format!("{MESSAGE_SELECT} WHERE m.channel_id = ? AND m.id < ? ORDER BY m.id DESC LIMIT ?"))
                .bind(channel_id)
                .bind(id)
                .bind(limit / 2)
                .fetch_all(pool)
                .await?;
            before.reverse();
            let after: Vec<MessageRow> = sqlx::query_as(&format!("{MESSAGE_SELECT} WHERE m.channel_id = ? AND m.id >= ? ORDER BY m.id ASC LIMIT ?"))
                .bind(channel_id)
                .bind(id)
                .bind(limit - limit / 2)
                .fetch_all(pool)
                .await?;
            before.extend(after);
            return Ok(before);
        }
        Page::After(id) => {
            return sqlx::query_as(&format!("{MESSAGE_SELECT} WHERE m.channel_id = ? AND m.id > ? ORDER BY m.id ASC LIMIT ?"))
                .bind(channel_id)
                .bind(id)
                .bind(limit)
                .fetch_all(pool)
                .await;
        }
    };
    rows.reverse();
    Ok(rows)
}

/// Reaction summaries per message, in the order each emoji was first used. `me` is relative to `viewer`
/// (pass 0 for broadcast payloads, where it is always false).
pub async fn reactions_for(pool: &SqlitePool, ids: &[i64], viewer: i64) -> sqlx::Result<HashMap<i64, Vec<ReactionOut>>> {
    let mut out: HashMap<i64, Vec<ReactionOut>> = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let mut qb = QueryBuilder::new("SELECT message_id, emoji, COUNT(*), MAX(user_id = ");
    qb.push_bind(viewer);
    qb.push("), MIN(created_at) AS first FROM reactions WHERE message_id IN (");
    let mut sep = qb.separated(", ");
    for id in ids {
        sep.push_bind(*id);
    }
    qb.push(") GROUP BY message_id, emoji ORDER BY first");
    let rows: Vec<(i64, String, i64, i64, i64)> = qb.build_query_as().fetch_all(pool).await?;
    for (mid, emoji, count, me, _) in rows {
        out.entry(mid).or_default().push(ReactionOut { emoji, count, me: me != 0 });
    }
    Ok(out)
}

pub async fn attachments_for(srv: &ServerCtx, ids: &[i64]) -> sqlx::Result<HashMap<i64, Vec<AttachmentOut>>> {
    let mut out: HashMap<i64, Vec<AttachmentOut>> = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let mut qb = QueryBuilder::new("SELECT * FROM attachments WHERE message_id IN (");
    let mut sep = qb.separated(", ");
    for id in ids {
        sep.push_bind(*id);
    }
    qb.push(") ORDER BY id");
    let rows: Vec<AttachmentRow> = qb.build_query_as().fetch_all(&srv.db).await?;
    for a in rows {
        if let Some(mid) = a.message_id {
            out.entry(mid).or_default().push(srv.attachment_out(&a));
        }
    }
    Ok(out)
}

pub async fn messages_out(srv: &ServerCtx, rows: &[MessageRow], viewer: i64) -> sqlx::Result<Vec<MessageOut>> {
    let ids: Vec<i64> = rows.iter().map(|m| m.id).collect();
    let mut reactions = reactions_for(&srv.db, &ids, viewer).await?;
    let mut files = attachments_for(srv, &ids).await?;
    let mut mentions = mentions_for(&srv.db, &ids).await?;
    Ok(rows
        .iter()
        .map(|m| {
            let mut out = MessageOut::new(m, reactions.remove(&m.id).unwrap_or_default());
            out.attachments = files.remove(&m.id).unwrap_or_default();
            for (kind, target) in mentions.remove(&m.id).unwrap_or_default() {
                if kind == "role" { &mut out.mention_roles } else { &mut out.mentions }.push(target.to_string());
            }
            out
        })
        .collect())
}

/// (kind, target id) of each message's stored mentions.
async fn mentions_for(pool: &SqlitePool, ids: &[i64]) -> sqlx::Result<HashMap<i64, Vec<(String, i64)>>> {
    let mut out: HashMap<i64, Vec<(String, i64)>> = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let mut qb = QueryBuilder::new("SELECT message_id, kind, target_id FROM message_mentions WHERE message_id IN (");
    let mut sep = qb.separated(", ");
    for id in ids {
        sep.push_bind(*id);
    }
    qb.push(")");
    let rows: Vec<(i64, String, i64)> = qb.build_query_as().fetch_all(pool).await?;
    for (mid, kind, target) in rows {
        out.entry(mid).or_default().push((kind, target));
    }
    Ok(out)
}
