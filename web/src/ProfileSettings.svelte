<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { Api, User } from './lib/api';
  import { uploadAvatar } from './lib/profile';
  import Avatar from './Avatar.svelte';
  export let api: Api;
  export let user: User & { status?: 'online' | 'away'; status_text?: string };
  export let onSaved: (user: User) => void;
  export let onError: (error: unknown) => void = () => {};
  let displayName = user.display_name;
  let status = user.status ?? 'online';
  let statusText = user.status_text ?? '';
  let picture: File | null = null;
  let preview = '';
  let removePicture = false;
  let busy = false;
  let error = '';
  let saved = false;
  function clearPreview() { if (preview) URL.revokeObjectURL(preview); preview = ''; }
  onDestroy(clearPreview);
  function choose(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0]; input.value = '';
    if (!file) return;
    if (file.size > 2 * 1024 * 1024) { error = 'Profile pictures can be at most 2 MB.'; return; }
    if (!['image/png', 'image/jpeg', 'image/webp', 'image/gif'].includes(file.type)) { error = 'Choose a PNG, JPEG, GIF or WebP picture.'; return; }
    clearPreview(); picture = file; preview = URL.createObjectURL(file); removePicture = false; error = ''; saved = false;
  }
  async function save(event: SubmitEvent) {
    event.preventDefault(); if (busy) return;
    busy = true; error = ''; saved = false;
    try {
      let updated = await api.request<User>('/me', 'PATCH', { display_name: displayName.trim(), status, status_text: statusText.trim() });
      onSaved(updated);
      if (picture) updated = await uploadAvatar(api, picture);
      else if (removePicture) updated = await api.request<User>('/me/avatar', 'DELETE');
      user = updated; onSaved(updated); picture = null; removePicture = false; clearPreview(); saved = true;
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Could not save your profile.'; onError(cause); }
    finally { busy = false; }
  }
</script>

<h2 id="dialog-title">Your profile</h2>
<p class="muted small">Your picture and status on this server.</p>
<form onsubmit={save}>
  <div class="picture-row">
    {#if preview}<img class="picture-preview" src={preview} alt="Preview of your selection"/>{:else}<Avatar user={{ ...user, avatar_url: removePicture ? null : user.avatar_url }} origin={api.origin} size={72}/>{/if}
    <div><label class="secondary picture-button">Choose picture<input type="file" accept="image/png,image/jpeg,image/gif,image/webp" disabled={busy} onchange={choose}/></label>
    {#if user.avatar_url || picture}<button type="button" class="secondary" disabled={busy} onclick={() => { picture = null; clearPreview(); removePicture = true; saved = false; }}>Remove picture</button>{/if}</div>
  </div>
  <p class="muted small">Up to 2 MB and 4096 pixels per side. Saved as a small still picture.</p>
  <label>Display name<input bind:value={displayName} required maxlength="32" disabled={busy}/></label>
  <label>Status<select bind:value={status} disabled={busy}><option value="online">Online</option><option value="away">Away</option></select></label>
  <label>Custom status<input bind:value={statusText} maxlength="128" placeholder="What are you up to?" disabled={busy}/></label>
  <p class="muted small">You appear offline when all your connections to this server close. Your status choice is kept for next time.</p>
  {#if error}<div class="error" role="alert">{error}</div>{/if}
  {#if saved}<div class="notice" role="status">Profile saved.</div>{/if}
  <button class="primary" disabled={busy}>{busy ? 'Saving…' : 'Save profile'}</button>
</form>

<style>
  .picture-row{display:flex;align-items:center;gap:16px;margin-block:18px}.picture-row>div{display:flex;flex-wrap:wrap;gap:8px}.picture-preview{width:72px;height:72px;object-fit:cover;border-radius:14px}
  .picture-button{position:relative;display:inline-flex;align-items:center;cursor:pointer}.picture-button input{position:absolute;inset:0;width:100%;height:100%;opacity:0;cursor:pointer}
</style>
