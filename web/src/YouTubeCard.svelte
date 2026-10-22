<script lang="ts">
  import type { YoutubeVideo } from './lib/youtube';
  export let video: YoutubeVideo;
  let loaded=false;
</script>
<span class="youtube-card">
  {#if loaded}
    <iframe title="YouTube video" src={video.embed} allow="autoplay; encrypted-media; picture-in-picture; fullscreen" allowfullscreen referrerpolicy="strict-origin-when-cross-origin"></iframe>
    <button type="button" class="secondary" onclick={()=>loaded=false}>Close YouTube player</button>
  {:else}
    <button type="button" class="youtube-play" onclick={()=>loaded=true}><span class="play-symbol" aria-hidden="true">▶</span><strong>Play YouTube video</strong><span class="small">{video.start?`Starts at ${Math.floor(video.start/60)}:${String(video.start%60).padStart(2,'0')} · `:''}Loads content from YouTube / Google</span></button>
  {/if}
</span>
<style>
.youtube-card{display:block;max-width:480px;width:100%;margin:10px 0;white-space:normal}.youtube-play{width:100%;aspect-ratio:16/9;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:12px;background:linear-gradient(145deg,#273e38,#111c22);border:1px solid #52655e;border-radius:12px;padding:16px;text-align:center}.youtube-play:hover{border-color:#a4e4c2}.play-symbol{display:grid;place-items:center;width:54px;height:40px;border-radius:12px;background:#c93636;color:white;font-size:22px}.youtube-card iframe{display:block;width:100%;aspect-ratio:16/9;border:0;border-radius:10px;background:#000}.youtube-card>.secondary{font-size:11px;margin-top:6px}
</style>
