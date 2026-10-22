<script lang="ts">
  import {storageLabel,GB} from './lib/resources';
  export let label: string;
  export let value: number | null;
  export let limit: number | null;
  export let unit = 'GB';
  export let detail = '';
  $: known = value !== null && Number.isFinite(value);
  $: bounded = limit !== null && Number.isFinite(limit) && limit > 0;
  $: fraction = known && bounded ? Math.max(0, Math.min(1, value! / limit!)) : 0;
  $: tone = fraction >= 0.9 ? '#ffaaa2' : fraction >= 0.7 ? '#f2cf7d' : '#a4e4c2';
  $: used = known ? unit==='GB'?storageLabel(value!*GB):`${value!.toLocaleString(undefined,{maximumFractionDigits:2})} ${unit}` : 'Unavailable';
  $: capacity = bounded ? unit==='GB'?storageLabel(limit!*GB):`${limit!.toLocaleString(undefined,{maximumFractionDigits:2})} ${unit}` : '';
</script>
<figure class="resource-gauge">
  <figcaption>{label}</figcaption>
  <svg viewBox="0 0 200 116" role="img" aria-label={`${label}: ${known ? used : 'unavailable'}${bounded ? ` out of ${capacity}` : ', no limit set'}`}>
    <path d="M20 100 A80 80 0 0 1 180 100" fill="none" stroke="currentColor" opacity="0.13" stroke-width="13" stroke-linecap="round"/>
    <path d="M20 100 A80 80 0 0 1 180 100" fill="none" stroke={tone} stroke-width="13" stroke-linecap="round" pathLength="100" stroke-dasharray={`${fraction * 100} 100`} opacity={fraction ? 1 : 0}/>
    <line x1="100" y1="100" x2="100" y2="39" stroke={tone} stroke-width="3" stroke-linecap="round" transform={`rotate(${-90 + fraction * 180} 100 100)`}/>
    <circle cx="100" cy="100" r="6" fill={tone}/>
    <text x="12" y="115">0</text><text x="188" y="115" text-anchor="end">{bounded ? capacity : 'No limit'}</text>
  </svg>
  <p><strong>{used}</strong>{bounded ? ` out of ${capacity}` : ' · No limit set'}</p>
  {#if detail}<p class="small muted">{detail}</p>{/if}
</figure>
<style>
  .resource-gauge{margin:0;padding:20px;border:1px solid #ffffff16;border-radius:16px;background:#ffffff03;text-align:center;min-width:0}.resource-gauge figcaption{font-weight:650;text-align:left;margin-bottom:12px}.resource-gauge svg{display:block;width:100%;max-width:280px;margin:0 auto;overflow:visible}.resource-gauge svg text{fill:currentColor;opacity:.65;font-size:10px}.resource-gauge p{margin:12px 0 0;font-size:13px}.resource-gauge p strong{font-size:18px}.resource-gauge path{transition:stroke-dasharray .5s ease,stroke .5s ease}@media(prefers-reduced-motion:reduce){.resource-gauge path{transition:none}}
</style>
