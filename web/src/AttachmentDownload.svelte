<script lang="ts">
 export let src:string;export let filename:string;export let label=filename;
 $: downloadUrl=(()=>{const url=new URL(src);url.searchParams.set('download','1');return url.href;})();
</script>
<a class="secondary download-attachment" href={downloadUrl} download={filename} aria-label={`Download ${label}`} rel="noopener noreferrer">Download</a>
<style>.download-attachment{font-size:12px;padding:7px 12px}</style>
