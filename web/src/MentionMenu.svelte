<script lang="ts">
 import{tick}from'svelte';import type{Snapshot}from'./lib/api';import{can}from'./lib/api';
 export let snapshot:Snapshot;export let value:string;export let cursor=0;export let onPick:(value:string,cursor:number)=>void;
 let selected=0,signature='';
 $: match=/(?:^|\s)([@#])([^\s<@#]*)$/.exec(value.slice(0,cursor));
 $: query=match?.[2].toLocaleLowerCase()??'';
 $: options=!match?[]:match[1]==='#'?snapshot.channels.filter(c=>c.name.toLocaleLowerCase().includes(query)).map(c=>({key:'c'+c.id,label:'#'+c.name,token:'<#'+c.id+'>'})):[...snapshot.members.filter(m=>m.user.display_name.toLocaleLowerCase().includes(query)||m.user.username.toLocaleLowerCase().includes(query)).map(m=>({key:'u'+m.user.id,label:'@'+m.user.display_name+' ('+m.user.username+')',token:'<@'+m.user.id+'>'})),...(can(snapshot.permissions,15,snapshot.is_owner)?[...snapshot.roles.filter(r=>!r.is_default&&r.name.toLocaleLowerCase().includes(query)).map(r=>({key:'r'+r.id,label:'@'+r.name+' · role',token:'<@&'+r.id+'>'})),...['everyone','here'].filter(n=>n.includes(query)).map(n=>({key:n,label:'@'+n,token:'@'+n}))]:[])].slice(0,8);
 $: if(signature!==value.slice(0,cursor)){signature=value.slice(0,cursor);selected=0;}
 function pick(index:number){if(!match||!options[index])return;const start=cursor-match[1].length-match[2].length;const token=options[index].token+' ';onPick(value.slice(0,start)+token+value.slice(cursor),start+token.length);}
 export function key(event:KeyboardEvent){if(!options.length)return false;if(event.key==='ArrowDown'||event.key==='ArrowUp'){event.preventDefault();selected=(selected+(event.key==='ArrowDown'?1:-1)+options.length)%options.length;return true;}if(event.key==='Enter'||event.key==='Tab'){event.preventDefault();pick(selected);return true;}if(event.key==='Escape'){cursor=0;event.preventDefault();return true;}return false;}
</script>
{#if options.length}<div class="mention-menu" role="listbox" aria-label="Mention suggestions">{#each options as option,index(option.key)}<button type="button" role="option" aria-selected={index===selected} class:chosen={index===selected} onmousedown={event=>event.preventDefault()} onclick={()=>pick(index)}>{option.label}</button>{/each}</div>{/if}
<style>.mention-menu{display:flex;flex-direction:column;max-height:240px;overflow:auto;background:#16282e;border:1px solid #52655e;border-radius:8px;padding:5px;margin:5px 0}.mention-menu button{text-align:left;padding:9px 12px;border-radius:5px}.mention-menu .chosen,.mention-menu button:hover{background:#34524a}</style>
