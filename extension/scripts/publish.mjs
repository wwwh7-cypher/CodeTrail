import {execFileSync} from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

// Publication is deliberately opt-in; packaging never calls this script.
// Requires VSCE_PAT (Azure DevOps PAT with Marketplace > Manage scope) and an
// existing VSIX built by package.mjs. The token must never be committed/printed.
if (!process.argv.includes('--confirmed')) {
  throw new Error(
    'Publishing is opt-in. Re-run with --confirmed after the user explicitly authorizes the VS Code Marketplace release.',
  );
}

const extension = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const root = path.dirname(extension);
const pkg = JSON.parse(fs.readFileSync(path.join(extension, 'package.json'), 'utf8'));
const target =
  process.argv.find((arg) => arg.startsWith('--target='))?.slice(9) ||
  (process.platform === 'win32'
    ? 'win32-x64'
    : process.platform === 'darwin'
      ? `darwin-${process.arch}`
      : `linux-${process.arch}`);
const vsix = path.join(root, 'output', `code-trail-${pkg.version}-${target}.vsix`);

if (!fs.existsSync(vsix)) {
  throw new Error(
    `VSIX not found: ${path.relative(root, vsix)}\n` +
      'Build it first: pwsh -File scripts/build.ps1, then npm --prefix extension run package.',
  );
}
if (!process.env.VSCE_PAT) {
  throw new Error(
    'VSCE_PAT is not set. Create an Azure DevOps PAT with Marketplace > Manage scope and set VSCE_PAT (never commit it).',
  );
}

const extraArgs = process.argv
  .slice(2)
  .filter((arg) => arg !== '--confirmed' && !arg.startsWith('--target='));
const vsce = path.join(extension, 'node_modules/@vscode/vsce/vsce');
execFileSync(
  process.execPath,
  [vsce, 'publish', '--packagePath', vsix, ...extraArgs],
  { cwd: extension, stdio: 'inherit', windowsHide: true },
);
