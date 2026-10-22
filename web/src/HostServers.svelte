<script lang="ts">
  import { Api, ApiError, deviceId } from './lib/api';
  import type { ServerSummary } from './lib/api';
  import type { HostServer, CreatedServer } from './lib/host-servers';
  import { validSlug, suggestSlug } from './lib/host-servers';
  export let api: Api;
  export let servers: HostServer[];
  export let onCreated: (result: CreatedServer) => void;
  export let onOpen: (server: ServerSummary) => void;
  export let onError: (error: unknown) => void;
  let name='',slug='',edited=false,busy=false,error='';
  $: link=`${api.origin}/s/${slug}`;
  async function create(event:SubmitEvent) {
    event.preventDefault();if(busy||!validSlug(slug))return;busy=true;error='';
    try{const result=await api.request<CreatedServer>('/host/servers','POST',{slug,name:name.trim()||slug,device_id:deviceId()});onCreated(result);}
    catch(cause){error=cause instanceof ApiError&&cause.code==='slug_taken'?'That address is already in use. Choose another.':cause instanceof ApiError&&cause.code==='too_many_servers'?'This host has reached its limit of 50 servers.':cause instanceof Error?cause.message:'Could not create the server.';onError(cause);}
    finally{busy=false;}
  }
</script>
<h3>Servers on this host</h3><p class="small muted">Host administrator · {api.origin} · {servers.length}/50 servers. Includes unlisted servers.</p>
{#if error}<p class="error" role="alert">{error}</p>{/if}
<form onsubmit={create}>
  <h3>Create server</h3>
  <label>Server name<input value={name} maxlength="100" required oninput={event=>{name=event.currentTarget.value;if(!edited)slug=suggestSlug(name);}}/></label>
  <label>Server address<input value={slug} pattern={'[a-z0-9\\-]{2,32}'} minlength="2" maxlength="32" required oninput={event=>{slug=event.currentTarget.value;edited=true;}}/></label>
  <p class="small muted">2–32 lowercase letters, numbers or hyphens. Link: <span class="server-link">{link}</span></p>
  <p class="small muted">You will own the new server and be signed in with your current username and password.</p>
  <button class="primary" disabled={busy||!validSlug(slug)||servers.length>=50}>{busy?'Creating…':'Create server'}</button>
</form>
{#each servers as server (server.slug)}<div class="settings-row"><div><strong>{server.name}</strong><p class="small muted">/s/{server.slug} · {server.member_count} members · {server.listed?'Listed':'Unlisted'} · {server.has_owner?'Owner assigned':'No owner yet'}</p><p class="small muted">Created {new Date(server.created_at).toLocaleDateString()}</p></div><button class="secondary" disabled={busy} onclick={()=>onOpen(server)}>Open server</button></div>{/each}
<style>.server-link{overflow-wrap:anywhere}form{margin:18px 0 24px}</style>
