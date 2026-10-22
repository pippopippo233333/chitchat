// Running inside the ChitChat desktop app (desktop/, Tauri), which bundles this client.
interface Bridge {core:{invoke(cmd:string,args?:Record<string,unknown>):Promise<unknown>}}
function bridge():Bridge|null{const w=typeof window==='undefined'?undefined:(window as unknown as {__TAURI__?:Bridge}).__TAURI__;return w?.core?.invoke?w:null;}
export const isDesktopApp=bridge()!==null;
/** A system notification from the desktop app (it stays in the tray, so these work with the window closed). */
export async function desktopNotify(title:string,body:string){await bridge()?.core.invoke('notify',{title,body});}
