<script lang="ts">
  import { PendingUploads } from './lib/pending-uploads';
  import PendingAttachments from './PendingAttachments.svelte';
  import { onMount, onDestroy } from 'svelte';
  import { Api, ApiError } from './lib/api';
  import type { Attachment, StorageUsage } from './lib/api';
  import { bytes, mediaUrl, allowedUpload, uploadAccept } from './lib/media';
  export let api: Api;
  export let channel: string;
  export let enabled: boolean;
  export let blocked=false;
  export let showTray=true;
  export let attachments: Attachment[] = [];
  export let uploading = false;
  export let onError: (error: unknown) => void;
  let input: HTMLInputElement;
  let usage: StorageUsage | null = null;
  let progress = 0, currentName = '', error = '';
  let request: XMLHttpRequest | null = null;
  let stopped = false, cleanupFailed=false;
  const uploadApi=api;
  const cleanup=new PendingUploads(async id=>{
    const response=await fetch(`${uploadApi.origin}${uploadApi.prefix}/attachments/${encodeURIComponent(id)}`,{method:'DELETE',credentials:'omit',keepalive:true,headers:{Authorization:`Bearer ${uploadApi.token}`}});
    if(!response.ok&&response.status!==404)throw new Error('Could not remove an unsent upload. Retry cleanup to release its storage quota.');
  },cause=>{if(!stopped){cleanupFailed=true;error=cause instanceof Error?cause.message:'Could not clean up an unsent upload.';}});
  let previousIds='';
  $: {const ids=attachments.map(attachment=>attachment.id);const key=ids.join(',');if(key!==previousIds){previousIds=key;cleanup.sync(ids);if(!stopped)void cleanup.flush().then(()=>{if(!stopped)void refreshUsage();});}}

  export async function discardPending(){cleanup.clear();attachments=[];await cleanup.flush();}
  export function holdForSend(ids:string[]){cleanup.hold(ids);}
  export function finishSend(ids:string[],sent:boolean){cleanup.finish(ids,sent);}
  async function retryCleanup(){await cleanup.flush();cleanupFailed=cleanup.queuedCount>0;if(!cleanupFailed){error='';void refreshUsage();}}
  function leavingPage(){request?.abort();cleanup.clear();attachments=[];}

  async function refreshUsage() { try { const result = await api.request<StorageUsage>('/me/storage'); if (!stopped) usage = result; } catch (cause) { if (!stopped) onError(cause); } }
  export async function addFiles(files: File[], voice = false) {
    if (!enabled || uploading || stopped || (blocked && !voice)) return 0;
    if (files.length + attachments.length > 10) { error = 'A message can have up to 10 files.'; return 0; }
    let added=0;
    error = ''; uploading = true;
    try {
      for (const file of files) {
        if (!enabled || stopped) break;
        if (!allowedUpload(file.name)) throw new Error('Choose an image, video, audio (MP3, OGG, WAV, FLAC, M4A), or a TXT, Markdown, CSV, LOG, JSON or PDF file.');
        if (usage && file.size > usage.max_file_mb * 1048576) throw new Error(`This file exceeds the ${usage.max_file_mb} MB file limit.`);
        currentName = file.name; progress = 0;
        const attachment = await new Promise<Attachment>((resolve, reject) => {
          const xhr = new XMLHttpRequest(); request = xhr;
          xhr.open('POST', `${api.origin}${api.prefix}/channels/${encodeURIComponent(channel)}/attachments?filename=${encodeURIComponent(file.name)}${voice ? '&voice=1' : ''}`);
          xhr.setRequestHeader('Authorization', `Bearer ${api.token}`); xhr.setRequestHeader('Content-Type', 'application/octet-stream'); xhr.timeout = 120000;
          xhr.upload.onprogress = event => { if (event.lengthComputable && !stopped) progress = Math.round(event.loaded / event.total * 100); };
          xhr.onload = () => {
            let data: any; try { data = JSON.parse(xhr.responseText); } catch { reject(new Error('The host returned an invalid upload response.')); return; }
            if (xhr.status < 200 || xhr.status >= 300) { const info = data.error; const detail = info?.quota ? ` (${info.quota} quota${info.limit_mb !== undefined ? `: ${info.limit_mb} MB` : ''})` : ''; reject(new ApiError(xhr.status, info?.code ?? 'upload_failed', (info?.message ?? 'Upload failed.') + detail, info?.retry_after_ms, info?.until)); }
            else resolve(data);
          };
          xhr.onerror = () => reject(new Error('Upload failed. Check your connection.')); xhr.ontimeout = () => reject(new Error('The upload timed out. Try a smaller file.')); xhr.onabort = () => reject(new DOMException('Upload cancelled.', 'AbortError'));
          xhr.send(file);
        });
        cleanup.add(attachment.id);
        if (!stopped) { attachments = [...attachments, attachment]; added++; }
      }
    } catch (cause) { if (!stopped) { error = cause instanceof Error ? cause.message : 'Upload failed.'; if (!(cause instanceof DOMException && cause.name === 'AbortError')) onError(cause); } }
    finally { if (!stopped) { uploading = false; currentName = ''; request = null; if (input) input.value = ''; void refreshUsage(); } }
    return added;
  }
  onMount(() => { void refreshUsage(); window.addEventListener('pagehide',leavingPage); });
  onDestroy(() => { stopped = true; request?.abort(); cleanup.dispose(); window.removeEventListener('pagehide',leavingPage); uploading = false; });
</script>

<div class="upload-tools"><input class="visually-hidden" type="file" multiple accept={uploadAccept} bind:this={input} aria-label="Choose files to attach" onchange={event => addFiles([...(event.currentTarget.files ?? [])])}/><button class="secondary" type="button" disabled={!enabled || blocked || uploading || attachments.length >= 10} onclick={() => input.click()}>+ Attach files</button>{#if usage}<span class="small muted">{usage.max_file_mb} MB/file · {usage.exempt ? 'Owner: user quota exempt' : `${bytes(usage.used_today_bytes)} / ${usage.user_daily_mb} MB today · ${bytes(usage.used_total_bytes)} / ${usage.user_total_mb} MB total`}</span>{/if}<span class="small muted">Images, videos, audio, TXT, Markdown, CSV, LOG, JSON and PDF. Drop files or paste an image.</span></div>
{#if error}<div class="error" role="alert">{error}{#if cleanupFailed}<button type="button" class="secondary" onclick={retryCleanup}>Retry cleanup</button>{/if}</div>{/if}
{#if uploading}<div class="upload-progress" role="status"><span>{currentName} · {progress === 100 ? 'Processing…' : `${progress}%`}</span><progress max="100" value={progress} aria-label="Upload progress"></progress><button class="secondary" type="button" onclick={() => request?.abort()}>Cancel upload</button></div>{/if}
{#if showTray}<PendingAttachments origin={api.origin} bind:attachments/>{/if}
