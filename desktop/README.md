# ChitChat desktop app

The ChitChat web client (`web/`) in a [Tauri](https://tauri.app) window that keeps running in the
system tray, so you get real system notifications with the window closed.

- It is not a host: like the website, it connects to the hosts you choose (first run asks for one).
- Closing the window hides it to the tray. Tray menu: Open, Start when the computer starts, Quit.
- Links to other sites open in your normal browser; no website ever loads inside the app.
- Notifications use the system's notification centre; the ChitChat sound plays as in the browser.

## Build

Needs Rust, Node.js and the Tauri prerequisites for your system
(<https://tauri.app/start/prerequisites/>; on Windows, WebView2 is already part of Windows 10/11).

```bash
cd desktop
npm install
npx tauri build            # builds web/ first, then the installer(s) in src-tauri/target/release/bundle/
npx tauri build --bundles nsis   # just the Windows installer
```
