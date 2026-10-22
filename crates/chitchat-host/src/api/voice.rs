//! Voice signaling over the WebSocket (docs/api.md section 4e). The media itself goes to the
//! SFU in `crate::voice`; this side checks permissions and room limits and keeps
//! `ServerCtx::voice` (who is where) in sync.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use str0m::change::{SdpAnswer, SdpOffer};
use tokio::sync::mpsc::UnboundedSender;

use super::Auth;
use crate::db;
use crate::events::Event;
use crate::host::{ServerCtx, VoiceState};
use crate::models::now_ms;
use crate::perms::{self, CONNECT_VOICE, SPEAK, STREAM, VIEW_CHANNEL};
use crate::voice::{self, Flags, VoiceOut};

const SPEAKING_EVERY: Duration = Duration::from_millis(100);

/// Why a voice request failed: sent to the client as `voice_end {reason, message}`.
pub type VoiceError = (&'static str, &'static str);

/// Per-WebSocket voice bookkeeping.
pub struct VoiceConn {
    /// Unique per connection; the SFU knows the peer connection by it.
    pub session: u64,
    pub out: UnboundedSender<VoiceOut>,
    /// The `Host` header the browser used: the address we advertise for UDP.
    pub host: Option<String>,
    speaking: bool,
    last_speaking: Option<Instant>,
}

impl VoiceConn {
    pub fn new(out: UnboundedSender<VoiceOut>, host: Option<String>) -> VoiceConn {
        VoiceConn { session: crate::ids::next_id() as u64, out, host, speaking: false, last_speaking: None }
    }
}

fn my_state(srv: &ServerCtx, user_id: i64, session: u64) -> Option<VoiceState> {
    srv.voice.lock().unwrap().get(&user_id).filter(|s| s.session == session).cloned()
}

fn flags(s: &VoiceState, perms: u64) -> Flags {
    Flags {
        can_speak: !s.suppressed,
        can_stream: s.streaming && perms::has(perms, STREAM) && !s.suppressed,
        self_mute: s.self_mute,
        self_deaf: s.self_deaf,
        screen_mids: s.screen_mids.iter().map(|m| m.as_str().into()).collect(),
    }
}

async fn timed_out(srv: &ServerCtx, user_id: i64) -> bool {
    let until: Option<i64> = sqlx::query_scalar("SELECT timed_out_until FROM users WHERE id = ?").bind(user_id).fetch_optional(&srv.db).await.ok().flatten().flatten();
    until.is_some_and(|t| t > now_ms())
}

async fn suppressed(srv: &ServerCtx, user_id: i64, perms: u64) -> bool {
    !perms::has(perms, SPEAK) || timed_out(srv, user_id).await
}

/// The address the browser reached us on, with the voice port: our ICE candidate.
async fn local_addr(host: Option<&str>, port: u16) -> Option<SocketAddr> {
    let host = host?;
    let name = match host.strip_prefix('[') {
        Some(rest) => rest.split(']').next()?,
        None => host.rsplit_once(':').map_or(host, |(h, _)| h),
    };
    if let Ok(ip) = name.parse::<IpAddr>() {
        return Some(SocketAddr::new(ip, port));
    }
    // The UDP socket listens on IPv4, like the default HTTP bind.
    tokio::net::lookup_host((name, port)).await.ok()?.find(SocketAddr::is_ipv4)
}

fn bool_field(data: &Value, key: &str) -> Option<bool> {
    data.get(key).and_then(Value::as_bool)
}

/// Handles one `voice_*` frame from the client.
pub async fn handle(srv: &Arc<ServerCtx>, auth: &Auth, vc: &mut VoiceConn, ty: &str, data: &Value) -> Result<(), VoiceError> {
    let user_id = auth.user.id;
    match ty {
        "voice_join" => join(srv, auth, vc, data).await,
        "voice_sdp" => {
            let (Some(sfu), Some(_)) = (voice::sfu(), my_state(srv, user_id, vc.session)) else { return Ok(()) };
            match data.get("type").and_then(Value::as_str) {
                Some("offer") => sfu.offer(vc.session, serde_json::from_value::<SdpOffer>(data.clone()).map_err(|_| ("bad_offer", "That voice offer isn't valid."))?),
                Some("answer") => sfu.answer(vc.session, serde_json::from_value::<SdpAnswer>(data.clone()).map_err(|_| ("bad_offer", "That voice answer isn't valid."))?),
                _ => return Err(("bad_offer", "voice_sdp needs type \"offer\" or \"answer\".")),
            }
            Ok(())
        }
        "voice_state" => {
            let wants_stream = bool_field(data, "streaming");
            if wants_stream == Some(true) && (!perms::has(auth.perms, STREAM) || timed_out(srv, user_id).await) {
                return Err(("missing_permission", "You don't have permission to share your screen."));
            }
            let state = {
                let mut v = srv.voice.lock().unwrap();
                let Some(s) = v.get_mut(&user_id).filter(|s| s.session == vc.session) else { return Ok(()) };
                if let Some(m) = bool_field(data, "self_mute") {
                    s.self_mute = m;
                }
                if let Some(d) = bool_field(data, "self_deaf") {
                    s.self_deaf = d;
                }
                if let Some(st) = wants_stream {
                    s.streaming = st;
                }
                if let Some(mids) = data.get("screen_mids").and_then(Value::as_array) {
                    s.screen_mids = mids.iter().filter_map(Value::as_str).filter(|m| !m.is_empty() && m.len() <= 32).take(4).map(str::to_string).collect();
                }
                s.clone()
            };
            if let Some(sfu) = voice::sfu() {
                sfu.set_flags(vc.session, flags(&state, auth.perms));
            }
            srv.emit(Event::new("voice_state_update", state.json(user_id)));
            Ok(())
        }
        "voice_leave" => {
            if my_state(srv, user_id, vc.session).is_some() {
                leave(srv, user_id, vc.session);
            }
            Ok(())
        }
        "voice_speaking" => {
            let Some(speaking) = bool_field(data, "speaking") else { return Ok(()) };
            let Some(s) = my_state(srv, user_id, vc.session) else { return Ok(()) };
            let silenced = s.suppressed || s.self_mute;
            let too_fast = vc.last_speaking.is_some_and(|t| t.elapsed() < SPEAKING_EVERY);
            // "Stopped speaking" always goes through so nobody stays lit up.
            if speaking == vc.speaking || (speaking && (silenced || too_fast)) {
                return Ok(());
            }
            vc.speaking = speaking;
            vc.last_speaking = Some(Instant::now());
            srv.emit(Event::new("voice_speaking", json!({ "user_id": user_id.to_string(), "channel_id": s.channel_id.to_string(), "speaking": speaking })));
            Ok(())
        }
        _ => Ok(()),
    }
}

async fn join(srv: &Arc<ServerCtx>, auth: &Auth, vc: &mut VoiceConn, data: &Value) -> Result<(), VoiceError> {
    let user_id = auth.user.id;
    let sfu = voice::sfu().ok_or(("voice_unavailable", "Voice isn't available on this host."))?;
    let channel_id = data.get("channel_id").and_then(Value::as_str).and_then(crate::ids::parse).ok_or(("unknown_channel", "No such voice channel."))?;
    let channel = match db::channel_row(&srv.db, channel_id).await {
        Ok(Some(c)) if c.kind == "voice" => c,
        _ => return Err(("unknown_channel", "No such voice channel.")),
    };
    if !perms::has(auth.perms, VIEW_CHANNEL | CONNECT_VOICE) {
        return Err(("missing_permission", "You don't have permission to join voice channels."));
    }
    let offer: SdpOffer = data.get("offer").cloned().and_then(|o| serde_json::from_value(o).ok()).ok_or(("bad_offer", "voice_join needs a valid offer."))?;
    let others = srv.voice.lock().unwrap().iter().filter(|(u, s)| s.channel_id == channel_id && **u != user_id).count() as i64;
    if !auth.is_owner && others >= channel.user_limit {
        return Err(("voice_full", "This voice channel is full."));
    }
    let local = local_addr(vc.host.as_deref(), sfu.port).await.ok_or(("voice_unavailable", "Can't work out this host's address for voice."))?;
    let state = VoiceState {
        channel_id,
        session: vc.session,
        self_mute: bool_field(data, "self_mute").unwrap_or(false),
        self_deaf: bool_field(data, "self_deaf").unwrap_or(false),
        suppressed: suppressed(srv, user_id, auth.perms).await,
        streaming: false,
        screen_mids: vec![],
    };
    let previous = srv.voice.lock().unwrap().insert(user_id, state.clone());
    if let Some(p) = previous.filter(|p| p.session != vc.session) {
        // One voice connection per user: the other tab or device drops out.
        sfu.leave(p.session, Some("moved"));
    }
    vc.speaking = false;
    sfu.join(vc.session, (srv.slug.as_str().into(), channel_id), local, offer, flags(&state, auth.perms), vc.out.clone());
    srv.emit(Event::new("voice_state_update", state.json(user_id)));
    Ok(())
}

/// Leaves voice for this connection (also used when the WebSocket closes).
pub fn leave(srv: &ServerCtx, user_id: i64, session: u64) {
    if let Some(sfu) = voice::sfu() {
        sfu.leave(session, None);
    }
    srv.voice_left(user_id, session);
}

/// After a permission change or a timeout: drop out without CONNECT_VOICE, update suppression.
pub async fn recheck(srv: &ServerCtx, user_id: i64, session: u64, perms: u64) -> Option<VoiceError> {
    let s = my_state(srv, user_id, session)?;
    if !perms::has(perms, VIEW_CHANNEL | CONNECT_VOICE) {
        leave(srv, user_id, session);
        return Some(("missing_permission", "You no longer have permission to be in voice."));
    }
    let now_suppressed = suppressed(srv, user_id, perms).await;
    // Losing STREAM (or a timeout) ends a screen share.
    let now_streaming = s.streaming && perms::has(perms, STREAM) && !timed_out(srv, user_id).await;
    if now_suppressed != s.suppressed || now_streaming != s.streaming {
        let state = {
            let mut v = srv.voice.lock().unwrap();
            let s = v.get_mut(&user_id).filter(|s| s.session == session)?;
            s.suppressed = now_suppressed;
            s.streaming = now_streaming;
            s.clone()
        };
        if let Some(sfu) = voice::sfu() {
            sfu.set_flags(session, flags(&state, perms));
        }
        srv.emit(Event::new("voice_state_update", state.json(user_id)));
    }
    None
}
