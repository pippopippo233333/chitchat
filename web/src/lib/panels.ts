import type { AiNotice } from './api';
export function noticeKey(origin: string, slug: string, channel: string) { return `chitchat.ai.notice:${origin}:${slug}:${channel}`; }
export function noticeSignature(notice: AiNotice | null | undefined) { return notice ? JSON.stringify({cloud:notice.cloud,providers:[...notice.providers].sort(),channels:notice.channel_ids===null?null:[...notice.channel_ids].sort()}) : ''; }
export function readPreference(key: string) { try { return localStorage.getItem(key); } catch { return null; } }
export function writePreference(key: string, value: string | null) { try { if(value===null)localStorage.removeItem(key);else localStorage.setItem(key,value); } catch { /* Keep the current page usable with storage disabled. */ } }
export function panelHeight(value: number, viewportHeight: number) { return Math.round(Math.min(Math.max(180,viewportHeight-150),Math.max(180,Number.isFinite(value)?value:360))); }
