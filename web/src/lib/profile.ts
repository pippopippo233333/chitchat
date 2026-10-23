import { Api, ApiError, type Member, type User } from './api';

export type Presence = 'online' | 'away' | 'offline';
export function memberPresence(member: Pick<Member, 'online'> & { status?: string }): Presence {
  return !member.online ? 'offline' : member.status === 'away' ? 'away' : 'online';
}

export function avatarUrl(path: string | null | undefined, origin: string): string {
  if (!path) return '';
  try {
    const url = new URL(path, origin);
    return url.origin === new URL(origin).origin && ['http:', 'https:'].includes(url.protocol)
      && !url.username && !url.password && /^\/s\/[^/]+\/api\/avatars\/\d+\/[a-f0-9]{64}\.webp$/.test(url.pathname) ? url.href : '';
  } catch { return ''; }
}

export async function uploadAvatar(api: Api, file: File): Promise<User> {
  if (file.size > 2 * 1024 * 1024) throw new Error('Profile pictures can be at most 2 MB.');
  const response = await fetch(`${api.origin}${api.prefix}/me/avatar`, {
    method: 'PUT', credentials: 'omit', cache: 'no-store',
    headers: { Authorization: `Bearer ${api.token}`, 'Content-Type': file.type || 'application/octet-stream' }, body: file,
  });
  if (!response.ok) {
    const payload = await response.json().catch(() => null);
    throw new ApiError(response.status, payload?.error?.code ?? 'upload_failed', payload?.error?.message ?? 'Could not upload your picture.');
  }
  return response.json();
}
