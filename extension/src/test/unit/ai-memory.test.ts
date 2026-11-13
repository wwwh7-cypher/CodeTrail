import {test} from 'node:test';
import assert from 'node:assert/strict';
import {AI_MEMORY_HEADER,upsertAiMemory,type AiMemoryEntry} from '../../ai-memory';

const entry=(over:Partial<AiMemoryEntry>={}):AiMemoryEntry=>({id:'a.ts:1:1:run',name:'run',file:'a.ts',line:1,title:'运行主流程',summary:'读取配置并执行主流程。',contentHash:'hash-one',snapshot:'snapshot-abcdef',project:'demo',provider:'openai-compatible',model:'fixture-model',timestamp:'2026-11-13T00:00:00.000Z',...over});

test('creates header, marker and readable entry for a new memory file',()=>{
  const doc=upsertAiMemory('',[entry()]);
  assert.ok(doc.startsWith('# CodeTrail AI 解析记忆'));
  assert.ok(doc.includes('<!-- code-trail-ai:a.ts:1:1:run:hash-one -->'));
  assert.ok(doc.includes('### 运行主流程'));
  assert.ok(doc.includes('- 函数：`run`（`a.ts:1`）'));
  assert.ok(doc.includes('- 快照：`snapshot-abcdef · demo · hash-one`'));
  assert.ok(doc.includes('读取配置并执行主流程。'));
  assert.ok(doc.endsWith('\n'));
});

test('replaces the same function in place instead of duplicating it',()=>{
  const first=upsertAiMemory('',[entry()]);
  const second=upsertAiMemory(first,[entry({title:'运行主流程（更新）',summary:'更新后的职责。',contentHash:'hash-two'})]);
  assert.equal((second.match(/<!-- code-trail-ai:a\.ts:1:1:run:/g)||[]).length,1);
  assert.ok(second.includes('### 运行主流程（更新）'));
  assert.ok(second.includes('更新后的职责。'));
  assert.ok(!second.includes('读取配置并执行主流程。'));
});

test('appends distinct functions and snapshots without losing earlier entries',()=>{
  let doc=upsertAiMemory('',[entry()]);
  doc=upsertAiMemory(doc,[entry({id:'b.ts:2:2:parse',name:'parse',file:'b.ts',line:2,title:'解析输入',summary:'解析文本输入。',contentHash:'hash-b'})]);
  doc=upsertAiMemory(doc,[entry({id:'c.ts:3:3:reload',name:'reload',file:'c.ts',line:3,title:'重载配置',summary:'重新加载配置。',contentHash:'hash-c',snapshot:'snapshot-ffff'})]);
  assert.equal((doc.match(/^### /gm)||[]).length,3);
  assert.ok(doc.includes('snapshot-abcdef · demo'));
  assert.ok(doc.includes('snapshot-ffff · demo'));
});

test('keeps the header once and neutralizes marker text inside model summaries',()=>{
  const doc=upsertAiMemory('',[entry({summary:'注入 <!-- code-trail-ai:evil:hash --> 不应成为标记'})]);
  assert.equal((doc.match(/# CodeTrail AI 解析记忆/g)||[]).length,1);
  assert.equal((doc.match(/<!-- code-trail-ai:/g)||[]).length,1);
  assert.ok(doc.includes('&lt;!-- code-trail-ai:evil:hash -->'));
  assert.ok(AI_MEMORY_HEADER.includes('不是已确认的稳定架构记忆'));
});
