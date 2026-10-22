// Uploads being sent are held until the send settles, avoiding a delete/send race.
export class PendingUploads {
 private pending=new Set<string>();private held=new Set<string>();private queue=new Set<string>();private deleting=new Map<string,Promise<void>>();private disposed=false;
 constructor(private remove:(id:string)=>Promise<void>,private failed:(error:unknown)=>void=()=>{}){}
 add(id:string){if(this.disposed){this.queue.add(id);void this.flush();}else this.pending.add(id);}
 sync(ids:string[]){const current=new Set(ids);for(const id of this.pending)if(!current.has(id)){this.pending.delete(id);this.queue.add(id);}void this.flush();}
 hold(ids:string[]){for(const id of ids){this.pending.delete(id);this.held.add(id);}}
 finish(ids:string[],sent:boolean){for(const id of ids){if(!this.held.delete(id))continue;if(!sent){if(this.disposed)this.queue.add(id);else this.pending.add(id);}}void this.flush();}
 clear(){for(const id of this.pending)this.queue.add(id);this.pending.clear();void this.flush();}
 dispose(){this.disposed=true;this.clear();}
 get queuedCount(){return this.queue.size;}
 async flush(){const tasks=[...this.queue].map(id=>{const existing=this.deleting.get(id);if(existing)return existing;const task=Promise.resolve().then(()=>this.remove(id)).then(()=>{this.queue.delete(id);},error=>this.failed(error)).finally(()=>this.deleting.delete(id));this.deleting.set(id,task);return task;});await Promise.all(tasks);}
}
