// Notification delivery/clicks only. No fetch interception, cache or offline data.
self.addEventListener('install',event=>event.waitUntil(self.skipWaiting()));
self.addEventListener('activate',event=>event.waitUntil(self.clients.claim()));
self.addEventListener('notificationclick',event=>{
 event.notification.close();const target=event.notification.data;
 if(!target||typeof target.origin!=='string'||typeof target.slug!=='string'||!/^[a-z0-9-]{2,32}$/.test(target.slug)||target.channel_id!==undefined&&!/^[0-9]+$/.test(target.channel_id))return;
 event.waitUntil((async()=>{
  const windows=await self.clients.matchAll({type:'window',includeUncontrolled:true});
  const client=windows.find(c=>c.focused&&new URL(c.url).origin===self.location.origin)??windows.find(c=>new URL(c.url).origin===self.location.origin);
  if(client){await client.focus();client.postMessage({type:'notification_click',target});return;}
  const url=new URL('/s/'+encodeURIComponent(target.slug),self.location.origin);url.searchParams.set('notify_host',target.origin);if(target.channel_id)url.searchParams.set('channel',target.channel_id);await self.clients.openWindow(url.href);
 })());
});
// Web Push from a ChitChat server on this site, sent only while no ChitChat page of yours is on screen.
// renotify: it may replace the page's own silent notice for the same message, and must still alert.
self.addEventListener('push',event=>{
 let p={};try{p=event.data?event.data.json():{};}catch{}
 const text=(v,max)=>typeof v==='string'?v.slice(0,max):'';const slug=/^[a-z0-9-]{2,32}$/.test(p.slug)?p.slug:'',channel=/^[0-9]+$/.test(p.channel_id)?p.channel_id:'';
 const target=slug?{origin:self.location.origin,slug,...(channel?{channel_id:channel}:{})}:undefined;
 event.waitUntil(self.registration.showNotification(text(p.title,120)||'ChitChat',{body:text(p.body,300)||'New message',tag:slug?self.location.origin+':'+slug+':'+channel:'chitchat',renotify:true,vibrate:[180,90,180],data:target}));
});
