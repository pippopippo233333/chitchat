<script lang="ts">
  import { Api } from './lib/api';
  import type { CallLimits, HostResources } from './lib/resources';
  export let api: Api;
  export let limits: CallLimits;
  export let onSaved: (value: HostResources) => void;
  let enabled = limits.enabled, maxCalls = limits.max_calls, maxPeople = limits.max_people, busy = false, error = '', saved = false;
  $: changed = enabled !== limits.enabled || maxCalls !== limits.max_calls || maxPeople !== limits.max_people;
  async function save(event: SubmitEvent) {
    event.preventDefault(); busy = true; error = ''; saved = false;
    try { const value = await api.request<HostResources>('/host/resources', 'PATCH', { calls: { enabled, max_calls: Number(maxCalls), max_people: Number(maxPeople) } }); limits = value.calls ?? limits; saved = true; onSaved(value); }
    catch (cause) { error = cause instanceof Error ? cause.message : 'Saving failed.'; }
    finally { busy = false; }
  }
</script>

<h3 class="section-heading">Private calls</h3>
<p class="small muted">Voice calls in direct messages, for every community on this host. Each person in a call uses about as much upload as one person in a voice room, so these limits keep calls from overloading the host.</p>
<form onsubmit={save}><fieldset disabled={busy}>
  <label class="check-label"><input type="checkbox" bind:checked={enabled}/>Allow private calls</label>
  {#if enabled}<label>Calls at the same time<input type="number" min="1" max="100" step="1" required bind:value={maxCalls}/></label>
  <label>People in one call (including invited people)<input type="number" min="2" max="25" step="1" required bind:value={maxPeople}/></label>{/if}
  {#if error}<p class="danger small" role="alert">{error}</p>{:else if saved && !changed}<p class="small muted" role="status">Saved. New limits apply to calls started from now on.</p>{/if}
  <button class="primary" disabled={!changed}>{busy ? 'Saving…' : 'Save call limits'}</button>
</fieldset></form>
