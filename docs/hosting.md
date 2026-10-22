# Hosting your own ChitChat

This guide covers the current host on Windows 11 and Ubuntu, including uploads, voice and screen sharing. The host supports HTTPS with generated or supplied certificates; it does not obtain or renew Let's Encrypt certificates automatically.

Your host is the computer running ChitChat. A server is a community inside that host. One host can run several servers, each with its own accounts and database. Keep the host awake while people use it.

## Choose how people will connect

| Method | Who can connect? | Router changes? | Browser HTTPS? |
|---|---|---|---|
| LAN only | Devices on your home/local network | No | Yes, with `--https` or supplied certificates |
| Tailscale + Serve | Invited devices on your private Tailscale network | No port forwarding | Yes, through Serve |
| Domain + HTTPS reverse proxy | People on the internet | Usually yes | Yes, once the proxy is configured |
| Cloudflare Tunnel | People using your public tunnel URL | No inbound port forwarding | Yes, through the tunnel |
| playit.gg TCP tunnel | People using its assigned address and port | No inbound port forwarding | A plain TCP tunnel does not add HTTPS |

HTTPS access to the page and UDP access for voice are separate requirements. HTTP reverse proxies and TCP tunnels do not automatically carry voice. A public URL makes the service reachable by strangers too; ChitChat accounts and permissions still control what they can do.

## 1. Start locally first

Use a Windows build on Windows and a Linux build on Ubuntu. If building this repository, install Rust and the platform's build prerequisites, run `cargo build --release -p chitchat-host` from the repository root, and build the client with `npm ci` followed by `npm run build` in `web/`.

The examples assume the executable and `web/dist` are in your current working directory. Repository builds put the executable in `target/release`; use that path or copy the appropriate executable into your chosen host folder. Relative paths such as `data` are relative to the folder where you run the command.

On Windows PowerShell:

```powershell
.\chitchat-host.exe --data-dir .\data server create friends --name "Friends"
.\chitchat-host.exe --data-dir .\data run --bind 127.0.0.1:8080 --web-dir .\web\dist
```

On Ubuntu:

```bash
chmod +x ./chitchat-host
./chitchat-host --data-dir ./data server create friends --name "Friends"
./chitchat-host --data-dir ./data run --bind 127.0.0.1:8080 --web-dir ./web/dist
```

Open `http://127.0.0.1:8080`. Pick **Friends**, create an account, expand **Setting up your own server?**, and enter the owner setup code printed in the terminal. Keep that code private; it claims the first owner account. A wrong or already-used code is rejected. This is separate from a normal account password.

Without any existing servers, starting the host creates a `dev` server automatically. `chitchat-host server list` lists communities. To add another, stop the running host, run `server create another-name --name "Another Community"`, then start it again. Accounts do not carry between communities.

The current default bind address, if omitted, is `0.0.0.0:8080`, which listens on all IPv4 interfaces. The examples deliberately specify loopback first. `127.0.0.1` means **this computer only**. `0.0.0.0` is a listening setting, not an address to give friends.

Check that text chat works locally before adding a tunnel, firewall rule, or router rule. Stop with Ctrl+C. Do not run the host as administrator/root just to use port 8080.

## 2. LAN only: connect from home Wi-Fi

Restart with `run --bind 0.0.0.0:8080` so other devices can reach it. Find the host's LAN IPv4 address: `ipconfig` on Windows or `ip -4 addr` on Ubuntu. It often looks like `192.168.1.50`; use the address of your Wi-Fi/Ethernet interface, not a VPN or loopback adapter.

On a second device on the same network, open `http://192.168.1.50:8080` (replace the example). Reserve that address in your router's DHCP settings if you want it to stay the same. Guest Wi-Fi or client isolation may prevent devices from reaching each other.

For microphone and screen sharing from another device, enable HTTPS as described in section 8 and use `https://192.168.1.50:8080`. With TLS enabled, plain HTTP on that port no longer answers. Voice additionally needs **UDP on the same port number as TCP**; examples below use 8080, so change both protocols if you choose 8090 or another port.

### Windows Firewall

On your trusted home network, use the **Private** Windows network profile. Open PowerShell **as administrator** to add this narrowly scoped LAN rule:

```powershell
New-NetFirewallRule -DisplayName "ChitChat LAN TCP 8080" -Direction Inbound -Action Allow -Protocol TCP -LocalPort 8080 -Profile Private -RemoteAddress LocalSubnet
New-NetFirewallRule -DisplayName "ChitChat LAN UDP 8080" -Direction Inbound -Action Allow -Protocol UDP -LocalPort 8080 -Profile Private -RemoteAddress LocalSubnet
```

These rules allow only the local subnet, not arbitrary internet clients. If you choose direct public hosting later, add separate TCP and UDP rules for the intended remote addresses; `-RemoteAddress Any` allows any source on the selected profile. Keep the firewall enabled. [Microsoft firewall command reference](https://learn.microsoft.com/en-us/powershell/module/netsecurity/new-netfirewallrule).

Remove this particular rule when no longer needed:

```powershell
Remove-NetFirewallRule -DisplayName "ChitChat LAN TCP 8080"
Remove-NetFirewallRule -DisplayName "ChitChat LAN UDP 8080"
```

### Ubuntu firewall (ufw)

Check `sudo ufw status`. If using SSH, allow your actual SSH port before enabling ufw so you do not lock yourself out. For the example LAN `192.168.1.0/24`:

```bash
sudo ufw allow OpenSSH
sudo ufw allow from 192.168.1.0/24 to any port 8080 proto tcp
sudo ufw allow from 192.168.1.0/24 to any port 8080 proto udp
sudo ufw enable
sudo ufw status
```

Replace the subnet with yours. If SSH uses a nonstandard port, allow that port instead of relying on the OpenSSH profile. Direct public TCP 8080 hosting uses `sudo ufw allow 8080/tcp`; a loopback-only tunnel generally needs no inbound 8080 rule. [Ubuntu firewall guide](https://documentation.ubuntu.com/server/how-to/security/firewalls/index.html).

For a host using port 8090, the equivalent unrestricted inbound rules are `sudo ufw allow 8090/tcp` and `sudo ufw allow 8090/udp`. Use the subnet-restricted forms above for LAN-only access. The host also offers an ICE-TCP fallback on the same TCP port as HTTPS. Browsers can use it when UDP is blocked; UDP remains preferable for latency. An HTTP proxy must support the separate media path; serving the page alone does not guarantee voice connectivity.

## 3. Private remote access with Tailscale

Install Tailscale on the host and each friend's device using the [official downloads](https://tailscale.com/download). Invite/share access using Tailscale's controls. Keep ChitChat on `127.0.0.1:8080` when using Serve on the same machine.

Enable the required MagicDNS/HTTPS settings if prompted, then run:

```text
tailscale serve --bg http://127.0.0.1:8080
tailscale serve status
```

Share the HTTPS address printed by Serve with permitted tailnet users. Tailscale access controls still apply. Friends do not use your home public IP, and you do not forward a router port. Use `tailscale serve reset` to remove the Serve configuration. **Funnel is different**: it makes the service public, so do not substitute it when you intend private access. [Tailscale Serve reference](https://tailscale.com/docs/reference/tailscale-cli/serve), [Funnel reference](https://tailscale.com/docs/reference/tailscale-cli/funnel).

This Serve configuration forwards the web page and WebSocket, not the SFU's UDP socket. For voice, the browser's hostname and port must also reach ChitChat over UDP at that same port. A direct tailnet connection with a certificate obtained separately using `tailscale cert` and supplied to the host (section 8) avoids relying on an HTTP proxy for that media path; allow it in your tailnet and host firewall policies.

## 4. Public hosting with port forwarding and a domain

Only choose this path if your connection has an inbound-reachable public address. In the router, reserve the host's LAN address. For a basic reachability test, forward **TCP external 8080 → host LAN address, TCP 8080**; the host must listen on `0.0.0.0:8080`, and its firewall must permit the incoming source.

For voice, also forward **UDP external 8080 → the same host, UDP 8080**. With direct hosting on 8090, forward both TCP 8090 and UDP 8090. Keep the external port equal to the host port: the SFU advertises the hostname and port used by the browser.

Use a phone on mobile data, not your own Wi-Fi, to test `http://YOUR_PUBLIC_IP:8080`. Some routers cannot test their public address from inside the same LAN. An HTTP check establishes connectivity; it does not provide encryption. Do not use real passwords over a public HTTP connection.

For actual use, point a domain at the public address and put an HTTPS reverse proxy in front of ChitChat. Configure WebSocket forwarding and proxy the full site, including `/api` and `/s/<slug>/api/ws`, to the host. With the proxy on the same machine, bind ChitChat to loopback and expose only the proxy's required ports (usually TCP 443, with TCP 80 used by some certificate methods). Remove an unneeded public 8080 forwarding rule afterward.

That loopback-only proxy setup covers web traffic. Voice additionally requires a UDP route on the public page's port to the SFU. A TCP reverse proxy on 443 does not provide that route, and a host bound only to loopback cannot receive external media. Direct HTTPS on the host with matching TCP/UDP ports is the straightforward configuration; verify any proxy deployment's UDP routing separately.

Automatic built-in Let's Encrypt is planned; **the current host does not obtain certificates itself**. Follow your chosen proxy's official certificate setup. If your public IP changes, use a supported dynamic DNS updater or change the domain record.

## 5. No port forwarding: Cloudflare Tunnel

A Cloudflare Tunnel runs an outbound connector on the host and exposes an HTTPS address. Keep ChitChat on loopback and point the HTTP service at `http://127.0.0.1:8080`.

For a temporary development check, install `cloudflared` from Cloudflare's official instructions, then:

```text
cloudflared tunnel --url http://127.0.0.1:8080
```

Open the printed `https://…trycloudflare.com` URL. It is a temporary public address; do not treat it as a stable community endpoint. For continued use, configure a named tunnel and a hostname on your Cloudflare-managed domain, route the whole hostname to ChitChat, and run the connector as a service. Keep the connector token private. [Quick Tunnel instructions](https://developers.cloudflare.com/tunnel/get-started/quick-tunnels/), [named tunnel setup](https://developers.cloudflare.com/tunnel/get-started/).

HTTP/WebSocket chat fits this tunnel model. The voice UDP transport needs a separate reachable route on the browser hostname and port; this HTTP tunnel does not provide it. ICE-TCP fallback requires a direct compatible TCP route to the host; this HTTP tunnel does not automatically carry it. A proxy/tunnel operator can process traffic at its TLS termination point; this project does not provide end-to-end encryption.

## 6. playit.gg

Install the official playit agent, link it to your account, and create a **custom TCP** tunnel targeting local `127.0.0.1:8080`. Share its assigned hostname **and port**. Both the playit agent and ChitChat must remain running. Custom TCP availability and price depend on the current plan; check the dashboard before committing to it. [Official playit support](https://playit.gg/support/).

A game-specific tunnel preset is not a general HTTP endpoint. A raw TCP tunnel carries the host's HTTP bytes; it does not automatically add a trusted HTTPS certificate. Browser voice requires HTTPS plus a separate UDP route on the same public hostname and port. Confirm that routing explicitly; a generic HTTP tunnel cannot carry voice automatically. The ICE-TCP fallback needs a raw TCP route to the host on the same public address and port.

## 7. Is CGNAT stopping port forwarding?

CGNAT means your internet provider shares an external IPv4 address between customers. Your router cannot open a port on the provider's router.

Compare the **WAN/Internet IPv4** in your home router with the public IPv4 reported by a trusted external IP-checking service. A mismatch suggests another NAT layer. WAN addresses in `100.64.0.0`–`100.127.255.255` are shared address space used for CGNAT. Private WAN addresses (`10.x.x.x`, `172.16.x.x`–`172.31.x.x`, `192.168.x.x`) can also mean upstream NAT or a second home router; they are clues, not proof of the provider's exact setup. [RFC 6598 shared address space](https://www.rfc-editor.org/info/rfc6598/).

Ask your ISP whether it offers inbound public IPv4. Alternatively, use Tailscale or an outbound tunnel. Even a public address can have provider-blocked ports. Do not use a Tailscale adapter's `100.x` address to diagnose your router's WAN connection.

## 8. Why HTTPS matters

HTTPS encrypts browser-to-endpoint traffic, including passwords and chat tokens. Browsers require a secure context for microphone and screen-capture APIs. Localhost is a development exception; `http://192.168.1.50:8080` on another device is not localhost, and a VPN alone does not change that browser rule. [Browser secure contexts](https://developer.mozilla.org/en-US/docs/Web/Security/Defenses/Secure_Contexts), [microphone API](https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia).

For the built-in self-signed certificate, stop the host and restart it with:

```bash
./chitchat-host --data-dir ./data run --bind 0.0.0.0:8090 --web-dir ./web/dist --https
```

On Windows use `.\chitchat-host.exe` with the same options. Open `https://HOST_LAN_IP:8090`. On the first visit, the browser warns because the certificate is self-signed. For your own known host, use **Advanced → continue** (wording varies by browser); browser policy may prevent this on managed devices. This is a manual user action, not something the client should bypass.

The generated certificate and private key live in `<data-dir>/tls/`. It covers localhost, the machine hostname (including its `.local` form), and local IP addresses. Add extra certificate names with repeatable `--tls-name chat.example.com` options. Protect the private key and include the TLS directory in private backups.

For a certificate obtained separately from Let's Encrypt or `tailscale cert`, supply both PEM paths:

```bash
./chitchat-host --data-dir ./data run --bind 0.0.0.0:8090 --web-dir ./web/dist --tls-cert ./fullchain.pem --tls-key ./privkey.pem
```

Use the hostname covered by that certificate. Certificate issuance and renewal are managed outside ChitChat. With either TLS mode, the port serves **HTTPS/WSS only**; old `http://` bookmarks must change to `https://`. HTTPS secures signaling and enables browser capture; it does not replace the UDP firewall/forwarding requirement for voice.

Keep the page and API on the same HTTPS host where possible. An HTTPS page calling an HTTP host can be blocked as mixed content; the same applies to `ws://` versus `wss://`. Keep the site path at `/`, not a subfolder, with the current client.

## 9. Upload bandwidth: leave headroom

The host sends a separate media stream to each recipient. These are planning estimates, not guaranteed bandwidth usage. The client estimates 40 kbit/s per forwarded voice stream, with a fully active room costing roughly `N × (N − 1) × 40 kbit/s`; 20 people is about 15.2 Mbit/s. Screen-share presets cap the sender near 2.5 Mbit/s for 720p30 or 4.5 Mbit/s for 1080p30, then the host forwards that stream to each viewer. Protocol overhead, codec settings and motion change the real cost. The conservative table below allows about 50 kbit/s for each voice stream.

| Activity sent by the host | Approximate host upload |
|---|---:|
| One active voice stream to 1 listener | 0.05 Mbps |
| One active voice stream to 5 listeners | 0.25 Mbps |
| One active voice stream to 10 listeners | 0.50 Mbps |
| One 1080p screen share to 1 viewer | 3–5 Mbps |
| One 1080p screen share to 5 viewers | 15–25 Mbps |

Multiple simultaneous speakers multiply the voice estimate. Measure **upload**, not the larger advertised download speed, and leave room for other household traffic. Tunnels do not remove the host's upload bottleneck. Text chat needs much less bandwidth than media.

## Troubleshooting and backups

- Local page does not open: check that the process is running, port 8080 is free, and the bind address is correct.
- Local works, LAN fails: check bind address, the active firewall profile/rule, the LAN IP, and guest-network isolation.
- LAN works, internet fails: check forwarding, WAN address/CGNAT, ISP filtering, or connector status.
- Page opens but chat does not connect: check WebSocket proxying and that both page/API use matching HTTPS/WSS origins.
- HTTP stops answering after enabling TLS: open the same address using `https://` instead.
- Voice is unavailable on LAN HTTP: enable HTTPS and open its HTTPS URL.
- Voice joins but never connects: allow/forward UDP on the same port number as the page, check the advertised hostname reaches this host, and check for a UDP-blocking network. ICE-TCP fallback requires a direct compatible TCP route to the host; this HTTP tunnel does not automatically carry it.
- Owner code fails: confirm the correct community and that it has no owner already. Never post the setup code into a public channel.

Store and back up the whole chosen data directory; it contains host settings, certificates and `servers/<slug>/` databases and files. Do not copy only the main `.db` file while the host runs: recent changes may still be in the `-wal` file. Keep backups and setup material private: they hold password hashes, messages and private keys.

### Automatic backups (Linux, systemd user service)

`scripts/chitchat-backup.py` (Python 3, standard library only) makes a consistent snapshot **while the host keeps running**: SQLite's online backup for each database, plus every stored file, setting and certificate. Unchanged files are hard links to the previous snapshot, so each day only costs the new files. Databases are copied first, then files; each snapshot is checked (database integrity, every attachment's file present) before it's published, and old ones are pruned only after a good one exists.

```bash
mkdir -p ~/chitchat/scripts && cp scripts/chitchat-backup.py ~/chitchat/scripts/
python3 ~/chitchat/scripts/chitchat-backup.py backup --data ~/chitchat/data --dest ~/chitchat-backups --keep 7
```

Daily at 04:00 (host time) with `~/.config/systemd/user/chitchat-backup.service`:

```ini
[Unit]
Description=ChitChat data backup (snapshot in ~/chitchat-backups)

[Service]
Type=oneshot
ExecStart=/usr/bin/python3 %h/chitchat/scripts/chitchat-backup.py backup --data %h/chitchat/data --dest %h/chitchat-backups --keep 7
Nice=10
IOSchedulingClass=idle
```

and `~/.config/systemd/user/chitchat-backup.timer`:

```ini
[Unit]
Description=Daily ChitChat backup

[Timer]
OnCalendar=*-*-* 04:00
RandomizedDelaySec=10min
Persistent=true

[Install]
WantedBy=timers.target
```

```bash
systemctl --user daemon-reload && systemctl --user enable --now chitchat-backup.timer
journalctl --user -u chitchat-backup -n 5     # last results
```

`Persistent=true` runs a missed backup after the machine was off. User services need lingering (`loginctl enable-linger`), as for the host itself.

**Check or restore** (restore never touches the live data; it copies into a new folder and checks it):

```bash
python3 ~/chitchat/scripts/chitchat-backup.py verify ~/chitchat-backups/2026-10-22_040312
python3 ~/chitchat/scripts/chitchat-backup.py restore ~/chitchat-backups/2026-10-22_040312 --to ~/chitchat-restore
systemctl --user stop chitchat
mv ~/chitchat/data ~/chitchat/data.before-restore && mv ~/chitchat-restore ~/chitchat/data
systemctl --user start chitchat
```

**Limit:** snapshots on the same disk don't survive a disk failure or a stolen laptop. Copy `~/chitchat-backups` to another device now and then (an external drive, or `rsync -aH` to another machine you control); `-H` keeps the hard links so the copy stays small.

Password recovery tooling remains future work.
