<script lang="ts">
  import { avatarUrl, type Presence } from './lib/profile';
  export let user: { id?: string; display_name: string; avatar_url?: string | null };
  export let origin: string;
  export let size = 38;
  export let status: Presence | undefined = undefined;
  let failed = false;
  $: url = avatarUrl(user.avatar_url, origin);
  $: { url; failed = false; }
  $: initials = user.display_name.trim().split(/\s+/).map(word => [...word][0]).slice(0, 2).join('').toUpperCase();
</script>

<span class="profile-avatar" style:width={`${size}px`} style:height={`${size}px`} aria-hidden="true">
  {#if url && !failed}<img src={url} alt="" loading="lazy" onerror={() => failed = true}/>{:else}<span>{initials || '?'}</span>{/if}
  {#if status}<span class={`presence ${status}`} title={status}></span>{/if}
</span>

<style>
  .profile-avatar{position:relative;display:inline-flex;align-items:center;justify-content:center;flex:none;border-radius:12px;background:var(--c-34514c,#34514c);color:var(--c-c6e9df,#c6e9df);font-weight:700;vertical-align:middle}
  img{width:100%;height:100%;object-fit:cover;border-radius:inherit}
  .presence{position:absolute;right:-2px;bottom:-2px;width:12px;height:12px;border:2px solid var(--c-1b2a30,#1b2a30);border-radius:50%;background:var(--c-778b96,#778b96)}
  .presence.online{background:var(--c-a7e4c9,#a7e4c9)}.presence.away{background:var(--c-edd28a,#edd28a)}
</style>
