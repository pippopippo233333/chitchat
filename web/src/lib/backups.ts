export interface BackupSnapshot {name:string;created:number|null;ok:boolean;seconds:number|null;servers:number;messages:number;files:number}
export interface HostBackups {configured:boolean;running:boolean;scheduled:boolean;next_run:number|null;last_failed:boolean;count:number;total_bytes:number;snapshots:BackupSnapshot[]}
