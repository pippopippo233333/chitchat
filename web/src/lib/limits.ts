import { ApiError } from './api';

export function contentLimits(content: string, attachments = 0) {
  const trimmed = content.trim();
  const characters = [...trimmed].length;
  const lines = trimmed.split('\n').length;
  const error = !trimmed && !attachments ? 'Write a message first.' : characters > 4000 ? 'Messages can have up to 4,000 characters.' : lines > 60 ? 'Messages can have up to 60 lines.' : '';
  return { characters, lines, error };
}

export function sendRestriction(error: unknown, now: number): { cooldown?: number; mutedUntil?: number } {
  if (!(error instanceof ApiError)) return {};
  if (error.code === 'timed_out') {
    const timestamp = Date.parse(error.until ?? '');
    // Keep a short local pause if an older host omits its timestamp; the host remains authoritative.
    return { mutedUntil: Number.isFinite(timestamp) && timestamp > now ? timestamp : now + 10000 };
  }
  if (error.code === 'rate_limited') {
    const retry = error.retryAfter;
    return { cooldown: now + (typeof retry === 'number' && Number.isFinite(retry) && retry > 0 ? retry : 1000) };
  }
  return {};
}
