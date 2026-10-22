import {describe,it,expect} from 'vitest';
import {youtubeVideo,youtubeVideos} from './youtube';
const id='dQw4w9WgXcQ';
describe('YouTube cards',()=>{
 it.each([`https://www.youtube.com/watch?v=${id}`,`https://youtu.be/${id}`,`https://youtube.com/shorts/${id}`,`https://m.youtube.com/live/${id}`])('recognizes supported links: %s',href=>expect(youtubeVideo(href)?.id).toBe(id));
 it.each(['https://youtube.com.evil.example/watch?v='+id,'https://evil.example/shorts/'+id,'https://youtube.com@evil.example/watch?v='+id,'javascript:alert(1)','https://youtube.com/watch?v=bad','https://youtube.com:444/watch?v='+id,'https://youtu.be/'+id+'/extra'])('rejects unsafe or malformed links: %s',href=>expect(youtubeVideo(href)).toBeNull());
 it('only builds a fixed privacy embed URL and parses start times',()=>{expect(youtubeVideo(`https://youtu.be/${id}?t=1h2m3s&other=evil`)?.embed).toBe(`https://www.youtube-nocookie.com/embed/${id}?autoplay=1&start=3723`);expect(youtubeVideo(`https://youtube.com/watch?v=${id}&t=90`)?.start).toBe(90);expect(youtubeVideo(`https://youtu.be/${id}#t=3m`)?.start).toBe(180);expect(youtubeVideo(`https://youtu.be/${id}?t=NaN`)?.start).toBe(0);});
 it('deduplicates links and caps cards per message',()=>{expect(youtubeVideos(`https://youtu.be/${id} https://youtube.com/watch?v=${id}`).length).toBe(1);expect(youtubeVideos(['aaaaaaaaaaa','bbbbbbbbbbb','ccccccccccc','ddddddddddd'].map(id=>`https://youtu.be/${id}`).join(' '))).toHaveLength(3);});
});
