//! Permission bits (see docs/api.md, "Role").

pub const VIEW_CHANNEL: u64 = 1 << 0;
pub const SEND_MESSAGES: u64 = 1 << 1;
pub const MANAGE_MESSAGES: u64 = 1 << 2;
pub const MANAGE_CHANNELS: u64 = 1 << 3;
#[allow(dead_code)]
pub const MANAGE_ROLES: u64 = 1 << 4;
#[allow(dead_code)]
pub const KICK_MEMBERS: u64 = 1 << 5;
#[allow(dead_code)]
pub const BAN_MEMBERS: u64 = 1 << 6;
#[allow(dead_code)]
pub const ATTACH_FILES: u64 = 1 << 7;
pub const ADD_REACTIONS: u64 = 1 << 8;
pub const ADMINISTRATOR: u64 = 1 << 9;
#[allow(dead_code)]
pub const MANAGE_SERVER: u64 = 1 << 10;
#[allow(dead_code)]
pub const CREATE_INVITES: u64 = 1 << 11;
pub const CONNECT_VOICE: u64 = 1 << 12;
pub const SPEAK: u64 = 1 << 13;
pub const STREAM: u64 = 1 << 14;

pub const ALL: u64 = (1 << 15) - 1;

/// What `@everyone` gets on a new server: VIEW + SEND + ADD_REACTIONS + CONNECT_VOICE + SPEAK = 12547.
pub const DEFAULT_EVERYONE: u64 = VIEW_CHANNEL | SEND_MESSAGES | ADD_REACTIONS | CONNECT_VOICE | SPEAK;

/// Resolves the owner and ADMINISTRATOR into "everything".
pub fn effective(base: u64, is_owner: bool) -> u64 {
    if is_owner || base & ADMINISTRATOR != 0 { ALL } else { base & ALL }
}

pub fn has(perms: u64, wanted: u64) -> bool {
    perms & ADMINISTRATOR != 0 || perms & wanted == wanted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_not_admin() {
        assert_eq!(DEFAULT_EVERYONE, 12547);
        assert!(!has(DEFAULT_EVERYONE, STREAM));
        assert!(!has(DEFAULT_EVERYONE, MANAGE_CHANNELS));
        assert!(has(effective(DEFAULT_EVERYONE, true), MANAGE_CHANNELS));
        assert_eq!(effective(ADMINISTRATOR, false), ALL);
    }
}
