# Security

ChitChat is alpha software run by individuals on their own computers. Security reports are very welcome.

## Reporting a problem

Please **don't open a public issue** for a vulnerability. Use GitHub's private reporting instead: the **Security** tab of this repository, then **Report a vulnerability**. Include what you found, how to reproduce it, and what an attacker could do with it.

You should get an answer within a week. Once a fix is released, the report can be made public, with credit if you want it.

## Scope

In scope: the host (`crates/chitchat-host`), the web client (`web/`), and the backup script (`scripts/`). For example: account takeover, permission bypasses, reading other servers' data, server-side request forgery through link previews or discovery, file upload problems, and crashes a remote user can trigger.

Out of scope: problems that need access to the host machine or its data folder, and the security of the machine, router or tunnel someone hosts on.

## For people hosting

- Keep the host updated, and serve it over HTTPS when it's reachable from the internet.
- The data folder and backups contain password hashes, messages and private keys: keep them private (the backup script makes them readable only by you).
- The host admin (the owner of the host's first server) can change host-wide settings: protect that account with a strong password.
