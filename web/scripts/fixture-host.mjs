// Development-only API fixture. Never imported by the application or production build.
import { createServer } from 'node:http';
import { WebSocketServer, WebSocket } from 'ws';

const port = Number(process.env.CHITCHAT_FIXTURE_PORT || 18080);
const serverInfo = { id: '1', slug: 'dev', name: 'The Common Room', description: 'A small corner for good company.', icon_url: null, requires_password: false, invite_only: false, registration_open: true, owner_id: '10', created_at: new Date().toISOString() };
const user = { id: '10', username: 'alex', display_name: 'Alex Morgan', avatar_url: null, created_at: new Date().toISOString() };
const sam = { ...user, id: '11', username: 'sam', display_name: 'Sam Rivera' };
const categories = [{ id: '20', name: 'The gathering place', position: 0 }];
let channels = [{ id: '30', name: 'general', type: 'text', topic: 'Everyday conversations, extraordinary people.', category_id: '20', position: 0, media_allowed: false, my_permissions: '32767' }, { id: '31', name: 'introductions', type: 'text', topic: 'Tell us a little about yourself.', category_id: '20', position: 1, media_allowed: false, my_permissions: '32767' }];
const members = [{ user, role_ids: ['50'], joined_at: user.created_at, online: true, is_owner: true }, { user: sam, role_ids: ['50'], joined_at: user.created_at, online: true, is_owner: false }];
const roles = [{ id: '50', name: '@everyone', color: '#a4e4c2', position: 0, permissions: '771', is_default: true }];
let nextId = 9007199254741000n;
const messages = Array.from({ length: 56 }, (_, index) => ({ id: String(nextId++), channel_id: '30', author: index % 2 ? user : sam, content: index === 55 ? 'The kettle is on. What has everyone been working on today?' : index === 54 ? 'Welcome to our little corner of the internet. Glad you found us 🌿' : `Earlier conversation ${index + 1}`, created_at: new Date(Date.now() - (56 - index) * 60000).toISOString(), edited_at: null, reply_to: null, reactions: index === 54 ? [{ emoji: '👋', count: 1, me: false }] : [], attachments: [] }));
const clients = new Set();
const broadcast = (type, data) => { for (const socket of clients) if (socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ type, data })); };
const snapshot = () => ({ user, is_owner: true, permissions: '32767', server: serverInfo, categories, channels, members, roles });
const server = createServer(async (req, res) => {
  res.setHeader('Access-Control-Allow-Origin', '*');
  res.setHeader('Access-Control-Allow-Headers', 'Authorization, Content-Type');
  res.setHeader('Access-Control-Allow-Methods', 'GET, POST, PATCH, PUT, DELETE');
  const json = (data, status = 200) => { res.writeHead(status, { 'Content-Type': 'application/json' }); res.end(data === undefined ? undefined : JSON.stringify(data)); };
  const fail = (status, code, message) => json({ error: { code, message } }, status);
  if (req.method === 'OPTIONS') { json(undefined, 204); return; }
  const url = new URL(req.url, `http://127.0.0.1:${port}`);
  if (url.pathname === '/api/host') { json({ name: 'Development fixture (not a real server)', version: 'fixture', servers: [serverInfo, { ...serverInfo, slug: 'other', name: 'Other server' }] }); return; }
  const path = url.pathname.replace(/^\/s\/(dev|other)\/api/, '');
  if (path === '/server/public') { json(serverInfo); return; }
  let body = {};
  try { let raw = ''; for await (const chunk of req) raw += chunk; body = raw ? JSON.parse(raw) : {}; } catch { fail(400, 'bad_request', 'Invalid JSON.'); return; }
  if (path === '/auth/login' || path === '/auth/register') {
    if (body.password === 'wrongpass') { fail(401, 'invalid_credentials', 'Wrong username or password.'); return; }
    if (path === '/auth/register') { user.username = body.username; user.display_name = body.display_name; }
    json({ token: 'fixture-token', user }, path.endsWith('register') ? 201 : 200); return;
  }
  if (req.headers.authorization !== 'Bearer fixture-token') { fail(401, 'unauthorized', 'Please sign in.'); return; }
  if (path === '/auth/logout') { json(undefined, 204); return; }
  if (path === '/channels' && req.method === 'POST') {
    const channel = { ...channels[0], ...body, id: String(nextId++), position: channels.length }; channels.push(channel); broadcast('channel_create', channel); json(channel, 201); return;
  }
  const route = /^\/channels\/(\d+)\/messages(?:\/(\d+))?(?:\/reactions\/(.+))?$/.exec(path);
  if (route) {
    const [, channelId, id, encodedEmoji] = route;
    if (!id && req.method === 'GET') {
      let page = messages.filter(message => message.channel_id === channelId);
      if (url.searchParams.has('before')) page = page.filter(message => BigInt(message.id) < BigInt(url.searchParams.get('before')));
      if (url.searchParams.has('after')) page = page.filter(message => BigInt(message.id) > BigInt(url.searchParams.get('after')));
      json(page.slice(-Number(url.searchParams.get('limit') || 50))); return;
    }
    if (!id && req.method === 'POST') {
      const message = { id: String(nextId++), channel_id: channelId, author: { ...user }, content: body.content, created_at: new Date().toISOString(), edited_at: null, reply_to: body.reply_to ?? null, nonce: body.nonce, reactions: [], attachments: [] };
      messages.push(message); broadcast('message_create', message); json(message, 201); return;
    }
    const index = messages.findIndex(message => message.id === id && message.channel_id === channelId);
    if (index < 0) { fail(404, 'unknown_message', 'Message not found.'); return; }
    if (encodedEmoji) {
      const emoji = decodeURIComponent(encodedEmoji), add = req.method === 'PUT';
      const reaction = messages[index].reactions.find(item => item.emoji === emoji);
      if (add && !reaction?.me) {
        if (reaction) { reaction.count++; reaction.me = true; } else messages[index].reactions.push({ emoji, count: 1, me: true });
        broadcast('reaction_add', { message_id: id, channel_id: channelId, emoji, user_id: user.id });
      } else if (!add && reaction?.me) {
        reaction.count--; reaction.me = false; messages[index].reactions = messages[index].reactions.filter(item => item.count > 0);
        broadcast('reaction_remove', { message_id: id, channel_id: channelId, emoji, user_id: user.id });
      }
      json(undefined, 204); return;
    }
    if (req.method === 'PATCH') { messages[index].content = body.content; messages[index].edited_at = new Date().toISOString(); broadcast('message_update', messages[index]); json(messages[index]); return; }
    if (req.method === 'DELETE') { messages.splice(index, 1); broadcast('message_delete', { id, channel_id: channelId }); json(undefined, 204); return; }
  }
  fail(404, 'not_found', 'Fixture endpoint not implemented.');
});
const wss = new WebSocketServer({ server });
wss.on('connection', socket => {
  socket.on('message', raw => {
    const event = JSON.parse(raw.toString());
    if (event.type === 'identify') {
      if (event.data.token !== 'fixture-token') { socket.close(4001); return; }
      clients.add(socket); socket.send(JSON.stringify({ type: 'ready', data: snapshot() }));
    }
  });
  socket.on('close', () => clients.delete(socket));
});
server.listen(port, '127.0.0.1', () => console.log(`Development fixture only: http://127.0.0.1:${port}\nSign in with any username/password (8+ characters); wrongpass tests errors.\nConnect through the client’s “another host” option. Data resets on restart.`));
