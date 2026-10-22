export function screenLayout(width: number, height: number, aspect: number, zoom: number) {
  width = Math.max(1,width); height = Math.max(1,height); aspect = Number.isFinite(aspect) && aspect > 0 ? aspect : 16/9;
  zoom = Math.max(1,Math.min(3,Number.isFinite(zoom)?zoom:1));
  const baseWidth = Math.min(width,height*aspect), videoWidth=baseWidth*zoom, videoHeight=videoWidth/aspect;
  const canvasWidth=Math.max(width,videoWidth),canvasHeight=Math.max(height,videoHeight);
  return {videoWidth,videoHeight,canvasWidth,canvasHeight,left:(canvasWidth-videoWidth)/2,top:(canvasHeight-videoHeight)/2};
}
export interface OrientationControl { lock?: (orientation: 'landscape') => Promise<void>; unlock?: () => void }
export class ShareOrientation {
  private version=0;
  private requested=false;
  constructor(private control: OrientationControl | undefined) {}
  async enter(nativeFullscreen: boolean,width: number,height: number) {
    if(!nativeFullscreen||width<=height||width<=0||height<=0){this.leave();return;}
    if(this.requested||!this.control?.lock)return;
    this.requested=true;const version=++this.version;
    try{await this.control.lock('landscape');if(version!==this.version)this.control.unlock?.();}
    catch{if(version===this.version)this.requested=false;}
  }
  leave(){this.version++;if(this.requested){this.requested=false;try{this.control?.unlock?.();}catch{/* Unsupported orientation control. */}}}
}
