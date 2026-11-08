import fs from 'node:fs';import path from 'node:path';import {fileURLToPath} from 'node:url';import {execFileSync} from 'node:child_process';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..'),out=path.join(root,'output/research-atlas'),binary=process.argv[2]||path.join(root,'bin/code-trail.exe');
const g=JSON.parse(fs.readFileSync(path.join(out,'architecture.graph.json'),'utf8'));
const descriptions={
 'backend/cmd/lumina/main.go':{main:['启动 Go 服务','读取配置并打开本地知识库，按配置接入嵌入与生成服务；启动 HTTP 服务并处理优雅关闭。跨包调用不一定能被当前 Rust 索引解析。']},
 'backend/internal/lumina/config.go':{ConfigFromEnv:['读取服务环境配置','从环境变量读取监听地址、数据目录、令牌和模型接口配置，检查超时并校验配置。']},
 'backend/internal/lumina/http.go':{
  NewHandler:['组装后端 HTTP 路由','注册项目快照、索引、问答、摘要、文献检索和 PDF 等路由。匿名路由回调独立建模，注册回调不等于直接执行回调。'],
  '<callback@141:59>':['处理项目问答或摘要请求','读取请求 JSON，将项目 ID、问答参数与 summary 开关交给 Service.Answer；把结果或错误写入响应。'],
  guard:['检查请求来源与访问令牌','设置响应头，检查本地访问 Host、跨域来源与 API Bearer 令牌，再调用后续处理器。'],
  writeJSON:['写入 JSON 响应','设置 JSON 响应类型与状态码，并编码响应对象。'],
  decodeJSON:['读取并校验请求 JSON','限制请求体大小，拒绝未知字段、多余 JSON 内容和解码失败。']},
 'backend/internal/lumina/rag.go':{
  validateSnapshot:['校验项目知识快照','检查项目、文档、分块的数量与归属，校验研究资料，重建文档分块计数并清空输入向量。'],
  Import:['导入项目知识快照','读取项目修订号并校验快照；按需构建嵌入索引，确认上下文未取消后通过 Store.Replace 提交，再返回新快照。'],
  buildIndex:['批量构建嵌入索引','每批最多 32 个分块请求嵌入，归一化并检查向量维度及总值预算，将向量和模型签名写入快照。接口方法调用仍可能未解析。'],
  Reindex:['重建项目向量索引','读取当前快照，重新构建嵌入索引；以原修订号提交替换，避免并发覆盖。'],
  lexicalScores:['计算词法检索分数','为问题与分块建立 TF-IDF 权重，计算余弦分数；不依赖外部嵌入模型。'],
  evidenceFor:['整理可引用证据','把命中的分块及所属文档整理为带编号、来源、页码与得分的证据；总内容受字符预算约束。'],
  retrieve:['检索项目相关分块','优先按已配置的嵌入模型计算余弦相似度，否则使用 TF-IDF；按得分选择最多 12 个分块并整理证据。'],
  Answer:['生成有证据的回答或摘要','读取项目快照；问答走相关检索，无问题的摘要按文档均衡选取证据。未配置生成模型时返回原文摘录，返回前检查修订冲突。']},
 'backend/internal/lumina/store.go':{
  OpenStore:['打开本地 JSON 知识库','初始化工作区标识，读取并校验 knowledge.json；新工作区可从空数据开始。数据大小与格式版本均有限制。'],
  Get:['读取独立项目快照','在读锁下查询项目，并返回深复制快照，避免调用者直接修改存储状态。'],
  Epoch:['读取项目修订号','在读锁下返回项目修订计数，供乐观并发检查使用。'],
  commit:['原子提交项目状态','检查期望修订号，复制下一版状态；写入临时 JSON 文件并同步、重命名成功后，替换内存中的状态。'],
  DeleteDocument:['删除文档及其分块','读取项目，筛除指定文档和关联分块，再按当前修订号提交替换；文档不存在时返回错误。']},
 'backend/internal/lumina/providers.go':{
  NewHTTPProvider:['创建模型 HTTP 适配器','使用指定配置和超时创建客户端，禁止模型接口重定向；默认超时为 90 秒。'],
  post:['请求模型服务 JSON 接口','将输入编码为 JSON，附加配置的 Bearer Key，执行受限响应读取并校验状态及 JSON 格式。'],
  Embed:['调用文本嵌入服务','支持 OpenAI 与 Ollama 协议，限制单批数量；检查返回数量、索引、维度并归一化向量。'],
  Generate:['调用有证据的生成模型','把问题和证据发送给 OpenAI 或 Ollama，要求依据证据回答并注明局限；校验响应是否完整。']},
 'backend/internal/lumina/papers.go':{search:['检索 Crossref 文献','校验检索词，向 Crossref 查询论文并把作者、DOI、期刊和 PDF 链接整理为统一响应。'],pdf:['代理允许来源的 PDF','限制 PDF 来源与重定向目标，验证响应大小及 PDF 文件头后返回文件。']},
 'backend/internal/lumina/network.go':{network:['读取论文引用关系','校验论文身份、方向与分页，向 Semantic Scholar 请求引用或参考文献，清理并返回关系结果。']}
};
const nodes=g.nodes.filter(n=>descriptions[n.file]?.[n.name]);const graph=path.join(out,'backend-preview.graph.json');
g.project='research-atlas / Go backend';delete g.review;g.changed=[];
const target=nodes.find(n=>n.name==='Answer').id,entry=nodes.find(n=>n.name==='<callback@141:59>').id;
g.entries=[entry];g.explicit_entries=true;
g.viewer={scope:'backend/',exclude_test_files:true,title:'从项目问答，走进检索与证据。',target,entry,module_groups:[
 ['启动与环境','backend/cmd/'],['配置校验','backend/internal/lumina/config.go'],['HTTP 路由','backend/internal/lumina/http.go'],['检索与问答','backend/internal/lumina/rag.go'],['JSON 存储','backend/internal/lumina/store.go'],['模型适配','backend/internal/lumina/providers.go'],['论文检索与 PDF','backend/internal/lumina/papers.go'],['引用网络','backend/internal/lumina/network.go'],['其余后端能力','backend/internal/lumina/']].map(([name,prefix])=>({name,prefix}))};
fs.writeFileSync(graph,JSON.stringify(g));
for(let i=0;i<nodes.length;i+=12){const batch=nodes.slice(i,i+12);const prepare=execFileSync(binary,['prepare-annotations',graph,...batch.flatMap(n=>['--function',n.id])],{windowsHide:true});fs.writeFileSync(path.join(out,`backend-source-batch-${i/12+1}.json`),prepare);
 const envelope={snapshot:g.snapshot,source:'conversation-model',annotations:batch.map(n=>({id:n.id,content_hash:n.content_hash,title:descriptions[n.file][n.name][0],summary:descriptions[n.file][n.name][1]}))};
 const input=path.join(out,`backend-annotations-${i/12+1}.json`);fs.writeFileSync(input,JSON.stringify(envelope,null,2));execFileSync(binary,['annotate',graph,'--input',input,'--out',graph],{windowsHide:true,stdio:'inherit'});
}
execFileSync(binary,['render',graph,'--out',path.join(out,'backend-preview.html')],{windowsHide:true,stdio:'inherit'});
console.log(JSON.stringify({annotated:nodes.length,target,entry}));
