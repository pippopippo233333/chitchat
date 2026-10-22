export interface ResourceCpu { limit_cores: number | null; usage_cores: number | null; enforced: boolean }
export interface ResourceRam { used_bytes: number | null; limit_bytes: number | null; enforced: boolean }
export interface HostResources {
  cpu: { cores_total: number; server: ResourceCpu; ai: ResourceCpu & { unit: string | null; running?: boolean } };
  ram: { total_bytes: number | null; available_bytes: number | null; server: ResourceRam; ai: ResourceRam };
  disk: { total_bytes: number | null; free_bytes: number | null };
  ai_disk_bytes: number | null;
}
export interface ServerStorage { used_bytes: number; limit_bytes: number | null; files_bytes: number; messages_bytes: number; ai_bytes: number; full_action: 'delete_oldest' | 'refuse' }
export const GB = 1024 ** 3;
export function giga(bytes: number | null | undefined) { return bytes === null || bytes === undefined ? null : bytes / GB; }
export function ramMb(gb: number | undefined) { return gb === undefined ? null : Math.round(gb * 1024); }

export function storageLabel(value:number|null|undefined){
 if(value==null||!Number.isFinite(value))return 'Unavailable';
 const bytes=Math.max(0,value),units=['B','KB','MB','GB'];let amount=bytes,index=0;
 while(amount>=1024&&index<units.length-1){amount/=1024;index++;}
 return `${amount.toLocaleString(undefined,{maximumFractionDigits:index?2:0})} ${units[index]}`;
}
