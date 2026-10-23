<script lang="ts">
  import YouTubeCard from './YouTubeCard.svelte';
  import { youtubeVideos } from './lib/youtube';
  import { messageLinks } from './lib/links';
  import type{Member,Role,Channel,CustomEmoji}from'./lib/api';import{mentionParts}from'./lib/social';
  import { customParts } from './lib/custom-emoji';
  export let emojis: CustomEmoji[] = [];
  export let origin = location.origin;
  export let members:Member[]=[];export let roles:Role[]=[];export let channels:Channel[]=[];export let selfId='';export let onMember:((id:string)=>void)|undefined=undefined;export let onChannel:((id:string)=>void)|undefined=undefined;
  export let content: string;
  export let onLink: (href: string) => void;
  $: parts = messageLinks(content);
  $: videos = youtubeVideos(content);
</script>

{#each parts as part}{#if part.href}<button type="button" role="link" class="message-link" onclick={() => onLink(part.href!)}>{part.text}</button>{:else}{#each mentionParts(part.text,members,roles,channels) as token}{#if token.kind==='user'&&onMember}<button type="button" class="mention" class:self-mention={token.id===selfId} onclick={()=>onMember(token.id!)}>{token.text}</button>{:else if token.kind==='channel'&&onChannel}<button type="button" class="mention" onclick={()=>onChannel(token.id!)}>{token.text}</button>{:else if token.kind}<span class="mention">{token.text}</span>{:else}{#each customParts(token.text,emojis,origin) as part}{#if part.url}<img class="inline-emoji" src={part.url} alt={`:${part.name}:`} title={`:${part.name}:`} loading="lazy"/>{:else}{part.text}{/if}{/each}{/if}{/each}{/if}{/each}

{#each videos as video (video.embed)}<YouTubeCard {video}/>{/each}

<style>.mention{color:#b8ead3;background:#34524a;border-radius:4px;padding:1px 4px;font:inherit;white-space:normal}.self-mention{color:#1b352a;background:#b3eacb}.inline-emoji{display:inline-block;width:1.6em;height:1.6em;object-fit:contain;vertical-align:middle}</style>
