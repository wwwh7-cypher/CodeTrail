
'use strict';
let G=JSON.parse(document.getElementById('graph-data').textContent);
const $=s=>document.querySelector(s), all=s=>[...document.querySelectorAll(s)];
const esc=value=>String(value??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const byId=new Map(G.nodes.map(n=>[n.id,n]));
const adj=new Map(), rev=new Map();
for(const n of G.nodes){adj.set(n.id,[]);rev.set(n.id,[])}
for(const e of G.edges){if(!adj.get(e.from).includes(e.to))adj.get(e.from).push(e.to);if(!rev.get(e.to).includes(e.from))rev.get(e.to).push(e.from)}
let functions=G.nodes.filter(n=>n.kind!=='module');
let choiceNodes=functions.length?functions:G.nodes;
let initial=byId.get(G.changed[0])||functions.find(n=>n.name==='parseConfig')||[...choiceNodes].sort((a,b)=>(rev.get(b.id)?.length||0)-(rev.get(a.id)?.length||0))[0];
const state={mode:'tour',entry:G.entries[0]||G.nodes[0]?.id||'',target:initial?.id||'',selected:initial?.id||'',depth:0,zoom:1,panX:0,panY:0,claim:G.claim||''};
let renderedPath=[], visibleIds=new Set(), pendingDownload=null, toastTimer, confirmedReviewKey='';
const snapshot=()=>G.snapshot;
function bfs(from,to){if(!byId.has(from)||!byId.has(to))return null;const q=[from],prev=new Map([[from,null]]);for(let i=0;i<q.length;i++){const id=q[i];if(id===to){const path=[];let at=id;while(at!==null){path.push(at);at=prev.get(at)}return path.reverse()}for(const next of adj.get(id)||[]){if(!prev.has(next)){prev.set(next,id);q.push(next)}}}return null}
function reach(id,map){const seen=new Set(id?[id]:[]),q=[...seen];for(let i=0;i<q.length;i++){for(const next of map.get(q[i])||[]){if(!seen.has(next)){seen.add(next);q.push(next)}}}return seen}
function claimed(){const ids=new Set(),ambiguous=[];for(const name of new Set(functions.map(n=>n.name))){if(name.startsWith('<'))continue;const escaped=name.replace(/[.*+?^${}()|[\]\\]/g,'\\$&');if(new RegExp('(?<![\\p{L}\\p{N}_$])'+escaped+'(?![\\p{L}\\p{N}_$])','u').test(state.claim)){const matches=functions.filter(n=>n.name===name);if(matches.length===1)ids.add(matches[0].id);else ambiguous.push(name)}}return{ids,ambiguous}}
function toast(message){$('#toast').textContent=message;$('#toast').classList.remove('hidden');clearTimeout(toastTimer);toastTimer=setTimeout(()=>$('#toast').classList.add('hidden'),3200)}
function download(name,content,type='text/plain;charset=utf-8'){const url=URL.createObjectURL(new Blob([content],{type}));const a=document.createElement('a');a.href=url;a.download=name;a.click();setTimeout(()=>URL.revokeObjectURL(url),1500)}
function openModal(title,html,action=null){$('#modal-title').textContent=title;$('#modal-body').innerHTML=html;pendingDownload=action;$('#modal-confirm').classList.toggle('hidden',!action);$('#modal').showModal()}
// Static-analysis boundaries are first-class: diagnostics and unresolved calls stay visible and explainable.
const UNRESOLVED_REASONS={
 'missing-owner':'调用所属函数缺失，图数据不完整',
 'shadowed-or-assigned-binding':'调用名被局部变量或赋值遮蔽',
 'shadowed-receiver':'接收者名称被局部变量遮蔽',
 'ambiguous-local-symbol':'同一作用域存在多个同名函数，无法唯一匹配',
 'dynamic-call':'动态调用：没有可静态确定的接收者',
 'external-or-unknown-symbol':'外部库或未收录的符号',
 'dynamic-or-unknown-receiver':'动态或未知接收者（框架回调、链式调用等）',
 'external-or-unresolved-module':'导入的模块不在扫描范围内',
 'ambiguous-import':'同一个导入名对应多个来源',
 'missing-export-or-re-export':'导入目标没有导出，或经由再导出',
 'ambiguous-export':'目标文件存在多个同名导出',
 'unknown-method-context':'方法缺少所属类上下文',
 'unknown-or-ambiguous-method':'this / self 方法无法唯一确定',
 'go-external-or-unknown-symbol':'Go：包外或未识别的符号',
 'go-ambiguous-package-symbol':'Go：同包内存在多个同名符号',
 'go-unknown-struct-field-type':'Go：结构体字段类型未知，无法继续追踪',
 'go-interface-or-unknown-method':'Go：接口方法或未知接收者类型',
 'go-unknown-receiver-method':'Go：无法静态确定接收者方法',
 'go-selector-requires-type-resolution':'Go：选择器调用需要完整类型解析'
};
const DIAGNOSTIC_NOTES=[
 ['文件超过 2 MB','文件体积超过分析上限（2 MB），其中的函数与调用不会进入图。'],
 ['非 UTF-8','文件无法按 UTF-8 解码，未纳入分析。'],
 ['语法恢复','解析器遇到未支持或较新的语法；该文件已识别的调用仍保留，其余部分可能缺失。'],
 ['无法读取目录项','目录读取失败，相关文件可能整体缺失。'],
 ['同名符号无法跨版本','基线匹配存在歧义，该符号被保守标为修改，需人工核对。'],
 ['Git archive','版本对比的基线来自 Git archive；文件重命名按删除 / 新增呈现。']
];
function reasonLabel(reason){return UNRESOLVED_REASONS[reason]||'未分类原因';}
function diagnosticNote(text){const hit=DIAGNOSTIC_NOTES.find(([needle])=>String(text).includes(needle));return hit?hit[1]:'扫描器报告该诊断；详情见文件与描述。';}
function diagnosticFilePath(text){const m=String(text).match(/^(.+?):(?:\d+:\d+:)?\s/);return m?m[1]:null;}
function diagnosticText(text){const path=diagnosticFilePath(text);if(!path)return String(text);const rest=String(text).slice(path.length+1).trim();const located=rest.match(/^(\d+:\d+):\s*(.*)$/);return located?`第 ${located[1]} 行 · ${located[2]}`:rest;}
function diagnosticsSummary(){const files=new Set();let unfiled=0;for(const d of G.diagnostics||[]){const path=diagnosticFilePath(d);if(path)files.add(path);else unfiled++;}return {files:[...files],unfiled,total:(G.diagnostics||[]).length};}
function diagnosticsLabel(){const s=diagnosticsSummary();if(!s.total)return '';if(s.files.length&&!s.unfiled)return `${s.files.length} 个文件不完整`;return `${s.total} 条诊断${s.files.length?`（涉及 ${s.files.length} 个文件）`:''}`;}
function unresolvedReasons(){const counts=new Map();for(const call of G.unresolved||[]){const key=call.reason||'unknown';counts.set(key,(counts.get(key)||0)+1);}return [...counts.entries()].sort((a,b)=>b[1]-a[1]||a[0].localeCompare(b[0]));}
function boundaryModalHtml(){
 const diagnostics=(G.diagnostics||[]).length?`<h3>文件诊断 · ${G.diagnostics.length}</h3><ul class="boundary-list">${G.diagnostics.map(d=>{const path=diagnosticFilePath(d);return `<li>${path?`<code>${esc(path)}</code> `:''}${esc(diagnosticText(d))}<div class="boundary-note">${esc(diagnosticNote(d))}</div></li>`;}).join('')}</ul><p class="small subtle">以上文件的分析结果可能不完整；已识别的调用仍然保留，未识别部分不会出现在图中。</p>`:'<h3>文件诊断 · 0</h3><p class="small subtle">本次扫描没有报告文件诊断：纳入范围内的文件都完成了静态解析。</p>';
 const reasons=unresolvedReasons(),reasonTotal=reasons.reduce((sum,[,count])=>sum+count,0);
 const breakdown=`<h3>未纳入图的调用 · ${reasonTotal} 处</h3><p class="small subtle">数量不等于缺陷率：多数来自静态分析无法确定接收者类型的外部调用或动态分发。按原因聚合如下。</p>${reasons.length?`<ul class="boundary-reasons">${reasons.map(([reason,count])=>`<li><span class="reason-label">${esc(reasonLabel(reason))}</span><span class="reason-code">${esc(reason)}</span><strong>${count}</strong></li>`).join('')}</ul>`:''}`;
 const samples=(G.unresolved||[]).length?`<details class="boundary-details"><summary>查看前 ${Math.min(G.unresolved.length,80)} 条调用样本</summary><pre>${esc(G.unresolved.slice(0,80).map(c=>c.file+':'+c.line+'  '+c.expression+'\n  ↳ '+c.reason).join('\n'))}</pre>${G.unresolved.length>80?'<p class="small subtle">面板仅展示前 80 处；完整列表保存在图 JSON，可用 code-trail query 检索。</p>':''}</details>`:'';
 const reviewNotes=G.review?.diagnostics?.length?`<h3>版本对比说明 · ${G.review.diagnostics.length}</h3><ul class="boundary-list">${G.review.diagnostics.map(d=>{const path=diagnosticFilePath(d);return `<li>${path?`<code>${esc(path)}</code> `:''}${esc(diagnosticText(d))}<div class="boundary-note">${esc(diagnosticNote(d))}</div></li>`;}).join('')}</ul>`:'';
 const next='<h3>下一步</h3><ul class="boundary-next"><li>在 VS Code 扩展中可按需启用语言服务补强，补充调用层级（仍不证明运行时完整）。</li><li>结合入口、测试与运行日志确认关键调用；静态可达性只是复核线索。</li><li>文件诊断对应的文件可单独打开阅读，或调整扫描范围后重新扫描。</li><li>函数说明均标明来源：源码注释与人工确认可用；名称提示待人工核对。</li></ul>';
 return `<p>这张图来自本地静态分析，保留每条已解析调用的文件与行号，但不推断运行时行为。以下内容说明它在哪里不完整。</p>${diagnostics}${breakdown}${samples}${reviewNotes}${next}`;
}
function openBoundaryModal(){openModal('静态分析的边界',boundaryModalHtml());}
function renderDiagnosticsBanner(){const el=$('#diagnostics-banner');if(!el)return;const total=(G.diagnostics||[]).length;el.classList.toggle('hidden',!total);if(!total)return;el.innerHTML=`<span class="symbol">⊙</span><span><strong>分析边界</strong> · ${esc(diagnosticsLabel())}</span><span class="hint">点击查看原因与下一步</span>`;}
function resetView(){state.zoom=1;state.panX=0;state.panY=0}
function applyTransform(){const group=$('#viewport');if(group)group.setAttribute('transform',`translate(${state.panX} ${state.panY}) scale(${state.zoom})`)}
// All nodes can be selected as an entry; inferred/explicit entries appear first.
all('[data-mode]').forEach(b=>b.onclick=()=>{state.mode=b.dataset.mode;resetView();render()});$('#search').oninput=renderFiles;$('#entry-select').onchange=e=>{state.entry=e.target.value;resetView();render()};$('#target-select').onchange=e=>{state.target=e.target.value;state.selected=e.target.value;resetView();render()};$('#depth').oninput=e=>{state.depth=+e.target.value;$('#depth-value').textContent=state.depth;resetView();render()};$('#claim-text').oninput=e=>{state.claim=e.target.value;updateImpact();drawGraph();renderInspector()};$('#theme').onclick=()=>{document.body.classList.toggle('light');if(state.mode!=='memory')drawGraph()};$('#zoom-in').onclick=()=>{state.zoom=Math.min(4,state.zoom*1.2);applyTransform()};$('#zoom-out').onclick=()=>{state.zoom=Math.max(.3,state.zoom/1.2);applyTransform()};$('#zoom-reset').onclick=()=>{resetView();applyTransform()};
$('#export-svg').onclick=()=>{const svg=$('#graph').cloneNode(true);svg.setAttribute('width',svg.viewBox.baseVal.width);svg.setAttribute('height',svg.viewBox.baseVal.height);svg.querySelector('#viewport')?.removeAttribute('transform');download('code-trail-'+state.mode+'.svg',new XMLSerializer().serializeToString(svg),'image/svg+xml');toast('已导出当前可见节点，完整画布 SVG')};
$('#show-unresolved').onclick=openBoundaryModal;$('#diagnostics-banner').onclick=openBoundaryModal;
$('#help').onclick=()=>openModal('跟着证据，读懂代码。','<p><b>架构导览</b>：选择入口与目标，BFS 找到最短静态路径；拖动深度滑块展开出边旁支。点击节点查看源码与调用证据。</p><p><b>修改影响</b>：手动选择一个修改函数，粘贴 Agent 声明。虚线标注声明未提及的潜在上游。当前版本按唯一函数名匹配，不推断自然语言语义。</p><p><b>架构记忆</b>：编辑稳定层草稿，人工确认后导出。审查记录独立下载，供按需查询；代码更新后应重新确认。</p><p>拖动空白处平移，使用 ＋ / − 缩放。Ctrl / ⌘ K 查找函数。所有操作均在本地，无外部依赖和网络请求。</p>');
$('#stable-text').oninput=updateMemorySize;$('#export-stable').onclick=()=>{const value=$('#stable-text').value;if(new TextEncoder().encode(value).length>1500||!value)return;openModal('确认这份稳定层记忆？',`<p>确认内容已反映你的架构理解。此操作下载一个 Markdown 文件；不会覆盖仓库内的文件。</p><pre>${esc(value)}</pre>`,()=>{download('ARCHITECTURE.md',value);toast('稳定层已确认并导出')})};$('#export-review').onclick=()=>{const value=reviewText();openModal('确认这份审查结论？',`<p>请复核调用证据和入口。确认仅针对当前快照；静态可达性不证明业务行为发生变化。</p><pre>${esc(value)}</pre>`,()=>{download('review-'+G.generated_unix+'.md',value.replace('# 待确认的静态影响面结论','# 已人工确认的静态影响面结论'));confirmedReviewKey=state.target+'|'+state.claim;$('#review-export-status').textContent='当前快照已确认并导出';toast('审查记录已确认并导出')})};$('#modal-cancel').onclick=()=>$('#modal').close();$('#modal-confirm').onclick=()=>{pendingDownload?.();$('#modal').close()};$('#modal').onclick=e=>{if(e.target===$('#modal')){const r=$('#modal').getBoundingClientRect();if(e.clientX<r.left||e.clientX>r.right||e.clientY<r.top||e.clientY>r.bottom)$('#modal').close()}};document.addEventListener('keydown',e=>{if((e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==='k'){e.preventDefault();$('#search').focus()}});
