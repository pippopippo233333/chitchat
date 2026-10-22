//! Live events pushed to WebSocket connections of one server.

use serde::Serialize;

use crate::models::ChannelOut;

pub struct Event {
    /// When set, only connections that can view channels receive it.
    pub channel_id: Option<i64>,
    /// Receivers re-read their own permissions before handling this event (role/owner changes).
    pub refresh_perms: bool,
    /// Only connections with these permissions receive it (0 = everyone).
    pub needs: u64,
    /// When set, only these users' connections receive it (direct messages, a user's own read
    /// state), whatever their permissions.
    pub users: Option<Vec<i64>>,
    pub kind: EventKind,
}

pub enum EventKind {
    /// A ready-to-send `{type, data}` frame.
    Frame(String),
    /// A channel object; each receiver fills in its own `my_permissions`.
    Channel { ty: &'static str, channel: ChannelOut },
    /// Close matching connections (logout, password change, kick, ban).
    Close(CloseTarget),
    /// IP rules changed: every connection re-checks its own address.
    IpRecheck,
}

pub struct CloseTarget {
    pub token_hash: Option<String>,
    pub user_id: Option<i64>,
    /// Keep connections using this token (e.g. the one that changed the password).
    pub except_token: Option<String>,
    pub close_code: u16,
    pub error_code: &'static str,
    pub message: String,
}

pub fn frame(ty: &str, data: impl Serialize) -> String {
    serde_json::json!({ "type": ty, "data": data }).to_string()
}

impl Event {
    pub fn new(ty: &str, data: impl Serialize) -> Event {
        Event { channel_id: None, refresh_perms: false, needs: 0, users: None, kind: EventKind::Frame(frame(ty, data)) }
    }

    pub fn channel(ty: &'static str, channel: ChannelOut, channel_id: i64) -> Event {
        Event { channel_id: Some(channel_id), refresh_perms: false, needs: 0, users: None, kind: EventKind::Channel { ty, channel } }
    }

    pub fn in_channel(mut self, channel_id: i64) -> Event {
        self.channel_id = Some(channel_id);
        self
    }

    pub fn close(target: CloseTarget) -> Event {
        Event { channel_id: None, refresh_perms: false, needs: 0, users: None, kind: EventKind::Close(target) }
    }

    pub fn ip_recheck() -> Event {
        Event { channel_id: None, refresh_perms: false, needs: 0, users: None, kind: EventKind::IpRecheck }
    }

    pub fn for_users(mut self, users: Vec<i64>) -> Event {
        self.users = Some(users);
        self
    }

    pub fn needs(mut self, perms: u64) -> Event {
        self.needs = perms;
        self
    }

    pub fn refresh(mut self) -> Event {
        self.refresh_perms = true;
        self
    }
}
