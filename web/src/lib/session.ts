import { sessionKey } from './api';
type Store = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>;
function read(store: Store, key: string) { try { return store.getItem(key); } catch { return null; } }
function remove(store: Store, key: string) { try { store.removeItem(key); } catch { /* Storage may be disabled by browser policy. */ } }
export function readSession(origin: string, slug: string, persistent: Store = localStorage, temporary: Store = sessionStorage) { const key = sessionKey(origin, slug); return read(temporary, key) ?? read(persistent, key) ?? ''; }
export function clearSession(origin: string, slug: string, persistent: Store = localStorage, temporary: Store = sessionStorage) { const key = sessionKey(origin, slug); remove(persistent, key); remove(temporary, key); }
export function saveSession(origin: string, slug: string, token: string, remember: boolean, persistent: Store = localStorage, temporary: Store = sessionStorage) {
  const key = sessionKey(origin, slug); clearSession(origin, slug, persistent, temporary);
  try { (remember ? persistent : temporary).setItem(key, token); return true; }
  catch { if (remember) { try { temporary.setItem(key, token); } catch { /* Current connection still works without browser storage. */ } } return false; }
}
