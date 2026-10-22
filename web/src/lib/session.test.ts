import { describe, expect, it } from 'vitest';
import { readSession, saveSession, clearSession } from './session';
import { sessionKey } from './api';
function store() { const entries = new Map<string,string>(); return { getItem: (key:string) => entries.get(key) ?? null, setItem: (key:string,value:string) => { entries.set(key,value); }, removeItem: (key:string) => { entries.delete(key); } }; }
describe('remembered sessions', () => {
  it('survives a new tab only when keep signed in is selected', () => {
    const persistent = store(), tab = store();
    saveSession('https://chat.example', 'dev', 'remembered', true, persistent, tab);
    expect(readSession('https://chat.example','dev',persistent,store())).toBe('remembered');
    saveSession('https://chat.example','dev','temporary',false,persistent,tab);
    expect(readSession('https://chat.example','dev',persistent,tab)).toBe('temporary');
    expect(readSession('https://chat.example','dev',persistent,store())).toBe('');
  });
  it('reads legacy temporary sessions and keeps hosts and communities isolated', () => {
    const persistent = store(), tab = store(); tab.setItem(sessionKey('https://chat.example','dev'),'legacy');
    expect(readSession('https://chat.example','dev',persistent,tab)).toBe('legacy');
    expect(readSession('https://elsewhere.example','dev',persistent,tab)).toBe('');
    expect(readSession('https://chat.example','friends',persistent,tab)).toBe('');
  });
  it('clears both stores on logout or expiry without clearing another community', () => {
    const persistent = store(), tab = store(), key = sessionKey('https://chat.example','dev');
    persistent.setItem(key,'old');tab.setItem(key,'new');persistent.setItem(sessionKey('https://chat.example','other'),'keep');
    clearSession('https://chat.example','dev',persistent,tab);
    expect(readSession('https://chat.example','dev',persistent,tab)).toBe('');
    expect(readSession('https://chat.example','other',persistent,tab)).toBe('keep');
  });
  it('falls back to tab storage if persistent storage is unavailable', () => {
    const unavailable = { getItem: () => {throw Error('blocked');}, setItem: () => {throw Error('blocked');}, removeItem: () => {throw Error('blocked');} }, tab = store();
    expect(saveSession('https://chat.example','dev','token',true,unavailable,tab)).toBe(false);
    expect(readSession('https://chat.example','dev',unavailable,tab)).toBe('token');
    expect(() => clearSession('https://chat.example','dev',unavailable,tab)).not.toThrow();
  });
});
