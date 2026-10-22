# ChitChat API, v0 (Phase 1)

Owner: Claude (Rust server). Consumer: Codex (web client in `web/`).
Change policy: additive changes are fine at any time; breaking changes are announced over the bridge first.

## 1. Hosting model and URLs

One host process (`chitchat-host`) listens on one port (default `8080`, HTTPS optional) and hosts several **servers**.
Each server has a URL **slug** (`[a-z0-9-]{2,32}`), and everything for a server lives under `/s/{slug}/`.

| Path | What |
|---|---|
| `/` | Web client (static files from `web/dist`, SPA fallback to `index.html` for any unknown non-`/api`, non-`/s/*/api` path) |
| `/api/host` | Host-level info (server list) |
| `/s/{slug}/api/...` | REST API for one server |
| `/s/{slug}/api/ws` | WebSocket for one server |

- **Base path**: the web client is served at `/` on the same origin. It should build its URLs from a configurable base URL (default: `location.origin`) so the future desktop app can point at any host.
- **CORS**: the host allows any origin, the methods `GET, POST, PATCH, PUT, DELETE`, and the headers `Authorization, Content-Type`. Auth uses bearer tokens, not cookies.
- Client-side routes are up to the web client (e.g. `/s/{slug}/c/{channel_id}`); the host serves `index.html` for them.

## 2. Conventions

- JSON everywhere, UTF-8. Request bodies are `Content-Type: application/json`.
- **IDs** are snowflakes (64-bit, time-sortable) serialized as **strings**. A bigger ID is newer.
- **Timestamps**: RFC 3339 UTC strings, e.g. `"2026-10-21T18:57:24.123Z"`.
- **Auth**: `Authorization: Bearer <token>` on every endpoint except those marked *public*. Tokens are per server: a token for `/s/a` is invalid on `/s/b`.
- **Errors**: non-2xx status with this body:
  ```json
  { "error": { "code": "invalid_credentials", "message": "Wrong username or password." } }
  ```
  The `message` is human-readable (safe to show); `code` is stable (switch on it).

| HTTP | code(s) |
|---|---|
| 400 | `bad_request`, `validation_failed` (extra field `"fields": {"username": "too_short"}`) |
| 401 | `unauthorized` (missing/expired token → go back to login), `invalid_credentials` |
| 403 | `forbidden`, `banned`, `ip_blocked`, `server_password_required`, `wrong_server_password`, `invite_required`, `invalid_invite`, `registration_closed`, `account_too_new` |
| 404 | `not_found`, `unknown_server`, `unknown_channel`, `unknown_message` |
| 409 | `username_taken` |
| 413 | `too_large` |
| 403 | `timed_out` (extra field `"until": "<timestamp>"`): automatic or moderator mute, can't send messages until then |
| 403 | `invalid_setup_code` |
| 429 | `rate_limited` (extra field `"retry_after_ms": 1200`); `message` says why (too fast, too much text, repeated message) |
| 500 | `internal` |

- **Validation limits**
  - `username`: 2–32 characters `[a-z0-9_.]`, unique per server (case-insensitive; stored lowercase)
  - `display_name`: 1–32 characters, any printable text (trimmed)
  - `password`: 8–128 characters
  - message `content`: 1–4000 characters after trimming, at most 60 lines (`fields.content`: `empty`, `too_long`, `too_many_lines`)

- **Built-in anti-spam** (always on; owner-tunable later). Per user:
  - at most 5 messages per 5 s
  - at most 12,000 characters per 60 s
  - the same text (case/whitespace-insensitive) at most 2 times per 60 s
  - breaking a limit returns `429 rate_limited`; 5 violations within 10 minutes → automatic 10-minute `timed_out`

## 3. Objects

### HostInfo
```json
{ "name": "ChitChat Host", "version": "0.1.0",
  "servers": [ ServerSummary, ... ] }
```
Servers the owner marked unlisted are left out (they can still be joined by slug).

### ServerSummary
```json
{ "slug": "gaming", "name": "Gaming", "description": "…", "icon_url": null,
  "requires_password": true, "invite_only": false, "registration_open": true }
```

### Server (full, for members)
`ServerSummary` plus `{ "id": "…", "owner_id": "…", "created_at": "…" }`.

### User (public)
```json
{ "id": "…", "username": "mario", "display_name": "Mario", "avatar_url": null, "created_at": "…" }
```

### Member
```json
{ "user": User, "role_ids": ["…"], "joined_at": "…", "online": true, "is_owner": false }
```

### Role
```json
{ "id": "…", "name": "Moderator", "color": "#e67e22", "position": 1, "permissions": "6",
  "is_default": false }
```
`permissions` is a bit field serialized as a **decimal string**. The `@everyone` role has `is_default: true` and applies to every member. Higher `position` = higher rank.

| Bit | Name |
|---|---|
| 1<<0 | VIEW_CHANNEL |
| 1<<1 | SEND_MESSAGES |
| 1<<2 | MANAGE_MESSAGES (delete or edit others' messages) |
| 1<<3 | MANAGE_CHANNELS |
| 1<<4 | MANAGE_ROLES |
| 1<<5 | KICK_MEMBERS |
| 1<<6 | BAN_MEMBERS |
| 1<<7 | ATTACH_FILES |
| 1<<8 | ADD_REACTIONS |
| 1<<9 | ADMINISTRATOR (everything) |
| 1<<10 | MANAGE_SERVER |
| 1<<11 | CREATE_INVITES |
| 1<<12 | CONNECT_VOICE |
| 1<<13 | SPEAK |
| 1<<14 | STREAM (screen share) |

The owner implicitly has every permission.

### Category
```json
{ "id": "…", "name": "Text Channels", "position": 0 }
```

### Channel
```json
{ "id": "…", "type": "text", "name": "general", "topic": "", "category_id": "…|null",
  "position": 0, "media_allowed": false, "user_limit": 10, "my_permissions": "12547" }
```
- `type`: `"text"` or `"voice"` (voice: see section 4e).
- `user_limit`: voice channels only (ignored for text), 1–99, default 10. Most people in the room at once; the owner can always join.
- `my_permissions`: the effective permissions of the current user in this channel, already computed by the server. Use it to show or hide UI (send box, delete buttons…).
- Channels where the user lacks VIEW_CHANNEL are not returned.

### Message
```json
{ "id": "…", "channel_id": "…",
  "author": { "id": "…", "username": "mario", "display_name": "Mario", "avatar_url": null },
  "content": "hello", "created_at": "…", "edited_at": null,
  "reply_to": "…|null",
  "reactions": [ { "emoji": "👍", "count": 3, "me": true } ],
  "attachments": [] }
```
`attachments` stays empty until Phase 3. `nonce` (see Send message) is echoed back when the client sent one.

## 4. REST endpoints

### Host
| Method & path | Auth | Description |
|---|---|---|
| `GET /api/host` | public | → `HostInfo` |

### Auth: `/s/{slug}/api/auth/...`
| Method & path | Auth | Body → Response |
|---|---|---|
| `GET /s/{slug}/api/server/public` | public | → `ServerSummary` (to draw the join/login screen) |
| `POST /auth/register` | public | `{ username, password, display_name, server_password?, invite_code?, setup_code?, device_id }` → `201 { token, user: User }` |
| `POST /auth/login` | public | `{ username, password, device_id }` → `{ token, user: User }` |
| `POST /auth/logout` | yes | → `204` (revokes this token) |
| `GET /me` | yes | → `{ user: User, permissions: "…", is_owner: bool }` (server-wide permissions) |
| `PATCH /me` | yes | `{ display_name?, password?: { old, new } }` → `User` |

- `device_id`: a random UUID the client generates once and keeps in `localStorage` (key: `chitchat.device_id`), shared by all servers on the same host. It is used for owner-configured device bans.
- `setup_code`: printed in the host console when a server is created. The first account that registers with it becomes **owner**.
- `server_password` is checked on **register only**. IP rules and bans are checked on every request.

### Server data (all need auth, prefix `/s/{slug}/api`)
| Method & path | Response |
|---|---|
| `GET /server` | `Server` |
| `GET /categories` | `Category[]` (sorted by position) |
| `GET /channels` | `Channel[]` (sorted by category position, then position) |
| `GET /members` | `Member[]` (Phase 1: all members; paging later for big servers) |
| `GET /members/{user_id}` | `Member` |
| `GET /roles` | `Role[]` |

### Messages
| Method & path | Body → Response | Permission |
|---|---|---|
| `GET /channels/{id}/messages?before={id}&after={id}&limit=50` | → `Message[]` **sorted oldest → newest** | VIEW_CHANNEL |
| `POST /channels/{id}/messages` | `{ content, reply_to?, nonce? }` → `201 Message` | SEND_MESSAGES |
| `PATCH /channels/{id}/messages/{mid}` | `{ content }` → `Message` | author only |
| `DELETE /channels/{id}/messages/{mid}` | → `204` | author or MANAGE_MESSAGES |
| `PUT /channels/{id}/messages/{mid}/reactions/{emoji}` | → `204` | ADD_REACTIONS |
| `DELETE /channels/{id}/messages/{mid}/reactions/{emoji}` | → `204` (removes your own reaction) | none |

- Paging: `limit` 1–100 (default 50). With no cursor you get the latest messages. `before` returns messages older than that id; `after` returns newer ones. If fewer than `limit` come back, there is no more history in that direction.
- `{emoji}` is URL-encoded (a Unicode emoji, at most 32 bytes).
- `nonce`: an optional client string (≤ 64 characters) echoed in the response and in the `message_create` event, so optimistic sends can be matched.

### Channels and categories
| Method & path | Permission |
|---|---|
| `POST /categories` `{ name, position? }` · `PATCH /categories/{id}` `{ name?, position? }` · `DELETE /categories/{id}` | MANAGE_CHANNELS |
| `POST /channels` `{ name, type, category_id?, topic?, position?, user_limit? }` → `201 Channel` | MANAGE_CHANNELS |
| `PATCH /channels/{id}` `{ name?, topic?, category_id?: string\|null, position?, media_allowed?, user_limit? }` → `Channel` | MANAGE_CHANNELS |
| `DELETE /channels/{id}` → `204` (its messages are deleted too) | MANAGE_CHANNELS |

Deleting a category keeps its channels; they get `category_id: null` (a `channel_update` event is sent for each).

## 4b. Owner controls (Phase 2)

"Owner" means `is_owner`. Owner-only actions return `403 forbidden` for everyone else, including ADMINISTRATOR.

### Server settings
| Method & path | Body → Response | Who |
|---|---|---|
| `GET /settings` | → `Settings` | MANAGE_SERVER |
| `PATCH /settings` | `{ name?, description?, invite_only?, registration_open?, listed?, discoverable?, ip_mode? }` → `Settings` | MANAGE_SERVER (`ip_mode` is owner only) |
| `PUT /settings/password` | `{ password: string \| null }` → `Settings` (null or "" removes it; 4–128 characters) | owner |

```json
Settings = { "name": "Gaming", "description": "", "has_password": true, "invite_only": false,
             "registration_open": true, "listed": true, "ip_mode": "off" }
```
- `name`: 1–64 characters; `description`: ≤ 500 characters.
- `listed: false` hides the server from `GET /api/host`; people can still join by slug or invite link.
- `registration_open: false` means no new accounts at all (invites included); existing members can still log in.
- A server `server_update` event goes to everyone after each change.

### Invites
| Method & path | Body → Response | Who |
|---|---|---|
| `POST /invites` | `{ max_uses?: 1–10000, expires_in?: seconds 60–31536000 }` → `201 Invite` | CREATE_INVITES |
| `GET /invites` | → `Invite[]` (only usable ones; MANAGE_SERVER sees everyone's, others see their own) | CREATE_INVITES |
| `DELETE /invites/{code}` | → `204` | the creator, or MANAGE_SERVER |
| `GET /invites/{code}` | *public* → `{ code, server: ServerSummary, expires_at, uses_left }`, or `404 invalid_invite` | anyone |

```json
Invite = { "code": "k7qf2mxdab", "creator_id": "…", "max_uses": 10, "uses": 3,
           "expires_at": "…|null", "created_at": "…" }
```
- Suggested link format for the client: `{base}/s/{slug}/invite/{code}` (client route; the host serves `index.html` for it).
- **Register** with `invite_code` to use one. On an `invite_only` server it's required (`invite_required`). On any server, an invalid, expired or used-up invite is rejected with `invalid_invite`.
- An invite does **not** skip the server password: if both are set, both are needed.
- The owner setup code skips all join checks.

### IP rules (owner only)
| Method & path | Body → Response |
|---|---|
| `GET /ip-rules` | → `IpRule[]` |
| `POST /ip-rules` | `{ cidr, action: "allow" \| "block", note? }` → `201 IpRule` (`cidr` like `203.0.113.7`, `203.0.113.0/24` or `2001:db8::/32`; host bits are cleared, e.g. `1.2.3.77/24` → `1.2.3.0/24`) |
| `DELETE /ip-rules/{id}` | → `204` |

```json
IpRule = { "id": "…", "cidr": "203.0.113.0/24", "action": "block", "note": "spammer", "created_at": "…" }
```
- `block` rules always apply. With `ip_mode: "allowlist"` (in `/settings`), **only** addresses matching an `allow` rule get in.
- The host machine itself (127.0.0.1 / ::1) is always allowed, so the owner can't lock themselves out.
- A blocked address gets `403 ip_blocked` on every request to that server (the `/server/public` preview included). Open WebSockets from a newly blocked address are closed with `4003` after an `ip_blocked` error frame.
- Behind a tunnel (Cloudflare, playit) every visitor appears with the tunnel's address; IP rules are only useful with direct connections, Tailscale or LAN for now.

### Roles
| Method & path | Body → Response |
|---|---|
| `POST /roles` | `{ name, color?: "#rrggbb" \| null, permissions?: "decimal string", position? }` → `201 Role` |
| `PATCH /roles/{id}` | same fields, all optional → `Role` (`@everyone`: only `permissions` and `color`) |
| `DELETE /roles/{id}` | → `204` (not `@everyone`) |
| `PUT /members/{user_id}/roles/{role_id}` | → `204` give a role |
| `DELETE /members/{user_id}/roles/{role_id}` | → `204` take it away |

All need MANAGE_ROLES, plus the **rank rule** (the owner is exempt):
- You can only edit, delete, give or take roles positioned **below** your own highest role.
- You can't grant permissions you don't have yourself.

Positions: `@everyone` is 0; the other roles are 1, 2, 3… (higher = more senior). A new role goes to position 1 (bottom) unless `position` is given. Moving a role re-numbers the others, so a `role_update` event follows for every role.

## 4c. Images and videos (Phase 3)

Uploads are allowed only in text channels with `media_allowed: true` (set by MANAGE_CHANNELS), and only for members with **ATTACH_FILES** (bit 7, not in the default 259).

**Step 1: upload** each file with the raw bytes as the body (no multipart):
```
POST /channels/{id}/attachments?filename=My%20Photo.jpg
Authorization: Bearer …
Content-Type: application/octet-stream   (ignored: the server detects the type from the content)
<file bytes>
→ 201 Attachment
```
**Step 2: send** a message that uses them: `POST /channels/{id}/messages { content, attachment_ids: ["…"], … }`.
- Up to 10 attachments; `content` may be empty when there are attachments.
- An attachment can be used once, only by its uploader, only in the channel it was uploaded to; otherwise `400 validation_failed` `fields.attachment_ids` = `unknown_attachment` / `too_many`.
- Uploads never sent are deleted after 1 hour.

```json
Attachment = { "id": "…", "filename": "My_Photo.webp", "kind": "image" | "video" | "audio" | "file", "mime": "image/webp",
               "size": 35921, "width": 3000, "height": 2000,
               "url": "/s/dev/api/files/{id}/file/My_Photo.webp?exp=…&sig=…",
               "thumbnail_url": "/s/dev/api/files/{id}/thumb/thumb.webp?exp=…&sig=…" | null }
```
- `Message.attachments` is now `Attachment[]`.
- `url` and `thumbnail_url` are relative to the host origin and **signed**: use them directly in `<img>`/`<video>` with no auth header. They expire after about 2–3 days; fresh ones come with every message fetch and event, so don't store them.
- Files are served with `Cache-Control: private, max-age=86400, immutable` and support range requests (video seeking).
- `width`/`height`/`thumbnail_url` can be null (e.g. videos when the host has no ffmpeg). Use the thumbnail for chat previews and the full `url` in a lightbox or player.

**Accepted**: PNG, JPEG, GIF, WebP, MP4, WebM, **audio** (MP3, OGG Opus/Vorbis, WAV, FLAC, M4A/AAC, and audio-only MP4/WebM), detected by content, and **documents** (`kind: "file"`): `.txt .md .csv .log .json` (must be UTF-8 text with no NUL bytes) and `.pdf` (must start with `%PDF-`), by file name plus that content check. Anything else (programs, scripts, HTML, SVG, archives…) → `415 unsupported_media`.

**Audio** (`kind: "audio"`, `mime` one of `audio/mpeg`, `audio/ogg`, `audio/wav`, `audio/flac`, `audio/mp4`, `audio/webm`): with ffmpeg on the host, only the first sound track is kept (tags such as title/artist, cover pictures and other metadata are removed, no re-encoding), and a file ffmpeg can't read is refused (`400`). Every attachment now has `duration_ms` (milliseconds, or null when unknown; also set for videos) and `voice` (true for recorded voice messages). Play with `<audio controls preload="metadata">`; the file URL supports Range requests (seeking). An MP4/WebM with no video stream is audio.

**Downloading**: add `&download=1` to any attachment `url` to get `Content-Disposition: attachment` (the browser saves it instead of playing or showing it). Use this for a Download button: `<a download>` is ignored for links to another host.

**Removing an unsent upload**: `DELETE /attachments/{id}` → `204` (only the uploader, only while it isn't in a sent message; otherwise `404 not_found`). Call it when the user discards a recording, removes a file from the composer, or leaves the draft: it frees the bytes from their daily and total limits right away (`GET /me/storage` shows the new usage). Uploads never sent are also deleted automatically after 1 hour.

**Voice messages**: upload the recording with `?filename=voice-message.webm&voice=1`, then send it in a message like any attachment. Recorder format: prefer `audio/webm;codecs=opus`, else `audio/ogg;codecs=opus`, else `audio/mp4` (Safari); match the file extension. Limits: at most 5 minutes (`400 voice_too_long`, `error.max_ms`), must be audio (`415`), and the server setting `voice_messages` must be on (`403 voice_messages_off`). Every member sees the setting as `server.voice_messages` in `GET /server`, the WebSocket `ready` snapshot and `server_update`: hide the mic button when it's false. The usual upload rules also apply (channel `media_allowed`, ATTACH_FILES, quotas).

**Documents** are stored exactly as uploaded and are **download-only**: their `url` answers with `Content-Disposition: attachment`, `X-Content-Type-Options: nosniff` and `Content-Security-Policy: default-src 'none'; sandbox`. Text types are served as `text/plain; charset=utf-8` (`mime` is `"text/plain"`), PDFs as `application/pdf`. They have no width, height or thumbnail. Show a file card (icon, `filename`, `size`, a download button), never an inline preview of the content. Call them "allowed file types", not "safe": no file type is guaranteed harmless.

**What the server does to files**
- Still images are re-encoded to **WebP**, which makes them smaller and removes hidden metadata such as GPS. Phone rotation is applied, and images are scaled down to at most 4096 px.
- Animated GIF/WebP are kept as-is.
- Videos have their metadata removed (if ffmpeg is installed on the host).
- Identical files are stored once.

**Limits** (owner settings, see below). Errors are `413` with `error.limit_mb`:
- `too_large`: over `max_file_mb`
- `quota_exceeded`, with `error.quota` = `"daily"` (last 24 h) or `"total"`: the user's upload quota
- `server_storage_full`: the server's storage limit (see 4g; with `storage_full_action: "delete_oldest"` this only happens when one file is bigger than the whole limit)

Other upload errors:
- `403 media_not_allowed`: the channel doesn't allow media
- `403 forbidden`: no ATTACH_FILES
- `429 rate_limited`: more than 10 uploads per minute

The owner is exempt from per-user quotas and the upload rate limit, but not from `max_file_mb` or the server cap.

| Method & path | Response |
|---|---|
| `GET /me/storage` | `{ used_today_bytes, used_total_bytes, max_file_mb, user_daily_mb, user_total_mb, exempt }` (show this near the upload button) |

**New `Settings` fields** (`GET/PATCH /settings`, MANAGE_SERVER):

| Field | Default | Range |
|---|---|---|
| `max_file_mb` | 25 | 1–2048 |
| `user_daily_mb` | 100 | 0–1000000 |
| `user_total_mb` | 1000 | 0–10000000 |
| `server_total_mb` | 10240 | 0–100000000 (0 = no limit) |
| `voice_messages` | true | members may send recorded voice messages (MANAGE_SERVER) |
| `discoverable` | false | shown in other hosts' server browsers (owner only, see 4h) |
| `storage_full_action` | `"delete_oldest"` | `"delete_oldest"` \| `"refuse"`: **owner only** (see 4g) |
| `media_retention_days` | 0 (keep forever) | 0–36500 |
| `storage_used_bytes` | read-only: storage used (stored files + message text, see 4g) | |

With `media_retention_days` set, older media is deleted automatically. The messages stay, and a `message_update` without those attachments is sent. Invalid values give `fields.<name>` = `out_of_range`.

## 4d. Moderation (Phase 4)

### Bans
| Method & path | Body → Response | Who |
|---|---|---|
| `GET /bans` | → `Ban[]` (active only) | BAN_MEMBERS |
| `POST /bans` | `{ user_id, reason?, duration_hours?: 1–87600 (omit = permanent), ban_ip?: bool, ban_device?: bool, delete_messages_hours?: 0–168 }` → `201 Ban` | BAN_MEMBERS |
| `DELETE /bans/{id}` | → `204` (unban) | BAN_MEMBERS |

```json
Ban = { "id": "…", "user": User | null, "reason": "spam", "ip_banned": true, "device_banned": true,
        "ips": ["203.0.113.5"],   // owner only
        "created_by": "…", "created_at": "…", "expires_at": "…" | null }
```

**What a ban does**
- It logs the account out everywhere: its sockets get an error frame `banned` and close `4003`.
- It hides the account from `/members` (a `member_leave` event is sent) and refuses its logins (`403 banned`).
- With `delete_messages_hours`, it removes the account's recent messages (a `message_delete` event is sent for each).

**Against ban evasion (the options)**
- `ban_ip` blocks every address the account used, for the whole server, like an IP block rule (`403 ip_blocked`). The host machine is never IP-banned. UI warning: shared Wi-Fi or mobile networks can block innocent people.
- `ban_device` blocks the browser's `device_id`: new accounts and logins from it get `403 banned`.
- Other options are the owner's anti-alt rules (below), invite-only mode and IP rules.

### Timeouts and kicks
| Method & path | Body → Response | Who |
|---|---|---|
| `PUT /members/{user_id}/timeout` | `{ minutes: 1–40320, reason? }` → `204` | KICK_MEMBERS |
| `DELETE /members/{user_id}/timeout` | → `204` (also clears automatic spam timeouts) | KICK_MEMBERS |
| `POST /members/{user_id}/kick` | `{ reason? }` → `204`: logs them out everywhere and removes their roles (sockets close `4003` with error `kicked`). They can log back in; use a ban to keep someone out. | KICK_MEMBERS |

- `Member.timed_out_until` is a timestamp while timed out (else null); a `member_update` is sent on change.
- Timed-out members get `403 timed_out { until }` on sending, editing, reacting and uploading.
- Automatic spam timeouts are saved the same way.

**Rank rule for all moderation:** nobody can act on themselves (`400 bad_request`) or on the owner, and non-owners can only act on members whose highest role is below their own (both `403 forbidden`).

### Anti-alt and anti-spam settings (`PATCH /settings`, MANAGE_SERVER)
| Field | Default | Range | Meaning |
|---|---|---|---|
| `new_account_restrict_hours` | 0 (off) | 0–720 | New accounts can't post links, images or videos for this many hours → `403 account_too_new { until }`. MANAGE_MESSAGES holders and the owner are exempt. |
| `max_accounts_per_ip` | 0 (no limit) | 0–100 | Registering beyond this many accounts from one address → `403 too_many_accounts`. The host machine is exempt. |
| `spam_messages_per_5s` | 5 | 1–50 | Message burst limit |
| `spam_chars_per_minute` | 12000 | 500–100000 | Text budget per minute |
| `spam_max_repeats` | 2 | 1–20 | Same text allowed per minute |
| `spam_timeout_minutes` | 10 | 0–1440 | Automatic timeout after 5 violations in 10 minutes (0 = never time out, only slow down) |

### Audit log
`GET /audit-log?before={id}&limit=50` (MANAGE_SERVER, BAN_MEMBERS or KICK_MEMBERS) → newest first:
```json
{ "id": "…", "actor_id": "…" | null, "action": "member_ban", "target_id": "…" | null, "details": { … }, "created_at": "…" }
```
- `actor_id: null` means the server did it automatically (e.g. a spam timeout).
- Actions:
  - `member_ban`, `member_unban`, `member_timeout`, `member_timeout_clear`, `member_kick`
  - `message_delete` (someone else's message, with a 200-character preview)
  - `settings_update`, `server_password`
  - `invite_create`, `invite_delete`, `ip_rule_add`, `ip_rule_delete`
  - `role_create`, `role_update`, `role_delete`, `member_role_add`, `member_role_remove`

### Session closing
- `POST /auth/logout` closes that token's WebSockets (`error` frame `unauthorized`, close `4001`).
- `PATCH /me` with a password change closes every other session of that user the same way.

### Host console (run on the host machine)
```text
chitchat-host server create <slug> --name "Name"     # prints the owner setup code (restart the host after)
chitchat-host server list
chitchat-host user reset-password <slug> <username>  # prints a temporary password, logs the account out everywhere
```

## 4e. Voice (Phase 5)

Voice uses WebRTC to an SFU built into the host: each browser has **one** `RTCPeerConnection` to the host, sends its microphone once, and the host forwards it to everyone else in the room. Media goes over **UDP on the same port number as HTTP** (e.g. 8080/udp); signaling goes over the existing WebSocket.

**Browsers allow the microphone only on HTTPS pages or on `localhost`.** Over plain `http://192.168.x.x`, `navigator.mediaDevices` is undefined: show "Voice needs a secure (https://) connection to this host" instead of a join button, and let people still see who is in voice. (HTTPS support on the host is coming next.)

**Permissions**: joining needs VIEW_CHANNEL + **CONNECT_VOICE** (bit 12); being heard needs **SPEAK** (bit 13). Both are in the new default `@everyone` = **12547** (existing servers were upgraded). Members without SPEAK, or timed out, can join and listen but are `suppressed`: the host drops their audio.

### VoiceState
```json
{ "user_id": "…", "channel_id": "…", "session_id": "…",
  "self_mute": false, "self_deaf": false, "suppressed": false, "streaming": false }
```
- One voice connection per user per server. Joining from another tab or device moves them: the old socket gets `voice_end {reason: "moved"}`.
- `session_id` is also the **MediaStream id** of that person's audio on your peer connection (`event.streams[0].id` in `ontrack`): map it to a user through the voice states.

### Client → server
| type | data |
|---|---|
| `voice_join` | `{ channel_id, offer: {type: "offer", sdp}, self_mute?, self_deaf? }`: create the `RTCPeerConnection`, `addTrack(micTrack)`, `createOffer`, `setLocalDescription`, **wait for ICE gathering to complete** (or just send the offer; the host doesn't need your candidates), then send `pc.localDescription`. Joining another channel = `voice_join` again with a new peer connection (close the old one). |
| `voice_sdp` | `{ type: "offer"\|"answer", sdp }`: the answer to an offer the host sent, or your own renegotiation offer (Phase 6). |
| `voice_state` | `{ self_mute?, self_deaf?, streaming?, screen_mids? }` (`streaming`, `screen_mids`: see Screen share below). Also stop sending while muted (`track.enabled = false`); the host drops muted audio anyway. Deafened = the host stops sending you audio; also mute your `<audio>` elements. |
| `voice_speaking` | `{ speaking }`: from your own voice-activity detection (Web Audio analyser). Send only on changes; at most ~10 per second. Ignored while muted or suppressed. |
| `voice_leave` | `{}` |

### Server → client
| type | data |
|---|---|
| `voice_sdp` | `{ type, sdp }`. `answer` → `setRemoteDescription`. `offer` (the host adds or removes someone's audio track) → `setRemoteDescription`, `createAnswer`, `setLocalDescription`, send it back as `voice_sdp`. **Handle these frames one at a time, in order.** |
| `voice_state_update` | `VoiceState` when someone joins, moves or changes mute/deaf/suppressed; `{ user_id, channel_id: null, previous_channel_id }` when they leave. Sent to everyone on the server (show who's in each voice channel). |
| `voice_speaking` | `{ user_id, channel_id, speaking }` |
| `voice_end` | `{ reason, message }`: your voice connection ended or was refused. Close the peer connection and show `message`. Reasons: `voice_full`, `missing_permission`, `unknown_channel`, `bad_offer`, `voice_unavailable`, `moved`, `connection_lost`, `channel_deleted`. |

**Screen share (Phase 6)**: needs **STREAM** (bit 14, not in the default `@everyone`; the owner grants it via a role).
1. `getDisplayMedia({video: {frameRate: 30}, audio: true})`, then `pc.addTransceiver(videoTrack, {direction: "sendonly"})` on your **existing** voice peer connection, plus the same for the shared audio track if there is one. Use `addTransceiver`, not `addTrack`: `addTrack` reuses a receive-only audio transceiver the host created for someone else's audio, and that sound would be lost.
2. Send your own offer: `createOffer` → `setLocalDescription` → `voice_sdp {type: "offer", sdp}`; the host replies with `voice_sdp` answer.
3. Send `voice_state {streaming: true, screen_mids: [videoTransceiver.mid, audioTransceiver.mid]}` (refused with `voice_end`/`missing_permission` without STREAM or while timed out). The host only forwards your video while `streaming` is true.
   - `screen_mids` (max 4, replaces the previous list, `[]` clears it): audio on these transceivers is the **shared sound**, not your mic. It follows the screen share (forwarded only while `streaming`, with STREAM and not suppressed) and **ignores `self_mute`**, so someone without a microphone (listen-only, `self_mute: true`) can still share a window's sound. Audio on any other transceiver is your mic.
   - Receivers get the shared sound as one more audio track with the sharer's `session_id` as stream id, next to their mic track: play every audio track (one element per `track.id`), not just the first one per stream.
4. To stop: `voice_state {streaming: false}` and `sender.replaceTrack(null)`. Keep the transceiver; to share again, `replaceTrack(newTrack)` + `streaming: true` (no renegotiation).
- Viewers receive a video track whose stream id is the sharer's `session_id`; show it when their VoiceState has `streaming: true`.
- Quality presets: set `sender.setParameters({encodings: [{maxBitrate, maxFramerate}]})`, e.g. 720p30 ≈ 2.5 Mbit/s, 1080p30 ≈ 4.5 Mbit/s. The host's upload is about that × viewers. No simulcast (if sent, only the `h` layer is forwarded).
- Losing STREAM or being timed out ends the share (`streaming` becomes false in a `voice_state_update`).

**Audio playback**: for each remote track create an `<audio autoplay>` element with `srcObject = event.streams[0]`. Use `getUserMedia({audio: {echoCancellation: true, noiseSuppression: true, autoGainControl: true}})`. Push-to-talk = toggle `track.enabled` and `voice_state` mute.

**Room size warning** (owner setting `user_limit`): the host uploads every speaker to every listener, roughly **N × (N−1) × 40 kbit/s** when everyone talks (10 people ≈ 3.6 Mbit/s, 20 ≈ 15 Mbit/s). In the channel settings, show that estimate and a warning above **15**: "Large rooms need a fast upload on the host and can stutter."

**Networking**: the host advertises the address the browser used to reach it (the `Host` header) with the HTTP port number, over UDP and over TCP (an ICE-TCP passive candidate on the **same TCP port as the web page**; the host tells those connections apart from HTTPS by their first bytes). So whatever address works for the web page works for voice. Open/forward the **UDP port too** for the best quality; on networks that block UDP the browser falls back to TCP by itself (a bit more delay on lossy links). No client change is needed. For testing, `CHITCHAT_VOICE_TCP_ONLY=1` makes the host offer only the TCP candidate.

## 4f. AI moderation (Phase 7)

**Only the owner** can set it up (`403 owner_only` for everyone else, admins included). Members are told when an AI reads their messages (see AiNotice). New and edited messages in the chosen text channels are queued and checked in the background; **the owner's own messages are never checked**. Checking takes seconds, so messages are sent normally and acted on afterwards.

**Pipeline**
1. **Laya** (optional): a local CPU classifier (`laya-serve`, by default `http://127.0.0.1:8000` on the host). It scores `toxic`, `harassment`, `threat` and `spam` from 0 to 1. A message is flagged when toxic/harassment/threat ≥ `threshold` (default 0.7) or spam ≥ `spam_threshold` (default 0.95; Laya's spam score is noisy). About 1–4 s per check on a laptop CPU, batched.
2. **LLM** (optional): Gemini (Gemma models, with the owner's free API key) or any OpenAI-compatible server (Ollama, LM Studio, llama.cpp, vLLM). With Laya, the LLM only **confirms what Laya flagged**, which makes the free tier last. Without Laya it checks every message. Requests are rate limited by `requests_per_minute`/`requests_per_day` (Gemini free tier for Gemma: 30/min, 14,400/day). If the LLM fails or is over its limit, Laya's verdict stands but the action is downgraded to a plain flag.
3. **Action** (owner's choice) when the final verdict is "flagged": `flag` (default: put it in the review queue), `hide` (delete the message, keep a copy in the queue), `timeout` (flag + time the author out for `timeout_minutes`). `hide` and `timeout` are written to the audit log with `actor_id: null` (`ai_hide`, `ai_timeout`).

### AiNotice (`ready.ai_moderation`, and the `ai_notice_update` event `{ai_moderation}`)
`null` when off. Otherwise:
```json
{ "channel_ids": ["…"], "providers": ["Laya (runs on this host)", "Google Gemini (gemma-4-31b-it)"], "cloud": true }
```
`channel_ids: null` = all text channels. Show members a notice in those channels, e.g. "Messages here are checked by an AI moderator: Laya (runs on this host), Google Gemini". When `cloud` is true add "Messages are sent to an outside AI service." `cloud` is true for Gemini and for OpenAI-compatible servers that aren't on this machine or the LAN.

### Owner endpoints
| Method & path | |
|---|---|
| `GET /ai/settings` → `{config, status, notice}` | owner |
| `PATCH /ai/settings` (body below) → same shape | owner |
| `POST /ai/test {content}` → `{verdict, action, ms, error}`: runs the pipeline on a sample text; nothing is stored or acted on | owner |
| `GET /ai/models` → `{models: [id…]}`: model ids the configured LLM offers (Gemini needs the key and lists every model, so show the `gemma-*` ones first) | owner |

`config` (API keys are never returned, only `has_api_key`):
```json
{ "enabled": false, "channel_ids": [], "action": "flag", "timeout_minutes": 10, "rules": "",
  "laya": null,
  "llm": null }
```
where `laya` is `{ "url": "http://127.0.0.1:8000", "has_api_key": false, "threshold": 0.7, "spam_threshold": 0.95 }` and `llm` is `{ "kind": "gemini"|"openai", "base_url": "", "has_api_key": true, "model": "gemma-4-31b-it", "requests_per_minute": 30, "requests_per_day": 14400, "cloud": true }` when set.

`status`: `{ queued, checked, flagged, llm_requests_today, last_error }` (counters since the host started). Show `last_error` prominently, e.g. "Laya at http://127.0.0.1:8000 failed: …".

PATCH body: every field optional.
- `laya`/`llm`: omit = keep, `null` = turn that provider off, object = set or merge. Inside them `api_key`: omit = keep the saved key, `""` = remove it, string = replace it.
- `channel_ids`: text channel ids (`[]` = all text channels). `action`: `flag`|`hide`|`timeout`. `timeout_minutes` 1–40320. `rules` ≤ 2000 chars (extra rules in plain words, given to the LLM). `threshold`/`spam_threshold` 0.05–1.
- `llm.kind`: `gemini` (`base_url` ignored) or `openai` (`base_url` required, e.g. `http://127.0.0.1:11434/v1` for Ollama). `llm.model`: free text; offer `GET /ai/models` as suggestions. Gemma ids on Gemini look like `gemma-3-27b-it`; check the list for the exact Gemma 4 ids.

`verdict` (test endpoint): `{ flagged, categories: ["toxic", …], scores: {toxic, harassment, threat, spam} | null, provider: "laya"|"llm"|"laya+llm", reason, unconfirmed }`.

**UI suggestions**: an "AI moderation" tab in owner settings with the enable toggle, channel picker, action choice (warn that `hide`/`timeout` act without a human), a Laya section (URL + thresholds), an LLM section (provider, key, model with suggestions, limits), extra rules, a "Try it" box using `/ai/test`, and the status line. Explain that Laya must be installed on the host (docs/hosting.md).

### Review queue (MANAGE_MESSAGES)
| Method & path | |
|---|---|
| `GET /ai/flags?status=open\|dismissed\|deleted\|resolved\|all&before&limit` → `[AiFlag]` (newest first, limit ≤ 100, default `open`) | MANAGE_MESSAGES |
| `POST /ai/flags/{id}/resolve {action: "dismiss"\|"delete_message"\|"resolved"}` → `AiFlag` | MANAGE_MESSAGES |

```json
{ "id": "…", "message_id": "…", "channel_id": "…", "author": "Author or null", "author_id": "…",
  "content": "the text as checked", "categories": ["toxic"], "scores": {}, "provider": "laya",
  "reason": "Laya: toxic", "action": "flagged", "status": "open",
  "created_at": "…", "resolved_by": null, "resolved_at": null }
```
`action`: `flagged`|`hidden`|`timed_out`. `status`: `open`|`dismissed`|`deleted`|`resolved`. `message_id` is null when the message is gone (hidden by the AI). `delete_message` deletes it if it still exists (`message_delete` event + audit entry). For stronger steps use the existing member actions (timeout/kick/ban) from a flag, then `resolve` with `resolved`.

Events (only to members with MANAGE_MESSAGES): `ai_flag_create` (AiFlag) and `ai_flag_update` (AiFlag). Show a badge with the open count.

## 4g. Resources and storage dashboard

### Storage (per server, owner): `GET /storage`
```json
{ "used_bytes": 800064, "limit_bytes": 10737418240, "files_bytes": 800054, "messages_bytes": 10,
  "ai_bytes": 0, "full_action": "delete_oldest" }
```
- `used_bytes` = `files_bytes` (stored uploads, identical files counted once) + `messages_bytes` (message text). This is what `server_total_mb` limits. `limit_bytes` is null when `server_total_mb` is 0 (no limit).
- `ai_bytes`: the AI review queue's text. Shown separately and **not** counted in `used_bytes`.
- Change the limit with `PATCH /settings {server_total_mb}` and the action with `PATCH /settings {storage_full_action}` (owner only, `403` otherwise).
- When a new message or upload would go over the limit:
  - `delete_oldest` (default): the host deletes the **oldest messages with their attachments** until usage is at most 90% of the limit, then stores the new one. Clients get `message_delete` for each deleted message, and the audit log gets one `storage_prune {messages, bytes}` entry (`actor_id: null`). An upload bigger than the whole limit is still refused.
  - `refuse`: `413 server_storage_full` (with `error.limit_mb`) for uploads **and** for new messages.
- Ask the owner to confirm before choosing `delete_oldest` with a low limit: deleted messages can't be brought back.
- Members without owner rights get `403`.

### Host resources (host admin only): `GET /host/resources`, `PATCH /host/resources`
CPU and RAM limits are **host-wide** (one process serves every server on the host), so only the **host admin** sees them: the owner of the server created first on this host. Everyone else gets `403 host_admin_only`; hide the section for them.

```json
{ "cpu": { "cores_total": 8,
           "server": { "limit_cores": null, "usage_cores": 0.12, "enforced": true },
           "ai": { "unit": "laya.service", "running": true, "limit_cores": null, "usage_cores": 0.0, "enforced": true } },
  "ram": { "total_bytes": 5580492800, "available_bytes": 2965340160,
           "server": { "used_bytes": 36974592, "limit_bytes": null, "enforced": true },
           "ai": { "used_bytes": 1400000000, "limit_bytes": null, "enforced": true } },
  "disk": { "total_bytes": 491000000000, "free_bytes": 430000000000 },
  "ai_disk_bytes": 1524522032 }
```
- `usage_cores` = how many cores' worth of CPU it used over the last ~2 s (0.5 = half a core). It's measured every 2 s, so poll every 2–5 s while the dashboard is open. Any measured value can be `null` (not measurable here, or the AI isn't running).
- `ai` = the AI classifier's service (Laya, the systemd user unit `laya.service`). `unit` is null when that service doesn't exist on this host. Cloud or Ollama LLMs are not covered.
- `ai_disk_bytes`: size of the AI's model files (`~/.cache/huggingface`), measured once a minute, **information only (no limit)**. `null` when not found.
- `disk`: the disk holding the host's data.
- `enforced: false` means the limit is saved and shown but **not enforced on this system** (show that next to the control). It's enforced when the host runs as a systemd **user** service on Linux, as in docs/hosting.md: cores become `CPUQuota`, RAM becomes `MemoryHigh` (above it the kernel slows the service and reclaims memory) plus `MemoryMax` = 125% of it (above that the service is killed and systemd restarts it). The limits persist across restarts.

`PATCH /host/resources` body, every field optional; `null` = unlimited:
```json
{ "server_cores": 2, "server_ram_mb": 1024, "ai_cores": 1.5, "ai_ram_mb": 2048 }
```
- Cores: 0.5 to `cores_total`, in steps of 0.5. RAM: at least 256 MB and at most the machine's RAM. Otherwise `400` with `fields.<name> = "out_of_range"`. Returns the same shape as GET. Audit: `host_limits_update`.
- Warn in the UI: a server RAM limit that's too low makes the server restart (dropping everyone for a few seconds); Laya needs about 1.5 GB of RAM.

**Gauge colours** (suggested): green below 70% of the limit, yellow 70–90%, red above 90%. With no limit, compare against the machine total (`cores_total`, `ram.total_bytes`, `disk.total_bytes`).

### Servers on this host (host admin only): `GET /host/servers`, `POST /host/servers`
Lets the host admin make more servers from the web (no CLI). Same `403 host_admin_only` rule as resources.
- `GET /host/servers` → every server on the host, listed or not: `ServerSummary` plus `{ listed, has_owner, member_count, created_at }`, oldest first.
- `POST /host/servers` `{ slug, name?, device_id? }` → `201 { server: ServerSummary, token, user: User }`.
  - `slug`: 2–32 characters `a-z 0-9 -` (`400 fields.slug = "invalid"`); `name` up to 100 characters (defaults to the slug). `409 slug_taken` if it exists; `403 too_many_servers` above 50.
  - The new server starts with `#general` and a `General` voice channel, and is **owned by the caller**: their account is copied onto it with the **same username, display name and password**. The returned `token` is already logged in there, so the client can store it for the new slug and open `/s/{slug}` directly.
  - Audit on the admin's server: `server_create {slug, name}`.

### Backups (host admin only): `GET /host/backups`, `POST /host/backups`
Shows the snapshots made by `scripts/chitchat-backup.py` (docs/hosting.md, "Automatic backups"). The backup folder is `$CHITCHAT_BACKUP_DIR`, or `~/chitchat-backups` when it exists. Same `403 host_admin_only` rule as resources.
```json
{ "configured": true, "running": false, "scheduled": true, "next_run": 1761192000000, "last_failed": false,
  "count": 7, "total_bytes": 52428800,
  "snapshots": [ { "name": "2026-10-22_195012", "created": 1761155412000, "ok": true, "seconds": 1.4,
                   "servers": 2, "messages": 1520, "files": 84 } ] }
```
- `configured: false`: no backup folder; everything else is empty.
- `scheduled` / `next_run` (unix ms): the `chitchat-backup.timer` systemd user timer is active and when it runs next.
- `last_failed`: the most recent run failed (the previous good snapshots are kept).
- `total_bytes`: the space all snapshots really use; files shared between snapshots (hard links) count once. `snapshots` is newest first; `created` is when the snapshot was finished.
- `POST /host/backups` starts one now → `202` with the same shape (`running: true`). It starts `chitchat-backup.service` when that unit backs up this host's data into this folder (same settings as the timer), otherwise runs `python3 scripts/chitchat-backup.py backup --keep 7` from the host's working directory. `409 backup_running` if one is already running, `404 backups_not_configured` without a backup folder. Poll GET while `running` is true. Audit: `backup_run`.

## 4h. Server discovery (no central server)
Hosts that turn discovery on publish their **discoverable** servers and the other hosts they know; every 30 minutes each one fetches the same from the hosts it knows (gossip), so the list spreads from host to host. No server exists only for discovery. A host joins the network by knowing one other host (a seed URL, or a seed list file such as a text file on GitHub), or when another host announces itself to it.

**Per server (owner only):** `PATCH /settings { discoverable: true }` (default `false`; DB v8). Only discoverable servers are shown to other hosts. `listed` is separate: it only controls `GET /api/host` on this host.

**Browse (public, for the client):** `GET /api/discovery/browse` → `404 discovery_off`, or:
```json
{ "servers": [ { "url": "https://chat.example.org/s/gaming", "host_url": "https://chat.example.org", "host_name": "ChitChat Host",
                 "local": false, "last_seen": 1761150000000, "slug": "gaming", "name": "Gaming", "description": "",
                 "requires_password": false, "invite_only": false, "registration_open": true, "member_count": 12 } ],
  "hosts": 3, "updated_at": 1761150000000 }
```
- Sorted by `member_count` (largest first). `local: true` = a server on this host. `last_seen` = when its host last answered (ms).
- Everything from other hosts is **untrusted text** written by strangers: render as plain text, never HTML. Opening `url` leaves this host (it's another site, with its own accounts): say so before navigating, and open it in the same tab or a new one, not in an iframe.
- The browser only talks to its own host; it never contacts other hosts while browsing.

**Host admin settings:** `GET /host/discovery`, `PATCH /host/discovery` (`403 host_admin_only` for others):
```json
{ "config": { "enabled": true, "public_url": "https://chat.example.org", "seeds": ["https://other.host:8090"],
              "seed_lists": ["https://raw.githubusercontent.com/someone/list/main/hosts.txt"], "blocked": [] },
  "hosts": [ { "url": "https://other.host:8090", "name": "Their Host", "servers": 2, "last_ok": 1761150000000, "fails": 0 } ],
  "last_round": 1761150000000 }
```
- PATCH body: any of `enabled`, `public_url` (null clears it), `seeds`, `seed_lists`, `blocked`. Host URLs are `https://name[:port]` with no path (`400 fields.<name> = "invalid"`); seed lists are https file URLs (one host per line, `#` comments, or a JSON array). `enabled: true` needs `public_url` (`fields.public_url = "required"`). Saving starts a round right away; results appear within seconds.
- `blocked`: hosts never fetched, shown or passed on (for spam).
- Audit: `discovery_update`.

**Between hosts (not for the client):** `GET /api/discovery` (this host's document: `{ version, name, url, servers, peers }`) and `POST /api/discovery/announce { url }` (`202`; the host checks that `url` really answers as itself before listing it; one announce per IP per minute).
- Safety: only `https://` hosts on public IP addresses are fetched (never LAN addresses), no redirects, answers over 512 KB are dropped, at most 50 servers per host and 500 hosts; text is trimmed. Hosts that don't answer for 7 days are forgotten.

## 5. WebSocket: `GET /s/{slug}/api/ws`

All frames are JSON text: `{ "type": "…", "data": { … } }`.

**Handshake**
1. The client connects, then sends within 10 s: `{"type":"identify","data":{"token":"…"}}`.
   (The token is not put in the URL, so it doesn't end up in logs.)
2. The server replies with `ready`:
   ```json
   {"type":"ready","data":{
     "user": User, "is_owner": false, "permissions": "…",
     "server": Server, "categories": [...], "channels": [...], "members": [...], "roles": [...],
     "voice_states": [VoiceState, ...], "ai_moderation": AiNotice|null }}
   ```
   This is the full snapshot, so the client doesn't need the REST list calls after connecting.
3. If it fails, the server sends `{"type":"error","data":{"code":"unauthorized","message":"…"}}` and closes with code **4001**.

**Keepalive**: the server sends WebSocket ping frames every 30 s; browsers answer automatically. If the connection drops, reconnect with backoff (1 s, 2 s, 5 s, 10 s max), identify again, and refetch the open channel with `after={last_id}`.

**Close codes**: `4001` unauthorized, `4003` banned/kicked (show the reason from the preceding `error` frame; don't auto-reconnect), `4008` rate limited, `4000` generic.

**Client → server**
| type | data |
|---|---|
| `identify` | `{ token }` |
| `typing` | `{ channel_id }` (send at most once every 5 s while typing) |
| `voice_join` · `voice_sdp` · `voice_state` · `voice_speaking` · `voice_leave` | see section 4e |

**Server → client events** (sent only to members who can see the channel concerned)
| type | data |
|---|---|
| `message_create` | `Message` (plus `nonce` if the sender gave one) |
| `message_update` | `Message` |
| `message_delete` | `{ id, channel_id }` |
| `reaction_add` / `reaction_remove` | `{ message_id, channel_id, emoji, user_id }` |
| `typing_start` | `{ channel_id, user_id }` (show for ~8 s) |
| `channel_create` / `channel_update` | `Channel` |
| `channel_delete` | `{ id }` |
| `category_create` / `category_update` | `Category` |
| `category_delete` | `{ id }` |
| `member_join` / `member_update` | `Member` |
| `member_leave` | `{ user_id }` |
| `presence_update` | `{ user_id, online }` |
| `role_create` / `role_update` | `Role` |
| `role_delete` | `{ id }` |
| `server_update` | `Server` |
| `voice_state_update` · `voice_speaking` · `voice_sdp` · `voice_end` | see section 4e |
| `ai_notice_update` · `ai_flag_create` · `ai_flag_update` | see section 4f |
| `permissions_update` | `{ permissions, is_owner }`: **your own** server-wide permissions changed (a role edit, or a role given to or taken from you). Use it as every channel's `my_permissions` too (Phase 2 has no per-channel overrides yet). |

In events, `Message.reactions[].me` is always `false` (one payload goes to everyone); keep your own reaction state from REST responses and `reaction_add`/`reaction_remove` events with your `user_id`.

Events you send yourself also come back to you (e.g. your own `message_create`); dedupe by `id` or `nonce`.

## 6. Dev setup

- Host dev URL: `http://127.0.0.1:8080`. A demo server with slug `dev` is created on first run, and its owner setup code is printed in the console.
- Web dev: run Vite on its own port and point the client's base URL at `http://127.0.0.1:8080` (CORS is open), or use Vite's proxy for `/api` and `/s`.
- The host serves `web/dist` if it exists (the path is configurable via `--web-dir`).
- HTTPS (needed for the microphone from other machines): `chitchat-host run --https` makes a self-signed certificate in `<data-dir>/tls/` covering `localhost`, the machine's hostname (`name`, `name.local`) and all its IP addresses (add more with `--tls-name chat.example.com`, repeatable). Browsers warn once; after "Advanced → continue" the page is a secure context. With a real certificate (Let's Encrypt, `tailscale cert`): `--tls-cert fullchain.pem --tls-key privkey.pem`; that certificate is served for its domain, while links by IP address or LAN name (`https://192.168.1.10:8090`, `name.local`) still get the self-signed one, so they keep working after the warning. With TLS on, the port serves only `https://`/`wss://`.
- Voice listens on UDP with the same port number as `--bind` (e.g. 8080/udp). Open or forward it along with the TCP port.
