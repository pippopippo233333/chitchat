import type { ServerSummary } from './api';
export interface DiscoveryServer extends Omit<ServerSummary,'icon_url'> { url:string; host_url:string; host_name:string; local:boolean; last_seen:number|null; member_count:number }
export interface DiscoveryBrowse { servers:DiscoveryServer[]; hosts:number; updated_at:number|null }
export interface DiscoveryConfig { enabled:boolean; public_url:string|null; seeds:string[]; seed_lists:string[]; blocked:string[] }
export interface DiscoveryStatus { config:DiscoveryConfig; hosts:{url:string;name:string;servers:number;last_ok:number|null;fails:number}[];last_round:number|null }
export function filterServers(servers:DiscoveryServer[],query:string) { const text=query.trim().toLocaleLowerCase();return servers.filter(server=>`${server.name} ${server.description} ${server.host_name}`.toLocaleLowerCase().includes(text)); }
export function discoveryDestination(server:DiscoveryServer,origin:string) {
 try{const target=new URL(server.url),host=new URL(server.host_url);if(!['http:','https:'].includes(target.protocol)||target.username||target.password||target.search||target.hash||host.username||host.password||host.search||host.hash||!['','/'].includes(host.pathname)||target.origin!==host.origin||!(/^[a-z0-9-]{2,32}$/).test(server.slug)||target.pathname!==`/s/${server.slug}`)return null;
 return{url:target.href,host:host.origin,local:server.local&&target.origin===new URL(origin).origin};}catch{return null;}
}
export const lines=(value:string)=>[...new Set(value.split(/\r?\n/).map(item=>item.trim()).filter(Boolean))];
export const seenAt=(value:number|null)=>value?new Date(value).toLocaleString():'Not checked yet';
