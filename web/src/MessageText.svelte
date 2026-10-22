<script lang="ts">
  import YouTubeCard from './YouTubeCard.svelte';
  import { youtubeVideos } from './lib/youtube';
  import { messageLinks } from './lib/links';
  export let content: string;
  export let onLink: (href: string) => void;
  $: parts = messageLinks(content);
  $: videos = youtubeVideos(content);
</script>

{#each parts as part}{#if part.href}<button type="button" role="link" class="message-link" onclick={() => onLink(part.href!)}>{part.text}</button>{:else}{part.text}{/if}{/each}

{#each videos as video (video.embed)}<YouTubeCard {video}/>{/each}
