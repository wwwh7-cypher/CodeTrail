import type {Memento} from 'vscode';
export interface Metrics {firstUse:number;lastUse:number;days:string[];activations:number;scans:number;reviews:number;errors:number;byokOpened:number;byokCompleted:number;roleRequests:number;memoryConfirmed:number}
export class LocalMetrics {
  constructor(private state:Memento,private enabled:()=>boolean){}
  async bump(field:keyof Pick<Metrics,'activations'|'scans'|'reviews'|'errors'|'byokOpened'|'byokCompleted'|'roleRequests'|'memoryConfirmed'>):Promise<void>{if(!this.enabled())return;const now=Date.now();const data=this.snapshot();if(!data.firstUse)data.firstUse=now;data.lastUse=now;data[field]++;data.days=[...new Set([...data.days,new Date(now).toISOString().slice(0,10)])].slice(-90);await this.state.update('localMetrics',data);}
  snapshot():Metrics{return this.state.get<Metrics>('localMetrics',{firstUse:0,lastUse:0,days:[],activations:0,scans:0,reviews:0,errors:0,byokOpened:0,byokCompleted:0,roleRequests:0,memoryConfirmed:0});}
  report(){const data=this.snapshot();return {...data,sevenDayReturnObserved:data.days.length>=2&&data.lastUse-data.firstUse>=24*3600_000&&data.lastUse-data.firstUse<=7*24*3600_000,byokCompletionRate:data.byokOpened?Math.min(1,data.byokCompleted/data.byokOpened):null,scope:'this machine only; no network telemetry',note:'Local usage indicators, not cohort retention statistics.'};}
}
