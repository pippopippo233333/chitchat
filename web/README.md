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
