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
