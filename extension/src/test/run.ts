import * as fs from 'node:fs/promises';
import { existsSync, statSync } from 'node:fs';
import * as path from 'node:path';
import { execFileSync } from 'node:child_process';
import { runTests } from '@vscode/test-electron';

function fileExists(candidate: string): boolean {
  try {
    return statSync(candidate).isFile();
  } catch {
    return false;
  }
}

/** Resolve the `code` launcher to the actual VS Code executable it starts. */
function fromCodeCommand(): string | undefined {
  const locator = process.platform === 'win32' ? 'where' : 'which';
  let output: string;
  try {
    output = execFileSync(locator, ['code'], { encoding: 'utf8', windowsHide: true, stdio: ['ignore', 'pipe', 'ignore'] });
  } catch {
    return undefined;
  }
  const candidates: string[] = [];
  for (const line of output.split(/\r?\n/).map((value) => value.trim()).filter(Boolean)) {
    const base = path.basename(line).toLowerCase();
    if (process.platform === 'win32') {
      if (/^code\.(cmd|bat)$/i.test(base)) {
        // <install>/bin/code.cmd -> <install>/Code.exe
        candidates.push(path.resolve(path.dirname(line), '..', 'Code.exe'));
      } else if (/^code\.exe$/i.test(base)) {
        candidates.push(line);
      }
      // The extensionless `code` shim is a shell script; Node cannot spawn it
      // directly on Windows, so it is intentionally skipped.
    } else {
      candidates.push(line);
    }
  }
  return candidates.find(fileExists);
}

/** Find a local VS Code install; returns undefined when nothing can be found. */
function detectVSCode(): string | undefined {
  const localAppData = process.env.LOCALAPPDATA;
  const programFiles = process.env.PROGRAMFILES;
  const programFilesX86 = process.env['PROGRAMFILES(X86)'];
  const candidates = [
    ...(localAppData ? [path.join(localAppData, 'Programs', 'Microsoft VS Code', 'Code.exe')] : []),
    ...(programFiles ? [path.join(programFiles, 'Microsoft VS Code', 'Code.exe')] : []),
    ...(programFilesX86 ? [path.join(programFilesX86, 'Microsoft VS Code', 'Code.exe')] : []),
    '/Applications/Visual Studio Code.app/Contents/MacOS/Electron',
    '/usr/share/code/code',
    '/usr/bin/code',
    '/snap/bin/code',
  ];
  const fromCommand = fromCodeCommand();
  return (fromCommand ? [fromCommand] : []).concat(candidates).find(fileExists);
}

async function main(): Promise<void> {
  const real = process.env.CODETRAIL_REAL;
  if (!real) {
    console.log(
      '[code-trail] Skipping VS Code integration tests: CODETRAIL_REAL is not set.\n' +
        '            Set CODETRAIL_REAL=<path to a Git repository> to run them.',
    );
    return;
  }
  if (!existsSync(real)) {
    console.error(`[code-trail] ERROR: CODETRAIL_REAL points to a missing path: ${real}`);
    process.exitCode = 1;
    return;
  }

  const configuredVSCode = process.env.CODETRAIL_VSCODE;
  const vscodeExecutablePath = configuredVSCode || detectVSCode();
  if (!vscodeExecutablePath || !fileExists(vscodeExecutablePath)) {
    console.error(
      '[code-trail] ERROR: VS Code executable not found. Set CODETRAIL_VSCODE=<path to Code.exe> and retry.\n' +
        `            Tried env CODETRAIL_VSCODE, \`where code\`/\`which code\`, and common install paths.`,
    );
    process.exitCode = 1;
    return;
  }

  const extension = path.resolve(__dirname, '../..');
  const repo = path.resolve(extension, '..');
  const out = path.join(repo, 'output/vscode-test');
  await fs.mkdir(out, { recursive: true });
  const fixture = path.join(out, 'fixture');
  await fs.mkdir(fixture, { recursive: true });
  await fs.writeFile(path.join(fixture, 'package.json'), '{"name":"codetrail-fixture","private":true}');
  await fs.writeFile(path.join(fixture, 'main.js'), 'function leaf(){return 1;}\nfunction main(){return leaf();}\nmain();\n');
  const git = (...args: string[]) => execFileSync('git', ['-C', fixture, ...args], { windowsHide: true, stdio: 'pipe' });
  try {
    git('rev-parse', 'HEAD');
  } catch {
    git('init', '-q');
    git('config', 'user.name', 'CodeTrail integration');
    git('config', 'user.email', 'test@example.invalid');
    git('add', '.');
    git('commit', '-qm', 'fixture');
  }
  await fs.writeFile(path.join(fixture, 'main.js'), 'function leaf(){return 2;}\nfunction main(){return leaf();}\nmain();\n');
  const workspace = path.join(out, 'tests.code-workspace');
  await fs.writeFile(
    workspace,
    JSON.stringify({
      folders: [{ path: real }, { path: fixture }],
      settings: {
        'security.workspace.trust.enabled': false,
        'extensions.autoUpdate': false,
        'codeTrail.autoRefresh': false,
      },
    }),
  );
  await runTests({
    vscodeExecutablePath,
    extensionDevelopmentPath: extension,
    extensionTestsPath: path.join(extension, 'out/test/suite.js'),
    launchArgs: [
      workspace,
      '--user-data-dir',
      path.join(out, 'user-data'),
      '--extensions-dir',
      path.join(out, 'extensions'),
      '--disable-extensions',
      '--skip-welcome',
      '--skip-release-notes',
      '--disable-workspace-trust',
    ],
    extensionTestsEnv: { CODETRAIL_REAL: real, CODETRAIL_FIXTURE: fixture, CODETRAIL_OUTPUT: out },
  });
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
