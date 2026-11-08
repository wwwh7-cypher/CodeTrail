import {execFileSync} from 'node:child_process';import path from 'node:path';import {fileURLToPath} from 'node:url';
// Publication is deliberately opt-in; packaging never calls this script.
if(!process.argv.includes('--confirmed'))throw new Error('本轮为本地交付。实际发布需用户明确授权后执行 npm run publish:marketplace -- --confirmed。');
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
execFileSync(process.execPath,[path.join(root,'node_modules/@vscode/vsce/vsce'),'publish','--allow-missing-repository'],{cwd:root,stdio:'inherit',windowsHide:true});
