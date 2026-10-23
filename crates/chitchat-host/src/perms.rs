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
/// `@everyone`, `@here` and role mentions notify people.
pub const MENTION_EVERYONE: u64 = 1 << 15;
/// Open and send direct messages.
pub const SEND_DMS: u64 = 1 << 16;

pub const ALL: u64 = (1 << 17) - 1;
pub const CHANNEL_OVERRIDES: u64 = SEND_MESSAGES | ATTACH_FILES | ADD_REACTIONS;

/// Apply @everyone first; across the member's other roles, an explicit allow wins a deny.
pub fn overwritten(base: u64, everyone: (u64, u64), roles: &[(u64, u64)]) -> u64 {
    if base & ADMINISTRATOR != 0 {
        return base;
    }
    let (allow, deny) = roles.iter().fold((0, 0), |(a, d), (ra, rd)| (a | ra, d | rd));
    (((base & !(everyone.1 & CHANNEL_OVERRIDES)) | (everyone.0 & CHANNEL_OVERRIDES)) & !(deny & CHANNEL_OVERRIDES)) | (allow & CHANNEL_OVERRIDES)
}

/// What `@everyone` gets on a new server: VIEW + SEND + ADD_REACTIONS + CONNECT_VOICE + SPEAK + SEND_DMS = 78083.
pub const DEFAULT_EVERYONE: u64 = VIEW_CHANNEL | SEND_MESSAGES | ADD_REACTIONS | CONNECT_VOICE | SPEAK | SEND_DMS;

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
        assert_eq!(DEFAULT_EVERYONE, 78083);
        assert!(!has(DEFAULT_EVERYONE, MENTION_EVERYONE));
        assert!(!has(DEFAULT_EVERYONE, STREAM));
        assert!(!has(DEFAULT_EVERYONE, MANAGE_CHANNELS));
        assert!(has(effective(DEFAULT_EVERYONE, true), MANAGE_CHANNELS));
        assert_eq!(effective(ADMINISTRATOR, false), ALL);
    }

    #[test]
    fn channel_rules_apply_defaults_then_roles_without_granting_admin() {
        let base = VIEW_CHANNEL | SEND_MESSAGES | ADD_REACTIONS;
        let restricted = overwritten(base, (0, SEND_MESSAGES | ATTACH_FILES), &[]);
        assert!(!has(restricted, SEND_MESSAGES));
        let writer = overwritten(base, (0, SEND_MESSAGES | ATTACH_FILES), &[(SEND_MESSAGES, 0)]);
        assert!(has(writer, SEND_MESSAGES));
        assert!(!has(writer, ATTACH_FILES));
        let both = overwritten(writer, (0, 0), &[(ATTACH_FILES, 0), (0, ATTACH_FILES)]);
        assert!(has(both, ATTACH_FILES));
        assert_eq!(overwritten(base, (ADMINISTRATOR, VIEW_CHANNEL), &[]), base);
        assert_eq!(overwritten(ALL, (0, CHANNEL_OVERRIDES), &[]), ALL);
    }
}
