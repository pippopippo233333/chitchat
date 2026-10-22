import { randomId } from './random';
export interface User { id: string; username: string; display_name: string; avatar_url: string | null; created_at?: string }
export interface ServerSummary { slug: string; name: string; description: string; icon_url: string | null; requires_password: boolean; invite_only: boolean; registration_open: boolean; voice_messages?: boolean }
export interface Server extends ServerSummary { id: string; owner_id: string; created_at: string }
export interface HostInfo { name: string; version: string; servers: ServerSummary[] }
export interface Category { id: string; name: string; position: number }
export interface Channel { id: string; type: 'text' | 'voice'; name: string; topic: string; category_id: string | null; position: number; media_allowed: boolean; my_permissions: string; user_limit?: number }
export interface VoiceState { user_id: string; channel_id: string; session_id: string; self_mute: boolean; self_deaf: boolean; suppressed: boolean; streaming: boolean }
export interface Member { user: User; role_ids: string[]; joined_at: string; online: boolean; is_owner: boolean; timed_out_until?: string | null }
export interface Role { id: string; name: string; color: string; position: number; permissions: string; is_default: boolean }
export interface Reaction { emoji: string; count: number; me: boolean }
export interface Attachment { id: string; filename: string; kind: 'image' | 'video' | 'audio' | 'file'; duration_ms?: number | null; voice?: boolean; mime: string; size: number; width: number | null; height: number | null; url: string; thumbnail_url: string | null }
export interface Message { id: string; channel_id: string; author: User; content: string; created_at: string; edited_at: string | null; reply_to: string | null; reactions: Reaction[]; attachments: Attachment[]; nonce?: string }
export interface Settings { name: string; description: string; has_password: boolean; invite_only: boolean; registration_open: boolean; listed: boolean; discoverable?: boolean; voice_messages?: boolean; ip_mode: 'off' | 'allowlist'; max_file_mb: number; user_daily_mb: number; user_total_mb: number; server_total_mb: number; media_retention_days: number; storage_used_bytes: number; new_account_restrict_hours: number; max_accounts_per_ip: number; spam_messages_per_5s: number; spam_chars_per_minute: number; spam_max_repeats: number; spam_timeout_minutes: number }
export interface Invite { code: string; creator_id: string; max_uses: number | null; uses: number; expires_at: string | null; created_at: string }
export interface InvitePreview { code: string; server: ServerSummary; expires_at: string | null; uses_left: number | null }
export interface IpRule { id: string; cidr: string; action: 'allow' | 'block'; note: string; created_at: string }
export interface Ban { id: string; user: User | null; reason: string; ip_banned: boolean; device_banned: boolean; ips?: string[]; created_by: string; created_at: string; expires_at: string | null }
export interface AuditEntry { id: string; actor_id: string | null; action: string; target_id: string | null; details: Record<string, unknown>; created_at: string }
export interface StorageUsage { used_today_bytes: number; used_total_bytes: number; max_file_mb: number; user_daily_mb: number; user_total_mb: number; exempt: boolean }
export interface Snapshot { user: User; is_owner: boolean; permissions: string; server: Server; categories: Category[]; channels: Channel[]; members: Member[]; roles: Role[]; voice_states?: VoiceState[]; ai_moderation?: AiNotice | null }
export interface AuthResult { token: string; user: User }
export interface Credentials { username: string; password: string; device_id: string; display_name?: string; server_password?: string; invite_code?: string; setup_code?: string }

export class ApiError extends Error {
  constructor(public status: number, public code: string, message: string, public retryAfter?: number, public until?: string, public fields?: Record<string, string>) { super(message); }
}

export function normalizeHost(value: string): string {
  const url = new URL(value);
  if (!['https:', 'http:'].includes(url.protocol) || url.username || url.password || url.search || url.hash || !['', '/'].includes(url.pathname)) {
    throw new Error('Enter a host address such as https://chat.example.com, without a path.');
  }
  return url.origin;
}

export class Api {
  readonly origin: string;
  constructor(origin: string, public slug = '', public token = '') { this.origin = normalizeHost(origin); }
  get prefix() { return `/s/${encodeURIComponent(this.slug)}/api`; }
  async request<T>(path: string, method = 'GET', body?: unknown, signal?: AbortSignal, hostLevel = false): Promise<T> {
    let response: Response;
    try {
      response = await fetch(`${this.origin}${hostLevel ? '' : this.prefix}${path}`, {
        method, signal, credentials: 'omit', cache: 'no-store',
        headers: { ...(body === undefined ? {} : { 'Content-Type': 'application/json' }), ...(!hostLevel && this.token ? { Authorization: `Bearer ${this.token}` } : {}) },
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      });
    } catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') throw error;
      throw new Error('Could not reach this host. Check the address and that the host is running.');
    }
    if (!response.ok) {
      const payload = await response.json().catch(() => null);
      throw new ApiError(response.status, payload?.error?.code ?? 'request_failed', payload?.error?.message ?? `Request failed (${response.status}).`, payload?.error?.retry_after_ms, payload?.error?.until, payload?.error?.fields);
    }
    if (response.status === 204) return undefined as T;
    try { return await response.json(); }
    catch { throw new Error('The host returned an invalid response. Check the host address.'); }
  }
  host(signal?: AbortSignal) { return this.request<HostInfo>('/api/host', 'GET', undefined, signal, true); }
  publicServer() { return this.request<ServerSummary>('/server/public'); }
  auth(mode: 'login' | 'register', credentials: Credentials) { return this.request<AuthResult>(`/auth/${mode}`, 'POST', credentials); }
  messages(channel: string, before?: string, signal?: AbortSignal) {
    const params = new URLSearchParams({ limit: '50', ...(before ? { before } : {}) });
    return this.request<Message[]>(`/channels/${encodeURIComponent(channel)}/messages?${params}`, 'GET', undefined, signal);
  }
  send(channel: string, content: string, reply_to?: string, nonce?: string, attachment_ids: string[] = []) { return this.request<Message>(`/channels/${encodeURIComponent(channel)}/messages`, 'POST', { content, ...(reply_to ? { reply_to } : {}), nonce, ...(attachment_ids.length ? { attachment_ids } : {}) }); }
  edit(channel: string, id: string, content: string) { return this.request<Message>(`/channels/${encodeURIComponent(channel)}/messages/${encodeURIComponent(id)}`, 'PATCH', { content }); }
  delete(channel: string, id: string) { return this.request<void>(`/channels/${encodeURIComponent(channel)}/messages/${encodeURIComponent(id)}`, 'DELETE'); }
  react(channel: string, id: string, emoji: string, remove: boolean) { return this.request<void>(`/channels/${encodeURIComponent(channel)}/messages/${encodeURIComponent(id)}/reactions/${encodeURIComponent(emoji)}`, remove ? 'DELETE' : 'PUT'); }
  socketUrl() { const url = new URL(`${this.origin}${this.prefix}/ws`); url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'; return url.href; }
}

export function sessionKey(origin: string, slug: string) { return `chitchat.session:${normalizeHost(origin)}:${slug}`; }
export function deviceId(): string {
  let id = localStorage.getItem('chitchat.device_id');
  if (!id) { id = randomId(); localStorage.setItem('chitchat.device_id', id); }
  return id;
}

export function can(permissions: string | undefined, bit: number, owner = false): boolean {
  if (owner) return true;
  try { const value = BigInt(permissions ?? '0'); return (value & (1n << 9n)) !== 0n || (value & (1n << BigInt(bit))) !== 0n; }
  catch { return false; }
}

export interface AiNotice { channel_ids: string[] | null; providers: string[]; cloud: boolean }
export interface AiLaya { url: string; has_api_key: boolean; threshold: number; spam_threshold: number }
export interface AiLlm { kind: 'gemini' | 'openai'; base_url: string; has_api_key: boolean; model: string; requests_per_minute: number; requests_per_day: number; cloud: boolean }
export interface AiConfig { enabled: boolean; channel_ids: string[]; action: 'flag' | 'hide' | 'timeout'; timeout_minutes: number; rules: string; laya: AiLaya | null; llm: AiLlm | null }
export interface AiSettings { config: AiConfig; status: { queued: number; checked: number; flagged: number; llm_requests_today: number; last_error: string | null }; notice: AiNotice | null }
export interface AiVerdict { flagged: boolean; categories: string[]; scores: Record<string, number> | null; provider: string; reason: string; unconfirmed: boolean }
export interface AiTest { verdict: AiVerdict; action: string; ms: number; error: string | null }
export interface AiFlag { id: string; message_id: string | null; channel_id: string; author: User | null; author_id: string; content: string; categories: string[]; scores: Record<string, number> | null; provider: string; reason: string; action: 'flagged' | 'hidden' | 'timed_out'; status: 'open' | 'dismissed' | 'deleted' | 'resolved'; created_at: string; resolved_by: string | null; resolved_at: string | null }
