import * as vscode from 'vscode';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import {createHash,randomBytes} from 'node:crypto';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import type {Graph,ApiConfig,PublicApiConfig,NodeInfo} from './types';
import {validateConfig,keyScope,listModels,generateRoles,cacheKey,type Narrative} from './provider';
import {LocalMetrics} from './metrics';
import {semanticNeighbors} from './semantic';
import {readChecklist,saveChecklist,reviewRevision} from './checklist';
import {AI_MEMORY_HEADER,upsertAiMemory,type AiMemoryEntry} from './ai-memory';

const exec=promisify(execFile);
const MAX_MESSAGE=2_000_000;
function errorText(e:unknown){return e instanceof Error?e.message:'操作失败';}
function canonicalPath(file:string){const value=path.resolve(file);return process.platform==='win32'?value.toLowerCase():value;}
function inside(root:string,file:string){const rel=path.relative(root,file);return rel!==''&&!rel.startsWith('..'+path.sep)&&rel!=='..'&&!path.isAbsolute(rel);}
function safeText(value:unknown,max:number){if(typeof value!=='string'||value.length>max)throw new Error('消息文本无效或超过限制');return value;}

export interface ScanOptions {review?:boolean;base?:string;head?:string;root?:string;claim?:string}
export class CodeTrailHost implements vscode.Disposable {
  panel?:vscode.WebviewPanel;graph?:Graph;root='';storage='';graphPath='';htmlPath='';stale=false;webviewReady=false;panelGeneration=0;
  private running?:Promise<unknown>;private watcher?:vscode.FileSystemWatcher;private refreshTimer?:NodeJS.Timeout;private configTimer?:NodeJS.Timeout;private persistQueue:Promise<void>=Promise.resolve();private disposed=false;
  private sourceColumn=vscode.window.activeTextEditor?.viewColumn;
  private readonly output=vscode.window.createOutputChannel('CodeTrail');private readonly status=vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left,30);
  private readonly metrics:LocalMetrics;private readonly disposables:vscode.Disposable[]=[];private draftId=0; private pendingMode=''; private lastOptions:ScanOptions={}; private sourceRevision=0;
  private aborters=new Set<AbortController>();private snippets=new Map<string,string>();private semanticCache=new Map<string,Awaited<ReturnType<typeof semanticNeighbors>>>();private pendingFocus='';
  constructor(readonly context:vscode.ExtensionContext){
    this.metrics=new LocalMetrics(context.globalState,()=>vscode.workspace.getConfiguration('codeTrail').get('localMetrics',true));
    this.status.text='$(type-hierarchy) CodeTrail';this.status.command='codeTrail.open';this.status.tooltip='架构导览与静态修改影响面';this.status.show();
    this.disposables.push(vscode.workspace.registerTextDocumentContentProvider('code-trail-baseline',{provideTextDocumentContent:uri=>this.snippets.get(uri.toString())||'Baseline excerpt unavailable.'}));
    void this.metrics.bump('activations');
    this.disposables.push(vscode.window.onDidChangeActiveTextEditor(editor=>{if(editor?.viewColumn&&editor.document.uri.scheme==='file')this.sourceColumn=editor.viewColumn;}));
    this.disposables.push(vscode.workspace.onDidChangeTextDocument(e=>{if(e.document.isDirty&&this.root&&inside(this.root,e.document.uri.fsPath)){this.stale=true;this.sourceRevision++;this.semanticCache.clear();void this.send({type:'stale'});}}));
    this.disposables.push(vscode.workspace.onDidChangeConfiguration(e=>{if(e.affectsConfiguration('codeTrail.entries')&&this.graph){this.stale=true;void this.send({type:'stale'});}if(['provider','apiBaseUrl','model'].some(k=>e.affectsConfiguration('codeTrail.'+k))){clearTimeout(this.configTimer);this.configTimer=setTimeout(()=>void this.refreshNarratives().catch(err=>this.send({type:'error',message:errorText(err)})),150);}}));
  }
  dispose(){this.disposed=true;this.watcher?.dispose();clearTimeout(this.refreshTimer);clearTimeout(this.configTimer);for(const a of this.aborters)a.abort();this.panel?.dispose();this.output.dispose();this.status.dispose();this.disposables.forEach(d=>d.dispose());}
  private binary(){const configured=vscode.workspace.getConfiguration('codeTrail').get<string>('binaryPath','');if(configured){if(!path.isAbsolute(configured))throw new Error('binaryPath 必须是绝对路径');return configured;}return this.context.asAbsolutePath(path.join('bin',process.platform==='win32'?'code-trail.exe':'code-trail'));}
  async cli(args:string[]):Promise<any>{
    const result=await exec(this.binary(),args,{windowsHide:true,timeout:120_000,maxBuffer:16*1024*1024,cwd:this.root||this.context.extensionPath});
    try{return JSON.parse(result.stdout);}catch{throw new Error('Rust CLI 未返回有效 JSON');}
  }
  private async chooseRoot(explicit?:string){
    if(explicit){const folder=vscode.workspace.workspaceFolders?.find(f=>canonicalPath(f.uri.fsPath)===canonicalPath(explicit));if(!folder)throw new Error('请选择当前 VS Code 工作区中的目录');return folder.uri.fsPath;}
    const folders=vscode.workspace.workspaceFolders||[];if(!folders.length)throw new Error('请先在 VS Code 中打开本地项目文件夹');if(folders.length===1)return folders[0].uri.fsPath;
    const chosen=await vscode.window.showQuickPick(folders.map(f=>({label:f.name,description:f.uri.fsPath,folder:f})),{placeHolder:'选择要分析的项目'});if(!chosen)throw new Error('已取消');return chosen.folder.uri.fsPath;
  }
  async scan(options:ScanOptions={}){
    if(!vscode.workspace.isTrusted)throw new Error('请先信任此工作区，再运行本机分析器');
    if(this.running)return this.running;
    this.running=this.performScan(options).finally(()=>{this.running=undefined;});return this.running;
  }
  private async performScan(options:ScanOptions){
    await this.persistQueue;const sourceRevision=this.sourceRevision;const previousRoot=this.root;const repo=await this.chooseRoot(options.root||this.root||undefined);const id=createHash('sha256').update(canonicalPath(repo)).digest('hex').slice(0,20);
    const storage=path.join(this.context.globalStorageUri.fsPath,'workspaces',id);await fs.mkdir(storage,{recursive:true});
    const output=path.join(storage,'current.html');const graph=path.join(storage,'current.graph.json');const entries=vscode.workspace.getConfiguration('codeTrail').get<string[]>('entries',[]);
    const args=[options.review?'review':'scan',repo,'--out',output,'--json',graph];if(!options.review)args.push('--cache',path.join(storage,'parse-cache.json'));for(const entry of entries)args.push('--entry',entry);
    if(options.review)args.push('--base',options.base||vscode.workspace.getConfiguration('codeTrail').get('reviewBase','HEAD'));if(options.head)args.push('--head',options.head);
    if(options.claim!==undefined){const claims=path.join(storage,'agent-claim.txt');await fs.writeFile(claims,options.claim);args.push('--claims',claims);}
    this.status.text='$(sync~spin) CodeTrail';
    try{
      const receipt=await vscode.window.withProgress({location:vscode.ProgressLocation.Window,title:options.review?'CodeTrail：计算 Git 修改影响':'CodeTrail：解析本地代码'},()=>this.cli(args));
      if(this.disposed)return;const parsed=JSON.parse(await fs.readFile(graph,'utf8')) as Graph;
      this.root=repo;this.storage=storage;this.graphPath=graph;this.htmlPath=output;this.graph=parsed;this.stale=this.sourceRevision!==sourceRevision||vscode.workspace.textDocuments.some(d=>d.isDirty&&inside(repo,d.uri.fsPath));this.lastOptions={...options,root:repo};
      if(options.review)this.pendingMode='review';if(this.graph){const groups=this.context.workspaceState.get('moduleGroups');if(groups){this.graph.viewer ||= {};this.graph.viewer.module_groups=groups;}}await this.applyCachedRoles();await this.persistGraph();if(this.panel&&this.webviewReady&&previousRoot===repo){await this.send({type:'graphUpdate',graph:this.graph});await this.sendReviewStates();if(this.pendingMode){await this.send({type:'mode',mode:this.pendingMode});this.pendingMode='';}}else this.openPanel();await this.metrics.bump(options.review?'reviews':'scans');this.watch();
      this.output.appendLine(JSON.stringify({event:options.review?'review':'scan',files:receipt.files,functions:receipt.functions,edges:receipt.edges,unresolved:receipt.unresolved_calls,duration:receipt.analysis_ms}));
      return receipt;
    }catch(e){await this.metrics.bump('errors');this.output.appendLine('Analysis failed; previous successful panel preserved.');throw e;}finally{this.status.text=this.stale?'$(warning) CodeTrail':'$(type-hierarchy) CodeTrail';}
  }
  private openPanel(){
    if(!this.graph)return;
    if(!this.panel){this.panel=vscode.window.createWebviewPanel('codeTrail','CodeTrail · '+this.graph.project,vscode.ViewColumn.Beside,{enableScripts:true,retainContextWhenHidden:true,localResourceRoots:[]});this.panel.onDidDispose(()=>{this.panel=undefined;this.webviewReady=false;for(const a of this.aborters)a.abort();});this.panel.webview.onDidReceiveMessage(message=>{void this.handle(message).catch(async e=>{await this.metrics.bump('errors');await this.send({type:'error',message:errorText(e)});});});}
    this.panel.title='CodeTrail · '+this.graph.project;this.webviewReady=false;this.panelGeneration++;this.panel.webview.html=this.webviewHtml(this.graph);this.panel.reveal(undefined,true);
  }
  private webviewHtml(graph:Graph){
    const template=require('node:fs').readFileSync(this.context.asAbsolutePath('media/viewer.html'),'utf8') as string;
    const nonce=randomBytes(18).toString('base64');const data=JSON.stringify(graph).replace(/[<>&\u2028\u2029]/g,c=>'\\u'+c.charCodeAt(0).toString(16).padStart(4,'0'));
    const csp=`<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'nonce-${nonce}'; style-src 'unsafe-inline'; img-src data: blob:; connect-src 'none'; font-src 'none';">`;
    return template.replace('/*__GRAPH_DATA__*/',()=>data).replace('<head>','<head>'+csp).replace(/<script(?![^>]*\bnonce=)/g,`<script nonce="${nonce}"`);
  }
  private async persistGraph(){if(!this.graph)return;const file=this.graphPath,data=JSON.stringify(this.graph);this.persistQueue=this.persistQueue.catch(()=>{}).then(async()=>{const temp=file+'.tmp';await fs.writeFile(temp,data);await fs.rename(temp,file);});await this.persistQueue;}
  private async send(message:unknown){if(this.panel)await this.panel.webview.postMessage(message);}
  private watch(){
    this.watcher?.dispose();this.watcher=vscode.workspace.createFileSystemWatcher(new vscode.RelativePattern(this.root,'**/*.{ts,tsx,js,jsx,mjs,cjs,py,svelte,go,json}'));
    const changed=(uri:vscode.Uri)=>{const rel=path.relative(this.root,uri.fsPath);if(rel.split(path.sep).some(v=>v.startsWith('.')||['node_modules','dist','build','vendor','target'].includes(v)))return;this.stale=true;this.sourceRevision++;this.semanticCache.clear();this.status.text='$(warning) CodeTrail';void this.send({type:'stale'});if(vscode.workspace.getConfiguration('codeTrail').get('autoRefresh',false)){clearTimeout(this.refreshTimer);this.refreshTimer=setTimeout(()=>{void this.scan(this.lastOptions).catch(e=>this.send({type:'error',message:errorText(e)}));},800);}};
    this.watcher.onDidChange(changed);this.watcher.onDidCreate(changed);this.watcher.onDidDelete(changed);
  }
  private apiConfig():ApiConfig{const cfg=vscode.workspace.getConfiguration('codeTrail');return validateConfig({provider:cfg.get('provider','openai-compatible'),apiBaseUrl:cfg.get('apiBaseUrl','https://api.openai.com/v1'),model:cfg.get('model',''),modelsUrl:cfg.get('modelsUrl','')});}
  async publicConfig():Promise<PublicApiConfig>{const config=this.apiConfig();return {...config,hasApiKey:!!await this.context.secrets.get(keyScope(config)),keyScope:new URL(config.apiBaseUrl).origin};}
  async saveConfig(raw:any){
    const config=validateConfig({provider:raw.provider,apiBaseUrl:safeText(raw.apiBaseUrl,2048),model:safeText(raw.model,200),modelsUrl:safeText(raw.modelsUrl||'',2048)});
    const key=raw.apiKey===undefined?'':safeText(raw.apiKey,4096).trim();if(/[\r\n]/.test(key))throw new Error('API Key 不能包含换行符');
    if(key)await this.context.secrets.store(keyScope(config),key);
    const cfg=vscode.workspace.getConfiguration('codeTrail');for(const [name,value]of Object.entries(config))await cfg.update(name,value,vscode.ConfigurationTarget.Global);
    clearTimeout(this.configTimer);await this.refreshNarratives();
    if(await this.context.secrets.get(keyScope(config)))await this.metrics.bump('byokCompleted');await this.send({type:'apiConfig',config:await this.publicConfig(),message:'API 设置已保存；Key 保存在 SecretStorage。'});return this.publicConfig();
  }
  async clearKey(){await this.context.secrets.delete(keyScope(this.apiConfig()));await this.send({type:'apiConfig',config:await this.publicConfig(),message:'当前地址的 API Key 已删除。'});}
  async showMode(mode:string){this.pendingMode=mode;if(!this.graph)await this.scan();else if(!this.panel)this.openPanel();if(this.webviewReady)await this.send({type:'mode',mode});if(mode==='settings')await this.metrics.bump('byokOpened');}
  async focusActive(){
    const editor=vscode.window.activeTextEditor;if(!editor)throw new Error('请先选择源码中的函数');
    const folder=vscode.workspace.getWorkspaceFolder(editor.document.uri);if(!folder)throw new Error('当前文件不属于工作区');
    if(!this.graph||this.stale||canonicalPath(this.root)!==canonicalPath(folder.uri.fsPath))await this.scan({root:folder.uri.fsPath});
    const file=path.relative(this.root,editor.document.uri.fsPath).split(path.sep).join('/'),line=editor.selection.active.line+1;
    const node=this.graph!.nodes.filter(n=>n.file===file&&n.kind!=='module'&&n.line<=line&&n.end_line>=line).sort((a,b)=>(a.end_line-a.line)-(b.end_line-b.line))[0];
    if(!node)throw new Error('光标处没有已索引函数');if(!this.panel)this.openPanel();this.pendingFocus=node.id;this.panel?.reveal(undefined,true);if(this.webviewReady){await this.send({type:'focus',id:node.id});this.pendingFocus='';}return node.id;
  }
  async importClaim(source:'clipboard'|'file'){
    if(!this.graph)await this.scan();let text='';if(source==='clipboard')text=await vscode.env.clipboard.readText();else{const files=await vscode.window.showOpenDialog({canSelectMany:false,filters:{'Agent 声明':['md','txt','json']}});if(!files?.length)return;const stat=await vscode.workspace.fs.stat(files[0]);if(stat.size>32_768)throw new Error('声明文件不能超过 32 KiB');text=Buffer.from(await vscode.workspace.fs.readFile(files[0])).toString('utf8');}
    safeText(text,32_768);if(this.graph){this.graph.claim=text;await this.persistGraph();await this.send({type:'claim',text});}return text;
  }
  private async confirm(message:string,detail:string){const graph=this.graph,revision=this.sourceRevision;const answer=await vscode.window.showWarningMessage(message,{modal:true,detail},'确认写入');if(answer==='确认写入'&&(graph!==this.graph||revision!==this.sourceRevision||this.running))throw new Error('确认期间源码或工作区已变化，请重新核对内容');return answer==='确认写入';}
  private async memory(args:string[]){if(!this.graph)throw new Error('请先扫描项目');return this.cli(['memory','--graph',this.graphPath,'--store',path.join(this.storage,'memory'),...args]);}
  private async current(){if(this.running)throw new Error('扫描正在进行，请等待完成后重试');if(this.stale)throw new Error('源码已变化，请先重新扫描，再确认架构记忆');if(!this.graph)throw new Error('请先扫描项目');}
  private async inputFile(text:string){const file=path.join(this.storage,`draft-${++this.draftId}.txt`);await fs.writeFile(file,text);return file;}
  private aiMemoryFile(){const fallback=path.join(this.root,'.codetrail','ai-memory.md');if(!this.root)return fallback;const configured=vscode.workspace.getConfiguration('codeTrail').get<string>('aiMemoryPath','.codetrail/ai-memory.md');const candidate=path.resolve(this.root,configured||'.codetrail/ai-memory.md');return candidate===fallback||inside(this.root,candidate)?candidate:fallback;}
  private async writeAiMemory(graph:Graph,roles:Map<string,Narrative>){
    if(!this.root||!vscode.workspace.getConfiguration('codeTrail').get<boolean>('aiMemory',true))return;
    try{
      const file=this.aiMemoryFile(),config=this.apiConfig(),timestamp=new Date().toISOString(),project=graph.project||path.basename(this.root);
      const entries=[...roles].map(([id,note])=>{
        const node=graph.nodes.find(n=>n.id===id);if(!node)return undefined;
        return {id,name:node.name,file:node.file,line:node.line,title:note.title,summary:note.summary,contentHash:node.content_hash,snapshot:graph.snapshot,project,provider:config.provider,model:config.model,timestamp} as AiMemoryEntry;
      }).filter((value):value is AiMemoryEntry=>Boolean(value));
      if(!entries.length)return;
      let previous='';try{previous=await fs.readFile(file,'utf8');}catch{}
      await fs.mkdir(path.dirname(file),{recursive:true});
      await fs.writeFile(file,upsertAiMemory(previous,entries),'utf8');
      this.output.appendLine(`AI 解析记忆已追加：${path.relative(this.root,file)}（${entries.length} 个函数）`);
    }catch(error){this.output.appendLine(`AI 解析记忆写入失败：${errorText(error)}`);}
  }
  async openAiMemory(){
    if(!this.root)throw new Error('请先在 VS Code 中打开本地项目文件夹');
    const file=this.aiMemoryFile();
    try{await fs.access(file);}catch{await fs.mkdir(path.dirname(file),{recursive:true});await fs.writeFile(file,AI_MEMORY_HEADER,'utf8');}
    await vscode.window.showTextDocument(vscode.Uri.file(file),{preview:false});
  }
  private async applyCachedRoles(){if(!this.graph)return;const config=this.apiConfig();let cache:Record<string,Narrative>={};try{cache=JSON.parse(await fs.readFile(path.join(this.storage,'roles.json'),'utf8'));}catch{}this.graph.viewer ||= {};this.graph.viewer.annotations ||= {};for(const node of this.graph.nodes){const note=cache[cacheKey(config,node)];if(note&&typeof note.title==='string'&&note.title.length<=60&&typeof note.summary==='string'&&note.summary.length<=400)this.graph.viewer.annotations[node.id]={...note,content_hash:node.content_hash,source:'byok-model',revision:this.graph.snapshot,confirmed:false};}}
  private async refreshNarratives(){if(!this.graph||this.disposed||this.running)return;const notes=this.graph.viewer?.annotations||{};for(const id of Object.keys(notes))if(notes[id].source==='byok-model')delete notes[id];await this.applyCachedRoles();await this.persistGraph();await this.send({type:'annotations',annotations:this.graph.viewer?.annotations||{}});}
  private async narrate(ids:string[]){
    await this.current();const graph=this.graph!;const nodes=ids.map(id=>graph.nodes.find(n=>n.id===id));if(nodes.some(n=>!n)||!nodes.length||nodes.length>12)throw new Error('请选择当前图中的 1–12 个函数');const config=this.apiConfig();const key=await this.context.secrets.get(keyScope(config));if(!key)throw new Error('请先在 API 设置中保存 API Key');
    const controller=new AbortController();this.aborters.add(controller);
    try{const roles=await vscode.window.withProgress({location:vscode.ProgressLocation.Notification,title:'CodeTrail：生成职责叙述',cancellable:true},async(_,token)=>{const subscription=token.onCancellationRequested(()=>controller.abort());try{return await generateRoles(config,key,nodes as NodeInfo[],controller.signal,graph.snapshot);}finally{subscription.dispose();}});
      if(this.graph!==graph||this.stale||this.running)throw new Error('生成期间代码已变化；结果未应用，请重新扫描');let cache:Record<string,Narrative>={};try{cache=JSON.parse(await fs.readFile(path.join(this.storage,'roles.json'),'utf8'));}catch{}
      const annotations=[...roles].map(([id,note])=>({id,content_hash:graph.nodes.find(n=>n.id===id)!.content_hash,...note}));
      const file=await this.inputFile(JSON.stringify({snapshot:graph.snapshot,source:'byok-model',annotations}));const validated=path.join(this.storage,'validated-annotations.graph.json');
      try{await this.cli(['annotate',this.graphPath,'--input',file,'--out',validated]);if(this.graph!==graph||this.stale||this.running)throw new Error('校验期间代码已变化，说明未应用');const checked=JSON.parse(await fs.readFile(validated,'utf8')) as Graph;graph.viewer ||= {};graph.viewer.annotations=checked.viewer?.annotations||{};}finally{await fs.unlink(file).catch(()=>{});await fs.unlink(validated).catch(()=>{});}
      for(const [id,note]of roles)cache[cacheKey(config,graph.nodes.find(n=>n.id===id)!)]=note;cache=Object.fromEntries(Object.entries(cache).slice(-2000));await fs.writeFile(path.join(this.storage,'roles.json'),JSON.stringify(cache));await this.persistGraph();await this.send({type:'annotations',annotations:graph.viewer.annotations});await this.metrics.bump('roleRequests');await this.metrics.bump('byokCompleted');await this.writeAiMemory(graph,roles);
    }finally{this.aborters.delete(controller);}
  }
  async exportMcp(){if(!this.graph)await this.scan();const config={mcpServers:{'code-trail':{command:this.binary(),args:['mcp','--graph',this.graphPath,'--store',path.join(this.storage,'memory')]}}};const document=await vscode.workspace.openTextDocument({language:'json',content:JSON.stringify(config,null,2)});await vscode.window.showTextDocument(document,vscode.ViewColumn.Beside);return config;}
  async metricsReport(){const report=this.metrics.report();const doc=await vscode.workspace.openTextDocument({language:'json',content:JSON.stringify(report,null,2)});await vscode.window.showTextDocument(doc,vscode.ViewColumn.Beside);return report;}
  private sourceViewColumn(){
    const groups=vscode.window.tabGroups.all.filter(g=>g.viewColumn!==this.panel?.viewColumn);
    return groups.find(g=>g.viewColumn===this.sourceColumn)?.viewColumn||groups[0]?.viewColumn||vscode.ViewColumn.Beside;
  }
  private async openSource(id:string,baseline:boolean){const graph=baseline?this.graph?.review?.before_graph:this.graph;const node=graph?.nodes.find(n=>n.id===id);if(!node)throw new Error('函数不属于当前快照');if(baseline){const uri=vscode.Uri.parse('code-trail-baseline:/'+encodeURIComponent(node.id));this.snippets.set(uri.toString(),`// BASELINE EXCERPT · ${node.file}:${node.line}\n${node.source}`);return vscode.window.showTextDocument(uri,{viewColumn:this.sourceViewColumn()});}
    const file=path.resolve(this.root,node.file);if(!inside(this.root,file))throw new Error('源码路径超出项目');const real=await fs.realpath(file);if(!inside(await fs.realpath(this.root),real))throw new Error('源码路径超出项目');const doc=await vscode.workspace.openTextDocument(vscode.Uri.file(real));return vscode.window.showTextDocument(doc,{viewColumn:this.sourceViewColumn(),selection:new vscode.Range(Math.max(0,node.line-1),0,Math.max(0,node.line-1),0)});
  }
  private async sendReviewStates(){if(this.graph)await this.send({type:'reviewStates',revision:reviewRevision(this.graph),states:await readChecklist(path.join(this.storage,'review-state.json'),this.graph)});}
  async openDiff(id?:string,requestedFile?:string){
    const graph=this.graph;if(!graph?.review)throw new Error('请先执行 Git 修改审查');const change=id?graph.review.changes.find(c=>c.before?.id===id||c.after?.id===id):undefined;
    const file=change?.after?.file||change?.before?.file||requestedFile;if(!file||(!change&&!graph.review.file_changes?.some(f=>f.path===file)))throw new Error('文件不在本次审查范围');
    if(!inside(this.root,path.resolve(this.root,file)))throw new Error('diff 路径超出项目');if(!/^[a-f0-9]{40,64}$/.test(graph.review.base_commit))throw new Error('无效的基线版本');
    if(/\.(png|jpe?g|gif|webm|pdf|zip)$/i.test(file)){await vscode.window.showInformationMessage('此项为二进制资源，请在 VS Code 源代码管理面板中审核。');return;}
    const readRevision=async(revision:string)=>{try{return(await exec('git',['-C',this.root,'show',revision+':'+file],{windowsHide:true,maxBuffer:4*1024*1024})).stdout;}catch{return '';}};
    const before=await readRevision(graph.review.base_commit);const beforeUri=vscode.Uri.parse('code-trail-baseline:/diff/'+graph.review.base_commit+'/'+encodeURIComponent(file));this.snippets.set(beforeUri.toString(),before);
    let afterUri=vscode.Uri.file(path.resolve(this.root,file));
    if(/^[a-f0-9]{40,64}$/.test(graph.review.head)){const content=await readRevision(graph.review.head);afterUri=vscode.Uri.parse('code-trail-baseline:/diff/'+graph.review.head+'/'+encodeURIComponent(file));this.snippets.set(afterUri.toString(),content);}
    else {try{await fs.access(afterUri.fsPath);}catch{afterUri=vscode.Uri.parse('code-trail-baseline:/deleted/'+encodeURIComponent(file));this.snippets.set(afterUri.toString(),'');}}
    await vscode.commands.executeCommand('vscode.diff',beforeUri,afterUri,'CodeTrail · '+file,{viewColumn:this.sourceViewColumn(),preview:true});return {file,before:beforeUri.toString(),after:afterUri.toString()};
  }
  async handle(message:any):Promise<void>{
    if(!message||typeof message!=='object'||JSON.stringify(message).length>MAX_MESSAGE)throw new Error('无效的面板消息');
    switch(message.type){
      case 'ready':this.webviewReady=true;await this.send({type:'hostReady',config:await this.publicConfig(),memory:this.graph?await this.memory(['status']):null,stale:this.stale});await this.sendReviewStates();if(this.pendingMode){await this.send({type:'mode',mode:this.pendingMode});this.pendingMode='';}if(this.pendingFocus){await this.send({type:'focus',id:this.pendingFocus});this.pendingFocus='';}break;
      case 'semanticNeighbors':{await this.current();const graph=this.graph!;const id=safeText(message.id,2000);const key=graph.snapshot+'|'+id;const result=this.semanticCache.get(key)||await semanticNeighbors(this.root,graph,id);if(this.graph!==graph||this.stale||this.running)return;this.semanticCache.set(key,result);if(this.semanticCache.size>300)this.semanticCache.delete(this.semanticCache.keys().next().value!);for(const node of result.nodes)if(!graph.nodes.some(n=>n.id===node.id))graph.nodes.push(node);for(const edge of result.edges)if(!graph.edges.some(e=>e.from===edge.from&&e.to===edge.to&&e.line===edge.line))graph.edges.push(edge);await this.persistGraph();await this.send({type:'semanticResult',...result,revision:message.revision});break;}
      case 'reviewState':{if(!this.graph||message.revision!==reviewRevision(this.graph))throw new Error('复核版本已过期');await this.current();await saveChecklist(path.join(this.storage,'review-state.json'),this.graph,safeText(message.key,3000),safeText(message.status,20));await this.sendReviewStates();break;}
      case 'openDiff':await this.openDiff(typeof message.id==='string'?message.id:undefined,typeof message.file==='string'?message.file:undefined);break;
      case 'moduleConfig':{if(!this.graph||!Array.isArray(message.groups)||message.groups.length>50||message.groups.some((g:any)=>typeof g.name!=='string'||g.name.length>80||typeof g.prefix!=='string'||g.prefix.includes('..')||path.isAbsolute(g.prefix)))throw new Error('模块配置无效');this.graph.viewer ||= {};this.graph.viewer.module_groups=message.groups;await this.context.workspaceState.update('moduleGroups',message.groups);await this.persistGraph();break;}
      case 'queryMemory':await this.send({type:'memoryQuery',result:await this.memory(['query',safeText(message.text,1000),...(message.includeStale?['--include-stale']:[])])});break;
      case 'scan':await this.scan({review:message.review===true,base:typeof message.base==='string'?safeText(message.base,256):undefined,claim:this.graph?.claim});break;
      case 'settings':await this.send({type:'apiConfig',config:await this.publicConfig()});await this.metrics.bump('byokOpened');break;
      case 'saveConfig':await this.saveConfig(message.config);break;
      case 'clearKey':await this.clearKey();break;
      case 'listModels':{const config=this.apiConfig();const key=await this.context.secrets.get(keyScope(config));const models=await listModels(config,key||'');await this.send({type:'models',models,message:models.length?`连接成功，获取 ${models.length} 个模型。`:'连接成功，模型列表为空；可以手动填写 Model ID。'});break;}
      case 'testConnection':{const config=this.apiConfig();const key=await this.context.secrets.get(keyScope(config));const models=await listModels(config,key||'');await this.send({type:'connection',ok:true,message:`连接与鉴权成功 · ${models.length} 个可见模型${config.model&&!models.includes(config.model)?'；当前 Model ID 未在列表中，请确认服务是否支持':''}`});break;}
      case 'narrate':if(!Array.isArray(message.ids)||message.ids.some((id:unknown)=>typeof id!=='string'))throw new Error('函数选择无效');await this.narrate(message.ids);break;
      case 'cancel':for(const a of this.aborters)a.abort();break;
      case 'openSource':await this.openSource(safeText(message.id,2000),message.baseline===true);break;
      case 'claimClipboard':await this.importClaim('clipboard');break;
      case 'claimFile':await this.importClaim('file');break;
      case 'memoryStatus':await this.send({type:'memoryStatus',memory:await this.memory(['status'])});break;
      case 'countTokens':{const text=safeText(message.text,65_536);const file=await this.inputFile(text);try{const result=await this.cli(['tokens',file]);await this.send({type:'tokens',...result,revision:message.revision});}finally{await fs.unlink(file).catch(()=>{});}break;}
      case 'saveStable':{await this.current();const text=safeText(message.text,65_536);if(!await this.confirm('确认保存这份稳定层架构记忆？',text))return;const file=await this.inputFile(text);try{await this.memory(['save-stable','--input',file,'--confirmed']);}finally{await fs.unlink(file).catch(()=>{});}await this.metrics.bump('memoryConfirmed');await this.send({type:'memoryStatus',memory:await this.memory(['status']),message:'稳定层已确认并保存。'});break;}
      case 'confirmReview':{await this.current();const conclusion=safeText(message.conclusion,16_000);if(!Array.isArray(message.ids)||message.ids.some((id:unknown)=>typeof id!=='string'))throw new Error('函数选择无效');const claim=safeText(message.claim||'',32_768);if(!await this.confirm('确认将此静态影响面结论写入审查历史？',conclusion))return;this.graph!.claim=claim;await this.persistGraph();const file=await this.inputFile(conclusion);try{const args=['confirm-review','--conclusion',file,'--confirmed'];for(const id of message.ids)args.push('--function',id);await this.memory(args);}finally{await fs.unlink(file).catch(()=>{});}await this.metrics.bump('memoryConfirmed');await this.send({type:'memoryStatus',memory:await this.memory(['status']),message:'审查记录已确认并保存。'});break;}
      case 'markStale':{const id=safeText(message.id,100);if(!await this.confirm('将这条审查记录标记为过时？',id))return;await this.memory(['mark-stale',id,'--confirmed']);await this.send({type:'memoryStatus',memory:await this.memory(['status'])});break;}
      case 'pruneMemory':if(await this.confirm('修剪所有过时的审查记录？','删除已标记过时或属于其他源码快照的记录。稳定层不受影响。')){await this.memory(['prune','--confirmed']);await this.send({type:'memoryStatus',memory:await this.memory(['status']),message:'过时记录已修剪。'});}break;
      case 'mcpConfig':await this.exportMcp();break;
      case 'export':{const name=safeText(message.name,150);const content=safeText(message.content,MAX_MESSAGE);const uri=await vscode.window.showSaveDialog({defaultUri:vscode.Uri.file(path.join(this.root,path.basename(name)))});if(uri)await vscode.workspace.fs.writeFile(uri,Buffer.from(content));break;}
      default:throw new Error('未知的面板操作');
    }
  }
  debugState(){return {root:this.root,storage:this.storage,graphPath:this.graphPath,graph:this.graph,stale:this.stale,webviewReady:this.webviewReady,panelOpen:!!this.panel,panelGeneration:this.panelGeneration,scanning:!!this.running,metrics:this.metrics.report()};}
  testHtml(){if(!this.graph)throw new Error('No graph');return this.webviewHtml(this.graph);}
}
