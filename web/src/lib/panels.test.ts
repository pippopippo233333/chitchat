import { describe, expect, it } from 'vitest';
import { noticeKey, noticeSignature, panelHeight } from './panels';
describe('panel preferences',()=>{
 it('keeps AI dismissals isolated to the host, community and channel',()=>{const key=noticeKey('https://one.test','dev','1');expect(noticeKey('https://two.test','dev','1')).not.toBe(key);expect(noticeKey('https://one.test','other','1')).not.toBe(key);expect(noticeKey('https://one.test','dev','2')).not.toBe(key);});
 it('reopens disclosure when provider, cloud status or scope changes, without reacting to reorder',()=>{const a={providers:['Laya','Local LLM'],cloud:false,channel_ids:['2','1']};const sig=noticeSignature(a);expect(noticeSignature({...a,providers:['Local LLM','Laya'],channel_ids:['1','2']})).toBe(sig);expect(noticeSignature({...a,cloud:true})).not.toBe(sig);expect(noticeSignature({...a,providers:['Cloud']})).not.toBe(sig);expect(noticeSignature({...a,channel_ids:null})).not.toBe(sig);expect(noticeSignature(null)).toBe('');});
 it('bounds panel height for dragging, keyboard resize and smaller viewports',()=>{expect(panelHeight(20,800)).toBe(180);expect(panelHeight(900,800)).toBe(650);expect(panelHeight(500,400)).toBe(250);expect(panelHeight(NaN,800)).toBe(360);});
});
