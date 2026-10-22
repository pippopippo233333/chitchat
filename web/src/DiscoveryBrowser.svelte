<script lang="ts">
 import {onMount,onDestroy} from 'svelte';import {Api,ApiError} from './lib/api';import type{ServerSummary}from'./lib/api';
 import type{DiscoveryBrowse,DiscoveryServer}from'./lib/discovery';import{filterServers,discoveryDestination,seenAt}from'./lib/discovery';
 export let origin:string;export let onRemote:(url:string)=>void;export let onLocal:(server:ServerSummary)=>void;
 let data:DiscoveryBrowse|null=null,query='',loading=false,error='',off=false;let pending:ReturnType<typeof discoveryDestination>=null;const abort=new AbortController();
 $: results=filterServers(data?.servers??[],query);
 async function load(){loading=true;error='';off=false;try{data=await new Api(origin).request<DiscoveryBrowse>('/api/discovery/browse','GET',undefined,abort.signal,true);}catch(cause){if(abort.signal.aborted)return;if(cause instanceof ApiError&&cause.code==='discovery_off')off=true;else error=cause instanceof Error?cause.message:'Could not load servers.';}finally{loading=false;}}
 function open(server:DiscoveryServer){const target=discoveryDestination(server,origin);if(!target)return;if(target.local)onLocal({...server,icon_url:null});else pending=target;}
 onMount(()=>{void load();});onDestroy(()=>abort.abort());
</script>
<h2 id="dialog-title">Browse servers</h2><p>Communities shared by connected hosts. Accounts belong to each community.</p>
{#if pending}<div class="confirm-box" role="alert"><h3>Open another host?</h3><p>Connect to {pending.host} from {origin}? This is another host with separate accounts.</p><p class="destination">{pending.url}</p>{#if pending.url.startsWith('http:')}<p class="small danger">This address uses an unencrypted HTTP connection.</p>{/if}<div class="button-row"><button class="secondary" onclick={()=>pending=null}>Stay here</button><button class="primary" onclick={()=>{onRemote(pending!.url);pending=null;}}>Connect in ChitChat</button><a class="secondary" href={pending.url} target="_blank" rel="noopener noreferrer" onclick={()=>pending=null}>Open server in new tab</a></div></div>{/if}
<label>Search servers<input type="search" bind:value={query} placeholder="Name, description or host"/></label>
<button class="secondary" disabled={loading} onclick={load}>{loading?'Refreshing…':'Refresh servers'}</button>
{#if off}<p class="notice">The server browser is off on this host.</p>{:else if error}<p class="error" role="alert">{error}</p>{:else if data}
 <p class="small muted">{results.length} servers · {data.hosts} hosts · Updated {seenAt(data.updated_at)}</p>
 {#each results as server (`${server.host_url}:${server.slug}`)}{@const target=discoveryDestination(server,origin)}<article class="browse-card"><h3>{server.name}</h3><p>{server.description||'No description provided.'}</p><p class="small muted">{server.host_name} · {server.member_count} members · {target?.local?'This host':server.host_url}</p><div class="badges">{#if server.requires_password}<span>Password required</span>{/if}{#if server.invite_only}<span>Invite required</span>{/if}{#if !server.registration_open}<span>Registration closed</span>{/if}</div><p class="small muted">Last seen: {seenAt(server.last_seen)}</p><button class="secondary" disabled={!target} onclick={()=>open(server)}>Open {server.name}</button>{#if !target}<p class="small danger">This host supplied an invalid server address.</p>{/if}</article>{/each}
 {#if !results.length}<p class="notice">{query?'No servers match your search.':'No servers shared yet. The host administrator can add discovery seeds.'}</p>{/if}
{/if}
<style>.browse-card{margin:14px 0;padding:16px;border:1px solid #3a4a53;border-radius:12px;overflow-wrap:anywhere}.browse-card h3{margin:0 0 8px}.browse-card p{margin:8px 0}.badges{display:flex;flex-wrap:wrap;gap:6px}.badges span{font-size:11px;border:1px solid #52655e;padding:4px 7px;border-radius:12px}.destination{overflow-wrap:anywhere}</style>
