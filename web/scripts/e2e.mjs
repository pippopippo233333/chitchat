// Run only against a disposable development server. Creates test accounts and a dedicated test channel.
// Credentials and setup codes are supplied through environment variables and never logged.
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { deflateSync } from 'node:zlib';
import { WebSocket } from 'ws';

const origin = new URL(process.env.CHITCHAT_E2E_URL || 'http://127.0.0.1:8080');
const slug = process.env.CHITCHAT_E2E_SLUG || 'dev';
const prefix = `/s/${encodeURIComponent(slug)}/api`;
const suffix = randomUUID().replaceAll('-', '').slice(0, 10);
const ownerName = process.env.CHITCHAT_E2E_OWNER_USERNAME || `e2e_owner_${suffix}`;
const ownerPassword = process.env.CHITCHAT_E2E_OWNER_PASSWORD || randomUUID();
const memberName = `e2e_member_${suffix}`;
const memberPassword = randomUUID();
const sockets = [];
const sessionTokens = [];
let ownerToken, channelId;

async function request(path, { method = 'GET', body, token, status = 200, host = false } = {}) {
  const response = await fetch(new URL(`${host ? '' : prefix}${path}`, origin), {
    method, signal: AbortSignal.timeout(15000),
    headers: { ...(token ? { Authorization: `Bearer ${token}` } : {}), ...(body ? { 'Content-Type': 'application/json' } : {}) },
    ...(body ? { body: JSON.stringify(body) } : {}),
  });
  const data = response.status === 204 ? undefined : await response.json();
  assert.equal(response.status, status, `${method} ${path}: expected ${status}, got ${response.status}${data?.error ? ` (${data.error.code}: ${data.error.message})` : ''}`);
  return data;
}

function connect(token) {
  const url = new URL(`${prefix}/ws`, origin); url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  const socket = new WebSocket(url); sockets.push(socket);
  const events = [], waiters = new Set();
  const wait = (type, predicate = () => true) => new Promise((resolve, reject) => {
    const existing = events.findIndex(event => event.type === type && predicate(event.data));
    if (existing >= 0) { resolve(events.splice(existing, 1)[0].data); return; }
    const timer = setTimeout(() => { waiters.delete(waiter); reject(new Error(`Timed out waiting for WS ${type}`)); }, 10000);
    const waiter = { type, predicate, resolve: data => { clearTimeout(timer); waiters.delete(waiter); resolve(data); }, reject: error => { clearTimeout(timer); waiters.delete(waiter); reject(error); } };
    waiters.add(waiter);
  });
  socket.on('open', () => socket.send(JSON.stringify({ type: 'identify', data: { token } })));
  socket.on('message', raw => {
    const event = JSON.parse(raw.toString());
    const waiter = [...waiters].find(item => item.type === event.type && item.predicate(event.data));
    if (waiter) waiter.resolve(event.data); else events.push(event);
  });
  socket.on('error', error => { for (const waiter of waiters) waiter.reject(error); });
  socket.on('close', () => { for (const waiter of waiters) waiter.reject(new Error('WebSocket closed during test')); });
  return { wait };
}

async function main() {
  assert.ok(['http:', 'https:'].includes(origin.protocol), 'Use an HTTP(S) host');
  assert.ok(process.env.CHITCHAT_SETUP_CODE || (process.env.CHITCHAT_E2E_OWNER_USERNAME && process.env.CHITCHAT_E2E_OWNER_PASSWORD), 'Set CHITCHAT_SETUP_CODE for a fresh test server, or CHITCHAT_E2E_OWNER_USERNAME and CHITCHAT_E2E_OWNER_PASSWORD for an existing test owner.');
  const host = await request('/api/host', { host: true }); assert.ok(Array.isArray(host.servers));
  const info = await request('/server/public'); assert.equal(info.slug, slug);
  const credentials = (username, password, extra = {}) => ({ username, password, display_name: username, device_id: randomUUID(), ...(process.env.CHITCHAT_E2E_SERVER_PASSWORD ? { server_password: process.env.CHITCHAT_E2E_SERVER_PASSWORD } : {}), ...(process.env.CHITCHAT_E2E_INVITE_CODE ? { invite_code: process.env.CHITCHAT_E2E_INVITE_CODE } : {}), ...extra });
  const existingOwner = !!process.env.CHITCHAT_E2E_OWNER_USERNAME;
  const owner = await request(existingOwner ? '/auth/login' : '/auth/register', { method: 'POST', body: credentials(ownerName, ownerPassword, existingOwner ? {} : { setup_code: process.env.CHITCHAT_SETUP_CODE }), status: existingOwner ? 200 : 201 });
  ownerToken = owner.token; sessionTokens.push(owner.token);
  const me = await request('/me', { token: ownerToken }); assert.equal(me.is_owner, true, 'Setup code must create the owner');
  const member = await request('/auth/register', { method: 'POST', body: credentials(memberName, memberPassword), status: 201 }); sessionTokens.push(member.token);
  const memberMe = await request('/me', { token: member.token }); assert.equal(memberMe.is_owner, false); assert.equal((BigInt(memberMe.permissions) & 512n) !== 0n, false, 'Default members must not be administrators');
  console.log('PASS discovery, owner/member registration, and default permissions');

  const channels = await request('/channels', { token: ownerToken }); assert.ok(Array.isArray(channels));
  const created = await request('/channels', { method: 'POST', token: ownerToken, body: { name: `e2e-${suffix}`, type: 'text', topic: 'Disposable automated smoke test' }, status: 201 }); channelId = created.id;
  const stream = connect(ownerToken); const ready = await stream.wait('ready'); assert.equal(ready.user.id, owner.user.id); assert.ok(ready.channels.some(channel => channel.id === channelId));
  const path = `/channels/${channelId}/messages`;
  const message = await request(path, { method: 'POST', token: member.token, body: { content: `Hello ${suffix}`, nonce: suffix }, status: 201 });
  assert.equal(typeof message.id, 'string'); assert.equal((await stream.wait('message_create', event => event.id === message.id)).nonce, suffix);
  assert.ok((await request(path, { token: ownerToken })).some(item => item.id === message.id));
  console.log('PASS channel lists, creation, WS identify/ready, live message_create, and history');

  const emoji = encodeURIComponent('👍'); const reactionPath = `${path}/${message.id}/reactions/${emoji}`;
  await request(reactionPath, { method: 'PUT', token: ownerToken, status: 204 });
  const reactionEvent = await stream.wait('reaction_add', event => event.message_id === message.id); assert.equal(reactionEvent.user_id, owner.user.id);
  const reacted = (await request(path, { token: ownerToken })).find(item => item.id === message.id); assert.equal(reacted.reactions.find(reaction => reaction.emoji === '👍')?.me, true);
  const edited = await request(`${path}/${message.id}`, { method: 'PATCH', token: member.token, body: { content: `Edited ${suffix}` } }); assert.ok(edited.edited_at);
  const update = await stream.wait('message_update', event => event.id === message.id); assert.equal(update.content, edited.content); assert.ok(update.reactions.every(reaction => reaction.me === false), 'Broadcast reaction ownership must not leak');
  await request(reactionPath, { method: 'DELETE', token: ownerToken, status: 204 }); await stream.wait('reaction_remove', event => event.message_id === message.id);
  await request(`${path}/${message.id}`, { method: 'DELETE', token: member.token, status: 204 }); await stream.wait('message_delete', event => event.id === message.id);
  assert.ok(!(await request(path, { token: ownerToken })).some(item => item.id === message.id));
  console.log('PASS edit, reactions and own flags, reaction removal, deletion, and live events');

  if (process.env.CHITCHAT_E2E_EXTENDED === '1') await extended(owner, member, stream, path, credentials);

  const invalid = await request(path, { method: 'POST', token: ownerToken, body: { content: Array(61).fill('line').join('\n') }, status: 400 }); assert.equal(invalid.error.fields?.content, 'too_many_lines');
  // Repeated text deterministically triggers one violation, without intentionally causing an auto-mute.
  const repeated = { content: `Rate limit check ${suffix}` };
  await request(path, { method: 'POST', token: member.token, body: repeated, status: 201 });
  await request(path, { method: 'POST', token: member.token, body: repeated, status: 201 });
  const limited = await request(path, { method: 'POST', token: member.token, body: repeated, status: 429 }); assert.equal(limited.error.code, 'rate_limited'); assert.ok(limited.error.retry_after_ms > 0);
  console.log('PASS 60-line validation and 429 anti-spam retry hint');
  if (process.env.CHITCHAT_E2E_EXTENDED === '1') await moderation(owner, member, credentials);
}

function png() {
  const crc = bytes => { let value = 0xffffffff; for (const byte of bytes) { value ^= byte; for (let bit = 0; bit < 8; bit++) value = (value >>> 1) ^ ((value & 1) ? 0xedb88320 : 0); } return (value ^ 0xffffffff) >>> 0; };
  const chunk = (type, data) => { const name = Buffer.from(type); const length = Buffer.alloc(4); length.writeUInt32BE(data.length); const checksum = Buffer.alloc(4); checksum.writeUInt32BE(crc(Buffer.concat([name, data]))); return Buffer.concat([length, name, data, checksum]); };
  const header = Buffer.alloc(13); header.writeUInt32BE(1, 0); header.writeUInt32BE(1, 4); header[8] = 8; header[9] = 2;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', header), chunk('IDAT', deflateSync(Buffer.from([0, 255, 0, 0]))), chunk('IEND', Buffer.alloc(0))]);
}

async function upload(token, file, name, status = 201) {
  const response = await fetch(new URL(`${prefix}/channels/${channelId}/attachments?filename=${encodeURIComponent(name)}`, origin), { method: 'POST', headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/octet-stream' }, body: file, signal: AbortSignal.timeout(20000) });
  const data = await response.json(); assert.equal(response.status, status, `Upload expected ${status}, got ${response.status} (${data.error?.code ?? ''})`); return data;
}

async function extended(owner, member, stream, path, credentials) {
  const original = await request('/settings', { token: ownerToken });
  assert.equal(original.has_password, false, 'Extended tests require a disposable server without an existing access password; the original secret cannot be restored.');
  let roleId, ruleId; const inviteCodes = [];
  const memberStream = connect(member.token); await memberStream.wait('ready');
  try {
    await request('/settings', { token: member.token, status: 403 });
    await request('/channels', { method: 'POST', token: member.token, body: { name: 'forbidden', type: 'text' }, status: 403 });
    await request('/settings', { method: 'PATCH', token: ownerToken, body: { description: `Smoke ${suffix}`, invite_only: true, listed: false } });
    const hidden = await request('/api/host', { host: true }); assert.ok(!hidden.servers.some(server => server.slug === slug));
    const invite = await request('/invites', { method: 'POST', token: ownerToken, body: { max_uses: 1, expires_in: 3600 }, status: 201 }); inviteCodes.push(invite.code);
    const preview = await request(`/invites/${invite.code}`); assert.equal(preview.server.slug, slug); assert.equal(preview.uses_left, 1);
    await request('/settings/password', { method: 'PUT', token: ownerToken, body: { password: 'e2e-access-only' } });
    const fresh = credentials(`e2e_invited_${suffix}`, randomUUID(), { invite_code: invite.code });
    await request('/auth/register', { method: 'POST', body: fresh, status: 403 });
    const invited = await request('/auth/register', { method: 'POST', body: { ...fresh, server_password: 'e2e-access-only' }, status: 201 }); sessionTokens.push(invited.token);
    const used = await request(`/invites/${invite.code}`, { status: 404 }); assert.equal(used.error.code, 'invalid_invite');
    await request('/settings/password', { method: 'PUT', token: ownerToken, body: { password: null } });
    await request('/settings', { method: 'PATCH', token: ownerToken, body: { invite_only: original.invite_only, listed: original.listed } });
    console.log('PASS settings visibility, invite preview/use limits, and invite + password enforcement');

    const rule = await request('/ip-rules', { method: 'POST', token: ownerToken, body: { cidr: '203.0.113.77/24', action: 'block', note: 'Disposable TEST-NET rule' }, status: 201 }); ruleId = rule.id; assert.equal(rule.cidr, '203.0.113.0/24');
    assert.ok((await request('/ip-rules', { token: ownerToken })).some(item => item.id === ruleId));
    await request('/settings', { method: 'PATCH', token: ownerToken, body: { ip_mode: 'allowlist' } });
    await request('/me', { token: ownerToken }); // Loopback owner recovery remains possible.
    await request('/settings', { method: 'PATCH', token: ownerToken, body: { ip_mode: original.ip_mode } });
    const role = await request('/roles', { method: 'POST', token: ownerToken, body: { name: `e2e-role-${suffix}`, color: '#88bb99', permissions: '1152' }, status: 201 }); roleId = role.id;
    await request(`/members/${member.user.id}/roles/${role.id}`, { method: 'PUT', token: ownerToken, status: 204 });
    const permissions = await memberStream.wait('permissions_update'); assert.equal((BigInt(permissions.permissions) & 128n) !== 0n, true);
    await request('/settings/password', { method: 'PUT', token: member.token, body: { password: null }, status: 403 });
    await request('/ip-rules', { token: member.token, status: 403 });
    const changed = await request(`/roles/${role.id}`, { method: 'PATCH', token: ownerToken, body: { name: `e2e-updated-${suffix}`, position: 1 } }); assert.equal(changed.name, `e2e-updated-${suffix}`);
    console.log('PASS IP normalization/loopback recovery, role assignment, permissions_update, and owner-only access');

    await request(`/channels/${channelId}`, { method: 'PATCH', token: ownerToken, body: { name: `e2e-media-${suffix}`, topic: 'Media smoke test', media_allowed: true } });
    const attachment = await upload(member.token, png(), 'pixel.png'); assert.equal(attachment.kind, 'image'); assert.equal(attachment.mime, 'image/webp');
    const media = await fetch(new URL(attachment.url, origin)); assert.equal(media.status, 200); assert.equal(media.headers.get('content-type'), 'image/webp');
    const bad = new URL(attachment.url, origin); bad.searchParams.set('sig', 'invalid'); assert.equal((await fetch(bad)).status, 404);
    const mediaMessage = await request(path, { method: 'POST', token: member.token, body: { content: '', attachment_ids: [attachment.id] }, status: 201 }); assert.equal(mediaMessage.attachments.length, 1);
    const duplicate = await request(path, { method: 'POST', token: member.token, body: { content: '', attachment_ids: [attachment.id] }, status: 400 }); assert.equal(duplicate.error.fields?.attachment_ids, 'unknown_attachment');
    const storage = await request('/me/storage', { token: member.token }); assert.ok(storage.used_total_bytes > 0); assert.equal(storage.exempt, false);
    assert.equal((await upload(ownerToken, Buffer.from('not image media'), 'bad.exe', 415)).error.code, 'unsupported_media');
    await request('/settings', { method: 'PATCH', token: ownerToken, body: { max_file_mb: 1 } });
    assert.equal((await upload(ownerToken, Buffer.alloc(2 * 1048576), 'too-big.png', 413)).error.code, 'too_large');
    await request('/settings', { method: 'PATCH', token: ownerToken, body: { user_total_mb: 0 } });
    assert.equal((await upload(member.token, png(), 'quota.png', 413)).error.code, 'quota_exceeded');
    await request('/settings', { method: 'PATCH', token: ownerToken, body: { user_total_mb: original.user_total_mb, max_file_mb: original.max_file_mb, new_account_restrict_hours: 1 } });
    const restricted = await request(path, { method: 'POST', token: member.token, body: { content: 'https://example.com/restricted' }, status: 403 }); assert.equal(restricted.error.code, 'account_too_new'); assert.ok(restricted.error.until);
    await request(`/channels/${channelId}`, { method: 'PATCH', token: ownerToken, body: { media_allowed: false } });
    assert.equal((await upload(ownerToken, png(), 'disabled.png', 403)).error.code, 'media_not_allowed');
    console.log('PASS raw upload/WebP, signed media URLs, attachment-only messages, quotas, account age, and channel media policy');
  } finally {
    const fields = ['name', 'description', 'invite_only', 'registration_open', 'listed', 'ip_mode', 'max_file_mb', 'user_daily_mb', 'user_total_mb', 'server_total_mb', 'media_retention_days', 'new_account_restrict_hours', 'max_accounts_per_ip', 'spam_messages_per_5s', 'spam_chars_per_minute', 'spam_max_repeats', 'spam_timeout_minutes'];
    await request('/settings', { method: 'PATCH', token: ownerToken, body: Object.fromEntries(fields.map(key => [key, original[key]])) });
    await request('/settings/password', { method: 'PUT', token: ownerToken, body: { password: null } });
    if (roleId) await request(`/roles/${roleId}`, { method: 'DELETE', token: ownerToken, status: 204 });
    if (ruleId) await request(`/ip-rules/${ruleId}`, { method: 'DELETE', token: ownerToken, status: 204 });
    for (const code of inviteCodes) await request(`/invites/${code}`, { method: 'DELETE', token: ownerToken, status: 204 });
  }
}

async function moderation(owner, member, credentials) {
  await request(`/members/${owner.user.id}/timeout`, { method: 'PUT', token: member.token, body: { minutes: 1 }, status: 403 });
  await request(`/members/${owner.user.id}/timeout`, { method: 'PUT', token: ownerToken, body: { minutes: 1 }, status: 400 });
  await request(`/members/${member.user.id}/timeout`, { method: 'PUT', token: ownerToken, body: { minutes: 1, reason: 'Smoke timeout' }, status: 204 });
  const timedOut = await request(`/channels/${channelId}/messages`, { method: 'POST', token: member.token, body: { content: `Muted ${suffix}` }, status: 403 }); assert.equal(timedOut.error.code, 'timed_out'); assert.ok(timedOut.error.until);
  await request(`/members/${member.user.id}/timeout`, { method: 'DELETE', token: ownerToken, status: 204 });
  await request(`/members/${member.user.id}/kick`, { method: 'POST', token: ownerToken, body: { reason: 'Smoke kick' }, status: 204 });
  await request('/me', { token: member.token, status: 401 });
  const login = await request('/auth/login', { method: 'POST', body: credentials(memberName, memberPassword) }); sessionTokens.push(login.token);
  const ban = await request('/bans', { method: 'POST', token: ownerToken, body: { user_id: member.user.id, reason: 'Smoke ban', duration_hours: 1, ban_ip: false, ban_device: false, delete_messages_hours: 0 }, status: 201 });
  const denied = await request('/auth/login', { method: 'POST', body: credentials(memberName, memberPassword), status: 403 }); assert.equal(denied.error.code, 'banned');
  assert.ok((await request('/bans', { token: ownerToken })).some(item => item.id === ban.id));
  await request(`/bans/${ban.id}`, { method: 'DELETE', token: ownerToken, status: 204 });
  const restored = await request('/auth/login', { method: 'POST', body: credentials(memberName, memberPassword) }); sessionTokens.push(restored.token);
  const audit = await request('/audit-log?limit=100', { token: ownerToken }); assert.ok(audit.some(entry => entry.action === 'member_ban')); assert.ok(audit.some(entry => entry.action === 'member_timeout'));
  console.log('PASS moderation rank protection, timeout/clear, kick/session revocation, ban/unban, and audit log');
}

try { await main(); console.log('REAL HOST SMOKE TEST PASSED'); }
catch (error) { console.error(`FAIL: ${error.message}`); process.exitCode = 1; }
finally {
  for (const socket of sockets) socket.terminate();
  if (channelId && ownerToken) {
    try { await request(`/channels/${channelId}`, { method: 'DELETE', token: ownerToken, status: 204 }); console.log('Cleaned up the channel created by this test.'); }
    catch (error) { console.error(`Cleanup: ${error.message}`); process.exitCode = 1; }
  }
  for (const token of sessionTokens) {
    try { const response = await fetch(new URL(`${prefix}/auth/logout`, origin), { method: 'POST', headers: { Authorization: `Bearer ${token}` }, signal: AbortSignal.timeout(15000) }); assert.ok([204, 401].includes(response.status), `Logout returned ${response.status}`); await request('/me', { token, status: 401 }); }
    catch (error) { console.error(`Logout: ${error.message}`); process.exitCode = 1; }
  }
  if (sessionTokens.length) console.log('Test accounts remain in this disposable server; session tokens were revoked.');
}
