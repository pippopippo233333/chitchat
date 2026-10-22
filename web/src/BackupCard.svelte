<script lang="ts">
 import{onMount,onDestroy}from'svelte';import{Api,ApiError}from'./lib/api';import type{HostBackups}from'./lib/backups';import{storageLabel}from'./lib/resources';
 export let api:Api;
 let data:HostBackups|null=null,loading=true,busy=false,hidden=false,error='',alive=true;
 let timer:ReturnType<typeof setTimeout>|undefined;const abort=new AbortController();
 const date=(value:number|null)=>value===null?'Unavailable':new Date(value).toLocaleString();
 function schedule(){clearTimeout(timer);if(alive&&data?.running)timer=setTimeout(()=>{if(document.visibilityState==='visible')void refresh();else schedule();},3000);}
 function failed(cause:unknown){if(cause instanceof ApiError&&cause.code==='host_admin_only'){hidden=true;data=null;}else error=cause instanceof ApiError&&cause.code==='backups_not_configured'?"Backups aren't set up on this host.":cause instanceof Error?cause.message:'Could not load backup status.';}
 async function refresh(){if(busy||!alive)return;busy=true;try{const result=await api.request<HostBackups>('/host/backups','GET',undefined,abort.signal);if(alive){data=result;error='';}}catch(cause){if(alive)failed(cause);}finally{if(alive){busy=false;loading=false;schedule();}}}
 async function start(){if(busy||!data?.configured||data.running)return;busy=true;error='';clearTimeout(timer);try{const result=await api.request<HostBackups>('/host/backups','POST',undefined,abort.signal);if(alive)data=result;}catch(cause){if(alive){if(cause instanceof ApiError&&cause.code==='backup_running'){busy=false;await refresh();return;}failed(cause);}}finally{if(alive){busy=false;schedule();}}}
 function visible(){if(document.visibilityState==='visible'&&data?.running)void refresh();}
 onMount(()=>{void refresh();document.addEventListener('visibilitychange',visible);});onDestroy(()=>{alive=false;abort.abort();clearTimeout(timer);document.removeEventListener('visibilitychange',visible);});
</script>
{#if !hidden}<section class="backup-card" aria-label="Host backups">
 <div class="backup-heading"><h3>Backups</h3><span class="small muted">Whole host · all communities</span></div>
 {#if loading}<p role="status">Loading backup status…</p>{/if}
 {#if error}<p class="error" role="alert">{error}</p>{/if}
 {#if data}
  {#if !data.configured}<p class="notice">Backups aren't set up on this host.</p><p class="small muted">See <code>docs/hosting.md</code> → Automatic backups for setup instructions.</p>
  {:else}
   {#if data.running}<p class="notice" role="status">Backup in progress… This status refreshes every three seconds.</p>{/if}
   {#if data.last_failed}<p class="error" role="alert">The most recent backup failed. Previous snapshots are kept. Check the host's backup logs and try again.</p>{/if}
   <div class="backup-summary"><div><strong>{data.count}</strong><span>snapshots kept</span></div><div><strong>{storageLabel(data.total_bytes)}</strong><span>disk space · shared files counted once</span></div></div>
   <p class="small">{#if data.scheduled}Automatic backups are on. Next run: <strong>{date(data.next_run)}</strong>.{:else}<span class="danger">Automatic backups are off.</span> You can still back up now.{/if}</p>
   {#if data.snapshots.length}<div class="backup-list">{#each data.snapshots as snapshot(snapshot.name)}<div class="backup-snapshot"><div><strong>{date(snapshot.created)}</strong><span class="small muted">{snapshot.name}</span></div><div><span class:danger={!snapshot.ok}>{snapshot.ok?'Verified':'Check failed'}</span><span class="small muted">{snapshot.servers} communities · {snapshot.messages} messages · {snapshot.files} files · {snapshot.seconds==null?'Duration unavailable':snapshot.seconds.toLocaleString(undefined,{maximumFractionDigits:1})+' s'}</span></div></div>{/each}</div>{:else}<p class="notice">No snapshots yet. Create the first backup below.</p>{/if}
   <p class="small muted">These copies are on the host's disk. Keep a copy on another device for protection against disk failure. Restoring is done on the host; it never replaces live data from this page.</p>
  {/if}
 {/if}
 <div class="button-row">{#if data?.configured}<button class="primary" disabled={busy||data.running} onclick={start}>{data.running?'Backing up…':'Back up now'}</button>{/if}<button class="secondary" disabled={busy} onclick={refresh}>Refresh backups</button></div>
</section>{/if}
<style>.backup-card{margin:22px 0;padding:20px;border:1px solid #3a4a53;border-radius:14px;background:#1d3036}.backup-heading{display:flex;flex-wrap:wrap;align-items:center;gap:10px;justify-content:space-between}.backup-heading h3{margin:0}.backup-summary{display:flex;flex-wrap:wrap;gap:20px;margin:16px 0}.backup-summary>div{display:flex;flex-direction:column;gap:5px}.backup-summary strong{font-size:22px}.backup-summary span{font-size:12px;color:#8cabb6}.backup-snapshot{display:flex;flex-wrap:wrap;justify-content:space-between;gap:10px;padding:12px 0;border-top:1px solid #3a4a53}.backup-snapshot>div{display:flex;flex-direction:column;gap:5px}.backup-snapshot .small{overflow-wrap:anywhere}.backup-card .button-row{margin-bottom:0}</style>
