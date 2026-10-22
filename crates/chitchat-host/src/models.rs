//! Database rows and the JSON objects the API sends (docs/api.md section 3).

use serde::Serialize;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// RFC 3339 UTC with milliseconds, e.g. `2026-10-21T18:57:24.123Z`.
pub fn ts(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).unwrap_or_default().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

// ---------------------------------------------------------------- rows

#[derive(sqlx::FromRow, Clone)]
pub struct ServerRow {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub password_hash: Option<String>,
    pub invite_only: bool,
    pub registration_open: bool,
    pub listed: bool,
    pub owner_id: Option<i64>,
    pub setup_code: Option<String>,
    pub created_at: i64,
    pub ip_mode: String,
    pub max_file_mb: i64,
    pub user_daily_mb: i64,
    pub user_total_mb: i64,
    pub server_total_mb: i64,
    pub media_retention_days: i64,
    #[allow(dead_code)]
    pub file_secret: Option<String>,
    pub new_account_restrict_hours: i64,
    pub max_accounts_per_ip: i64,
    pub spam_messages_per_5s: i64,
    pub spam_chars_per_minute: i64,
    pub spam_max_repeats: i64,
    pub spam_timeout_minutes: i64,
    /// "delete_oldest" or "refuse" (see `api::storage`).
    pub storage_full_action: String,
    /// Shown to other hosts' server browsers (see `discovery`).
    pub discoverable: bool,
    /// Members may record voice messages (audio uploads with `voice=1`).
    pub voice_messages: bool,
}

#[derive(sqlx::FromRow, Clone)]
pub struct AttachmentRow {
    pub id: i64,
    pub channel_id: i64,
    pub message_id: Option<i64>,
    pub uploader_id: i64,
    pub blob_hash: String,
    pub thumb_hash: Option<String>,
    pub filename: String,
    pub kind: String,
    pub mime: String,
    pub size: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub created_at: i64,
    pub duration_ms: Option<i64>,
    /// Recorded in the app as a voice message.
    pub voice: bool,
}

#[derive(Serialize, Clone)]
pub struct AttachmentOut {
    pub id: String,
    pub filename: String,
    /// "image", "video", "audio" or "file".
    pub kind: String,
    pub mime: String,
    pub size: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub voice: bool,
    pub url: String,
    pub thumbnail_url: Option<String>,
}

#[derive(sqlx::FromRow, Clone)]
pub struct UserRow {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub password_hash: String,
    pub avatar_url: Option<String>,
    pub created_at: i64,
    pub timed_out_until: Option<i64>,
    pub signup_ip: Option<String>,
    pub signup_device: Option<String>,
}

#[derive(sqlx::FromRow, Clone)]
pub struct RoleRow {
    pub id: i64,
    pub name: String,
    pub color: Option<String>,
    pub position: i64,
    pub permissions: i64,
    pub is_default: bool,
}

#[derive(sqlx::FromRow, Clone)]
pub struct CategoryRow {
    pub id: i64,
    pub name: String,
    pub position: i64,
}

#[derive(sqlx::FromRow, Clone)]
pub struct ChannelRow {
    pub id: i64,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub name: String,
    pub topic: String,
    pub category_id: Option<i64>,
    pub position: i64,
    pub media_allowed: bool,
    /// Voice channels: most people in the room at once.
    pub user_limit: i64,
}

#[derive(sqlx::FromRow, Clone)]
pub struct MessageRow {
    pub id: i64,
    pub channel_id: i64,
    pub content: String,
    pub created_at: i64,
    pub edited_at: Option<i64>,
    pub reply_to: Option<i64>,
    pub author_id: i64,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

// ---------------------------------------------------------------- API objects

#[derive(Serialize, Clone)]
pub struct ServerSummary {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub requires_password: bool,
    pub invite_only: bool,
    pub registration_open: bool,
}

impl ServerSummary {
    pub fn new(slug: &str, row: &ServerRow) -> Self {
        ServerSummary {
            slug: slug.to_string(),
            name: row.name.clone(),
            description: row.description.clone(),
            icon_url: row.icon_url.clone(),
            requires_password: row.password_hash.is_some(),
            invite_only: row.invite_only,
            registration_open: row.registration_open,
        }
    }
}

#[derive(Serialize, Clone)]
pub struct ServerOut {
    #[serde(flatten)]
    pub summary: ServerSummary,
    pub id: String,
    pub owner_id: Option<String>,
    pub created_at: String,
    /// Whether members may send voice messages (hide the mic button when false).
    pub voice_messages: bool,
}

impl ServerOut {
    pub fn new(slug: &str, row: &ServerRow) -> Self {
        ServerOut {
            summary: ServerSummary::new(slug, row),
            id: row.id.to_string(),
            owner_id: row.owner_id.map(|v| v.to_string()),
            created_at: ts(row.created_at),
            voice_messages: row.voice_messages,
        }
    }
}

#[derive(Serialize, Clone)]
pub struct UserOut {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub created_at: String,
}

impl From<&UserRow> for UserOut {
    fn from(u: &UserRow) -> Self {
        UserOut { id: u.id.to_string(), username: u.username.clone(), display_name: u.display_name.clone(), avatar_url: u.avatar_url.clone(), created_at: ts(u.created_at) }
    }
}

#[derive(Serialize, Clone)]
pub struct MemberOut {
    pub user: UserOut,
    pub role_ids: Vec<String>,
    pub joined_at: String,
    pub online: bool,
    pub is_owner: bool,
    /// Set while the member is timed out.
    pub timed_out_until: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct RoleOut {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub position: i64,
    pub permissions: String,
    pub is_default: bool,
}

impl From<&RoleRow> for RoleOut {
    fn from(r: &RoleRow) -> Self {
        RoleOut {
            id: r.id.to_string(),
            name: r.name.clone(),
            color: r.color.clone(),
            position: r.position,
            permissions: (r.permissions as u64).to_string(),
            is_default: r.is_default,
        }
    }
}

#[derive(Serialize, Clone)]
pub struct CategoryOut {
    pub id: String,
    pub name: String,
    pub position: i64,
}

impl From<&CategoryRow> for CategoryOut {
    fn from(c: &CategoryRow) -> Self {
        CategoryOut { id: c.id.to_string(), name: c.name.clone(), position: c.position }
    }
}

#[derive(Serialize, Clone)]
pub struct ChannelOut {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    pub topic: String,
    pub category_id: Option<String>,
    pub position: i64,
    pub media_allowed: bool,
    pub user_limit: i64,
    pub my_permissions: String,
}

impl ChannelOut {
    pub fn new(c: &ChannelRow, perms: u64) -> Self {
        ChannelOut {
            id: c.id.to_string(),
            kind: c.kind.clone(),
            name: c.name.clone(),
            topic: c.topic.clone(),
            category_id: c.category_id.map(|v| v.to_string()),
            position: c.position,
            media_allowed: c.media_allowed,
            user_limit: c.user_limit,
            my_permissions: perms.to_string(),
        }
    }
}

#[derive(Serialize, Clone)]
pub struct AuthorOut {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct ReactionOut {
    pub emoji: String,
    pub count: i64,
    pub me: bool,
}

#[derive(Serialize, Clone)]
pub struct MessageOut {
    pub id: String,
    pub channel_id: String,
    pub author: AuthorOut,
    pub content: String,
    pub created_at: String,
    pub edited_at: Option<String>,
    pub reply_to: Option<String>,
    pub reactions: Vec<ReactionOut>,
    pub attachments: Vec<AttachmentOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
}

impl MessageOut {
    pub fn new(m: &MessageRow, reactions: Vec<ReactionOut>) -> Self {
        MessageOut {
            id: m.id.to_string(),
            channel_id: m.channel_id.to_string(),
            author: AuthorOut { id: m.author_id.to_string(), username: m.username.clone(), display_name: m.display_name.clone(), avatar_url: m.avatar_url.clone() },
            content: m.content.clone(),
            created_at: ts(m.created_at),
            edited_at: m.edited_at.map(ts),
            reply_to: m.reply_to.map(|v| v.to_string()),
            reactions,
            attachments: Vec::new(),
            nonce: None,
        }
    }
}
