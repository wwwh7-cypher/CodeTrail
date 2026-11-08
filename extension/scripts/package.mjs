import fs from 'node:fs';import path from 'node:path';import {fileURLToPath} from 'node:url';import {execFileSync} from 'node:child_process';
const extension=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..'),root=path.dirname(extension);
const binary=process.argv.find(a=>a.startsWith('--binary='))?.slice(9)||path.join(root,'bin',process.platform==='win32'?'code-trail.exe':'code-trail');
if(!fs.existsSync(binary))throw new Error('Build the Rust release binary first with scripts/build.ps1 or cargo build --release --locked.');
fs.mkdirSync(path.join(extension,'bin'),{recursive:true});fs.copyFileSync(binary,path.join(extension,'bin',process.platform==='win32'?'code-trail.exe':'code-trail'));
execFileSync(binary,['template','--out',path.join(extension,'media/viewer.html')],{stdio:'inherit',windowsHide:true});fs.copyFileSync(path.join(root,'LICENSE'),path.join(extension,'LICENSE'));
const target=process.argv.find(a=>a.startsWith('--target='))?.slice(9)||(process.platform==='win32'?'win32-x64':process.platform==='darwin'?'darwin-'+process.arch:'linux-'+process.arch);
const version=JSON.parse(fs.readFileSync(path.join(extension,'package.json'),'utf8')).version;const output=path.join(root,'output',`code-trail-${version}-${target}.vsix`);fs.mkdirSync(path.dirname(output),{recursive:true});
execFileSync(process.execPath,[path.join(extension,'node_modules/@vscode/vsce/vsce'),'package','--target',target,'--out',output,'--allow-missing-repository'],{cwd:extension,stdio:'inherit',windowsHide:true});console.log(output);
