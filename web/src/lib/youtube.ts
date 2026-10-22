import { messageLinks } from './links';
export interface YoutubeVideo { id: string; start: number; embed: string }
function seconds(value: string | null) {
  if (!value) return 0;
  if (/^\d+$/.test(value)) return Math.min(604800,Number(value));
  const match=/^(?:(\d+)h)?(?:(\d+)m)?(?:(\d+)s)?$/.exec(value);
  return match ? Math.min(604800,Number(match[1]||0)*3600+Number(match[2]||0)*60+Number(match[3]||0)) : 0;
}
export function youtubeVideo(href: string): YoutubeVideo | null {
  let url: URL; try { url=new URL(href); } catch { return null; }
  if (!['http:','https:'].includes(url.protocol)||url.username||url.password||url.port) return null;
  const host=url.hostname.toLowerCase();let id: string | null=null;
  if (host==='youtu.be') id=/^\/([^/]+)\/?$/.exec(url.pathname)?.[1]??null;
  else if (['youtube.com','www.youtube.com','m.youtube.com'].includes(host)) {
    if(url.pathname==='/watch')id=url.searchParams.get('v');
    else id=/^\/(?:shorts|live)\/([^/]+)\/?$/.exec(url.pathname)?.[1]??null;
  }
  if(!id||!(/^[A-Za-z0-9_-]{11}$/).test(id))return null;
  const start=seconds(url.searchParams.get('t')??url.searchParams.get('start')??new URLSearchParams(url.hash.slice(1)).get('t'));
  return {id,start,embed:`https://www.youtube-nocookie.com/embed/${id}?autoplay=1${start?`&start=${start}`:''}`};
}
export function youtubeVideos(content: string) {
  const seen=new Set<string>();const videos: YoutubeVideo[]=[];
  for(const part of messageLinks(content)){const video=part.href?youtubeVideo(part.href):null;if(video&&!seen.has(video.embed)){seen.add(video.embed);videos.push(video);if(videos.length===3)break;}}
  return videos;
}
