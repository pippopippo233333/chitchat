<script lang="ts">
  import AudioPlayer from './AudioPlayer.svelte';
  import AttachmentDownload from './AttachmentDownload.svelte';
  import type { Attachment } from './lib/api';
  import { duration } from './lib/recorder';
  import { bytes, mediaUrl } from './lib/media';
  export let attachments: Attachment[];
  export let origin: string;
  export let onView: (attachment: Attachment) => void;
</script>

<div class="message-media">{#each attachments as attachment (attachment.id)}
  {#if mediaUrl(attachment.url, origin)}<figure>
    {#if attachment.kind === 'image'}<button type="button" class="image-preview" aria-label={`View ${attachment.filename}`} onclick={() => onView(attachment)}><img src={mediaUrl(attachment.thumbnail_url || attachment.url, origin)} alt={attachment.filename} loading="lazy" onerror={event => { (event.currentTarget as HTMLImageElement).alt = `${attachment.filename} (expired or unavailable; reload the channel)`; }}/></button>
    {:else if attachment.kind === 'audio'}<div class="audio-message"><strong>{attachment.voice?'Voice message':attachment.filename}</strong>{#if attachment.duration_ms != null}<span class="small muted">{duration(attachment.duration_ms)}</span>{/if}<AudioPlayer src={mediaUrl(attachment.url,origin)} label={attachment.voice?'Voice message':attachment.filename} durationMs={attachment.duration_ms}/><AttachmentDownload src={mediaUrl(attachment.url,origin)} filename={attachment.filename} label={attachment.voice?'voice message':attachment.filename}/></div>
    {:else if attachment.kind === 'video'}<video controls preload="metadata" poster={mediaUrl(attachment.thumbnail_url, origin)} src={mediaUrl(attachment.url, origin)} aria-label={attachment.filename}><track kind="captions"/></video>{:else}<div class="file-download"><span aria-hidden="true">↧</span><div><strong>{attachment.filename}</strong><p class="small muted">{bytes(attachment.size)}</p></div><AttachmentDownload src={mediaUrl(attachment.url,origin)} filename={attachment.filename}/></div>{/if}
    <figcaption>{attachment.voice ? 'Voice message' : attachment.filename} · {bytes(attachment.size)}</figcaption>
  </figure>{/if}
{/each}</div>

<style>.audio-message{display:flex;gap:8px;flex-wrap:wrap;align-items:center;max-width:400px}.audio-message :global(.audio-player){max-width:400px}.audio-message strong{overflow-wrap:anywhere}</style>
