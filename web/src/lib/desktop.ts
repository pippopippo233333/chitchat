// Running inside the ChitChat desktop app (desktop/, Tauri), which bundles this client.
interface Bridge {core:{invoke(cmd:string,args?:Record<string,unknown>):Promise<unknown>};event?:{listen(name:string,handler:(event:{payload:unknown})=>void):Promise<()=>void>}}
function bridge():Bridge|null{const w=typeof window==='undefined'?undefined:(window as unknown as {__TAURI__?:Bridge}).__TAURI__;return w?.core?.invoke?w:null;}
export const isDesktopApp=bridge()!==null;
/** A system notification from the desktop app (it stays in the tray, so these work with the window closed
 *  unless whenClosed is false). target comes back through onDesktopNotificationClick when it is clicked. */
export async function desktopNotify(title:string,body:string,target:unknown,whenClosed:boolean){await bridge()?.core.invoke('notify',{title,body,target:JSON.stringify(target),whenClosed});}
/** Calls handler with the target of a clicked desktop notification (still to be validated); returns a stop function. */
export function onDesktopNotificationClick(handler:(target:unknown)=>void):()=>void{
 const listening=bridge()?.event?.listen('notification-click',event=>{try{handler(JSON.parse(String(event.payload)));}catch{/* Not ours. */}});
 return()=>{void listening?.then(stop=>stop());};
}
