'use strict';
/**
 * Shared browser tooling resolution for developer-only scripts.
 *
 * CodeTrail itself never needs Node or a browser at runtime. These scripts are
 * optional verification/documentation helpers, so they deliberately avoid
 * adding dependencies to the repository: Playwright is used only when it is
 * already installed locally, and the browser is a local Chrome/Chromium.
 *
 * Resolution order:
 *   1. PLAYWRIGHT_MODULE / CHROMIUM_PATH environment variables (explicit wins).
 *   2. A `playwright` module resolvable from the current Node.js install.
 *   3. Well-known per-user locations (including the bundled local Codex runtime
 *      and a global npm prefix) used by developer machines.
 *
 * Chromium resolution order:
 *   1. CHROMIUM_PATH.
 *   2. Newest `%LOCALAPPDATA%\ms-playwright\chromium-*` build (Playwright cache).
 *   3. System Chrome / Edge / Chromium install directories.
 *
 * Clear, actionable errors are thrown when nothing is found.
 */
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

function isFile(file) {
  try {
    return fs.statSync(file).isFile();
  } catch {
    return false;
  }
}

function isDirectory(dir) {
  try {
    return fs.statSync(dir).isDirectory();
  } catch {
    return false;
  }
}

function playwrightModuleCandidates() {
  const home = os.homedir();
  const appData = process.env.APPDATA || path.join(home, 'AppData', 'Roaming');
  return [
    // Bundled runtime used by local agent tooling on some developer machines.
    path.join(home, '.cache', 'codex-runtimes', 'codex-primary-runtime', 'dependencies', 'node', 'node_modules', 'playwright'),
    path.join(appData, 'npm', 'node_modules', 'playwright'),
    path.join(home, 'AppData', 'Roaming', 'npm', 'node_modules', 'playwright'),
  ];
}

function resolvePlaywright() {
  const fromEnv = process.env.PLAYWRIGHT_MODULE;
  if (fromEnv) {
    const resolved = path.resolve(fromEnv);
    if (!isDirectory(resolved)) {
      throw new Error(`PLAYWRIGHT_MODULE is set but is not a directory: ${resolved}`);
    }
    try {
      return require(resolved);
    } catch (error) {
      throw new Error(`PLAYWRIGHT_MODULE could not be loaded: ${resolved}\n${error.message}`);
    }
  }
  try {
    return require('playwright');
  } catch {
    // Fall through to local per-user installations.
  }
  const tried = [];
  for (const candidate of playwrightModuleCandidates()) {
    tried.push(candidate);
    if (!isDirectory(candidate)) continue;
    try {
      return require(candidate);
    } catch {
      // Try the next candidate.
    }
  }
  throw new Error(
    [
      'Playwright is not available to this script.',
      'Install it in a scratch location (for example: npm install --save-dev playwright)',
      'or point PLAYWRIGHT_MODULE at an installed playwright module:',
      '  $env:PLAYWRIGHT_MODULE = "C:\\path\\to\\node_modules\\playwright"   # PowerShell',
      `Tried require('playwright') and: ${tried.join(' , ')}`,
    ].join('\n'),
  );
}

function playwrightCacheRoots() {
  const roots = [];
  if (process.env.LOCALAPPDATA) roots.push(path.join(process.env.LOCALAPPDATA, 'ms-playwright'));
  roots.push(path.join(os.homedir(), 'AppData', 'Local', 'ms-playwright'));
  return roots;
}

function chromiumCandidates() {
  const candidates = [];
  for (const root of playwrightCacheRoots()) {
    let versions = [];
    try {
      versions = fs
        .readdirSync(root, { withFileTypes: true })
        .filter((entry) => entry.isDirectory() && /^chromium-\d+$/.test(entry.name))
        .map((entry) => entry.name)
        .sort((a, b) => Number.parseInt(b.slice('chromium-'.length), 10) - Number.parseInt(a.slice('chromium-'.length), 10));
    } catch {
      // No Playwright browser cache for this user.
    }
    for (const version of versions) {
      const base = path.join(root, version);
      candidates.push(
        path.join(base, 'chrome-win', 'chrome.exe'),
        path.join(base, 'chrome-mac', 'Chromium.app', 'Contents', 'MacOS', 'Chromium'),
        path.join(base, 'chrome-linux', 'chrome'),
      );
    }
  }
  const programFiles = process.env.PROGRAMFILES;
  const programFilesX86 = process.env['PROGRAMFILES(X86)'];
  const localAppData = process.env.LOCALAPPDATA;
  if (programFiles) candidates.push(path.join(programFiles, 'Google', 'Chrome', 'Application', 'chrome.exe'));
  if (programFilesX86) candidates.push(path.join(programFilesX86, 'Google', 'Chrome', 'Application', 'chrome.exe'));
  if (localAppData) candidates.push(path.join(localAppData, 'Google', 'Chrome', 'Application', 'chrome.exe'));
  if (programFiles) candidates.push(path.join(programFiles, 'Microsoft', 'Edge', 'Application', 'msedge.exe'));
  if (programFilesX86) candidates.push(path.join(programFilesX86, 'Microsoft', 'Edge', 'Application', 'msedge.exe'));
  candidates.push(
    'C:/Program Files/Google/Chrome/Application/chrome.exe',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/usr/bin/google-chrome',
    '/usr/bin/chromium',
    '/usr/bin/chromium-browser',
    '/usr/bin/microsoft-edge',
  );
  return candidates;
}

function findChromium() {
  const fromEnv = process.env.CHROMIUM_PATH;
  if (fromEnv) {
    const resolved = path.resolve(fromEnv);
    if (!isFile(resolved)) {
      throw new Error(`CHROMIUM_PATH is set but is not a file: ${resolved}`);
    }
    return resolved;
  }
  const tried = [];
  for (const candidate of chromiumCandidates()) {
    tried.push(candidate);
    if (isFile(candidate)) return candidate;
  }
  throw new Error(
    [
      'No Chrome/Chromium executable was found for the browser scripts.',
      'Set CHROMIUM_PATH to a chrome.exe / Chromium binary:',
      '  $env:CHROMIUM_PATH = "C:\\path\\to\\chrome.exe"   # PowerShell',
      'or install Playwright browsers (npx playwright install chromium).',
      `Tried: ${tried.join(' , ')}`,
    ].join('\n'),
  );
}

/**
 * Launch headless Chromium with the resolved executable.
 * @param {import('playwright').LaunchOptions} [options]
 */
async function launch(options = {}) {
  const { chromium } = resolvePlaywright();
  return chromium.launch({ headless: true, executablePath: findChromium(), ...options });
}

module.exports = { resolvePlaywright, findChromium, launch };
