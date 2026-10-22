export const maxRecordingMs=300000;
export const recordingFormats=[{mime:'audio/webm;codecs=opus',extension:'webm'},{mime:'audio/ogg;codecs=opus',extension:'ogg'},{mime:'audio/mp4',extension:'m4a'}];
export function recordingFormat(supported:(mime:string)=>boolean){return recordingFormats.find(format=>supported(format.mime))??null;}
export function duration(ms:number|null|undefined){const seconds=Math.max(0,Math.floor((ms??0)/1000));return `${Math.floor(seconds/60)}:${String(seconds%60).padStart(2,'0')}`;}
export class RecordingSession{
 private generation=0;private recorder:MediaRecorder|null=null;private stream:MediaStream|null=null;private limit:ReturnType<typeof setTimeout>|undefined;private clock:ReturnType<typeof setInterval>|undefined;
 constructor(private capture:()=>Promise<MediaStream>,private make:(stream:MediaStream,mime:string)=>MediaRecorder,private stopped:(blob:Blob)=>void,private failed:(error:Error)=>void,private elapsed:(ms:number)=>void){}
 async start(mime:string){this.cancel();const version=this.generation;let stream:MediaStream|undefined;
  try{stream=await this.capture();if(version!==this.generation){stream.getTracks().forEach(track=>track.stop());return false;}this.stream=stream;const recorder=this.make(stream,mime);this.recorder=recorder;const chunks:Blob[]=[];
   recorder.ondataavailable=event=>{if(event.data.size)chunks.push(event.data);};
   recorder.onstop=()=>{if(version!==this.generation)return;this.release();this.recorder=null;const blob=new Blob(chunks,{type:recorder.mimeType||mime});if(blob.size)this.stopped(blob);else this.failed(new Error('No sound was recorded. Try again.'));};
   recorder.onerror=()=>{if(version!==this.generation)return;this.cancel();this.failed(new Error('Recording failed. Check your microphone and try again.'));};
   stream.getTracks().forEach(track=>track.onended=()=>this.stop());recorder.start(1000);const start=Date.now();this.elapsed(0);this.clock=setInterval(()=>this.elapsed(Math.min(maxRecordingMs,Date.now()-start)),250);this.limit=setTimeout(()=>this.stop(),maxRecordingMs);return true;
  }catch(error){if(version===this.generation)this.cancel();else stream?.getTracks().forEach(track=>track.stop());throw error;}
 }
 stop(){clearTimeout(this.limit);clearInterval(this.clock);if(this.recorder?.state==='recording')this.recorder.stop();}
 cancel(){this.generation++;const recorder=this.recorder;this.recorder=null;if(recorder){recorder.onstop=null;recorder.ondataavailable=null;recorder.onerror=null;if(recorder.state!=='inactive')recorder.stop();}this.release();}
 private release(){clearTimeout(this.limit);clearInterval(this.clock);const stream=this.stream;this.stream=null;stream?.getTracks().forEach(track=>{track.onended=null;track.stop();});}
}
