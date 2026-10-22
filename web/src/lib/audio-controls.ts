export const clampVolume=(value:number)=>Math.max(0,Math.min(200,Number.isFinite(value)?value:100));
export function seekProgress(position:number,length:number,ended=false){return length>0?ended?1000:Math.max(0,Math.min(1000,position/length*1000)):0;}
export function seekPosition(progress:number,length:number){return length>0?Math.max(0,Math.min(1000,progress))/1000*length:0;}
export class MediaVolume{
 private context:AudioContext|null=null;private source:MediaElementAudioSourceNode|null=null;private gain:GainNode|null=null;private unavailable=false;private disposed=false;
 constructor(private audio:HTMLMediaElement,private createContext:()=>AudioContext=()=>new AudioContext()){}
 set(value:number){let percent=clampVolume(value);
  if(this.disposed)return{percent:0,boostUnavailable:true};
  if(percent>100&&!this.gain&&!this.unavailable){
   try{const context=this.createContext();this.context=context;const gain=context.createGain();gain.connect(context.destination);this.gain=gain;this.source=context.createMediaElementSource(this.audio);this.source.connect(gain);}
   catch{this.unavailable=true;this.gain?.disconnect();this.gain=null;if(this.source&&this.context){try{this.source.connect(this.context.destination);}catch{/* The player will report a playback error. */}}else{void this.context?.close().catch(()=>{});this.context=null;}}
  }
  if(this.gain){this.audio.volume=1;this.gain.gain.value=percent/100;void this.context?.resume().catch(()=>{});}
  else{percent=Math.min(100,percent);this.audio.volume=percent/100;}
  return{percent,boostUnavailable:this.unavailable};
 }
 get usesGain(){return this.gain!==null;}
 async resume(){if(this.context?.state!=='closed')await this.context?.resume();}
 dispose(){this.disposed=true;this.source?.disconnect();this.gain?.disconnect();void this.context?.close().catch(()=>{});this.context=null;this.source=null;this.gain=null;}
}
