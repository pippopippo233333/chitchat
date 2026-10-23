# ChitChat

A self-hosted chat app in the spirit of Discord: text channels, voice and screen sharing, roles and moderation, running on your own computer. One small program (the **host**) serves the web client and keeps everything on your disk. There is no company server in between.

> **Status: alpha.** It works and is used daily by a small group, but expect rough edges and breaking changes. Read [Limitations](#limitations) before relying on it.

## Features

- **Servers and channels**: one host runs several servers (communities), each with its own accounts, text channels, voice channels and categories.
- **Messages**: replies, edits, reactions (any emoji, plus custom server emoji), mentions, pins, threads, search and YouTube embeds.
- **Direct messages and private calls**: one-to-one chats, and voice calls in them that ring the other person, who can invite others in. Hosts limit how many calls run at once.
- **Notifications**: unread and mention badges, per-channel settings, and Web Push to phones and browsers while ChitChat is closed.
- **Files**: images (resized, metadata removed), videos, documents, audio files and recorded voice messages, with per-user and per-server storage limits.
- **Voice and screen sharing**: built-in WebRTC SFU (no extra media server), with screen audio where the browser supports it.
- **Roles and permissions**, invites (limited or unlimited, revocable), bans, timeouts, IP rules, slow mode, anti-spam, and an audit log.
- **Optional AI moderation**: a local classifier and/or an LLM you choose (local or cloud, with a notice to members).
- **Server discovery without a central server**: hosts that opt in share their listed servers with each other (gossip).
- **Host dashboard**: CPU, RAM and disk use with limits, storage per server, backups, and one-click signed updates from GitHub Releases (with automatic rollback).
- **Desktop app** for Windows, macOS and Linux (Tauri), with system notifications, a tray icon and its own updates. On phones, install the web client from the browser.
- **Backups**: a script that makes daily, checked snapshots while the host keeps running, and restores them into a new folder.

## Quick start

You need [Rust](https://rustup.rs) (stable) and [Node.js](https://nodejs.org) 22.12 or newer. [ffmpeg](https://ffmpeg.org) on the PATH is optional: with it, the host removes metadata from videos and audio and makes video thumbnails.

```sh
cargo build --release -p chitchat-host
npm ci --prefix web
npm --prefix web run build
./target/release/chitchat-host --data-dir data server create friends --name "Friends"
./target/release/chitchat-host --data-dir data run --bind 127.0.0.1:8080 --web-dir web/dist
```

`server create` prints a one-time setup code. Open `http://127.0.0.1:8080/s/friends`, register with the setup code, and you are the server's owner.

To let others in (LAN, Tailscale, a domain with HTTPS, or a tunnel), follow the **[hosting guide](docs/hosting.md)**. Voice and the microphone need HTTPS (or `localhost`).

## Documentation

- [docs/hosting.md](docs/hosting.md): installing, connecting people, HTTPS, voice, firewalls, backups.
- [docs/api.md](docs/api.md): the host's HTTP and WebSocket API (what the web client uses).
- [web/README.md](web/README.md): developing the web client.

## Repository layout

- `crates/chitchat-host/`: the host (Rust: axum, SQLite via sqlx, str0m for voice).
- `web/`: the web client (Svelte 5 and TypeScript, built with Vite).
- `scripts/`: the backup script and its tests.
- `docs/`: hosting guide and API.

## Limitations

- Accounts belong to one server: the same person registers separately on each server.
- No end-to-end encryption. Whoever runs the host can read everything on it, like most chat servers.
- No native mobile app: on phones, use the browser or install the web client to the home screen. The desktop app's installers aren't code-signed yet, so Windows and antivirus programs may warn about new versions.
- Backups stay on the host's own disk; copy them somewhere else yourself.
- Tested mainly on Windows 11 and Ubuntu hosts with Chromium-based browsers (desktop and Android); other browsers may have gaps.

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md). Please report security problems privately as described in [SECURITY.md](SECURITY.md).

## License

[GNU Affero General Public License v3.0](LICENSE). If you run a modified version for other people over a network, you must offer them its source code.
