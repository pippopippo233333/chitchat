<script lang="ts">
 import{onMount}from'svelte';import{presets,parseHex}from'./lib/theme';
 export let color:string|null;export let onChange:(color:string|null)=>void;
 const size=220;let canvas:HTMLCanvasElement,dragging=false,hexInput=color??'';
 // HSV with full value: the wheel picks hue (angle) and strength (distance from the centre).
 function fromHsv(h:number,s:number){const f=(n:number)=>{const k=(n+h/60)%6;return 1-s*Math.max(0,Math.min(k,4-k,1));};return'#'+[f(5),f(3),f(1)].map(v=>Math.round(v*255).toString(16).padStart(2,'0')).join('');}
 function toHsv(hex:string){const p=parseHex(hex);if(!p)return{h:0,s:0};const[r,g,b]=p.rgb,max=Math.max(r,g,b),d=max-Math.min(r,g,b);let h=0;if(d)h=max===r?((g-b)/d+6)%6:max===g?(b-r)/d+2:(r-g)/d+4;return{h:h*60,s:max?d/max:0};}
 $: marker=(()=>{const{h,s}=toHsv(color??'#a4e4c2'),a=h*Math.PI/180,r=s*(size/2-6);return{x:size/2+r*Math.cos(a),y:size/2+r*Math.sin(a)};})();
 onMount(()=>{const g=canvas.getContext('2d');if(!g)return;const c=size/2;for(let deg=0;deg<360;deg++){g.beginPath();g.moveTo(c,c);g.arc(c,c,c,(deg-0.7)*Math.PI/180,(deg+0.7)*Math.PI/180);g.closePath();g.fillStyle=`hsl(${deg} 100% 50%)`;g.fill();}const white=g.createRadialGradient(c,c,0,c,c,c);white.addColorStop(0,'#fff');white.addColorStop(1,'#fff0');g.fillStyle=white;g.beginPath();g.arc(c,c,c,0,Math.PI*2);g.fill();});
 function pick(event:PointerEvent){const box=canvas.getBoundingClientRect(),x=(event.clientX-box.left)*size/box.width-size/2,y=(event.clientY-box.top)*size/box.height-size/2;const s=Math.min(1,Math.hypot(x,y)/(size/2-6)),h=(Math.atan2(y,x)*180/Math.PI+360)%360;set(fromHsv(h,s));}
 function set(next:string|null){color=next;hexInput=next??'';onChange(next);}
 function typed(){const v=hexInput.trim().startsWith('#')?hexInput.trim():'#'+hexInput.trim();if(parseHex(v)&&v.length===7)set(v.toLowerCase());}
</script>
<h2 id="dialog-title">Appearance</h2><p class="small muted">Choose the colour of ChitChat on this device. Everything follows it; warnings stay red and orange so they still stand out.</p>
<div class="theme-presets" role="group" aria-label="Preset colours">{#each presets as preset}<button class="theme-swatch" class:selected={color===preset.color} style:background={preset.color??'#a4e4c2'} title={preset.name} aria-label={preset.name} aria-pressed={color===preset.color} onclick={()=>set(preset.color)}></button>{/each}</div>
<div class="theme-wheel"><canvas bind:this={canvas} width={size} height={size} aria-label="Colour wheel" onpointerdown={event=>{dragging=true;canvas.setPointerCapture(event.pointerId);pick(event);}} onpointermove={event=>{if(dragging)pick(event);}} onpointerup={()=>dragging=false} onpointercancel={()=>dragging=false}></canvas><span class="theme-marker" style:left={marker.x/size*100+'%'} style:top={marker.y/size*100+'%'} style:background={color??'#a4e4c2'}></span></div>
<label>Colour code<input bind:value={hexInput} placeholder="#a4e4c2" maxlength="7" spellcheck="false" onchange={typed} onkeydown={event=>{if(event.key==='Enter')typed();}}/></label>
<button class="secondary" disabled={color===null} onclick={()=>set(null)}>Reset to the original colours</button>
<style>
 .theme-presets{display:flex;flex-wrap:wrap;gap:10px;margin:18px 0}
 .theme-swatch{width:36px;height:36px;border-radius:50%;border:2px solid #ffffff22}
 .theme-swatch.selected{outline:3px solid #ffffff;outline-offset:2px}
 .theme-wheel{position:relative;width:min(220px,100%);aspect-ratio:1;margin:8px auto 4px;touch-action:none}
 .theme-wheel canvas{width:100%;height:100%;border-radius:50%;cursor:crosshair;display:block}
 .theme-marker{position:absolute;width:20px;height:20px;border-radius:50%;border:3px solid #ffffff;box-shadow:0 0 0 1px #0008;transform:translate(-50%,-50%);pointer-events:none}
</style>
