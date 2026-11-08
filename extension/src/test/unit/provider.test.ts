import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {once} from 'node:events';
import {validateConfig,keyScope,endpoint,listModels,generateRoles,cacheKey} from '../../provider';
import type {ApiConfig,NodeInfo} from '../../types';

const node:NodeInfo={id:'a.ts:1:1:run',name:'run',file:'a.ts',line:1,end_line:1,source:'function run(){return 1}',kind:'function',language:'TypeScript',role:'',role_source:'no-description',symbol_key:'a.ts::run',content_hash:'hash'};
test('API URL validation and endpoint selection',()=>{const c:ApiConfig={provider:'openai-compatible',apiBaseUrl:'https://example.com/v1/chat/completions',model:'test-model',modelsUrl:''};assert.equal(endpoint(c,'models'),'https://example.com/v1/models');assert.equal(endpoint(c,'generate'),c.apiBaseUrl);assert.throws(()=>validateConfig({...c,apiBaseUrl:'http://example.com/v1'}));assert.throws(()=>validateConfig({...c,apiBaseUrl:'https://secret@example.com'}));assert.notEqual(keyScope(c),keyScope({...c,apiBaseUrl:'https://another.example/v1'}));assert.notEqual(cacheKey(c,node),cacheKey(c,{...node,content_hash:'new'}));});
test('OpenAI and Anthropic provider contracts, model list and unknown IDs',async()=>{
  let mode='good';const requests:Array<{url:string;headers:any;body:any}>=[];
  const server=createServer(async(req,res)=>{const chunks=[];for await(const c of req)chunks.push(c);const body=chunks.length?JSON.parse(Buffer.concat(chunks).toString()):{};requests.push({url:req.url||'',headers:req.headers,body});res.setHeader('content-type','application/json');
    if(mode==='error'){res.statusCode=401;res.end(JSON.stringify({error:'fixture-secret-should-not-escape'}));return;}
    if(req.method==='GET'){res.end(JSON.stringify({data:[{id:'fixture-model'},{id:'fixture-model'}]}));return;}
    const envelope:any={snapshot:mode==='stale'?'other':'',source:'byok-model',annotations:[{id:mode==='unknown'?'unknown':node.id,content_hash:mode==='hash'?'other':node.content_hash,title:'返回固定值',summary:'返回固定数值，供调用方使用。'}]};if(mode==='edges')envelope.edges=[];if(mode==='duplicate')envelope.annotations.push(envelope.annotations[0]);const result=JSON.stringify(envelope);
    res.end(JSON.stringify(req.url?.endsWith('/messages')?{content:[{type:'text',text:result}]}:{choices:[{message:{content:result}}]}));
  });server.listen(0,'127.0.0.1');await once(server,'listening');const port=(server.address() as any).port;
  try{const c:ApiConfig={provider:'openai-compatible',apiBaseUrl:`http://127.0.0.1:${port}/v1`,model:'fixture-model',modelsUrl:''};assert.deepEqual(await listModels(c,'fixture-key'),['fixture-model']);const roles=await generateRoles(c,'fixture-key',[node]);assert.equal(roles.get(node.id)?.summary,'返回固定数值，供调用方使用。');assert.equal(roles.get(node.id)?.title,'返回固定值');assert.equal(requests.at(-1)?.headers.authorization,'Bearer fixture-key');assert.equal(requests.at(-1)?.body.model,'fixture-model');await generateRoles({...c,provider:'anthropic'},'anthropic-fixture',[node]);assert.equal(requests.at(-1)?.headers['x-api-key'],'anthropic-fixture');assert.equal(requests.at(-1)?.headers['anthropic-version'],'2023-06-01');for(const invalid of ['unknown','stale','hash','edges','duplicate']){mode=invalid;await assert.rejects(generateRoles(c,'fixture-key',[node]));}mode='error';await assert.rejects(listModels(c,'fixture-key'),e=>e instanceof Error&&e.message.includes('401')&&!e.message.includes('fixture-secret'));}finally{server.close();await once(server,'close');}
});
