<script lang="ts">
 import type{Attachment}from'./lib/api';import{bytes,mediaUrl}from'./lib/media';
 export let attachments:Attachment[]=[];export let origin:string;export let disabled=false;export let inComposer=false;
</script>
{#if attachments.length}<div class="attachment-tray" class:composer-attachments={inComposer} aria-label="Pending attachments">{#each attachments as attachment (attachment.id)}<div class="pending-attachment">{#if attachment.kind==='image'}<img src={mediaUrl(attachment.thumbnail_url||attachment.url,origin)} alt={attachment.filename}/>{/if}<div><strong>{attachment.voice?'Voice message':attachment.filename}</strong><span>{bytes(attachment.size)}</span></div><button type="button" class="icon-button" {disabled} aria-label={`Remove ${attachment.filename}`} onclick={()=>attachments=attachments.filter(item=>item.id!==attachment.id)}>×</button></div>{/each}</div>{/if}
<style>.composer-attachments{margin:0!important;padding:12px;border-bottom:1px solid #3b4f58}</style>
