//! Voice rooms: a WebRTC SFU (str0m) on one UDP socket with the same port number as HTTP,
//! signaled over the WebSocket (docs/api.md section 4e). Each browser has one peer
//! connection to the host; the SFU forwards every speaker's audio to the others in the room.
//!
//! The SFU runs on its own thread (str0m is sans-IO and synchronous, like its chat example).
//! The async side sends it [`Cmd`]s and gets [`VoiceOut`]s back per connection.
//!
//! Networks that block UDP fall back to ICE-TCP (RFC 6544): browsers connect to the HTTP
//! port itself, and [`Demux`] hands those connections (RFC 4571 framed STUN) to the SFU
//! instead of the web server, so no extra port has to be opened.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::io::{self, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::pin::Pin;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

use serde_json::Value;
use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::format::{Codec, FormatParams};
use str0m::media::{Direction, Frequency, KeyframeRequest, KeyframeRequestKind, MediaData, MediaKind, Mid, Pt};
use str0m::net::{Protocol, Receive, TcpType, Transmit};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc::UnboundedSender;

/// A voice room: (server slug, channel id).
pub type Room = (Arc<str>, i64);

/// Sent back to a WebSocket connection.
pub enum VoiceOut {
    /// An SDP offer or answer for the browser: `{"type": "offer"|"answer", "sdp": "..."}`.
    Sdp(Value),
    /// The peer connection is gone (reason is a short code for the client).
    Ended(&'static str),
}

/// What a member may do in the room right now.
#[derive(Clone)]
pub struct Flags {
    /// Has SPEAK and isn't timed out.
    pub can_speak: bool,
    /// Has STREAM and isn't timed out (screen share video).
    pub can_stream: bool,
    pub self_mute: bool,
    pub self_deaf: bool,
    /// Their screen share's transceivers: audio there is the shared window's sound, which
    /// follows `can_stream` instead of the microphone mute.
    pub screen_mids: Vec<Mid>,
}

enum Cmd {
    Join {
        conn: u64,
        room: Room,
        local: SocketAddr,
        offer: SdpOffer,
        flags: Flags,
        out: UnboundedSender<VoiceOut>,
    },
    Offer {
        conn: u64,
        offer: SdpOffer,
    },
    Answer {
        conn: u64,
        answer: SdpAnswer,
    },
    Flags {
        conn: u64,
        flags: Flags,
    },
    Leave {
        conn: u64,
        reason: Option<&'static str>,
    },
    /// An ICE-TCP connection: frames for it go to `tx` (already length-prefixed).
    TcpOpen {
        peer: SocketAddr,
        tx: UnboundedSender<Vec<u8>>,
    },
    TcpData {
        peer: SocketAddr,
        data: Vec<u8>,
    },
    TcpClosed {
        peer: SocketAddr,
    },
}

pub struct Sfu {
    tx: std::sync::Mutex<Sender<Cmd>>,
    /// Commands nudge the SFU's socket wait with an empty datagram so they aren't delayed.
    waker: Option<(UdpSocket, SocketAddr)>,
    pub port: u16,
}

static SFU: OnceLock<Sfu> = OnceLock::new();

/// The running SFU, if the UDP socket could be bound.
pub fn sfu() -> Option<&'static Sfu> {
    SFU.get()
}

/// Binds the UDP socket and starts the SFU thread.
pub fn start(bind: SocketAddr) -> std::io::Result<()> {
    str0m::crypto::from_feature_flags().install_process_default();
    let socket = UdpSocket::bind(bind)?;
    let local = socket.local_addr()?;
    let wake_ip = match local.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    let waker = UdpSocket::bind(SocketAddr::new(wake_ip, 0)).ok().map(|w| (w, SocketAddr::new(wake_ip, local.port())));
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new().name("voice-sfu".into()).spawn(move || run(socket, rx))?;
    let _ = SFU.set(Sfu { tx: std::sync::Mutex::new(tx), waker, port: local.port() });
    Ok(())
}

impl Sfu {
    fn send(&self, cmd: Cmd) {
        let _ = self.tx.lock().unwrap().send(cmd);
        if let Some((w, to)) = &self.waker {
            let _ = w.send_to(&[], to);
        }
    }

    /// Starts (or replaces) the peer connection for `conn`; the answer comes back on `out`.
    pub fn join(&self, conn: u64, room: Room, local: SocketAddr, offer: SdpOffer, flags: Flags, out: UnboundedSender<VoiceOut>) {
        self.send(Cmd::Join { conn, room, local, offer, flags, out });
    }

    pub fn offer(&self, conn: u64, offer: SdpOffer) {
        self.send(Cmd::Offer { conn, offer });
    }

    pub fn answer(&self, conn: u64, answer: SdpAnswer) {
        self.send(Cmd::Answer { conn, answer });
    }

    pub fn set_flags(&self, conn: u64, flags: Flags) {
        self.send(Cmd::Flags { conn, flags });
    }

    /// Drops the connection. With a reason, the client is told `voice_end` with it.
    pub fn leave(&self, conn: u64, reason: Option<&'static str>) {
        self.send(Cmd::Leave { conn, reason });
    }
}

fn sdp_json<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

// ---------------------------------------------------------------- the SFU loop

/// How long an ICE-TCP connection may stay open without a peer connection accepting it.
const TCP_UNCLAIMED: Duration = Duration::from_secs(15);
const TCP_MAX_CONNS: usize = 256;

struct TcpPeer {
    tx: UnboundedSender<Vec<u8>>,
    opened: Instant,
    claimed: bool,
}

/// Where packets go out: the UDP socket, or the ICE-TCP connection to that peer.
struct Net {
    udp: UdpSocket,
    tcp: HashMap<SocketAddr, TcpPeer>,
}

impl Net {
    fn send(&self, t: &Transmit) {
        match t.proto {
            Protocol::Udp => {
                let _ = self.udp.send_to(&t.contents, t.destination);
            }
            Protocol::Tcp => {
                if let (Some(peer), Ok(len)) = (self.tcp.get(&t.destination), u16::try_from(t.contents.len())) {
                    let mut frame = Vec::with_capacity(t.contents.len() + 2);
                    frame.extend_from_slice(&len.to_be_bytes());
                    frame.extend_from_slice(&t.contents);
                    let _ = peer.tx.send(frame);
                }
            }
            _ => {}
        }
    }
}

fn run(socket: UdpSocket, rx: Receiver<Cmd>) {
    let mut clients: Vec<Client> = vec![];
    let mut queue: VecDeque<Propagated> = VecDeque::new();
    let mut buf = vec![0; 2000];
    let mut net = Net { udp: socket, tcp: HashMap::new() };

    loop {
        for c in clients.iter_mut().filter(|c| !c.rtc.is_alive()) {
            c.end("connection_lost");
        }
        clients.retain(|c| c.rtc.is_alive());
        // Dropping the sender closes the connection.
        net.tcp.retain(|_, p| !p.tx.is_closed() && (p.claimed || p.opened.elapsed() < TCP_UNCLAIMED));

        loop {
            match rx.try_recv() {
                Ok(cmd) => handle_cmd(cmd, &mut clients, &mut net),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }

        // Commands arrive from the async side, so never sleep long on the socket.
        let mut timeout = Instant::now() + Duration::from_millis(50);
        for c in clients.iter_mut() {
            timeout = timeout.min(poll_until_timeout(c, &mut queue, &net));
        }

        if let Some(p) = queue.pop_front() {
            propagate(&p, &mut clients);
            continue;
        }

        let wait = timeout.saturating_duration_since(Instant::now()).max(Duration::from_millis(1));
        let _ = net.udp.set_read_timeout(Some(wait));
        buf.resize(2000, 0);
        match net.udp.recv_from(&mut buf) {
            Ok((n, source)) => {
                deliver(&mut clients, Protocol::Udp, &buf[..n], source);
            }
            // Timeouts, and Windows' WSAECONNRESET after an ICMP "port unreachable".
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionReset) => {}
            Err(e) => tracing::warn!("voice socket read failed: {e}"),
        }

        let now = Instant::now();
        for c in &mut clients {
            c.handle_input(Input::Timeout(now));
        }
    }
}

fn receive(proto: Protocol, data: &[u8], source: SocketAddr, destination: SocketAddr, now: Instant) -> Option<Input<'_>> {
    let contents = data.try_into().ok()?;
    Some(Input::Receive(now, Receive { proto, source, destination, contents }))
}

/// Gives a packet to the peer connection that accepts it; false if none does.
fn deliver(clients: &mut [Client], proto: Protocol, data: &[u8], source: SocketAddr) -> bool {
    let now = Instant::now();
    // The socket listens on 0.0.0.0, so the destination is each client's own
    // advertised address (accepts() doesn't look at it; ICE does).
    let Some(c) = clients.iter_mut().find(|c| receive(proto, data, source, c.local, now).is_some_and(|i| c.rtc.accepts(&i))) else {
        return false;
    };
    if let Some(input) = receive(proto, data, source, c.local, now) {
        c.handle_input(input);
    }
    true
}

/// Opus allows stereo both ways (`stereo=1; sprop-stereo=1`), so shared game or video sound
/// keeps both channels; a mono microphone is still sent as mono.
fn rtc_builder() -> str0m::RtcConfig {
    let mut b = Rtc::builder();
    let codecs = b.codec_config();
    codecs.enable_opus(false, false);
    codecs.add_config(
        Pt::new_with_value(111),
        None,
        Codec::Opus,
        Frequency::FORTY_EIGHT_KHZ,
        Some(2),
        FormatParams { min_p_time: Some(10), use_inband_fec: Some(true), stereo: Some(true), sprop_stereo: Some(true), ..Default::default() },
    );
    b
}

/// Our ICE candidates: UDP, plus TCP on the same port for networks that block UDP.
/// CHITCHAT_VOICE_TCP_ONLY=1 offers only the TCP one (to test the fallback).
fn local_candidates(local: SocketAddr) -> Option<Vec<Candidate>> {
    let tcp = Candidate::builder().tcp().host(local).tcptype(TcpType::Passive).build().ok()?;
    if std::env::var_os("CHITCHAT_VOICE_TCP_ONLY").is_some_and(|v| v == "1") {
        return Some(vec![tcp]);
    }
    Some(vec![Candidate::host(local, "udp").ok()?, tcp])
}

fn handle_cmd(cmd: Cmd, clients: &mut Vec<Client>, net: &mut Net) {
    match cmd {
        Cmd::Join { conn, room, local, offer, flags, out } => {
            clients.retain(|c| c.conn != conn);
            let mut rtc = rtc_builder().build(Instant::now());
            match local_candidates(local) {
                Some(cands) => {
                    for cand in cands {
                        rtc.add_local_candidate(cand);
                    }
                }
                None => {
                    tracing::warn!("voice: bad local candidate {local}");
                    let _ = out.send(VoiceOut::Ended("voice_unavailable"));
                    return;
                }
            }
            let answer = match rtc.sdp_api().accept_offer(offer) {
                Ok(a) => a,
                Err(e) => {
                    tracing::debug!("voice: offer rejected: {e}");
                    let _ = out.send(VoiceOut::Ended("bad_offer"));
                    return;
                }
            };
            let _ = out.send(VoiceOut::Sdp(sdp_json(&answer)));
            let mut client = Client::new(conn, room, local, rtc, flags, out);
            for track in clients.iter().filter(|c| c.room == client.room).flat_map(|c| c.tracks_in.iter()) {
                client.tracks_out.push(TrackOut { track_in: Arc::downgrade(&track.id), state: TrackOutState::ToOpen });
            }
            clients.push(client);
        }
        Cmd::Offer { conn, offer } => {
            if let Some(c) = clients.iter_mut().find(|c| c.conn == conn) {
                c.handle_offer(offer);
            }
        }
        Cmd::Answer { conn, answer } => {
            let opened = clients.iter_mut().find(|c| c.conn == conn).map(|c| c.handle_answer(answer)).unwrap_or_default();
            // A new viewer can only start decoding video from a keyframe.
            for (origin, mid) in opened {
                if let Some(o) = clients.iter_mut().find(|c| c.conn == origin) {
                    o.force_keyframe(mid);
                }
            }
        }
        Cmd::Flags { conn, flags } => {
            if let Some(c) = clients.iter_mut().find(|c| c.conn == conn) {
                let started_stream = flags.can_stream && !c.flags.can_stream;
                c.flags = flags;
                if started_stream {
                    let mids: Vec<Mid> = c.tracks_in.iter().filter(|t| t.id.kind == MediaKind::Video).map(|t| t.id.mid).collect();
                    for mid in mids {
                        c.force_keyframe(mid);
                    }
                }
            }
        }
        Cmd::Leave { conn, reason } => {
            if let Some(pos) = clients.iter().position(|c| c.conn == conn) {
                let mut c = clients.remove(pos);
                if let Some(r) = reason {
                    c.end(r);
                }
                c.rtc.disconnect();
            }
        }
        Cmd::TcpOpen { peer, tx } => {
            if net.tcp.len() < TCP_MAX_CONNS {
                net.tcp.insert(peer, TcpPeer { tx, opened: Instant::now(), claimed: false });
            }
        }
        Cmd::TcpData { peer, data } => {
            if net.tcp.contains_key(&peer)
                && deliver(clients, Protocol::Tcp, &data, peer)
                && let Some(p) = net.tcp.get_mut(&peer)
            {
                p.claimed = true;
            }
        }
        Cmd::TcpClosed { peer } => {
            net.tcp.remove(&peer);
        }
    }
}

fn poll_until_timeout(c: &mut Client, queue: &mut VecDeque<Propagated>, net: &Net) -> Instant {
    loop {
        if !c.rtc.is_alive() {
            return Instant::now();
        }
        match c.poll_output(net) {
            Propagated::Timeout(t) => return t,
            p => queue.push_back(p),
        }
    }
}

/// Hands one client's output to the others in the same room.
fn propagate(p: &Propagated, clients: &mut [Client]) {
    let Some(origin) = p.origin() else { return };
    let Some(room) = clients.iter().find(|c| c.conn == origin).map(|c| c.room.clone()) else { return };
    for c in clients.iter_mut().filter(|c| c.conn != origin && c.room == room) {
        match p {
            Propagated::TrackOpen(_, t) => c.tracks_out.push(TrackOut { track_in: t.clone(), state: TrackOutState::ToOpen }),
            Propagated::MediaData(_, data) => c.write_media(origin, data),
            Propagated::KeyframeRequest(_, req, source, mid_in) if *source == c.conn => c.forward_keyframe_request(*req, *mid_in),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------- one peer connection

struct TrackIn {
    origin: u64,
    mid: Mid,
    kind: MediaKind,
}

struct TrackInEntry {
    id: Arc<TrackIn>,
    last_keyframe_request: Option<Instant>,
}

struct TrackOut {
    track_in: Weak<TrackIn>,
    state: TrackOutState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TrackOutState {
    ToOpen,
    Negotiating(Mid),
    Open(Mid),
    ToStop(Mid),
    NegotiatingStop(Mid),
}

impl TrackOut {
    fn mid(&self) -> Option<Mid> {
        match self.state {
            TrackOutState::ToOpen => None,
            TrackOutState::Negotiating(m) | TrackOutState::Open(m) | TrackOutState::ToStop(m) | TrackOutState::NegotiatingStop(m) => Some(m),
        }
    }
}

struct Client {
    conn: u64,
    room: Room,
    /// The address advertised as our ICE candidate to this browser.
    local: SocketAddr,
    rtc: Rtc,
    flags: Flags,
    out: UnboundedSender<VoiceOut>,
    /// Server-side renegotiation waits until the first connection is up.
    connected: bool,
    pending: Option<SdpPendingOffer>,
    tracks_in: Vec<TrackInEntry>,
    tracks_out: Vec<TrackOut>,
    ended: bool,
}

// MediaData is the common case (every packet); boxing it would allocate per packet.
#[allow(clippy::large_enum_variant)]
enum Propagated {
    Noop,
    Timeout(Instant),
    TrackOpen(u64, Weak<TrackIn>),
    MediaData(u64, MediaData),
    /// (from, request, the client that owns the track, its mid there)
    KeyframeRequest(u64, KeyframeRequest, u64, Mid),
}

impl Propagated {
    fn origin(&self) -> Option<u64> {
        match self {
            Propagated::TrackOpen(c, _) | Propagated::MediaData(c, _) | Propagated::KeyframeRequest(c, ..) => Some(*c),
            _ => None,
        }
    }
}

impl Client {
    fn new(conn: u64, room: Room, local: SocketAddr, rtc: Rtc, flags: Flags, out: UnboundedSender<VoiceOut>) -> Client {
        Client { conn, room, local, rtc, flags, out, connected: false, pending: None, tracks_in: vec![], tracks_out: vec![], ended: false }
    }

    fn end(&mut self, reason: &'static str) {
        if !self.ended {
            self.ended = true;
            let _ = self.out.send(VoiceOut::Ended(reason));
        }
    }

    fn handle_input(&mut self, input: Input) {
        if self.rtc.is_alive()
            && let Err(e) = self.rtc.handle_input(input)
        {
            tracing::debug!("voice: client {} input error: {e}", self.conn);
            self.rtc.disconnect();
        }
    }

    fn poll_output(&mut self, net: &Net) -> Propagated {
        if self.negotiate_if_needed() {
            return Propagated::Noop;
        }
        match self.rtc.poll_output() {
            Ok(Output::Transmit(t)) => {
                net.send(&t);
                Propagated::Noop
            }
            Ok(Output::Timeout(t)) => Propagated::Timeout(t),
            Ok(Output::Event(e)) => self.handle_event(e),
            Err(e) => {
                tracing::debug!("voice: client {} poll error: {e}", self.conn);
                self.rtc.disconnect();
                Propagated::Noop
            }
        }
    }

    fn handle_event(&mut self, e: Event) -> Propagated {
        match e {
            Event::Connected => {
                self.connected = true;
                Propagated::Noop
            }
            Event::IceConnectionStateChange(IceConnectionState::Disconnected) => {
                self.rtc.disconnect();
                Propagated::Noop
            }
            Event::MediaAdded(m) => {
                tracing::debug!("voice: client {} added {:?} {:?} {:?}", self.conn, m.kind, m.mid, m.direction);
                let entry = TrackInEntry { id: Arc::new(TrackIn { origin: self.conn, mid: m.mid, kind: m.kind }), last_keyframe_request: None };
                let weak = Arc::downgrade(&entry.id);
                self.tracks_in.push(entry);
                Propagated::TrackOpen(self.conn, weak)
            }
            Event::MediaData(data) => {
                // Muted (by themselves, or without SPEAK / timed out), or screen video/audio
                // without STREAM or while not streaming: nothing leaves the host.
                let video = self.tracks_in.iter().any(|t| t.id.mid == data.mid && t.id.kind == MediaKind::Video);
                let screen = video || self.flags.screen_mids.contains(&data.mid);
                let allowed = if screen { self.flags.can_stream } else { self.flags.can_speak && !self.flags.self_mute };
                if !allowed {
                    return Propagated::Noop;
                }
                if video {
                    tracing::trace!("voice: client {} video {:?}", self.conn, data.mid);
                }
                if !data.contiguous {
                    self.request_keyframe(data.mid);
                }
                Propagated::MediaData(self.conn, data)
            }
            Event::KeyframeRequest(mut req) => {
                tracing::debug!("voice: client {} wants a keyframe on {:?}", self.conn, req.mid);
                let Some(t) = self.tracks_out.iter().find(|t| t.mid() == Some(req.mid)) else { return Propagated::Noop };
                let Some(track_in) = t.track_in.upgrade() else { return Propagated::Noop };
                req.rid = None;
                Propagated::KeyframeRequest(self.conn, req, track_in.origin, track_in.mid)
            }
            _ => Propagated::Noop,
        }
    }

    fn request_keyframe(&mut self, mid: Mid) {
        let Some(entry) = self.tracks_in.iter_mut().find(|t| t.id.mid == mid) else { return };
        if entry.id.kind != MediaKind::Video || entry.last_keyframe_request.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
            return;
        }
        if let Some(mut w) = self.rtc.writer(mid) {
            let _ = w.request_keyframe(None, KeyframeRequestKind::Fir);
            entry.last_keyframe_request = Some(Instant::now());
        }
    }

    /// Asks this client's encoder for a keyframe on one of its incoming video tracks.
    fn force_keyframe(&mut self, mid: Mid) {
        if let Some(entry) = self.tracks_in.iter_mut().find(|t| t.id.mid == mid && t.id.kind == MediaKind::Video)
            && let Some(mut w) = self.rtc.writer(mid)
        {
            let _ = w.request_keyframe(None, KeyframeRequestKind::Pli);
            entry.last_keyframe_request = Some(Instant::now());
        }
    }

    fn forward_keyframe_request(&mut self, req: KeyframeRequest, mid_in: Mid) {
        if self.tracks_in.iter().any(|t| t.id.mid == mid_in)
            && let Some(mut w) = self.rtc.writer(mid_in)
        {
            let r = w.request_keyframe(req.rid, req.kind);
            tracing::debug!("voice: asked client {} for a keyframe on {mid_in:?}: {r:?}", self.conn);
        }
    }

    fn write_media(&mut self, origin: u64, data: &MediaData) {
        if self.flags.self_deaf {
            return;
        }
        let Some(mid) = self.tracks_out.iter().find(|o| o.track_in.upgrade().is_some_and(|i| i.origin == origin && i.mid == data.mid)).and_then(TrackOut::mid) else {
            return;
        };
        // Simulcast: forward only the high layer (or the single unlayered stream).
        if data.rid.is_some() && data.rid != Some("h".into()) {
            tracing::trace!("voice: skip rid {:?}", data.rid);
            return;
        }
        let Some(writer) = self.rtc.writer(mid) else {
            tracing::debug!("voice: client {} no writer for {mid:?}", self.conn);
            return;
        };
        let Some(pt) = writer.match_params(data.params) else {
            tracing::debug!("voice: client {} no matching payload type for {:?} on {mid:?}", self.conn, data.params.spec().codec);
            return;
        };
        tracing::trace!("voice: write to client {} {mid:?} pt {pt:?} {} bytes", self.conn, data.data.len());
        if let Err(e) = writer.write(pt, data.network_time, data.time, data.data.clone()) {
            tracing::debug!("voice: client {} write failed: {e}", self.conn);
            self.rtc.disconnect();
        }
    }

    /// Offers new outgoing tracks (and stops ones whose speaker left), one change at a time.
    fn negotiate_if_needed(&mut self) -> bool {
        if !self.connected || self.pending.is_some() {
            return false;
        }
        for t in &mut self.tracks_out {
            if let TrackOutState::Open(m) = t.state
                && t.track_in.upgrade().is_none()
            {
                t.state = TrackOutState::ToStop(m);
            }
        }
        let mut change = self.rtc.sdp_api();
        for t in &mut self.tracks_out {
            match t.state {
                TrackOutState::ToOpen => {
                    if let Some(track_in) = t.track_in.upgrade() {
                        // The stream id is the speaker's connection; the client maps it with voice_states.
                        let mid = change.add_media(track_in.kind, Direction::SendOnly, Some(track_in.origin.to_string()), None, None);
                        t.state = TrackOutState::Negotiating(mid);
                    }
                }
                TrackOutState::ToStop(m) => {
                    change.stop_media(m);
                    t.state = TrackOutState::NegotiatingStop(m);
                }
                _ => {}
            }
        }
        // Tracks whose speaker left before they were ever offered.
        self.tracks_out.retain(|t| t.state != TrackOutState::ToOpen || t.track_in.strong_count() > 0);
        if !change.has_changes() {
            return false;
        }
        let Some((offer, pending)) = change.apply() else { return false };
        tracing::debug!("voice: offering client {}: {:?}", self.conn, self.tracks_out.iter().map(|t| (t.mid(), t.state == TrackOutState::ToOpen)).collect::<Vec<_>>());
        let _ = self.out.send(VoiceOut::Sdp(sdp_json(&offer)));
        self.pending = Some(pending);
        true
    }

    fn handle_offer(&mut self, offer: SdpOffer) {
        // The browser's offer wins over ours (glare): redo our changes afterwards.
        self.pending = None;
        match self.rtc.sdp_api().accept_offer(offer) {
            Ok(answer) => {
                let _ = self.out.send(VoiceOut::Sdp(sdp_json(&answer)));
            }
            Err(e) => {
                tracing::debug!("voice: client {} renegotiation offer rejected: {e}", self.conn);
                return;
            }
        }
        for t in &mut self.tracks_out {
            match t.state {
                TrackOutState::Negotiating(_) => t.state = TrackOutState::ToOpen,
                TrackOutState::NegotiatingStop(m) => t.state = TrackOutState::ToStop(m),
                _ => {}
            }
        }
    }

    /// Returns the newly opened video tracks as (origin client, its mid).
    fn handle_answer(&mut self, answer: SdpAnswer) -> Vec<(u64, Mid)> {
        let Some(pending) = self.pending.take() else { return vec![] };
        if let Err(e) = self.rtc.sdp_api().accept_answer(pending, answer) {
            tracing::debug!("voice: client {} answer rejected: {e}", self.conn);
            self.rtc.disconnect();
            return vec![];
        }
        let mut opened = vec![];
        for t in &mut self.tracks_out {
            if let TrackOutState::Negotiating(m) = t.state {
                t.state = TrackOutState::Open(m);
                if let Some(i) = t.track_in.upgrade().filter(|i| i.kind == MediaKind::Video) {
                    opened.push((i.origin, i.mid));
                }
            }
        }
        self.tracks_out.retain(|t| !matches!(t.state, TrackOutState::NegotiatingStop(_)));
        opened
    }
}

// ---------------------------------------------------------------- ICE-TCP on the HTTP port

/// An `axum_server` acceptor that passes ICE-TCP connections to the SFU and everything else
/// (TLS, plain HTTP) to the inner acceptor.
#[derive(Clone)]
pub struct Demux<A>(pub A);

impl<A, S> axum_server::accept::Accept<TcpStream, S> for Demux<A>
where
    A: axum_server::accept::Accept<TcpStream, S> + Clone + Send + 'static,
    A::Future: Send,
    S: Send + 'static,
{
    type Stream = A::Stream;
    type Service = A::Service;
    type Future = Pin<Box<dyn Future<Output = io::Result<(A::Stream, A::Service)>> + Send>>;

    fn accept(&self, stream: TcpStream, service: S) -> Self::Future {
        let inner = self.0.clone();
        Box::pin(async move {
            if sfu().is_some() && is_ice_tcp(&stream).await {
                serve_tcp(stream);
                return Err(io::Error::other("ICE-TCP connection handed to voice"));
            }
            inner.accept(stream, service).await
        })
    }
}

/// RFC 4571 framing (a 2-byte length) around a STUN message (its magic cookie at bytes 4..8).
async fn is_ice_tcp(stream: &TcpStream) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut buf = [0u8; 10];
    loop {
        let n = match tokio::time::timeout_at(deadline, stream.peek(&mut buf)).await {
            Ok(Ok(n)) => n,
            _ => return false,
        };
        if n >= buf.len() {
            return buf[2] & 0xc0 == 0 && buf[6..10] == [0x21, 0x12, 0xa4, 0x42];
        }
        // EOF, a TLS record or an HTTP method: decided without waiting for more.
        if n == 0 || buf[0] == 0x16 || buf[0].is_ascii_alphabetic() || tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn serve_tcp(stream: TcpStream) {
    let (Some(sfu), Ok(peer)) = (sfu(), stream.peer_addr()) else { return };
    let _ = stream.set_nodelay(true);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    sfu.send(Cmd::TcpOpen { peer, tx });
    tokio::spawn(async move {
        let (mut r, mut w) = stream.into_split();
        let writer = async {
            while let Some(frame) = rx.recv().await {
                w.write_all(&frame).await?;
            }
            io::Result::Ok(())
        };
        tokio::select! {
            _ = read_frames(&mut r, sfu, peer) => {}
            _ = writer => {}
        }
        sfu.send(Cmd::TcpClosed { peer });
    });
}

async fn read_frames(r: &mut tokio::net::tcp::OwnedReadHalf, sfu: &Sfu, peer: SocketAddr) -> io::Result<()> {
    loop {
        let len = r.read_u16().await? as usize;
        let mut data = vec![0; len];
        r.read_exact(&mut data).await?;
        sfu.send(Cmd::TcpData { peer, data });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn first_bytes_are_ice_tcp(bytes: &[u8]) -> bool {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).await.unwrap();
        client.write_all(bytes).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        is_ice_tcp(&server).await
    }

    // (On Windows, antivirus loopback inspection can hold the fake TLS bytes back for the 5 s timeout.)
    #[tokio::test]
    async fn tells_ice_tcp_from_web_traffic() {
        // Length 20, then a STUN binding request header with the magic cookie.
        let stun = [0, 20, 0x00, 0x01, 0, 0, 0x21, 0x12, 0xa4, 0x42, 1, 2, 3, 4];
        assert!(first_bytes_are_ice_tcp(&stun).await);
        assert!(!first_bytes_are_ice_tcp(&[0x16, 0x03, 0x01, 0x02, 0x00, 0x01, 0x00, 0x01, 0xfc, 0x03, 0x03]).await);
        assert!(!first_bytes_are_ice_tcp(b"GET / HTTP/1.1\r\n").await);
    }
}
