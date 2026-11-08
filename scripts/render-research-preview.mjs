import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {execFileSync} from 'node:child_process';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const output=path.join(root,'output/research-atlas');
const source=JSON.parse(fs.readFileSync(path.join(output,'architecture.graph.json'),'utf8'));
const descriptions=JSON.parse(fs.readFileSync(path.join(root,'assets/research-atlas-notes.json'),'utf8'));
const annotations={};
for(const n of source.nodes){const note=descriptions[n.file]?.[n.name];if(note)annotations[n.id]={...note,content_hash:n.content_hash,source:'source-reviewed',revision:source.snapshot,confirmed:false,evidence:{path:n.file,line:n.line,end_line:n.end_line}};}
source.project='research-atlas / app';
source.entries=['src/lib/domain/import-bibliography.ts:116:8:parseBibliography'];source.explicit_entries=true;source.changed=[];delete source.review;
source.viewer={title:'搜索一个函数，看懂相邻的代码。',scope:'src/',default_view:'network',entry:source.entries[0],target:source.entries[0],annotations};
const graph=path.join(output,'frontend-preview.graph.json');fs.writeFileSync(graph,JSON.stringify(source));
const binary=process.argv[2]||path.join(root,'target/debug/code-trail.exe');
execFileSync(binary,['render',graph,'--out',path.join(output,'frontend-preview.html')],{stdio:'inherit',windowsHide:true});
