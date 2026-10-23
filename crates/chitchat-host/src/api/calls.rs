//! Private calls: voice in a direct message (docs/api.md 4o). The media and signaling are the
//! normal voice ones (`super::voice`); this decides who may be in a call, rings people and keeps
//! everything about a call between its people. The host admin sets whether calls are allowed and
//! how big and how many they may be (Resources, `super::resources::call_limits`).

use std::sync::Arc;

use serde_json::{Value, json};

use super::Auth;
use super::voice::VoiceError;
use crate::events::Event;
use crate::host::{Call, ServerCtx};
use crate::models::{ChannelRow, UserRow};

const HIDDEN: VoiceError = ("unknown_channel", "No such voice channel.");

fn channel_arg(data: &Value) -> Option<i64> {
    data.get("channel_id").and_then(Value::as_str).and_then(crate::ids::parse)
}

/// Before `user` joins the DM `channel`'s call (`others` already in it): may they? Starts the call
/// if nobody is in it yet, and returns who to ring then.
pub async fn admit(srv: &Arc<ServerCtx>, channel: &ChannelRow, user: i64, others: i64) -> Result<Option<Vec<i64>>, VoiceError> {
    let limits = super::resources::call_limits();
    if !limits.enabled {
        return Err(("calls_disabled", "Calls are turned off on this host."));
    }
    let members = channel.dm_members();
    let mut calls = srv.calls.lock().unwrap();
    // A call nobody is in (its first join failed) doesn't count or keep invitations.
    calls.retain(|id, _| *id == channel.id || !srv.in_voice(*id).is_empty());
    if let Some(call) = calls.get(&channel.id).filter(|_| others > 0) {
        if !call.allows(user) {
            return Err(HIDDEN);
        }
        if others + 1 > i64::from(limits.max_people) {
            return Err(("voice_full", "This call is full."));
        }
        return Ok(None);
    }
    if !members.contains(&user) {
        return Err(HIDDEN);
    }
    let active = calls.keys().filter(|id| **id != channel.id).count();
    if active >= limits.max_calls as usize {
        return Err(("calls_busy", "Too many calls are going on on this server right now. Try again in a little while."));
    }
    calls.insert(channel.id, Call { members: members.clone(), invited: vec![] });
    Ok(Some(members.into_iter().filter(|m| *m != user).collect()))
}

/// Rings `to` about the call in `channel_id` (over the WebSocket, and a push if they're away).
pub fn ring(srv: &Arc<ServerCtx>, from: &UserRow, channel_id: i64, to: Vec<i64>, invited: bool) {
    if to.is_empty() {
        return;
    }
    let data = json!({
        "channel_id": channel_id.to_string(),
        "from_user_id": from.id.to_string(),
        "from_name": from.display_name,
        "invited": invited,
    });
    srv.emit(Event::new("call_ring", data).for_users(to.clone()));
    super::push::call_ring(srv.clone(), to, from.display_name.clone(), channel_id);
}

/// `call_decline {channel_id}`: says no to a ring. The people in the call are told.
pub fn decline(srv: &ServerCtx, user: i64, data: &Value) -> Result<(), VoiceError> {
    let Some(channel_id) = channel_arg(data) else { return Ok(()) };
    let audience = srv.calls.lock().unwrap().get(&channel_id).filter(|c| c.allows(user)).map(Call::audience);
    if let Some(audience) = audience {
        srv.emit(Event::new("call_declined", json!({ "channel_id": channel_id.to_string(), "user_id": user.to_string() })).for_users(audience));
    }
    Ok(())
}

/// `call_invite {channel_id, user_id}`: someone in the call brings in another member of the
/// server. They may then join the call (not read the direct message).
pub async fn invite(srv: &Arc<ServerCtx>, auth: &Auth, data: &Value) -> Result<(), VoiceError> {
    let me = auth.user.id;
    let (Some(channel_id), Some(target)) = (channel_arg(data), data.get("user_id").and_then(Value::as_str).and_then(crate::ids::parse)) else {
        return Err(("bad_request", "call_invite needs channel_id and user_id."));
    };
    let in_call = srv.voice.lock().unwrap().get(&me).is_some_and(|s| s.channel_id == channel_id);
    if !in_call || !srv.calls.lock().unwrap().contains_key(&channel_id) {
        return Err(("not_in_call", "Join the call before inviting people."));
    }
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE id = ?").bind(target).fetch_optional(&srv.db).await.ok().flatten();
    if exists.is_none() || super::moderation::user_banned(srv, target).await.unwrap_or(true) {
        return Err(("unknown_user", "That person isn't a member of this server."));
    }
    let max = super::resources::call_limits().max_people as usize;
    let member = {
        let mut calls = srv.calls.lock().unwrap();
        let Some(call) = calls.get_mut(&channel_id) else { return Err(("not_in_call", "Join the call before inviting people.")) };
        // Someone the call already allows (like the other person after declining) is just rung again.
        if !call.allows(target) {
            if call.audience().len() >= max {
                return Err(("voice_full", "This call can't have more people."));
            }
            call.invited.push(target);
        }
        call.members.contains(&target)
    };
    if srv.in_voice(channel_id).contains(&target) {
        return Ok(());
    }
    // The invited person sees who is already in the call.
    let states: Vec<Value> = srv.voice.lock().unwrap().iter().filter(|(_, s)| s.channel_id == channel_id).map(|(u, s)| s.json(*u)).collect();
    for state in states {
        srv.emit(Event::new("voice_state_update", state).for_users(vec![target]));
    }
    ring(srv, &auth.user, channel_id, vec![target], !member);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{Host, VoiceState};
    use crate::{db, perms};

    fn in_voice(srv: &ServerCtx, user: i64, channel_id: i64) {
        let state = VoiceState { channel_id, session: user as u64, self_mute: false, self_deaf: false, suppressed: false, streaming: false, screen_mids: vec![] };
        srv.voice.lock().unwrap().insert(user, state);
    }

    #[tokio::test]
    async fn calls_stay_between_their_people() {
        let dir = std::env::temp_dir().join(format!("chitchat-calls-{}", crate::ids::next_id()));
        let host = Host::load(&dir).await.unwrap();
        host.create_server("calls", "Calls").await.unwrap();
        let srv = host.get("calls").await.unwrap();
        let mut users = vec![];
        for name in ["ann", "ben", "cat", "dan"] {
            let id = crate::ids::next_id();
            sqlx::query("INSERT INTO users (id, username, display_name, password_hash, created_at) VALUES (?, ?, ?, 'unused', 0)")
                .bind(id)
                .bind(name)
                .bind(name)
                .execute(&srv.db)
                .await
                .unwrap();
            users.push(id);
        }
        let (ann, ben, cat, dan) = (users[0], users[1], users[2], users[3]);
        let dm = crate::ids::next_id();
        sqlx::query("INSERT INTO channels (id, type, name, topic, position, media_allowed, user_limit, dm_a, dm_b) VALUES (?, 'dm', '', '', 0, 1, 0, ?, ?)")
            .bind(dm)
            .bind(ann)
            .bind(ben)
            .execute(&srv.db)
            .await
            .unwrap();
        let channel = db::any_channel_row(&srv.db, dm).await.unwrap().unwrap();

        // Only the two people in the direct message can start its call; the other one is rung.
        assert_eq!(admit(&srv, &channel, cat, 0).await.unwrap_err().0, "unknown_channel");
        assert_eq!(admit(&srv, &channel, ann, 0).await.unwrap(), Some(vec![ben]));
        in_voice(&srv, ann, dm);
        assert!(srv.voice_states(ann).len() == 1 && srv.voice_states(ben).len() == 1);
        assert!(srv.voice_states(cat).is_empty(), "nobody else sees the call");
        assert_eq!(admit(&srv, &channel, cat, 1).await.unwrap_err().0, "unknown_channel");

        // Someone in the call invites cat, who may then join and see it.
        let auth =
            Auth { user: db::user_by_id(&srv.db, ann).await.unwrap().unwrap(), srv: srv.clone(), perms: perms::DEFAULT_EVERYONE, is_owner: false, token_hash: "test".into() };
        invite(&srv, &auth, &json!({ "channel_id": dm.to_string(), "user_id": cat.to_string() })).await.unwrap();
        assert_eq!(srv.voice_states(cat).len(), 1);
        assert_eq!(admit(&srv, &channel, cat, 1).await.unwrap(), None);
        in_voice(&srv, cat, dm);
        assert!(srv.voice_states(dan).is_empty());

        // Calls hold at most four people by default (ann, ben, cat and one more invite).
        invite(&srv, &auth, &json!({ "channel_id": dm.to_string(), "user_id": dan.to_string() })).await.unwrap();
        in_voice(&srv, ben, dm);
        in_voice(&srv, dan, dm);
        let eve = crate::ids::next_id();
        sqlx::query("INSERT INTO users (id, username, display_name, password_hash, created_at) VALUES (?, 'eve', 'eve', 'unused', 0)").bind(eve).execute(&srv.db).await.unwrap();
        assert_eq!(invite(&srv, &auth, &json!({ "channel_id": dm.to_string(), "user_id": eve.to_string() })).await.unwrap_err().0, "voice_full");

        // When the last person leaves, the call and its invitations end.
        srv.voice.lock().unwrap().clear();
        srv.end_call_if_empty(dm);
        assert!(srv.calls.lock().unwrap().is_empty());
        assert_eq!(admit(&srv, &channel, cat, 0).await.unwrap_err().0, "unknown_channel");
        let _ = std::fs::remove_dir_all(dir);
    }
}
