import * as vscode from 'vscode';
import * as path from 'node:path';
import {createHash} from 'node:crypto';
import type {Graph,NodeInfo,Edge} from './types';

function relative(root:string,file:string){const value=path.relative(root,file);return !value.startsWith('..'+path.sep)&&value!=='..'&&!path.isAbsolute(value)?value.split(path.sep).join('/'):null;}
async function timeout<T>(promise:Thenable<T>,ms:number):Promise<T>{let timer:NodeJS.Timeout|undefined;try{return await Promise.race([Promise.resolve(promise),new Promise<never>((_,reject)=>{timer=setTimeout(()=>reject(new Error('语言服务查询超时')),ms);})]);}finally{if(timer)clearTimeout(timer);}}
export interface SemanticResult {id:string;supported:boolean;nodes:NodeInfo[];edges:Edge[];incoming:number;outgoing:number;truncated:boolean;reason?:string}
export async function semanticNeighbors(root:string,graph:Graph,id:string):Promise<SemanticResult>{
  const result:SemanticResult={id,supported:false,nodes:[],edges:[],incoming:0,outgoing:0,truncated:false};const selected=graph.nodes.find(n=>n.id===id);if(!selected)return {...result,reason:'Unknown function'};
  const file=path.resolve(root,selected.file);if(relative(root,file)===null)return result;
  const doc=await vscode.workspace.openTextDocument(vscode.Uri.file(file));if(doc.isDirty)return {...result,reason:'Unsaved buffer differs from analyzed snapshot'};
  const line=Math.max(0,Math.min(doc.lineCount-1,selected.line-1));const character=Math.max(0,doc.lineAt(line).text.indexOf(selected.name));
  let prepared:vscode.CallHierarchyItem[]|undefined;
  try{prepared=await timeout(vscode.commands.executeCommand<vscode.CallHierarchyItem[]>('vscode.prepareCallHierarchy',doc.uri,new vscode.Position(line,character)),12_000);}catch{return result;}
  if(!prepared?.length)return result;const item=prepared.find(i=>i.selectionRange.start.line===line)||prepared[0];
  const [incoming,outgoing]=await Promise.all([
    timeout(vscode.commands.executeCommand<vscode.CallHierarchyIncomingCall[]>('vscode.provideIncomingCalls',item),12_000).catch(()=>[]),
    timeout(vscode.commands.executeCommand<vscode.CallHierarchyOutgoingCall[]>('vscode.provideOutgoingCalls',item),12_000).catch(()=>[])
  ]);result.supported=true;result.incoming=incoming?.length||0;result.outgoing=outgoing?.length||0;result.truncated=result.incoming>100||result.outgoing>100;
  async function nodeFor(value:vscode.CallHierarchyItem):Promise<NodeInfo|null>{
    if(value.uri.scheme!=='file')return null;const file=relative(root,value.uri.fsPath);if(file===null)return null;
    const row=value.selectionRange.start.line+1;const existing=graph.nodes.find(n=>n.file===file&&n.line<=row&&n.end_line>=row&&n.kind!=='module'&&n.name===value.name.replace(/\(.*$/,''));if(existing)return existing;
    const doc=await vscode.workspace.openTextDocument(value.uri);const source=doc.getText(value.range).slice(0,12000),name=value.name;
    return {id:`${file}:${row}:${value.selectionRange.start.character+1}:${name}`,name,file,line:row,end_line:value.range.end.line+1,kind:'function',role:'语言服务识别的函数；职责说明待生成。',role_source:'no-description',source,language:doc.languageId,symbol_key:`${file}::${value.detail||''}::${name}::language-service`,content_hash:createHash('sha256').update(source.replace(/\r\n/g,'\n')).digest('hex')};
  }
  const added=new Map<string,NodeInfo>();
  for(const call of (incoming||[]).slice(0,100)){const from=await nodeFor(call.from);if(!from)continue;added.set(from.id,from);for(const range of call.fromRanges.slice(0,50))result.edges.push({from:from.id,to:id,file:from.file,line:range.start.line+1,resolution:'language-service-static'});}
  for(const call of (outgoing||[]).slice(0,100)){const to=await nodeFor(call.to);if(!to)continue;added.set(to.id,to);for(const range of call.fromRanges.slice(0,50))result.edges.push({from:id,to:to.id,file:selected.file,line:range.start.line+1,resolution:'language-service-static'});}
  result.nodes=[...added.values()];return result;
}
