<script lang="ts">
  import type { CustomEmoji } from './lib/api';
  import { customReaction, emojiImageUrl, reactionName } from './lib/custom-emoji';
  export let value: string;
  export let emojis: CustomEmoji[] = [];
  export let origin: string;
  $: emoji = emojis.find(item => customReaction(item) === value);
  $: url = emoji ? emojiImageUrl(emoji.url, origin) : '';
</script>
{#if url}<img src={url} alt={`:${emoji!.name}:`} loading="lazy"/>{:else}{value.startsWith('custom:') ? `:${reactionName(value, emojis).replaceAll(' ', '_')}:` : value}{/if}
<style>img{width:1.35em;height:1.35em;object-fit:contain;vertical-align:middle}</style>
