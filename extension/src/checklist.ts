import * as fs from 'node:fs/promises';
import type {Graph} from './types';
export function reviewRevision(graph:Graph){return graph.snapshot+'|'+(graph.review?.base_commit||'');}
export async function readChecklist(file:string,graph:Graph):Promise<Record<string,string>>{try{const saved=JSON.parse(await fs.readFile(file,'utf8'));return saved.revision===reviewRevision(graph)&&saved.states&&typeof saved.states==='object'?saved.states:{};}catch{return {};}}
export async function saveChecklist(file:string,graph:Graph,key:string,status:string):Promise<Record<string,string>>{
  if(!['unseen','reviewed','todo'].includes(status))throw new Error('无效复核状态');
  if(!graph.review?.changes.some(c=>c.key===key)&&!graph.nodes?.some(n=>n.symbol_key===key)&&!graph.review?.before_graph?.nodes?.some(n=>n.symbol_key===key))throw new Error('只能标记本次修改中的符号');
  const states=await readChecklist(file,graph);states[key]=status;const temp=file+'.tmp';await fs.writeFile(temp,JSON.stringify({revision:reviewRevision(graph),states}));await fs.rename(temp,file);return states;
}
