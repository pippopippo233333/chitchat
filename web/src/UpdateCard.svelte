<script lang="ts">
 import{onMount,onDestroy}from'svelte';import{Api,ApiError}from'./lib/api';
 export let api:Api;
 interface Status{current:string;platform:string;latest:{version:string;notes:string;published_at:string}|null;check_error:string|null;applying:boolean;apply_error:string|null;last_result:{ok:boolean;message:string}|null}
 let data:Status|null=null,loading=true,busy=false,hidden=false,confirming=false,error='',progress='',alive=true;
 const abort=new AbortController();let timer:ReturnType<typeof setTimeout>|undefined;
 async function refresh(){busy=true;try{const result=await api.request<Status>('/host/update','GET',undefined,abort.signal);if(alive){data=result;error='';}}catch(cause){if(!alive)return;if(cause instanceof ApiError&&cause.code==='host_admin_only')hidden=true;else error=cause instanceof Error?cause.message:'Could not check for updates.';}finally{if(alive){busy=false;loading=false;}}}
 // While the host restarts, its address stops answering for a moment; it's done when it reports the new version.
 async function waitFor(version:string,started:number){if(!alive)return;try{const res=await fetch(api.origin+'/api/host',{cache:'no-store',signal:abort.signal});const info=await res.json();if(info?.version===version){progress='';busy=false;await refresh();return;}}catch{/* restarting */}
  if(Date.now()-started>5*60000){progress='';busy=false;error='The host has not come back with the new version yet. Check it on the host machine.';await refresh();return;}
  if(Date.now()-started>20000&&!data?.applying)try{const s=await api.request<Status>('/host/update','GET',undefined,abort.signal);if(alive&&!s.applying&&s.apply_error){data=s;progress='';busy=false;return;}}catch{/* restarting */}
  timer=setTimeout(()=>void waitFor(version,started),3000);}
 async function apply(){if(!data?.latest)return;confirming=false;busy=true;error='';const version=data.latest.version;try{await api.request('/host/update','POST',undefined,abort.signal);progress=`Installing ${version}. The host restarts, so ChitChat is offline for about a minute…`;void waitFor(version,Date.now());}catch(cause){if(alive){busy=false;error=cause instanceof Error?cause.message:'The update could not start.';}}}
 onMount(()=>void refresh());onDestroy(()=>{alive=false;abort.abort();clearTimeout(timer);});
</script>
{#if !hidden}<section class="update-card" aria-label="Host updates">
 <div class="update-heading"><h3>Updates</h3>{#if data}<span class="small muted">ChitChat host {data.current} · {data.platform}</span>{/if}</div>
 {#if loading}<p role="status">Checking for updates…</p>{/if}
 {#if error}<p class="error" role="alert">{error}</p>{/if}
 {#if data}
  {#if data.last_result}<p class={data.last_result.ok?'notice':'error'} role="status">{data.last_result.message}</p>{/if}
  {#if data.apply_error}<p class="error" role="alert">The update failed and nothing was changed: {data.apply_error}</p>{/if}
  {#if progress}<p class="notice" role="status">{progress}</p>
  {:else if data.latest}
   <p><strong>Version {data.latest.version} is available.</strong></p>
   {#if data.latest.notes}<p class="small notes">{data.latest.notes}</p>{/if}
   <p class="small muted">Every community's database is backed up first. If the new version fails to start, the old one comes back by itself.</p>
   {#if confirming}<div class="confirm-box" role="alert"><p>Install {data.latest.version} now? Everyone is disconnected for about a minute while the host restarts, then reconnects automatically.</p><div class="button-row"><button class="secondary" onclick={()=>confirming=false}>Cancel</button><button class="primary" disabled={busy} onclick={apply}>Update and restart</button></div></div>
   {:else}<div class="button-row"><button class="primary" disabled={busy||data.applying} onclick={()=>confirming=true}>{data.applying?'Installing…':'Update now'}</button></div>{/if}
  {:else if data.check_error}<p class="small muted">{data.check_error}</p>
  {:else}<p class="small">This host is up to date.</p>{/if}
 {/if}
 {#if !progress}<div class="button-row"><button class="secondary" disabled={busy} onclick={refresh}>Check again</button></div>{/if}
</section>{/if}
<style>.update-card{margin:22px 0;padding:20px;border:1px solid #3a4a53;border-radius:14px;background:#1d3036}.update-heading{display:flex;flex-wrap:wrap;align-items:center;gap:10px;justify-content:space-between;margin-bottom:12px}.update-card p{margin:8px 0}.notes{white-space:pre-wrap;max-height:160px;overflow:auto;color:#c0cfd5}.button-row{display:flex;gap:8px;flex-wrap:wrap;margin-top:10px}</style>
