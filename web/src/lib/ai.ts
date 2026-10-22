import type { AiConfig, AiFlag, AiNotice } from './api';
export type KeyChange = { mode: 'keep' | 'replace' | 'remove'; value: string };
export function keyPatch(change: KeyChange) { return change.mode === 'keep' ? {} : { api_key: change.mode === 'remove' ? '' : change.value }; }
export function aiPatch(config: AiConfig, layaKey: KeyChange, llmKey: KeyChange) {
  return { enabled: config.enabled, channel_ids: [...config.channel_ids], action: config.action, timeout_minutes: config.timeout_minutes, rules: config.rules,
    laya: config.laya ? { url: config.laya.url, threshold: config.laya.threshold, spam_threshold: config.laya.spam_threshold, ...keyPatch(layaKey) } : null,
    llm: config.llm ? { kind: config.llm.kind, base_url: config.llm.base_url, model: config.llm.model, requests_per_minute: config.llm.requests_per_minute, requests_per_day: config.llm.requests_per_day, ...keyPatch(llmKey) } : null };
}
export function coversChannel(notice: AiNotice | null | undefined, channel: { id: string; type: string } | undefined) { return !!notice && channel?.type === 'text' && (notice.channel_ids === null || notice.channel_ids.includes(channel.id)); }
export function mergeFlags(existing: AiFlag[], updates: AiFlag[], status = 'all') {
  const rows = new Map(existing.map(row => [row.id, row])); updates.forEach(row => rows.set(row.id, row));
  return [...rows.values()].filter(row => status === 'all' || row.status === status).sort((a, b) => BigInt(a.id) > BigInt(b.id) ? -1 : BigInt(a.id) < BigInt(b.id) ? 1 : 0);
}
