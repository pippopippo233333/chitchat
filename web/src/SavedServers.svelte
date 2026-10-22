<script lang="ts">
 import type{SavedServer,ServerTarget}from'./lib/saved-servers';import{parseServerLink,serverIdentity}from'./lib/saved-servers';
 export let servers:SavedServer[];export let busy:boolean;export let error:string;export let onOpen:(server:SavedServer)=>void;export let onRemove:(server:SavedServer)=>void;export let onConnect:(target:ServerTarget)=>void;
 let link='',validation='',remove:SavedServer|null=null;
 function connect(event:SubmitEvent){event.preventDefault();const target=parseServerLink(link);if(!target){validation='Paste a server link such as https://host.example/s/community, or an invite link.';return;}validation='';onConnect(target);}
</script>
<h2 id="dialog-title">Your servers</h2><p>Only communities you choose or join appear here. Your list can include different hosts and is saved on this device.</p>
{#if error||validation}<p class="error" role="alert">{error||validation}</p>{/if}
<form onsubmit={connect}><label>Add a server by link<input type="url" bind:value={link} required placeholder="https://host.example/s/community"/></label><p class="small muted">Each host has separate accounts. Sign in or register there to add the community to your list.</p><button class="primary" disabled={busy}>{busy?'Connecting…':'Connect to server'}</button></form>
{#if remove}<div class="confirm-box"><h3>Remove {remove.name}?</h3><p>This signs you out of that community and removes it from this device’s list. Your account and messages remain on its host.</p><div class="button-row"><button class="secondary" disabled={busy} onclick={()=>remove=null}>Keep server</button><button class="secondary danger" disabled={busy} onclick={()=>{onRemove(remove!);remove=null;}}>Remove and sign out</button></div></div>{/if}
{#each servers as server (serverIdentity(server))}<div class="settings-row"><div><strong>{server.name}</strong><p class="small muted">{server.origin}/s/{server.slug}</p></div><div class="button-row"><button class="secondary" disabled={busy} onclick={()=>onOpen(server)}>Open {server.name}</button><button class="secondary danger" disabled={busy} aria-label={`Remove ${server.name} from your list`} onclick={()=>remove=server}>Remove</button></div></div>{/each}
{#if !servers.length}<p class="notice">Your list is empty. Paste a link above, or browse a host and join a community.</p>{/if}
<style>form{margin:20px 0}.settings-row p{overflow-wrap:anywhere}.button-row{flex-shrink:0}</style>
