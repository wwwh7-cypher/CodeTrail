import * as vscode from 'vscode';
import {CodeTrailHost,type ScanOptions} from './host';
let host:CodeTrailHost|undefined;
export function activate(context:vscode.ExtensionContext){
  host=new CodeTrailHost(context);context.subscriptions.push(host);
  const register=(name:string,fn:(...args:any[])=>unknown)=>context.subscriptions.push(vscode.commands.registerCommand(name,async(...args:any[])=>{try{return await fn(...args);}catch(e){await vscode.window.showErrorMessage('CodeTrail: '+(e instanceof Error?e.message:'操作失败'));throw e;}}));
  register('codeTrail.open',(options?:ScanOptions)=>host!.scan(options));
  register('codeTrail.focus',()=>host!.focusActive());
  register('codeTrail.review',async(options?:ScanOptions)=>{const base=options?.base??await vscode.window.showInputBox({title:'Git 审查基线',value:vscode.workspace.getConfiguration('codeTrail').get('reviewBase','HEAD'),prompt:'与当前工作区（含暂存和未暂存修改）比较'});if(base!==undefined)return host!.scan({...options,review:true,base});});
  register('codeTrail.settings',()=>host!.showMode('settings'));register('codeTrail.memory',()=>host!.showMode('memory'));
  register('codeTrail.claimClipboard',()=>host!.importClaim('clipboard'));register('codeTrail.claimFile',()=>host!.importClaim('file'));
  register('codeTrail.mcpConfig',()=>host!.exportMcp());register('codeTrail.metrics',()=>host!.metricsReport());register('codeTrail.clearKey',()=>host!.clearKey());
  register('codeTrail.openAiMemory',()=>host!.openAiMemory());
  // Exported API supports extension integration tests and local tooling, not webview script execution.
  return {scan:(options?:ScanOptions)=>host!.scan(options),state:()=>host!.debugState(),config:()=>host!.publicConfig(),settings:(value:unknown)=>host!.saveConfig(value),html:()=>host!.testHtml(),openDiff:(id:string)=>host!.openDiff(id),dispose:()=>host!.dispose()};
}
export function deactivate(){host=undefined;}
