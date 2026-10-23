<script lang="ts">
 import{onMount,tick}from'svelte';import{EMOJI_GROUPS,loadRecent,rememberEmoji,searchEmoji,type EmojiEntry}from'./lib/emoji';
 // anchor: the button that opened the picker (the picker is placed next to it). extra: server emoji, shown first.
 export let anchor:HTMLElement;export let onPick:(emoji:string)=>void;export let onClose:()=>void;export let extra:{name:string,items:EmojiEntry[]}[]=[];
 export let label='Choose a reaction';
 let root:HTMLDivElement,search:HTMLInputElement,query='',recent=loadRecent(),top=0,left=0;
 $: custom=extra.flatMap(group=>group.items);
 $: groups=[{name:'Recently used',items:recent.filter(emoji=>!emoji.startsWith('custom:')||custom.some(item=>item.emoji===emoji)).map(emoji=>custom.find(item=>item.emoji===emoji)??{emoji,words:''})},...extra,...EMOJI_GROUPS];
 $: found=[...custom.filter(item=>query.trim()&&query.toLowerCase().split(/\s+/).filter(Boolean).every(word=>item.words.toLowerCase().includes(word))),...searchEmoji(query)];
 function pick(emoji:string){recent=rememberEmoji(emoji);onPick(emoji);onClose();}
 function place(){
  const box=anchor.getBoundingClientRect(),width=root.offsetWidth,height=root.offsetHeight,gap=6;
  left=Math.max(8,Math.min(box.right-width,innerWidth-width-8));
  top=box.top-height-gap>=8?box.top-height-gap:Math.min(box.bottom+gap,innerHeight-height-8);
 }
 function outside(event:PointerEvent){if(!root.contains(event.target as Node)&&!anchor.contains(event.target as Node))onClose();}
 // Phones skip the autofocus so the keyboard doesn't cover the grid. The anchor moves when the messages scroll; scrolling inside the picker is fine.
 function scrolled(event:Event){if(!root.contains(event.target as Node))onClose();}
 function key(event:KeyboardEvent){if(event.key==='Escape'){event.preventDefault();event.stopPropagation();onClose();anchor.focus();}}
 onMount(()=>{void tick().then(()=>{place();if(matchMedia('(hover: hover)').matches)search.focus();});addEventListener('pointerdown',outside,true);addEventListener('resize',place);addEventListener('scroll',scrolled,true);return()=>{removeEventListener('pointerdown',outside,true);removeEventListener('resize',place);removeEventListener('scroll',scrolled,true);};});
</script>
<div bind:this={root} class="emoji-picker" role="dialog" aria-label={label} style:top={`${top}px`} style:left={`${left}px`} onkeydown={key} tabindex="-1">
 <input bind:this={search} bind:value={query} type="search" placeholder="Search emoji" aria-label="Search emoji"/>
 <div class="emoji-scroll">
  {#if query.trim()}
   {#if found.length}<div class="emoji-grid">{#each found as item(item.emoji)}<button type="button" title={item.words.split(' ')[0]} aria-label={item.words.split(' ')[0]} onclick={()=>pick(item.emoji)}>{#if item.url}<img src={item.url} alt=""/>{:else}{item.emoji}{/if}</button>{/each}</div>
   {:else}<p class="small muted none">No emoji found.</p>{/if}
  {:else}
   {#each groups as group(group.name)}{#if group.items.length}<p class="eyebrow">{group.name}</p><div class="emoji-grid">{#each group.items as item(item.emoji)}<button type="button" title={item.words.split(' ')[0]||item.emoji} aria-label={item.words.split(' ')[0]||item.emoji} onclick={()=>pick(item.emoji)}>{#if item.url}<img src={item.url} alt=""/>{:else}{item.emoji}{/if}</button>{/each}</div>{/if}{/each}
  {/if}
 </div>
</div>
<style>.emoji-grid img{width:23px;height:23px;object-fit:contain;vertical-align:middle}.emoji-picker{position:fixed;z-index:20;width:min(320px,calc(100vw - 16px));background:#16282e;border:1px solid #52655e;border-radius:11px;padding:10px;box-shadow:0 18px 50px #0008;display:flex;flex-direction:column;gap:8px;outline:none}.emoji-picker input{width:100%;padding:8px 10px;font-size:13px}.emoji-scroll{max-height:min(300px,50vh);overflow:auto;scrollbar-width:thin;scrollbar-color:#3d5058 transparent}.eyebrow{margin:8px 2px 4px}.emoji-grid{display:grid;grid-template-columns:repeat(8,1fr);gap:2px}.emoji-grid button{font-size:21px;line-height:1;padding:6px 0;border-radius:6px}.emoji-grid button:hover,.emoji-grid button:focus-visible{background:#34524a}.none{padding:14px 4px;text-align:center}</style>
