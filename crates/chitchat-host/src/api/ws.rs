//! `GET /s/{slug}/api/ws`: identify, ready snapshot, then live events (docs/api.md section 5).

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ConnectInfo;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, header};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::broadcast::error::RecvError;

use super::voice::{self as voice_api, VoiceConn, VoiceError};
use super::{Auth, Srv};
use crate::db;
use crate::events::{Event, EventKind, frame};
use crate::host::ServerCtx;
use crate::perms::{self, SEND_MESSAGES, VIEW_CHANNEL};
use crate::voice::VoiceOut;

const IDENTIFY_TIMEOUT: Duration = Duration::from_secs(10);
const PING_EVERY: Duration = Duration::from_secs(30);
const TYPING_EVERY: Duration = Duration::from_secs(3);

pub const CLOSE_GENERIC: u16 = 4000;
pub const CLOSE_UNAUTHORIZED: u16 = 4001;
pub const CLOSE_BANNED: u16 = 4003;

#[derive(Deserialize)]
struct ClientFrame {
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    data: Value,
}

pub async fn handler(Srv(srv): Srv, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).map(str::to_string);
    ws.max_message_size(64 * 1024).on_upgrade(move |socket| run(socket, srv, addr.ip(), host))
}

async fn send_voice_end(socket: &mut WebSocket, (reason, message): VoiceError) -> bool {
    socket.send(Message::Text(frame("voice_end", json!({ "reason": reason, "message": message })).into())).await.is_ok()
}

async fn close(socket: &mut WebSocket, code: u16, reason: &str) {
    let _ = socket.send(Message::Close(Some(CloseFrame { code, reason: reason.to_string().into() }))).await;
}

async fn send_error(socket: &mut WebSocket, code: &str, message: &str) {
    let _ = socket.send(Message::Text(frame("error", json!({ "code": code, "message": message })).into())).await;
}

async fn identify(socket: &mut WebSocket, srv: &Arc<ServerCtx>) -> Option<Auth> {
    let first = tokio::time::timeout(IDENTIFY_TIMEOUT, socket.recv()).await;
    let token = match first {
        Ok(Some(Ok(Message::Text(text)))) => {
            serde_json::from_str::<ClientFrame>(&text).ok().filter(|f| f.ty == "identify").and_then(|f| f.data.get("token").and_then(Value::as_str).map(str::to_string))
        }
        _ => None,
    };
    let Some(token) = token else {
        send_error(socket, "unauthorized", "Send identify first.").await;
        close(socket, CLOSE_UNAUTHORIZED, "identify expected").await;
        return None;
    };
    match super::auth::authenticate(srv.clone(), token.trim()).await {
        Ok(auth) => Some(auth),
        Err(e) => {
            send_error(socket, e.code, &e.message).await;
            close(socket, CLOSE_UNAUTHORIZED, "unauthorized").await;
            None
        }
    }
}

async fn run(mut socket: WebSocket, srv: Arc<ServerCtx>, ip: IpAddr, host: Option<String>) {
    let Some(mut auth) = identify(&mut socket, &srv).await else { return };
    let user_id = auth.user.id;
    // Subscribe before the snapshot so nothing between the two is missed.
    let mut rx = srv.events.subscribe();
    let ready = match super::server::ready_snapshot(&auth).await {
        Ok(r) => r,
        Err(_) => {
            close(&mut socket, CLOSE_GENERIC, "server error").await;
            return;
        }
    };
    if socket.send(Message::Text(frame("ready", ready).into())).await.is_err() {
        return;
    }
    if srv.connect(user_id) {
        srv.emit(Event::new("presence_update", json!({ "user_id": user_id.to_string(), "online": true })));
    }
    // A new connection counts as on screen until the page says otherwise (`visibility`).
    let mut visible = true;
    srv.set_watching(user_id, true);

    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await;
    let mut last_typing: HashMap<i64, Instant> = HashMap::new();
    let (voice_tx, mut voice_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut vc = VoiceConn::new(voice_tx, host);

    loop {
        tokio::select! {
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if let Ok(f) = serde_json::from_str::<ClientFrame>(&text) {
                        if f.ty == "typing" {
                            typing(&srv, &auth, &f.data, &mut last_typing).await;
                        } else if f.ty == "visibility" {
                            if let Some(v) = f.data.get("visible").and_then(Value::as_bool)
                                && v != visible
                            {
                                visible = v;
                                srv.set_watching(user_id, v);
                            }
                        } else if f.ty.starts_with("voice_")
                            && let Err(e) = voice_api::handle(&srv, &auth, &mut vc, &f.ty, &f.data).await
                                && !send_voice_end(&mut socket, e).await {
                                    break;
                                }
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            },
            ev = rx.recv() => match ev {
                Ok(ev) => {
                    if ev.refresh_perms {
                        if let Ok((p, owner)) = db::user_perms(&srv.db, user_id).await
                            && (p != auth.perms || owner != auth.is_owner) {
                                auth.perms = p;
                                auth.is_owner = owner;
                                let update = frame("permissions_update", json!({ "permissions": p.to_string(), "is_owner": owner }));
                                if socket.send(Message::Text(update.into())).await.is_err() {
                                    break;
                                }
                            }
                        if let Some(e) = voice_api::recheck(&srv, user_id, vc.session, auth.perms).await
                            && !send_voice_end(&mut socket, e).await {
                                break;
                            }
                    }
                    match &ev.users {
                        Some(users) if !users.contains(&user_id) => continue,
                        Some(_) => {}
                        None if ev.channel_id.is_some() && !perms::has(auth.perms, VIEW_CHANNEL) => continue,
                        None => {}
                    }
                    if ev.needs != 0 && !perms::has(auth.perms, ev.needs) {
                        continue;
                    }
                    let text = match &ev.kind {
                        EventKind::Close(t) => {
                            let hit = t.token_hash.as_ref().is_some_and(|h| *h == auth.token_hash)
                                || t.user_id == Some(user_id);
                            let spared = t.except_token.as_ref().is_some_and(|h| *h == auth.token_hash);
                            if hit && !spared {
                                send_error(&mut socket, t.error_code, &t.message).await;
                                close(&mut socket, t.close_code, t.error_code).await;
                                break;
                            }
                            continue;
                        }
                        EventKind::IpRecheck => {
                            if !srv.ip_allowed(ip) {
                                send_error(&mut socket, "ip_blocked", "Your network isn't allowed to access this server.").await;
                                close(&mut socket, CLOSE_BANNED, "ip_blocked").await;
                                break;
                            }
                            continue;
                        }
                        EventKind::Frame(s) => s.clone(),
                        EventKind::Channel { ty, channel } => {
                            let mut c = channel.clone();
                            c.my_permissions = auth.perms.to_string();
                            frame(ty, c)
                        }
                    };
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(_)) => {
                    // Too far behind: the client reconnects and gets a fresh snapshot.
                    close(&mut socket, CLOSE_GENERIC, "too many events, reconnect").await;
                    break;
                }
                Err(RecvError::Closed) => break,
            },
            Some(out) = voice_rx.recv() => {
                let sent = match out {
                    VoiceOut::Sdp(sdp) => socket.send(Message::Text(frame("voice_sdp", sdp).into())).await.is_ok(),
                    VoiceOut::Ended(reason) => {
                        srv.voice_left(user_id, vc.session);
                        let message = match reason {
                            "moved" => "You joined voice somewhere else.",
                            "bad_offer" => "The voice connection couldn't be set up.",
                            "channel_deleted" => "This voice channel was deleted.",
                            _ => "The voice connection was lost.",
                        };
                        send_voice_end(&mut socket, (reason, message)).await
                    }
                };
                if !sent {
                    break;
                }
            }
            _ = ping.tick() => {
                if socket.send(Message::Ping(Default::default())).await.is_err() {
                    break;
                }
            }
        }
    }

    voice_api::leave(&srv, user_id, vc.session);
    if visible {
        srv.set_watching(user_id, false);
    }
    if srv.disconnect(user_id) {
        srv.emit(Event::new("presence_update", json!({ "user_id": user_id.to_string(), "online": false })));
    }
}

async fn typing(srv: &ServerCtx, auth: &Auth, data: &Value, last: &mut HashMap<i64, Instant>) {
    let Some(cid) = data.get("channel_id").and_then(Value::as_str).and_then(crate::ids::parse) else { return };
    if last.get(&cid).is_some_and(|t| t.elapsed() < TYPING_EVERY) {
        return;
    }
    let Ok(Some(ch)) = db::any_channel_row(&srv.db, cid).await else { return };
    let allowed = match ch.kind.as_str() {
        "text" => perms::has(auth.perms, VIEW_CHANNEL | SEND_MESSAGES),
        "dm" => ch.dm_members().contains(&auth.user.id) && perms::has(auth.perms, perms::SEND_DMS),
        _ => false,
    };
    if !allowed {
        return;
    }
    last.insert(cid, Instant::now());
    srv.emit(super::messages::scoped(Event::new("typing_start", json!({ "channel_id": cid.to_string(), "user_id": auth.user.id.to_string() })), &ch));
}
