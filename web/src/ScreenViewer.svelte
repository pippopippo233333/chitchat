<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import { screenLayout, ShareOrientation, type OrientationControl } from './lib/viewer-layout';
  export let stream: MediaStream;
  export let name: string;
  export let resumeAudio: () => Promise<void>;
  export let deafened: boolean;
  export let playbackBlocked = false;
  let zoom = 1, expanded = false, nativeFullscreen = false, panning = false, controlsVisible = true;
  let width = 1, height = 1, sourceWidth = 16, sourceHeight = 9;
  let container: HTMLElement, viewport: HTMLDivElement;
  let hideTimer: ReturnType<typeof setTimeout> | undefined;
  const orientation = new ShareOrientation(screen.orientation as OrientationControl);
  $: layout = screenLayout(width,height,sourceWidth/sourceHeight,zoom);
  const points = new Map<number,{x:number;y:number}>();
  let pan: {x:number;y:number;left:number;top:number} | null = null;
  let pinch: {distance:number;zoom:number} | null = null;
  let tap: {x:number;y:number;time:number} | null = null;
  function showControls() { controlsVisible=true; clearTimeout(hideTimer); if(expanded)hideTimer=setTimeout(()=>controlsVisible=false,3000); }
  function measure(node: HTMLDivElement) {
    const observer=new ResizeObserver(()=>{width=node.clientWidth;height=node.clientHeight;if(zoom===1){node.scrollLeft=0;node.scrollTop=0;}});
    observer.observe(node);return{destroy(){observer.disconnect();}};
  }
  function bindVideo(node: HTMLVideoElement, source: MediaStream) {
    const metadata=()=>{if(node.videoWidth&&node.videoHeight){sourceWidth=node.videoWidth;sourceHeight=node.videoHeight;void orientation.enter(nativeFullscreen,sourceWidth,sourceHeight);}};
    node.addEventListener('loadedmetadata',metadata);node.addEventListener('resize',metadata);
    const update=(next:MediaStream)=>{node.srcObject=next;void node.play().catch(()=>{});};update(source);
    return{update,destroy(){node.removeEventListener('loadedmetadata',metadata);node.removeEventListener('resize',metadata);node.pause();node.srcObject=null;}};
  }
  async function setZoom(value:number,anchor?:{x:number;y:number}) {
    const rect=viewport.getBoundingClientRect(),local=anchor?{x:anchor.x-rect.left,y:anchor.y-rect.top}:{x:width/2,y:height/2};
    const point={x:(viewport.scrollLeft+local.x-layout.left)/layout.videoWidth,y:(viewport.scrollTop+local.y-layout.top)/layout.videoHeight};
    zoom=Math.max(1,Math.min(3,value));showControls();await tick();
    viewport.scrollLeft=zoom===1?0:layout.left+point.x*layout.videoWidth-local.x;
    viewport.scrollTop=zoom===1?0:layout.top+point.y*layout.videoHeight-local.y;
  }
  function closeExpanded(){expanded=false;controlsVisible=true;clearTimeout(hideTimer);orientation.leave();if(document.fullscreenElement===container)void document.exitFullscreen().catch(()=>{});}
  async function fullscreen(){if(expanded){closeExpanded();return;}expanded=true;showControls();try{await container.requestFullscreen?.();}catch{/* Expanded fallback on unsupported browsers. */}}
  function fullscreenChanged(){if(document.fullscreenElement===container){nativeFullscreen=true;expanded=true;showControls();void orientation.enter(true,sourceWidth,sourceHeight);}else if(nativeFullscreen){nativeFullscreen=false;closeExpanded();}}
  function startPan(event:PointerEvent){
    if(event.button!==0)return;event.preventDefault();(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);points.set(event.pointerId,{x:event.clientX,y:event.clientY});
    if(points.size===1){pan={x:event.clientX,y:event.clientY,left:viewport.scrollLeft,top:viewport.scrollTop};tap={x:event.clientX,y:event.clientY,time:Date.now()};}
    else if(points.size===2){const[a,b]=[...points.values()];pinch={distance:Math.hypot(a.x-b.x,a.y-b.y),zoom};pan=null;tap=null;}panning=zoom>1;
  }
  function movePan(event:PointerEvent){
    if(!points.has(event.pointerId))return;points.set(event.pointerId,{x:event.clientX,y:event.clientY});
    if(tap&&Math.hypot(event.clientX-tap.x,event.clientY-tap.y)>8)tap=null;
    if(points.size===2&&pinch){const[a,b]=[...points.values()];if(pinch.distance>0)void setZoom(pinch.zoom*Math.hypot(a.x-b.x,a.y-b.y)/pinch.distance,{x:(a.x+b.x)/2,y:(a.y+b.y)/2});}
    else if(points.size===1&&pan&&zoom>1){viewport.scrollLeft=pan.left+pan.x-event.clientX;viewport.scrollTop=pan.top+pan.y-event.clientY;}
  }
  function endPan(event:PointerEvent){if(event.type!=='pointercancel'&&tap&&Date.now()-tap.time<500&&expanded){if(controlsVisible){controlsVisible=false;clearTimeout(hideTimer);}else showControls();}points.delete(event.pointerId);panning=false;pan=null;pinch=null;tap=null;}
  function wheelZoom(node:HTMLElement){const wheel=(event:WheelEvent)=>{if(!event.ctrlKey&&!event.metaKey)return;event.preventDefault();void setZoom(zoom*Math.exp(-event.deltaY*0.01),{x:event.clientX,y:event.clientY});};node.addEventListener('wheel',wheel,{passive:false});return{destroy(){node.removeEventListener('wheel',wheel);}};}
  function keyboard(event:KeyboardEvent){showControls();if(event.key==='+'||event.key==='='){event.preventDefault();void setZoom(zoom+0.25);}else if(event.key==='-'){event.preventDefault();void setZoom(zoom-0.25);}else if(event.key==='0'){event.preventDefault();void setZoom(1);}else if(event.key.startsWith('Arrow')&&zoom>1){event.preventDefault();if(event.key==='ArrowLeft')viewport.scrollLeft-=50;if(event.key==='ArrowRight')viewport.scrollLeft+=50;if(event.key==='ArrowUp')viewport.scrollTop-=50;if(event.key==='ArrowDown')viewport.scrollTop+=50;}}
  function escape(event:KeyboardEvent){if(event.key==='Escape'&&expanded)closeExpanded();}
  onDestroy(()=>{clearTimeout(hideTimer);orientation.leave();if(document.fullscreenElement===container)void document.exitFullscreen().catch(()=>{});});
</script>
<svelte:window onkeydown={escape}/>
<svelte:document onfullscreenchange={fullscreenChanged}/>
<figure class="screen-viewer" class:expanded bind:this={container}>
  <figcaption>{name}’s screen</figcaption>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions (Events from the toolbar buttons keep the overlay visible during interaction.) -->
  <div class="screen-viewer-controls" class:hidden-controls={expanded&&!controlsVisible} inert={expanded&&!controlsVisible} onpointerdown={showControls} onkeydown={showControls} role="group" aria-label="Screen controls">
    <button class="secondary" disabled={zoom<=1} aria-label={`Zoom out ${name} screen`} onclick={()=>setZoom(zoom-0.25)}>−</button>
    <label><span>Zoom</span><input type="range" min="1" max="3" step="0.25" value={zoom} aria-label={`Zoom ${name} screen`} oninput={event=>setZoom(Number(event.currentTarget.value))}/><output>{Math.round(zoom*100)}%</output></label>
    <button class="secondary" disabled={zoom>=3} aria-label={`Zoom in ${name} screen`} onclick={()=>setZoom(zoom+0.25)}>+</button>
    <button class="secondary" onclick={()=>setZoom(1)}>Reset zoom</button>
    <button class="secondary" onclick={fullscreen}>{expanded?'Exit fullscreen':'Fullscreen'}</button>
    <button class="secondary" disabled={deafened||!playbackBlocked} onclick={resumeAudio}>{deafened?'Audio off (deafened)':playbackBlocked?'Audio paused — enable':'Audio enabled'}</button>
  </div>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex a11y_no_noninteractive_element_interactions (This focusable application implements keyboard pan and zoom.) -->
  <div class="screen-viewport" class:panning class:zoomed={zoom>1} bind:this={viewport} role="application" tabindex="0" aria-label={`Shared screen from ${name}. Pinch or Control-wheel to zoom; drag or arrow keys to pan; plus, minus or zero adjusts zoom. Tap to show controls.`} use:measure use:wheelZoom onpointerdown={startPan} onpointermove={movePan} onpointerup={endPan} onpointercancel={endPan} onkeydown={keyboard}>
    <div class="screen-canvas" style:width={`${layout.canvasWidth}px`} style:height={`${layout.canvasHeight}px`}><video style:width={`${layout.videoWidth}px`} style:height={`${layout.videoHeight}px`} style:left={`${layout.left}px`} style:top={`${layout.top}px`} autoplay muted playsinline disablepictureinpicture disableremoteplayback use:bindVideo={stream} aria-label={`${name} shared screen`}><track kind="captions"/></video></div>
  </div>
  <p class="small muted">100% fits the whole screen. Pinch or Ctrl + wheel to zoom; drag to pan above 100%. Audio enabled means playback is allowed; the sharer must also send sound.</p>
</figure>
<style>
.screen-viewer{min-width:0;position:relative}.screen-viewer-controls{display:flex;gap:6px;align-items:center;flex-wrap:wrap;margin-bottom:10px}.screen-viewer-controls button{font-size:11px;padding:6px 9px}.screen-viewer-controls label{display:flex;align-items:center;gap:7px;font-size:12px}.screen-viewer-controls input{width:100px;padding:0}.screen-viewer-controls output{min-width:38px}.screen-viewport{position:relative;width:100%;aspect-ratio:16/9;max-height:60dvh;overflow:hidden;background:#000;border-radius:10px;overscroll-behavior:contain;scrollbar-width:none;touch-action:none}.screen-viewport.zoomed{overflow:auto;cursor:grab}.screen-viewport.panning{cursor:grabbing}.screen-canvas{position:relative}.screen-viewport video{position:absolute;display:block;max-width:none!important;max-height:none!important;object-fit:contain;border-radius:0!important;pointer-events:none;user-select:none}.screen-viewer p{margin:9px 0}.screen-viewer.expanded,.screen-viewer:fullscreen{position:fixed;inset:0;z-index:1000;margin:0!important;padding:0!important;width:100%;height:100%;max-width:none!important;background:#000;border:0;border-radius:0;box-sizing:border-box;overflow:hidden}.expanded figcaption,.expanded p{display:none}.expanded .screen-viewport{position:absolute;inset:0;width:100%;height:100%;max-height:none;aspect-ratio:auto;border-radius:0}.expanded .screen-viewer-controls{position:absolute;z-index:2;bottom:max(12px,env(safe-area-inset-bottom));left:50%;transform:translateX(-50%);width:max-content;max-width:calc(100% - 20px);justify-content:center;padding:8px;background:#15252be8;border-radius:10px;transition:opacity .2s;margin:0}.expanded .hidden-controls{opacity:0;pointer-events:none}.expanded .screen-viewer-controls input,.expanded .screen-viewer-controls label span{display:none}
</style>
