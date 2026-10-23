# ChitChat web client

A Svelte 5 and TypeScript client for the self-hosted ChitChat host. Vite produces static files that the host serves directly; no separate frontend hosting service is required.

## Development

Use Node.js 22.12 or newer (Node.js 24 recommended). Run these commands from the repository root:

```sh
npm ci --prefix web
npm --prefix web run dev
```

Start a local ChitChat host on `http://127.0.0.1:8080`. The development UI normally runs at `http://127.0.0.1:5173`; Vite proxies host API requests and WebSockets to port 8080. You can also select another host from the client.

Each community has its own accounts. The client saves only communities you choose or join, and its server list can include different hosts. HTTPS clients need HTTPS target hosts. The saved list and remembered sign-ins are local to the browser or device.

## Validation and production build

```sh
npm --prefix web run check
npm --prefix web test
npm --prefix web run build
```

The production output is `web/dist`. Serve it at the host's root URL with the host's `--web-dir web/dist` option. API routes return JSON; client routes such as `/s/community` use the SPA fallback.

Microphone recording and screen sharing require HTTPS or localhost. Audio capture for a shared tab, window or screen depends on the browser and operating system. Voice networking and host setup are covered in [the hosting guide](../docs/hosting.md).

## Source layout

- `src/App.svelte`: chat shell, sessions and server navigation.
- `src/lib/`: API types, state, media and voice logic, plus unit tests.
- `src/`: reusable chat, media, moderation and administration components.

The client talks to the host API described in [the API contract](../docs/api.md).

## Search, mentions and notifications

Use Search messages in the sidebar or header to search the current community, including your own direct messages. Filter by author, conversation or dates, and jump to the original message. Type `@` for member/role suggestions or `#` for a channel link; mass and role pings require the owner-controlled mention permission.

Direct messages stay inside one community and can be opened from a member profile or the Direct messages section. They are not end-to-end encrypted: the host operator can read stored messages. Members can turn off incoming DMs in Notifications.

Notifications includes account-wide and per-channel preferences, browser opt-in, and device sound/volume controls. Unread badges are shared across signed-in devices; other saved communities refresh about once a minute. Browser notifications use a notification-only service worker where supported. Keep the page running to receive alerts; closed or suspended pages do not receive Web Push in this build.

## Profiles, threads and server emoji

Members can set a profile picture, online/away preference and custom status per server. Pictures are re-encoded and stored with the server backup. Pins open a searchable conversation location; threads are child text channels with their own history, unread state and notifications. Role posting rules can allow or deny writing, uploading files and adding reactions at the parent or thread level. Owners and administrators bypass posting restrictions.

Server managers can add custom still emoji for messages and reactions. The picker includes standard emoji, search and recently used choices. Custom image references resolve only against the connected host. The host enforces posting rules and upload limits; hiding a client button is never the authorization check.
