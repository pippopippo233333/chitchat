import { describe, expect, it } from 'vitest';
import { aiPatch, coversChannel, mergeFlags } from './ai';
import type { AiConfig, AiFlag } from './api';
const config: AiConfig = { enabled: true, channel_ids: [], action: 'flag', timeout_minutes: 10, rules: '', laya: { url: 'http://127.0.0.1:8000', has_api_key: true, threshold: 0.7, spam_threshold: 0.95 }, llm: { kind: 'openai', base_url: 'http://localhost:11434/v1', has_api_key: true, model: 'qa', requests_per_minute: 30, requests_per_day: 100, cloud: false } };
const keep = { mode: 'keep' as const, value: 'stale-input' };
describe('AI moderation contracts', () => {
  it('omits saved/masked keys and server-derived fields while retaining settings', () => {
    const patch = aiPatch(config, keep, keep);
    expect(patch.laya).toEqual({ url: config.laya!.url, threshold: 0.7, spam_threshold: 0.95 });
    for (const key of ['api_key', 'has_api_key', 'cloud']) expect(patch.llm).not.toHaveProperty(key);
    expect(JSON.stringify(patch)).not.toContain('stale-input');
  });
  it('distinguishes removing, replacing, and disabling providers', () => {
    const patch = aiPatch(config, { mode: 'remove', value: 'ignored' }, { mode: 'replace', value: 'new-key' });
    expect(patch.laya).toHaveProperty('api_key', ''); expect(patch.llm).toHaveProperty('api_key', 'new-key');
    expect(aiPatch({ ...config, laya: null, llm: null }, keep, keep)).toMatchObject({ laya: null, llm: null });
  });
  it('discloses AI only in covered text channels', () => {
    const channel = { id: '1', type: 'text' }, notice = { channel_ids: null, cloud: true, providers: ['Cloud'] };
    expect(coversChannel(null, channel)).toBe(false); expect(coversChannel(notice, channel)).toBe(true);
    expect(coversChannel(notice, { ...channel, type: 'voice' })).toBe(false);
    expect(coversChannel({ ...notice, channel_ids: ['2'] }, channel)).toBe(false);
    expect(coversChannel({ ...notice, channel_ids: ['1'] }, channel)).toBe(true);
  });
  it('merges live resolution over pages and orders full precision ids', () => {
    const a = { id: '9007199254740992', status: 'open' } as AiFlag, b = { id: '9007199254740993', status: 'open' } as AiFlag;
    expect(mergeFlags([a], [b, { ...a, status: 'resolved' }], 'open')).toEqual([b]);
    expect(mergeFlags([b], [a], 'all').map(row => row.id)).toEqual([b.id, a.id]);
  });
});
