<script lang="ts">
 import AudioPlayer from './AudioPlayer.svelte';
 import{onMount,onDestroy}from'svelte';import Icon from './Icon.svelte';import{RecordingSession,recordingFormat,duration}from'./lib/recorder';
 export let enabled:boolean;export let canStart=true;export let busy=false;export let onAttach:(file:File)=>Promise<void>;
 export let getMicrophone:()=>Promise<MediaStream>=()=>navigator.mediaDevices.getUserMedia({audio:{echoCancellation:true,noiseSuppression:true,autoGainControl:true},video:false});
 export let microphoneAvailable:()=>Promise<boolean|null>=async()=>{const devices=await navigator.mediaDevices?.enumerateDevices();return devices&&devices.length?devices.some(device=>device.kind==='audioinput'):null;};
 let generation=0;let errorTimer:ReturnType<typeof setTimeout>|undefined;
 function report(message:string){clearTimeout(errorTimer);error=message;if(message)errorTimer=setTimeout(()=>error='',5000);}
 let phase:'idle'|'requesting'|'recording'|'preview'|'attaching'='idle',elapsed=0,error='',available=!!navigator.mediaDevices?.getUserMedia&&typeof MediaRecorder!=='undefined',url='',blob:Blob|null=null;
 const format=typeof MediaRecorder!=='undefined'?recordingFormat(mime=>MediaRecorder.isTypeSupported(mime)):null;
 const session=new RecordingSession(()=>getMicrophone(),(stream,mime)=>new MediaRecorder(stream,{mimeType:mime,audioBitsPerSecond:96000}),value=>{blob=value;url=URL.createObjectURL(value);phase='preview';},cause=>{report(cause.message);phase='idle';},value=>elapsed=value);
 $: busy=phase!=='idle';
 $: if(!enabled&&phase!=='idle')cancel();
 async function detect(){try{const detected=await microphoneAvailable();if(detected!==null)available=detected;}catch{/* The browser may hide devices until permission is granted. */}}
 async function start(){if(!enabled||!canStart||!format)return;const version=++generation;report('');phase='requesting';try{const started=await session.start(format.mime);if(started&&version===generation)phase='recording';}catch(cause){if(version!==generation)return;phase='idle';if(cause instanceof DOMException&&cause.name==='NotFoundError'){available=false;report('No microphone is available. You can still attach audio files.');}else report(cause instanceof DOMException&&cause.name==='NotAllowedError'?'Microphone access was refused. Allow it in your browser to record a voice message.':cause instanceof Error?cause.message:'Could not open the microphone.');}}
 function cancel(){generation++;report('');session.cancel();if(url)URL.revokeObjectURL(url);url='';blob=null;phase='idle';}
 async function attach(){if(!blob||!format||!enabled)return;const version=generation;phase='attaching';report('');try{await onAttach(new File([blob],`voice-message.${format.extension}`,{type:blob.type}));if(version===generation)cancel();}catch(cause){if(version!==generation)return;report(cause instanceof Error?cause.message:'Could not attach recording.');phase='preview';}}
 onMount(()=>{void detect();navigator.mediaDevices?.addEventListener('devicechange',detect);});onDestroy(()=>{cancel();busy=false;navigator.mediaDevices?.removeEventListener('devicechange',detect);});
</script>
{#if available&&format}<button type="button" class="record-button" disabled={!enabled||!canStart||phase!=='idle'} aria-label="Record voice message" title="Record voice message (up to 5 minutes)" onclick={start}><Icon name="mic" size={18}/></button>{/if}
{#if phase!=='idle'||error}<div class="record-panel" role="group" aria-label="Voice message recorder">
 {#if error}<p class="error" role="alert">{error}</p>{/if}
 {#if phase==='requesting'}<span role="status">Opening microphone…</span><button type="button" class="secondary" onclick={cancel}>Cancel recording</button>
 {:else if phase==='recording'}<span role="status">Recording {duration(elapsed)} / 5:00</span><button type="button" class="secondary" onclick={()=>session.stop()}>Stop recording</button><button type="button" class="secondary" onclick={cancel}>Discard recording</button>
 {:else if phase==='preview'||phase==='attaching'}<span>Voice message · {duration(elapsed)}</span><AudioPlayer src={url} label="Preview voice message" durationMs={elapsed}/><button type="button" class="secondary" disabled={phase==='attaching'||!enabled} onclick={attach}>{phase==='attaching'?'Attaching…':'Attach voice message'}</button><button type="button" class="secondary" disabled={phase==='attaching'} onclick={cancel}>Discard recording</button><span class="small muted">Attach, then press Send to share it.</span>{/if}
</div>{/if}
<style>.record-button{width:38px;height:38px;display:grid;place-items:center;border:1px solid #52655e;border-radius:9px;flex-shrink:0;color:#bce6d0}.record-panel{display:flex;align-items:center;gap:8px;flex-wrap:wrap;flex-basis:100%;order:3;font-size:12px;padding:10px;border-top:1px solid #3b4f58;min-width:0}.record-panel :global(.audio-player){max-width:360px}.record-panel button{font-size:12px}.record-panel .error{width:100%}</style>
