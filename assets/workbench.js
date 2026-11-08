// Reader-facing views share the exact analyzed nodes and edges. Descriptions never add edges.
const ROOT_GRAPH = G;
if(G.review)state.mode='review';
const hostApi = typeof acquireVsCodeApi === 'function' ? acquireVsCodeApi() : null;
const sourceNotes = G.viewer?.annotations || {};
const viewOptions = G.viewer || {};
state.view = viewOptions.default_view || 'network';
state.depth = state.view === 'network' ? 1 : 0;
state.limit = 9;
state.scope = viewOptions.scope || '';
if (byId.has(viewOptions.entry)) state.entry = viewOptions.entry;
if (byId.has(viewOptions.target)) state.target = state.selected = viewOptions.target;
let memoryEditing = false, memoryDirty = false, tokenRevision = 0, tokenTimer;
let latestTokens = null, currentMemory = null, hiddenNodeCount = 0;

function postHost(message) {
  if (hostApi) hostApi.postMessage(message);
  else toast('此操作在 VS Code 扩展中执行；当前为离线 HTML 预览。');
}
function inScope(n) { return !!n && (!state.scope || n.file.startsWith(state.scope)) && !(G.viewer?.exclude_test_files && /(?:_test\.go|\.(?:test|spec)\.[^.]+)$/.test(n.file)); }
function selectedTargets() { return state.target === '__all__' ? G.changed.filter(id => byId.has(id)) : byId.has(state.target) ? [state.target] : []; }
function currentImpact() { const ids = new Set(); for (const id of selectedTargets()) for (const n of reach(id, rev)) ids.add(n); return ids; }
function sourceNote(n) { const note = sourceNotes[n.id]; return note && (!note.content_hash || note.content_hash === n.content_hash) ? note : null; }
const domainWords = { Bibliography:'文献内容',Bibtex:'BibTeX',BibTeX:'BibTeX',Ris:'RIS',Csl:'CSL',Json:'JSON',Authors:'作者',Author:'作者',Format:'格式',Fields:'字段',Field:'字段',Value:'值',Group:'括号分组',Entry:'条目',Entries:'条目',Import:'导入',Title:'标题',DOI:'DOI',Doi:'DOI',Year:'年份',Text:'文本',Paper:'论文',Papers:'论文',Document:'文档',Documents:'文档',Project:'项目',Projects:'项目',Config:'配置',File:'文件',Files:'文件',Search:'检索',Query:'查询',Answer:'回答',Evidence:'证据',Chunks:'分块',Chunk:'分块',Metadata:'元数据',Network:'网络',Relations:'关联',Record:'记录',Page:'页面',Pages:'页面',Latex:'LaTeX',Workspace:'工作区',Settings:'设置',User:'用户',Result:'结果',Results:'结果',Response:'响应',Request:'请求',Session:'会话',Cache:'缓存',List:'列表',Columns:'列',Column:'列',Routes:'路由',Server:'服务',App:'应用' };
function nameHint(n) {
  if (n.kind === 'module') return '模块顶层执行';
  if (n.name.startsWith('<')) return '匿名回调';
  const m = n.name.match(/^(parse|normalize|detect|split|decode|encode|validate|load|save|create|delete|remove|fetch|search|find|read|write|build|render|open|close|update|handle|get|set|collect|filter|format|calculate|compute|rank|scan|resolve|clear|start|stop|import|export|preview|pick|ensure)(.*)$/i);
  if (!m) return n.name === 'main' ? '启动应用' : n.name === 'collapse' ? '整理文本空白' : n.name;
  const verbs = {parse:'解析',normalize:'统一',detect:'识别',split:'拆分',decode:'解码',encode:'编码',validate:'校验',load:'加载',save:'保存',create:'创建',delete:'删除',remove:'移除',fetch:'获取',search:'检索',find:'查找',read:'读取',write:'写入',build:'构建',render:'渲染',open:'打开',close:'关闭',update:'更新',handle:'处理',get:'获取',set:'设置',collect:'收集',filter:'筛选',format:'格式化',calculate:'计算',compute:'计算',rank:'排序',scan:'扫描',resolve:'解析',clear:'清理',start:'启动',stop:'停止',import:'导入',export:'导出',preview:'预览',pick:'选择',ensure:'检查'};
  const words = m[2].replace(/([a-z0-9])([A-Z])/g, '$1 $2').replace(/[_-]/g, ' ').trim().split(/\s+/).filter(Boolean);
  const translated = words.map(w => domainWords[w] || domainWords[w[0]?.toUpperCase() + w.slice(1)] || w).join('');
  return verbs[m[1].toLowerCase()] + (translated || '数据');
}
function describe(n) {
  const note = sourceNote(n);
  if (note && n.role_source!=='llm-unverified') return { title:note.title || nameHint(n), summary:note.summary || nameHint(n), provenance:note.source==='conversation-model'?'当前对话模型说明 · 待人工确认':note.source==='byok-model'?'BYOK 模型说明 · 待人工确认':'源码阅读摘要 · 待人工确认', basis:'note' };
  if (n.role_source === 'llm-unverified') return {title:nameHint(n),summary:n.role,provenance:'BYOK 模型叙述 · 待人工确认',basis:'model'};
  if (['source-comment','source-docstring'].includes(n.role_source)) return {title:nameHint(n),summary:n.role,provenance:'来自源码注释',basis:'comment'};
  if (n.kind === 'module') return {title:'执行模块初始化',summary:'执行文件顶层的调用与初始化逻辑。',provenance:'基于语法结构',basis:'structural'};
  const calls = (adj.get(n.id) || []).length;
  const returns = /\breturn\b/.test(n.source);
  const structural = calls ? `继续调用 ${calls} 个已解析函数，并完成函数体中其余操作。` : returns ? '函数体包含返回语句，但没有已解析的对外调用。' : '函数体没有已解析的对外调用，也没有显式返回语句。';
  if (n.name.startsWith('<')) return {title:'匿名回调',summary:structural,provenance:'结构提示 · 匿名回调没有源码注释，未生成职责说明',basis:'none'};
  const hint = nameHint(n);
  const named = hint !== n.name;
  return {title:hint,summary:named?`${hint}；${structural}`:structural,provenance:named?'名称提示 · 未生成职责说明，业务含义待核对':'未生成职责说明 · 仅结构提示',basis:named?'name-hint':'none'};
}
function scopeFunctions() { return functions.filter(n => inScope(n) && !n.name.startsWith('<')); }
function option(n) { const d = describe(n); return `<option value="${esc(n.id)}">${esc(d.title === n.name ? n.name : d.title + ' · ' + n.name)}</option>`; }
function renderFiles() {
  const text = $('#search').value.toLowerCase();
  const found = scopeFunctions().filter(n => `${n.name} ${n.file} ${describe(n).title} ${describe(n).summary}`.toLowerCase().includes(text));
  found.sort((a,b)=>Number(!!sourceNote(b))-Number(!!sourceNote(a))||a.file.localeCompare(b.file)||a.line-b.line);
  $('#file-list').innerHTML = found.slice(0,100).map(n => { const d = describe(n); const secondary = d.title === n.name ? n.file + ':' + n.line : n.name; return `<button class="file ${n.id === state.selected ? 'active' : ''}" data-node="${esc(n.id)}" title="${esc(n.file + ':' + n.line)}"><span class="file-title">${esc(d.title)}</span><span class="file-symbol">${esc(secondary)}</span></button>`; }).join('') || '<p class="small muted">没有匹配的函数。</p>';
  if (found.length > 100) $('#file-list').insertAdjacentHTML('beforeend','<p class="small muted">输入关键词查找其余函数。</p>');
  all('#file-list [data-node]').forEach(b => b.onclick = () => focusFunction(b.dataset.node));
}
function focusFunction(id) {
  if (!byId.has(id)) return;
  state.selected = state.target = id; state.limit = 9;
  $('#target-select').value = id;
  resetView(); render();
}
function visibleGraph() {
  const path = state.mode === 'review' ? [] : bfs(state.entry,state.target);
  renderedPath = path || [];
  const seeds = state.mode === 'review' ? selectedTargets() : path?.length ? path : [state.selected || state.target].filter(id => byId.has(id));
  let seen;
  if (state.mode === 'review') seen = currentImpact();
  else {
    seen = new Set(seeds);
    let layer = [...seen];
    for (let depth=0; depth<state.depth; depth++) {
      const next=[];
      for (const id of layer) {
        const neighbors = state.view === 'network' ? [...(adj.get(id)||[]),...(rev.get(id)||[])] : (adj.get(id)||[]);
        for (const target of neighbors) if (!seen.has(target) && byId.has(target) && inScope(byId.get(target)) && !byId.get(target).name.startsWith('<')) {seen.add(target);next.push(target);}
      }
      layer=next;
    }
  }
  const ordered=[...seen].filter(id=>byId.has(id));
  const priority=id=>(seeds.includes(id)?1000:0)+(id===state.selected?500:0)+(sourceNote(byId.get(id))?20:0)+Math.min(10,(adj.get(id)||[]).length+(rev.get(id)||[]).length);
  ordered.sort((a,b)=>priority(b)-priority(a)||a.localeCompare(b));
  const limit=Math.max(state.limit,seeds.length);
  hiddenNodeCount=Math.max(0,ordered.length-limit);
  return new Set(ordered.slice(0,limit));
}
const textMeasure=document.createElement('canvas').getContext('2d');
function textLines(text,width,font,maxLines=2) {
  textMeasure.font=font;const lines=[];let line='';
  for (const ch of String(text)) {if(line && textMeasure.measureText(line+ch).width>width){lines.push(line);line=ch;}else line+=ch;}
  if(line)lines.push(line);
  if(lines.length>maxLines) {lines.length=maxLines;let last=lines[maxLines-1];while(last&&textMeasure.measureText(last+'…').width>width)last=last.slice(0,-1);lines[maxLines-1]=last+'…';}
  return lines;
}
function svgText(text,x,y,width,font,lineHeight,className,maxLines=2) {
  return `<text class="${className}" x="${x}" y="${y}">${textLines(text,width,font,maxLines).map((line,i)=>`<tspan x="${x}" dy="${i?lineHeight:0}">${esc(line)}</tspan>`).join('')}</text>`;
}
function roundedRoute(points){
  points=points.filter((p,i)=>!i||p.x!==points[i-1].x||p.y!==points[i-1].y);
  if(points.length<2)return '';
  let d=`M${points[0].x} ${points[0].y}`;
  for(let i=1;i<points.length-1;i++){
    const a=points[i-1],b=points[i],c=points[i+1],ab=Math.hypot(b.x-a.x,b.y-a.y),bc=Math.hypot(c.x-b.x,c.y-b.y),r=Math.min(11,ab/2,bc/2);
    if(!ab||!bc)continue;const p={x:b.x-(b.x-a.x)/ab*r,y:b.y-(b.y-a.y)/ab*r},q={x:b.x+(c.x-b.x)/bc*r,y:b.y+(c.y-b.y)/bc*r};
    d+=` L${p.x} ${p.y} Q${b.x} ${b.y} ${q.x} ${q.y}`;
  }
  return d+` L${points.at(-1).x} ${points.at(-1).y}`;
}
function connectionRoute(a,b,w,h,gapX,gapY,edge,primary){
  const spread=(neighbors,target)=>Math.max(-36,Math.min(36,(neighbors.indexOf(target)-(neighbors.length-1)/2)*11));
  const sourceOffset=primary?0:spread(adj.get(edge.from)||[],edge.to),targetOffset=primary?0:spread(rev.get(edge.to)||[],edge.from);
  if(edge.from===edge.to){const x=a.x+w+25;return roundedRoute([{x:a.x+w,y:a.y+54},{x,y:a.y+54},{x,y:a.y+h-40},{x:a.x+w,y:a.y+h-40}]);}
  const forward=b.c>=a.c,sameColumn=a.c===b.c;
  const s={x:a.x+(forward?w:0),y:a.y+h/2+sourceOffset};
  const t={x:b.x+(sameColumn?w:forward?0:w),y:b.y+h/2+targetOffset};
  const laneA=a.x+(forward?w+gapX/2:-gapX/2);
  const laneB=b.x+(sameColumn?w+gapX/2:forward?-gapX/2:w+gapX/2);
  if(Math.abs(laneA-laneB)<1)return roundedRoute([s,{x:laneA,y:s.y},{x:laneA,y:t.y},t]);
  const bridge=a.y-gapY/2;
  return roundedRoute([s,{x:laneA,y:s.y},{x:laneA,y:bridge},{x:laneB,y:bridge},{x:laneB,y:t.y},t]);
}
function drawGraph() {
  visibleIds=visibleGraph();const ids=[...visibleIds];const pathSet=new Set(renderedPath);const targets=new Set(selectedTargets());
  const isPath=state.mode==='tour'&&state.view==='path';
  const noPath=isPath&&!renderedPath.length;
  $('#graph-empty').classList.toggle('hidden',ids.length>0&&!noPath);
  $('#graph-empty').textContent=!ids.length?'当前范围没有可展示的函数。':noPath?'已解析的静态边中没有这条到达路径。切换网状图，可查看目标的直接上下文。':'';
  const areaWidth=$('#canvas').clientWidth||800;
  const cols=isPath?Math.min(3,Math.max(1,ids.length)):Math.min(areaWidth<520?2:3,Math.max(1,ids.length));
  const nodeW=220,nodeH=166,gapX=62,gapY=58,marginX=48,marginY=52;
  const rows=Math.max(1,Math.ceil(ids.length/cols));const width=cols*nodeW+(cols-1)*gapX+marginX*2;const height=rows*nodeH+(rows-1)*gapY+marginY*2;
  const positions=new Map();const slots=[];for(let r=0;r<rows;r++)for(let c=0;c<cols;c++)slots.push({r,c});
  const main=renderedPath.filter(id=>visibleIds.has(id));
  const mainRow=rows===1?0:Math.floor(rows/2);
  if(main.length&&main.length<=cols){main.forEach((id,i)=>{positions.set(id,{x:marginX+i*(nodeW+gapX),y:marginY+mainRow*(nodeH+gapY),r:mainRow,c:i});});}
  const unused=slots.filter(s=>![...positions.values()].some(p=>p.r===s.r&&p.c===s.c));
  const rest=ids.filter(id=>!positions.has(id));
  rest.forEach((id,i)=>{const slot=unused[i];positions.set(id,{x:marginX+slot.c*(nodeW+gapX),y:marginY+slot.r*(nodeH+gapY),...slot});});
  const scale=Math.min(1,areaWidth/width);$('#canvas').style.height=Math.max(520,Math.ceil(height*scale))+'px';
  const styles=getComputedStyle(document.body),color=k=>styles.getPropertyValue(k).trim();
  const edgeColors={main:color('--edge'),branch:color('--branch')};
  let svg=`<defs>${Object.entries(edgeColors).map(([key,value])=>`<marker id="arrow-${key}" viewBox="0 0 12 12" refX="10" refY="6" markerWidth="10" markerHeight="10" markerUnits="userSpaceOnUse" orient="auto"><path d="M1 1 L11 6 L1 11 Z" fill="${value}"/></marker>`).join('')}</defs><g id="viewport">`;
  const seen=new Set();let edgeCount=0;
  for(const edge of G.edges) {
    if(!positions.has(edge.from)||!positions.has(edge.to))continue;const key=edge.from+'|'+edge.to;if(seen.has(key))continue;seen.add(key);
    const a=positions.get(edge.from),b=positions.get(edge.to);const primary=state.mode==='review'||(pathSet.has(edge.from)&&renderedPath[renderedPath.indexOf(edge.from)+1]===edge.to);
    const type=primary?'main':'branch';let d;
    d=connectionRoute(a,b,nodeW,nodeH,gapX,gapY,edge,primary);
    svg+=`<path class="edge-halo" d="${d}" fill="none" stroke="${color('--panel')}" stroke-width="8"/><path class="edge-line ${type}" data-from="${esc(edge.from)}" data-to="${esc(edge.to)}" d="${d}" fill="none" stroke="${edgeColors[type]}" stroke-width="${primary?3.2:2.5}" opacity="1" marker-end="url(#arrow-${type})"><title>${esc(byId.get(edge.from).name+' 调用 '+byId.get(edge.to).name+' · '+edge.file+':'+edge.line)}</title></path>`;edgeCount++;
  }
  for(const id of ids){const n=byId.get(id),p=positions.get(id),info=describe(n),selected=id===state.selected,entry=id===state.entry,target=targets.has(id),missing=state.mode==='review'&&!claimed().ids.has(id)&&!target;
    const border=selected||target?color('--accent'):pathSet.has(id)?color('--edge'):color('--line');
    svg+=`<g class="graph-node" data-node="${esc(id)}" tabindex="0" role="button" aria-label="${esc(info.title+'，'+n.name+'。'+info.summary)}"><title>${esc(info.title+'\n'+n.name+'\n'+info.summary+'\n'+n.file+':'+n.line+'\n'+info.provenance)}</title><rect class="node-card" x="${p.x}" y="${p.y}" width="${nodeW}" height="${nodeH}" rx="9" fill="${selected||target?color('--accent-bg'):color('--panel')}" stroke="${border}" stroke-width="${selected||target?2:1.5}" ${missing?'stroke-dasharray="6 4"':''}/><rect x="${p.x}" y="${p.y+14}" width="3" height="28" rx="1.5" fill="${entry?color('--edge'):target?color('--accent'):color('--branch')}"/>`;
    svg+=svgText(info.title,p.x+15,p.y+26,nodeW-30,'600 15px sans-serif',19,'node-title',1);
    svg+=svgText(n.name,p.x+15,p.y+49,nodeW-30,'12px Consolas',16,'node-symbol',2);
    svg+=svgText(info.summary,p.x+15,p.y+91,nodeW-30,'12px sans-serif',19,'node-summary',2);
    const file=(n.file.split('/').pop()||'')+':'+n.line;
    svg+=`<line x1="${p.x+15}" x2="${p.x+nodeW-15}" y1="${p.y+133}" y2="${p.y+133}" stroke="${color('--line')}"/>`;
    svg+=svgText(file,p.x+15,p.y+151,nodeW-75,'10px Consolas',12,'node-foot',1);
    svg+=`<text class="node-count" text-anchor="end" x="${p.x+nodeW-13}" y="${p.y+151}">↑${(rev.get(id)||[]).length} ↓${(adj.get(id)||[]).length}</text></g>`;
  }
  svg+='</g>';$('#graph').innerHTML=svg;$('#graph').setAttribute('viewBox',`0 0 ${width} ${height}`);$('#graph').setAttribute('xmlns','http://www.w3.org/2000/svg');applyTransform();
  all('#graph .graph-node').forEach(el=>{const choose=()=>{state.selected=el.dataset.node;renderInspector();renderFiles();drawGraph();};el.onclick=choose;el.ondblclick=()=>focusFunction(el.dataset.node);el.onkeydown=e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();choose();}};});
  all('#graph .edge-line').forEach(el=>el.onclick=()=>{const evidence=G.edges.filter(e=>e.from===el.dataset.from&&e.to===el.dataset.to);openModal('调用关系',`<p>${esc(describe(byId.get(el.dataset.from)).title)} → ${esc(describe(byId.get(el.dataset.to)).title)}</p><p><code>${esc(byId.get(el.dataset.from).name)}</code> 调用 <code>${esc(byId.get(el.dataset.to).name)}</code></p>${evidence.map(e=>`<div class="evidence">${esc(e.file)}:${e.line}<br>${esc(e.resolution)}</div>`).join('')}`);});
  $('#path-count').textContent=`${ids.length} 个函数 · ${edgeCount} 条调用`;
  $('#network-meta-text').textContent=hiddenNodeCount?`已展示 ${ids.length} 个相关函数，另有 ${hiddenNodeCount} 个；可继续展开。`:'当前范围的已解析调用均已展示。';
  $('#expand-network').classList.toggle('hidden',!hiddenNodeCount);
}

function renderInspector(){
  const n=byId.get(state.selected);if(!n){$('#inspector').innerHTML='<p class="muted">选择一个函数查看上下文。</p>';return;}
  const info=describe(n),incoming=G.edges.filter(e=>e.to===n.id),outgoing=G.edges.filter(e=>e.from===n.id);
  const described=['note','comment','model','structural'].includes(info.basis);
  let html=`<h2>函数上下文<span class="badge">${esc(n.language)}</span></h2>`;
  html+=info.title===n.name?`<div class="selected-name">${esc(n.name)}</div>`:`<div class="selected-title">${esc(info.title)}</div><div class="selected-name">${esc(n.name)}</div>`;
  html+=`<div class="location">${esc(n.file)}<br>第 ${n.line}–${n.end_line} 行</div>`;
  if(described)html+=`<p class="role">${esc(info.summary)}</p>`;
  else html+=`<div class="role-missing"><span class="role-missing-label">未生成职责说明</span><p>${info.basis==='none'?'源码没有注释，静态分析只读取函数名与已解析调用，不推断业务职责。下面只保留结构提示。':'标题是按命名规则推导的定位提示，不构成职责说明。下面只保留结构提示。'}</p></div><div class="structural-hint">${esc(info.summary)}</div>`;
  html+=`<div class="source-tag">${esc(info.provenance)}</div>`;
  if(hostApi)html+=`<button id="open-editor" style="margin-top:13px">在编辑器中查看源码</button>`;
  html+='<div class="divider"></div>';
  if(state.mode==='tour'){
    html+=`<h2>这条路径在做什么<span class="small muted">${renderedPath.length?renderedPath.length+' 步':'暂无路径'}</span></h2>`;
    html+=renderedPath.map((id,i)=>{const f=byId.get(id),d=describe(f);return `<div class="step ${id===n.id?'active':''}"><span class="step-no">${i+1}</span><div><button class="step-button" data-select="${esc(id)}">${esc(d.title)}</button><div class="step-symbol">${esc(f.name)}</div><div class="step-note">${esc(d.summary)}</div></div></div>`;}).join('')||'<p class="muted">当前目标未与所选入口形成静态路径。网状图仍显示它的相邻调用。</p>';
  }else{
    const match=claimed(),upstream=currentImpact(),unmentioned=[...upstream].filter(id=>!match.ids.has(id)&&!selectedTargets().includes(id));
    html+=`<h2>声明对照</h2><p class="muted">${match.ids.size} 个函数被明确提及，${unmentioned.length} 个潜在上游未提及。</p>${match.ambiguous.length?`<p class="accent">同名函数需要消歧：${esc(match.ambiguous.join('、'))}</p>`:''}`;
  }
  function neighbors(edges,direction){const seen=new Set();return edges.filter(e=>{const id=direction==='in'?e.from:e.to;if(seen.has(id))return false;seen.add(id);return true;}).slice(0,12).map(e=>{const f=byId.get(direction==='in'?e.from:e.to);return `<button class="relation-button" data-select="${esc(f.id)}"><span class="relation-name">${esc(describe(f).title)}</span><code>${esc(f.name)}</code><span>${esc(e.file)}:${e.line}</span></button>`;}).join('');}
  html+=`<div class="divider"></div><h2>谁调用它<span class="small muted">${new Set(incoming.map(e=>e.from)).size}</span></h2>${neighbors(incoming,'in')||'<p class="muted">暂无已解析的直接调用方。</p>'}<div class="divider"></div><h2>它继续调用谁<span class="small muted">${new Set(outgoing.map(e=>e.to)).size}</span></h2>${neighbors(outgoing,'out')||'<p class="muted">暂无已解析的直接被调用方。</p>'}<details style="margin-top:20px"><summary style="cursor:pointer;font-size:13px">查看函数源码</summary><pre class="code">${esc(n.source)}</pre></details>`;
  $('#inspector').innerHTML=html;all('#inspector [data-select]').forEach(b=>b.onclick=()=>{state.selected=b.dataset.select;if(!visibleIds.has(state.selected))focusFunction(state.selected);else{renderInspector();renderFiles();drawGraph();}});
  if($('#open-editor'))$('#open-editor').onclick=()=>postHost({type:'openSource',id:n.id,baseline:false});
}
function inlineMarkdown(text){return esc(text).replace(/`([^`]+)`/g,'<code>$1</code>').replace(/\*\*([^*]+)\*\*/g,'<strong>$1</strong>');}
function markdownReading(text){
  const lines=String(text).split(/\r?\n/);let html='',list='',code=false,codeText=[];
  const closeList=()=>{if(list){html+=`</${list}>`;list='';}};
  for(const line of lines){if(line.trim().startsWith('```')){closeList();if(code){html+=`<pre>${esc(codeText.join('\n'))}</pre>`;codeText=[];}code=!code;continue;}if(code){codeText.push(line);continue;}
    const heading=line.match(/^(#{1,3})\s+(.+)$/),bullet=line.match(/^\s*[-*]\s+(.+)$/),number=line.match(/^\s*\d+[.)]\s+(.+)$/);
    if(heading){closeList();html+=`<h${heading[1].length}>${inlineMarkdown(heading[2])}</h${heading[1].length}>`;}
    else if(bullet||number){const kind=bullet?'ul':'ol';if(list!==kind){closeList();html+=`<${kind}>`;list=kind;}html+=`<li>${inlineMarkdown((bullet||number)[1])}</li>`;}
    else {closeList();if(/^\s*---+\s*$/.test(line))html+='<hr>';else if(line.trim())html+=`<p>${inlineMarkdown(line)}</p>`;}
  }
  closeList();if(codeText.length)html+=`<pre>${esc(codeText.join('\n'))}</pre>`;return html;
}
function reviewText(){
  const ids=currentImpact(),targets=selectedTargets();
  return `# 待确认的静态影响面结论\n\n## 修改范围\n${targets.map(id=>'- '+describe(byId.get(id)).title+'：`'+byId.get(id).name+'`').join('\n')}\n\n## 潜在影响\n- 相关函数（含所选函数）：${ids.size}\n- 可到达入口：${G.entries.filter(id=>ids.has(id)).map(id=>byId.get(id).name).join('、')||'未到达当前入口'}\n\n## 源码快照\n${snapshot()}\n\n## 调用证据\n${G.edges.filter(e=>ids.has(e.from)&&ids.has(e.to)).map(e=>'- `'+byId.get(e.from).name+'` → `'+byId.get(e.to).name+'`，'+e.file+':'+e.line).join('\n')}\n\n## 使用边界\n这是静态可达性线索，不证明运行时行为变化。未提及不等于声明错误。\n`;
}
function renderRecord(){
  const ids=currentImpact(),targets=selectedTargets(),entries=G.entries.filter(id=>ids.has(id)),edges=G.edges.filter(e=>ids.has(e.from)&&ids.has(e.to));
  const date=G.generated_unix?new Date(G.generated_unix*1000).toLocaleString('zh-CN',{timeZone:'Asia/Singapore',hour12:false}):'当前快照';
  $('#review-record').innerHTML=`<dl class="record-summary"><dt>状态</dt><dd>待你复核确认</dd><dt>触发函数</dt><dd>${targets.map(id=>esc(describe(byId.get(id)).title)+'<br><code>'+esc(byId.get(id).name)+'</code>').join('<br>')||'未选择'}</dd><dt>扫描时间</dt><dd>${esc(date)}</dd><dt>潜在影响</dt><dd>${ids.size} 个函数，${entries.length} 个入口</dd><dt>源码快照</dt><dd><code>${esc(snapshot().slice(0,16))}…</code></dd></dl><h3 style="font-size:14px;margin-top:22px">涉及的函数</h3><div class="record-functions">${[...ids].slice(0,15).map(id=>`<span class="record-chip">${esc(describe(byId.get(id)).title)}</span>`).join('')}</div><details style="margin-top:20px"><summary style="font-size:13px;cursor:pointer">查看调用证据 · ${edges.length} 处</summary>${edges.slice(0,30).map(e=>`<div class="record-call">${esc(describe(byId.get(e.from)).title)} → ${esc(describe(byId.get(e.to)).title)}<small>${esc(e.file)} · 第 ${e.line} 行</small></div>`).join('')||'<p class="muted">暂无已解析的调用。</p>'}</details><p class="record-boundary">确认仅针对当前源码快照。静态可达性不证明业务行为发生变化，代码更新后需要重新确认。</p>`;
}
function renderMemory(){
  if(!$('#stable-text').dataset.initialized){
    const entry=byId.get(state.entry),path=bfs(state.entry,state.target)||[];
    $('#stable-text').value=`# ${G.project} 架构记忆\n\n## 入口与职责\n- ${entry?describe(entry).title+'：`'+entry.name+'`':'待确认'}\n\n## 核心流程\n${path.map((id,i)=>`${i+1}. ${describe(byId.get(id)).title}：${describe(byId.get(id)).summary}`).join('\n')||'待确认'}\n\n## 关键决策\n- 调用关系来自静态解析，函数说明只用于帮助阅读。\n\n## 有效性\n- 源码快照：${snapshot().slice(0,16)}\n- 代码更新后重新扫描并确认。\n`;
    $('#stable-text').dataset.initialized='true';
  }
  $('#stable-preview').innerHTML=markdownReading($('#stable-text').value);
  $('#stable-preview').classList.toggle('hidden',memoryEditing);$('#stable-text').classList.toggle('hidden',!memoryEditing);
  $('#memory-read').classList.toggle('active',!memoryEditing);$('#memory-edit').classList.toggle('active',memoryEditing);
  $('#review-export-status').textContent=confirmedReviewKey===state.target+'|'+state.claim?'已确认并导出':'尚未确认';
  renderRecord();updateMemorySize();$('#query-example').textContent='code-trail query <graph.json> impact "'+(byId.get(state.target)?.name||'<function>')+'"';
}
function updateMemorySize(){
  const text=$('#stable-text').value,bytes=new TextEncoder().encode(text).length;
  if(hostApi){const revision=++tokenRevision;latestTokens=null;clearTimeout(tokenTimer);tokenTimer=setTimeout(()=>postHost({type:'countTokens',text,revision}),250);$('#memory-size').textContent='正在计算 token 预算…';$('#export-stable').disabled=true;}
  else {$('#memory-size').textContent=`${bytes.toLocaleString()} / 1,500 UTF-8 字节 · 离线预算`;$('#memory-bar').style.width=Math.min(100,bytes/1500*100)+'%';$('#memory-bar').style.background=bytes>1500?'var(--accent)':'var(--green)';$('#export-stable').disabled=bytes>1500||!text.trim();}
}
function updateImpact(){const ids=currentImpact(),entries=G.entries.filter(id=>ids.has(id)),match=claimed(),missing=[...ids].filter(id=>!match.ids.has(id)&&!selectedTargets().includes(id));$('#impact-summary').innerHTML=`<strong>${ids.size}</strong> 个潜在受影响函数<br>${entries.length} 个入口可达 · ${missing.length} 个上游未在声明中提及`;}
function render(){
  const memory=state.mode==='memory',settings=state.mode==='settings',review=state.mode==='review';
  document.body.classList.toggle('review-mode',review);all('[data-mode]').forEach(b=>b.classList.toggle('active',b.dataset.mode===state.mode));
  $('#graph-workspace').classList.toggle('hidden',memory||settings);$('#memory-workspace').classList.toggle('hidden',!memory);$('#settings-workspace').classList.toggle('hidden',!settings);
  $('#claims-panel').classList.toggle('hidden',!review);$('#entry-control').classList.toggle('hidden',review);$('#depth-control').classList.toggle('hidden',review);$('#view-switch').classList.toggle('hidden',review);
  $('#mode-crumb').textContent={tour:'架构导览',review:'修改影响',memory:'架构记忆',settings:'API 设置'}[state.mode];
  $('#eyebrow').textContent=G.project;$('#headline').textContent=settings?'连接你的模型服务。':memory?'把已确认的理解，留到下一次。':review?'看清这次修改的潜在影响。':(viewOptions.title||'先看懂职责，再沿调用关系探索。');
  $('#description').textContent=settings?'设置 API URL、Model ID 和 Key。':memory?'默认以阅读视图展示，编辑和导出时保留 Markdown 格式。':review?'调用方 → 被调用方；声明未提及的上游仅作为人工复核线索。':'每个节点包含功能简述；点击查看上下文，双击将函数设为新的探索目标。';
  $('#panel-title').textContent=review?'潜在上游调用网络':state.view==='network'?'函数调用网络':'入口到目标的最短路径';$('#target-label').textContent=review?'修改函数':'目标函数';
  all('[data-view]').forEach(b=>b.classList.toggle('active',b.dataset.view===state.view));$('#depth').value=state.depth;$('#depth-value').textContent=state.depth;
  $('#legend-main').textContent=review?'反向影响链':'主路径调用';$('#legend-side').textContent='旁支调用（清晰实线）';
  $('#boundary-notice').textContent=`已解析调用均保留文件与行号。${G.unresolved.length} 处调用未纳入图，${diagnosticsLabel()||'没有文件诊断'}。点击「分析边界」或统计第 4 项可查看原因与下一步。函数说明标明来源；名称提示待人工核对。动态调用或框架隐式连接可能遗漏。`;
  renderDiagnosticsBanner();
  $('#command-preview').textContent='code-trail query graph.json '+(review?'impact "'+(byId.get(state.target)?.name||'<function>')+'"':'path "'+(byId.get(state.entry)?.name||'<entry>')+'" "'+(byId.get(state.target)?.name||'<target>')+'"');
  if(memory)renderMemory();else if(!settings){drawGraph();renderInspector();if(review)updateImpact();}
  const path=renderedPath.map(id=>`<span class="flow-step">${esc(describe(byId.get(id)).title)}</span>`).join('<span class="flow-arrow">→</span>');
  $('#flow-steps').innerHTML=path||'<span class="muted">当前范围未找到静态到达路径，可在网状图中查看相邻调用。</span>';
  $('#flow-reading').classList.toggle('hidden',memory||settings||review);renderFiles();
}

// Add reader controls without coupling the static viewer to an editor runtime.
$('.nav').insertAdjacentHTML('beforeend','<button data-mode="settings"><span>⚙</span>API 设置<span class="key">04</span></button>');
$('#graph-workspace .controls').insertAdjacentHTML('afterbegin','<div class="view-switch" id="view-switch"><button data-view="network" type="button">网状调用图</button><button data-view="path" type="button">路径导览</button></div>');
$('#graph-workspace .controls').insertAdjacentHTML('afterend','<div class="flow-reading" id="flow-reading"><span class="flow-label">这条路径在做什么</span><div class="flow-steps" id="flow-steps"></div></div>');
$('#canvas').insertAdjacentHTML('afterend','<div class="network-meta"><span id="network-meta-text"></span><button id="expand-network">展开更多函数</button></div>');
$('#graph-workspace .work-tools').insertAdjacentHTML('afterbegin','<button id="maximize-graph">放大阅读</button>');
$('#canvas-label')?.remove();$('.intro-note')?.remove();$('.legend .basis')?.remove();
$('#graph-workspace').before($('#settings-workspace'));
$('#stable-text').insertAdjacentHTML('beforebegin','<div class="memory-view-tabs"><button id="memory-read" type="button" class="active">阅读视图</button><button id="memory-edit" type="button">编辑 Markdown</button></div><article id="stable-preview" class="memory-preview markdown-reading"></article>');
$('#stable-text').classList.add('hidden');$('#review-record').classList.remove('code');
$('#search').placeholder='查找名称或功能…  Ctrl K';
$('#depth-control').firstChild.textContent='扩展层级';
const scoped=scopeFunctions();
const entryNodes=[...G.entries.map(id=>byId.get(id)).filter(Boolean),...scoped.filter(n=>!G.entries.includes(n.id))];
$('#entry-select').innerHTML=entryNodes.map(option).join('');$('#entry-select').value=state.entry;
$('#target-select').innerHTML=scoped.map(option).join('');$('#target-select').value=state.target;
$('#repo-name').textContent=G.project;$('#crumb').textContent=G.project;
const fileCount=new Set(G.nodes.filter(inScope).map(n=>n.file)).size;
const scopeEdges=G.edges.filter(e=>inScope(byId.get(e.from))&&inScope(byId.get(e.to)));
const scopeUnresolved=G.unresolved.filter(e=>byId.has(e.from)&&inScope(byId.get(e.from)));
function boundaryStatHtml(value){return `<div class="stat stat-action" id="stat-boundary" role="button" tabindex="0" title="未纳入图的调用不影响已解析边；点击查看按原因聚合与文件诊断"><strong>${value}</strong><span>未纳入图的调用</span><span class="foot">原因 ›</span></div>`;}
function bindBoundaryStat(){const el=$('#stats');if(!el||el.dataset.bound)return;el.dataset.bound='1';const open=e=>{if(e.type==='keydown'&&!(e.key==='Enter'||e.key===' '))return;if(!e.target.closest('#stat-boundary'))return;e.preventDefault();openBoundaryModal();};el.addEventListener('click',open);el.addEventListener('keydown',open);}
$('#stats').innerHTML=[[fileCount,'源码文件'],[scoped.length,'具名函数'],[scopeEdges.length,'已解析调用']].map(([number,label])=>`<div class="stat"><strong>${number}</strong><span>${label}</span></div>`).join('')+boundaryStatHtml(scopeUnresolved.length);
bindBoundaryStat();
all('[data-mode]').forEach(b=>b.onclick=()=>{state.mode=b.dataset.mode;resetView();render();if(state.mode==='settings'&&hostApi)postHost({type:'settings'});if(state.mode==='memory'&&hostApi)postHost({type:'memoryStatus'});});
all('[data-view]').forEach(b=>b.onclick=()=>{state.view=b.dataset.view;state.depth=state.view==='network'?1:0;state.limit=9;resetView();render();});
$('#depth').oninput=e=>{state.depth=+e.target.value;state.limit=9;resetView();render();};
$('#entry-select').onchange=e=>{state.entry=e.target.value;state.limit=9;resetView();render();};
$('#target-select').onchange=e=>focusFunction(e.target.value);
$('#expand-network').onclick=()=>{state.limit+=9;resetView();render();};
$('#maximize-graph').onclick=()=>{const active=document.body.classList.toggle('is-maximized');$('#maximize-graph').textContent=active?'退出大图':'放大阅读';resetView();drawGraph();};
$('#api-settings-button').onclick=()=>{state.mode='settings';render();if(hostApi)postHost({type:'settings'});};
$('#memory-read').onclick=()=>{memoryEditing=false;renderMemory();};$('#memory-edit').onclick=()=>{memoryEditing=true;renderMemory();$('#stable-text').focus();};
$('#stable-text').oninput=()=>{memoryDirty=true;$('#stable-preview').innerHTML=markdownReading($('#stable-text').value);updateMemorySize();};
$('#export-stable').onclick=()=>{const text=$('#stable-text').value;if($('#export-stable').disabled)return;openModal('确认这份架构记忆？',`<p>复核后下载 Markdown 文件，保留当前入口、流程和关键决策。</p><article class="markdown-reading">${markdownReading(text)}</article>`,()=>{download('ARCHITECTURE.md',text);toast(hostApi?'已请求选择保存位置':'架构记忆已确认并导出');});};
$('#export-review').onclick=()=>{const text=reviewText();openModal('确认这份静态影响面结论？',`<p>请先核对函数职责与调用证据。确认仅针对当前源码快照。</p><article class="markdown-reading">${markdownReading(text)}</article>`,()=>{download('review-'+G.generated_unix+'.md',text.replace('待确认的','已人工确认的'));confirmedReviewKey=state.target+'|'+state.claim;$('#review-export-status').textContent='已确认并导出';});};
const portableDownload=download;
download=function(name,content,type='text/plain;charset=utf-8'){if(hostApi)postHost({type:'export',name,content});else portableDownload(name,content,type);};
function applyApi(config){$('#api-provider').value=config.provider;$('#api-url').value=config.apiBaseUrl;$('#api-model').value=config.model;$('#api-models-url').value=config.modelsUrl||'';$('#api-key').value='';$('#api-key').placeholder=config.hasApiKey?'当前地址已有 Key；留空保留':'输入 API Key';$('#settings-scope').textContent=config.keyScope||'VS Code SecretStorage';}
$('#api-form').onsubmit=e=>{e.preventDefault();const config={provider:$('#api-provider').value,apiBaseUrl:$('#api-url').value,model:$('#api-model').value,modelsUrl:$('#api-models-url').value,apiKey:$('#api-key').value};if(hostApi){postHost({type:'saveConfig',config});$('#api-status').textContent='正在保存…';}else{$('#api-status').textContent='当前为离线 HTML，可预览设置布局；保存与连接测试请在 VS Code 扩展中执行。';}$('#api-key').value='';};
$('#api-list').onclick=()=>postHost({type:'listModels'});$('#api-test').onclick=()=>postHost({type:'testConnection'});$('#api-delete').onclick=()=>postHost({type:'clearKey'});
$('#api-provider').onchange=e=>{if(!$('#api-url').value||['https://api.openai.com/v1','https://api.anthropic.com/v1'].includes($('#api-url').value))$('#api-url').value=e.target.value==='anthropic'?'https://api.anthropic.com/v1':'https://api.openai.com/v1';};
$('#help').onclick=()=>openModal('沿着职责与调用关系阅读代码',`<article class="markdown-reading"><h2>网状调用图</h2><p>同时展示直接调用方与被调用方；浅绿色为主路径，暖色实线为旁支。箭头指向被调用函数。扩展层级增加邻接范围，展开更多按钮增加可见函数数量。</p><h2>函数节点</h2><p>上方是功能名称：来自源码注释时直接展示，否则是按命名规则推导的定位提示。下方保留真实标识符与源码位置。没有注释的函数会明确标注「未生成职责说明」，而不是补写猜测。</p><h2>分析边界</h2><p>统计第 4 项「未纳入图的调用」与顶部「分析边界」提示都可以打开边界说明：按原因聚合未解析调用、列出文件诊断，并给出下一步。数量不是缺陷率。</p><h2>有界上下文</h2><p>默认展示排版后的内容；需要修改时切换编辑 Markdown，导出与确认预览保持正常阅读排版。</p></article>`);
let resizeTimer;window.addEventListener('resize',()=>{clearTimeout(resizeTimer);resizeTimer=setTimeout(()=>{if(['tour','review'].includes(state.mode)){resetView();drawGraph();}},150);});
document.addEventListener('keydown',e=>{if(e.key==='Escape'&&!$('#modal').open&&document.body.classList.contains('is-maximized')){$('#maximize-graph').click();}});
window.addEventListener('message',event=>{const m=event.data;if(!m||typeof m!=='object')return;
  if(m.type==='hostReady'||m.type==='apiConfig'){if(m.config)applyApi(m.config);if(m.message)$('#api-status').textContent=m.message;if(m.type==='hostReady')$('#api-status').textContent='API Key 由 SecretStorage 保存；网页不直接访问模型接口。';if(m.memory)currentMemory=m.memory;}
  else if(m.type==='models'){ $('#api-models').innerHTML=m.models.map(id=>`<option value="${esc(id)}"></option>`).join('');$('#api-status').textContent=m.message;}
  else if(m.type==='connection')$('#api-status').textContent=m.message;
  else if(m.type==='error'){toast(m.message);$('#api-status').textContent=m.message;}
  else if(m.type==='mode'){state.mode=m.mode;render();}
  else if(m.type==='claim'){state.claim=m.text;$('#claim-text').value=m.text;render();}
  else if(m.type==='stale'){toast('源码已变化；重新扫描后再确认记忆。');}
  else if(m.type==='roles'){for(const[id,summary]of Object.entries(m.roles)){const n=byId.get(id);if(n){n.role=String(summary);n.role_source='llm-unverified';}}render();}
  else if(m.type==='tokens'&&m.revision===tokenRevision){latestTokens=m.tokens;$('#memory-size').textContent=`${m.tokens} / ${m.limit} tokens · ${m.tokenizer}`;$('#memory-bar').style.width=Math.min(100,m.tokens/m.limit*100)+'%';$('#export-stable').disabled=m.tokens>m.limit||!$('#stable-text').value.trim();}
  else if(m.type==='memoryStatus'){currentMemory=m.memory;if(!memoryDirty&&currentMemory.stable){$('#stable-text').value=currentMemory.stable;$('#stable-text').dataset.initialized='true';}if(state.mode==='memory')renderMemory();if(m.message)toast(m.message);}
});
// Boot is performed by explorer.js after navigation and animation state is ready.
