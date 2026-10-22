import type { ServerSummary, User } from './api';
export interface HostServer extends ServerSummary { listed: boolean; has_owner: boolean; member_count: number; created_at: string | number }
export interface CreatedServer { server: ServerSummary; token: string; user: User }
export const validSlug = (value: string) => /^[a-z0-9-]{2,32}$/.test(value);
export function suggestSlug(name: string) { return name.normalize('NFKD').replace(/[\u0300-\u036f]/g,'').toLowerCase().replace(/[^a-z0-9]+/g,'-').replace(/^-|-$/g,'').slice(0,32).replace(/-$/,''); }
