export interface SyncServer {host:string;port:number;username:string;namespace:string;fingerprint:string}
export interface SyncDocument {
  schema:number;petId:string;revision:number;updatedAt:number
  pet:{energy:number;hunger:number;affection:number;personality:'gentle'|'playful'|'quiet'|'warm';mood:number;moodReason:string;moodAt:number}
  memories:{id:string;text:string;at:number;deleted:boolean}[]
  conversations:{id:string;session:string;role:'user'|'assistant';text:string;at:number}[]
  moodHistory:{id:string;value:number;reason:string;at:number}[]
  social?:{enabled:boolean;proactive:boolean;analyzed:string[];lastOutreachAt:number;lastResponseAt:number;unanswered:number;lastTopic:string;reservation:string;reservedUntil:number;day:number;sentToday:number}
}
export interface ChatView {status:SyncView;session:string;reply:string|null}
export interface HistoryStatus {archived:number;pending:number;processed:number;files:number;pendingFiles:number;pendingDirectories:number;fileErrors:number;lastScan?:string;lastLearn?:string;scanError?:string;learningError?:string;invalidLines?:string;oversizedLines?:string}
export interface SyncView {mode:'local'|'server';server:SyncServer|null;capture:boolean;captureWarning?:boolean;document:SyncDocument;pending:number;message:string;lastSync:number;active:boolean;running:boolean;history?:HistoryStatus}
