#!/usr/bin/env node
'use strict';
/**
 * Generate the README screenshots (docs/screenshots/{architecture,modules,review}.png).
 *
 * Developer-only helper: CodeTrail itself needs neither Node nor a browser.
 * Requires a built CLI (`bin/code-trail*`, or `--bin`) or pre-generated HTML
 * (`--architecture-html` / `--modules-html` / `--review-html`), plus Playwright
 * and a local Chrome/Chromium (see scripts/lib/browser-env.cjs).
 *
 * Default run (deterministic, works from a clean checkout after `scripts/build.ps1`):
 *   node scripts/render-doc-screenshots.cjs
 *   - architecture/modules: scan examples/config-service
 *   - review: temporary Git copy of examples/config-service with demo edits
 *
 * Reuse already rendered previews instead of scanning (e.g. the richer local
 * research-atlas previews under output/, after re-rendering them with the final
 * binary):
 *   node scripts/render-doc-screenshots.cjs \
 *     --architecture-html output/research-atlas/frontend-preview.html \
 *     --modules-html output/research-atlas/backend-preview.html \
 *     --review-html output/research-atlas/commit-review.html
 *
 * Options:
 *   --project <dir>          Project to scan for architecture/modules (default: examples/config-service)
 *   --bin <exe>              CLI binary (default: bin/code-trail.exe, or CODETRAIL_BIN)
 *   --out <dir>              PNG output directory (default: docs/screenshots)
 *   --architecture-html <f>  Reuse this HTML for architecture.png instead of scanning
 *   --modules-html <f>       Reuse this HTML for modules.png (default: same as architecture)
 *   --review-html <f>        Reuse this HTML for review.png instead of building a demo review
 *   --review-base <ref>      Generate review.png from the project's Git history (e.g. HEAD~1)
 *   --review-head <ref>      Optional head ref for --review-base (e.g. HEAD)
 *   --width <px>             Screenshot viewport width (default: 1600)
 *   --height <px>            Screenshot viewport height (default: 1100)
 *   --help                   Show this help
 */
const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const { launch } = require('./lib/browser-env.cjs');

const ROOT = path.resolve(__dirname, '..');
const WORK = path.join(ROOT, 'output', 'doc-screenshots');

function log(message) {
  console.log(`[render-doc-screenshots] ${message}`);
}

function fail(message) {
  console.error(`[render-doc-screenshots] ERROR: ${message}`);
  process.exit(1);
}

function resolveExisting(file, label) {
  const resolved = path.resolve(ROOT, file);
  if (!fs.existsSync(resolved)) fail(`${label} does not exist: ${resolved}`);
  return resolved;
}

function parseArgs(argv) {
  const options = {
    project: 'examples/config-service',
    bin: process.env.CODETRAIL_BIN || path.join('bin', process.platform === 'win32' ? 'code-trail.exe' : 'code-trail'),
    out: 'docs/screenshots',
    architectureHtml: '',
    modulesHtml: '',
    reviewHtml: '',
    reviewBase: '',
    reviewHead: '',
    width: 1600,
    height: 1100,
    help: false,
  };
  const valueOptions = {
    '--project': 'project',
    '--bin': 'bin',
    '--out': 'out',
    '--architecture-html': 'architectureHtml',
    '--modules-html': 'modulesHtml',
    '--review-html': 'reviewHtml',
    '--review-base': 'reviewBase',
    '--review-head': 'reviewHead',
    '--width': 'width',
    '--height': 'height',
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--help' || arg === '-h') {
      options.help = true;
      continue;
    }
    const key = valueOptions[arg];
    if (!key) fail(`unknown argument ${arg} (use --help)`);
    const value = argv[i + 1];
    if (value === undefined || value.startsWith('--')) fail(`missing value for ${arg}`);
    i += 1;
    options[key] = key === 'width' || key === 'height' ? Number(value) : value;
  }
  for (const key of ['width', 'height']) {
    if (!Number.isInteger(options[key]) || options[key] < 320) fail(`--${key} must be an integer >= 320`);
  }
  if (options.reviewHead && !options.reviewBase) fail('--review-head requires --review-base');
  return options;
}

function printHelp() {
  const header = fs.readFileSync(__filename, 'utf8').split('\n').filter((line) => line.startsWith(' *')).map((line) => line.slice(2)).join('\n');
  console.log(header);
}

function run(command, args, cwd) {
  log(`$ ${command} ${args.join(' ')}`);
  execFileSync(command, args, { cwd: cwd || ROOT, stdio: 'inherit', windowsHide: true });
}

function git(cwd, args) {
  return execFileSync('git', ['-C', cwd, ...args], { encoding: 'utf8', windowsHide: true }).trim();
}

function resolveBinary(options) {
  const resolved = path.resolve(ROOT, options.bin);
  if (fs.existsSync(resolved)) return resolved;
  return '';
}

function fileAge(file) {
  try {
    return fs.statSync(file).mtime.toISOString();
  } catch {
    return 'unknown';
  }
}

function findPreGenerated(candidates) {
  for (const candidate of candidates) {
    const resolved = path.join(ROOT, candidate);
    if (fs.existsSync(resolved)) return resolved;
  }
  return '';
}

function htmlPath() {
  return path.join(WORK, 'architecture.html');
}

function buildArchitecture(options, binary) {
  if (options.architectureHtml) {
    const file = resolveExisting(options.architectureHtml, '--architecture-html');
    log(`reusing architecture HTML: ${path.relative(ROOT, file)} (rendered ${fileAge(file)})`);
    log('note: pre-generated HTML embeds the viewer assets from whenever it was rendered; re-render it with the final binary if the UI changed.');
    return file;
  }
  if (!binary) {
    const fallback = findPreGenerated([
      'output/research-atlas/frontend-preview.html',
      'output/eval/config-service.html',
      'output/doc-screenshots/architecture.html',
    ]);
    if (!fallback) {
      fail(
        [
          'the Rust CLI was not found and no pre-generated HTML is available.',
          `Looked for: ${path.resolve(ROOT, options.bin)} and known output/*.html previews.`,
          'Build it first:  pwsh -File scripts/build.ps1   (Windows)',
          '               cargo build --release --locked   (other platforms)',
          'or pass --architecture-html with an existing generated HTML file.',
        ].join('\n'),
      );
    }
    log(`warning: CLI not found; reusing pre-generated HTML: ${path.relative(ROOT, fallback)} (rendered ${fileAge(fallback)})`);
    log('note: screenshots will show that renderer version, not necessarily the current working tree.');
    return fallback;
  }
  const project = resolveExisting(options.project, '--project');
  if (!fs.statSync(project).isDirectory()) fail(`--project is not a directory: ${project}`);
  fs.mkdirSync(WORK, { recursive: true });
  const out = htmlPath();
  run(binary, ['scan', project, '--out', out, '--cache', path.join(WORK, 'parse-cache.json')]);
  return out;
}

function copyDirectory(source, destination, skip) {
  fs.mkdirSync(destination, { recursive: true });
  for (const entry of fs.readdirSync(source, { withFileTypes: true })) {
    if (skip.has(entry.name)) continue;
    const from = path.join(source, entry.name);
    const to = path.join(destination, entry.name);
    if (entry.isDirectory()) copyDirectory(from, to, skip);
    else if (entry.isFile()) fs.copyFileSync(from, to);
  }
}

function editFile(file, transform, description) {
  const before = fs.readFileSync(file, 'utf8');
  const after = transform(before);
  if (after === before) {
    fail(
      [
        `the built-in demo review expects the bundled example project; could not apply: ${description}`,
        `file: ${path.relative(ROOT, file)}`,
        'Pass --review-html <existing review html> or --review-base <git ref> for a custom --project.',
      ].join('\n'),
    );
  }
  fs.writeFileSync(file, after);
}

function buildDemoReview(options, binary) {
  const demo = path.join(WORK, 'config-service');
  fs.rmSync(demo, { recursive: true, force: true });
  copyDirectory(resolveExisting(options.project, '--project'), demo, new Set(['.git', 'node_modules', 'target', 'output', 'bin']));
  git(demo, ['init', '-q']);
  git(demo, ['config', 'user.name', 'CodeTrail docs screenshots']);
  git(demo, ['config', 'user.email', 'docs@example.invalid']);
  git(demo, ['add', '-A']);
  git(demo, ['commit', '-qm', 'baseline']);
  editFile(path.join(demo, 'config.ts'), (text) => text.replace('return raw.split("\\n").filter(Boolean);', 'return raw.split(/\\r?\\n/).filter(Boolean);'), 'modify splitLines()');
  editFile(path.join(demo, 'jobs.py'), (text) => text.replace('return {"archived": True, "record": record}', 'return {"archived": True, "record": record, "schema": 2}'), 'modify archive_record()');
  fs.appendFileSync(
    path.join(demo, 'server.ts'),
    '\n// 重置路由入口：热重载时重新装配应用。\nexport function resetRoutes(config: string[]) {\n  return registerRoutes(config);\n}\n',
  );
  const status = git(demo, ['status', '--porcelain']);
  if (!status) fail('the built-in demo review produced no Git changes; pass --review-html or --review-base');
  log(`demo review changes:\n${status.split('\n').map((line) => `  ${line}`).join('\n')}`);
  const out = path.join(WORK, 'review.html');
  run(binary, ['review', demo, '--base', 'HEAD', '--out', out, '--report', path.join(WORK, 'review.md')]);
  return out;
}

function buildReview(options, binary) {
  if (options.reviewHtml) {
    const file = resolveExisting(options.reviewHtml, '--review-html');
    log(`reusing review HTML: ${path.relative(ROOT, file)} (rendered ${fileAge(file)})`);
    log('note: pre-generated HTML embeds the viewer assets from whenever it was rendered; re-render it with the final binary if the UI changed.');
    return file;
  }
  if (options.reviewBase) {
    if (!binary) fail('--review-base needs the Rust CLI; build it first (scripts/build.ps1) or pass --bin.');
    const project = resolveExisting(options.project, '--project');
    fs.mkdirSync(WORK, { recursive: true });
    const out = path.join(WORK, 'review.html');
    const args = ['review', project, '--base', options.reviewBase, '--out', out, '--report', path.join(WORK, 'review.md')];
    if (options.reviewHead) args.push('--head', options.reviewHead);
    run(binary, args);
    return out;
  }
  const project = resolveExisting(options.project, '--project');
  const isBundledExample = path.resolve(project) === path.resolve(ROOT, 'examples', 'config-service');
  if (!isBundledExample) {
    if (!binary) {
      const fallback = findPreGenerated(['output/research-atlas/commit-review.html', 'output/native-test/review.html']);
      if (fallback) {
        log(`warning: CLI not found; reusing pre-generated review HTML: ${path.relative(ROOT, fallback)} (rendered ${fileAge(fallback)})`);
        return fallback;
      }
    }
    fail(
      [
        'only the bundled examples/config-service demo can generate review.png without extra input.',
        'For a custom --project pass one of:',
        '  --review-html <existing review html>',
        '  --review-base HEAD~1 --review-head HEAD   (render the last commit)',
      ].join('\n'),
    );
  }
  if (!binary) fail('generating the demo review needs the Rust CLI; build it first (scripts/build.ps1) or pass --bin/--review-html.');
  return buildDemoReview(options, binary);
}

async function capture(browser, htmlFile, pngFile, options, prepare) {
  const page = await browser.newPage({ viewport: { width: options.width, height: options.height } });
  const pageErrors = [];
  page.on('pageerror', (error) => pageErrors.push(error.message));
  try {
    const url = `file:///${path.resolve(htmlFile).replaceAll('\\', '/')}`;
    await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60000 });
    await page.waitForSelector('.graph-node', { timeout: 30000 });
    await page.waitForTimeout(700);
    if (prepare) await prepare(page);
    await page.waitForTimeout(700);
    fs.mkdirSync(path.dirname(pngFile), { recursive: true });
    await page.screenshot({ path: pngFile });
  } finally {
    await page.close();
  }
  const bytes = fs.statSync(pngFile).size;
  if (pageErrors.length) log(`warning: ${path.basename(pngFile)} had page errors: ${pageErrors.join(' | ')}`);
  if (bytes < 10_000) fail(`screenshot looks empty (${bytes} bytes): ${pngFile}`);
  log(`wrote ${path.relative(ROOT, pngFile)} (${Math.round(bytes / 1024)} KB)`);
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    printHelp();
    return;
  }
  const binary = resolveBinary(options);
  if (!binary && !options.architectureHtml && !options.reviewHtml) {
    log('warning: Rust CLI not found; falling back to pre-generated HTML where possible.');
  } else if (binary) {
    log(`using CLI: ${binary}`);
  }
  const outDir = path.resolve(ROOT, options.out);
  const architectureHtml = buildArchitecture(options, binary);
  const modulesHtml = options.modulesHtml ? resolveExisting(options.modulesHtml, '--modules-html') : architectureHtml;
  const reviewHtml = buildReview(options, binary);

  const browser = await launch();
  try {
    await capture(browser, architectureHtml, path.join(outDir, 'architecture.png'), options, async (page) => {
      // Show one extra expansion level and fit the whole network so the screenshot
      // is readable straight from the README.
      await page.evaluate(() => {
        const depth = document.querySelector('#depth');
        if (depth) {
          depth.value = '2';
          depth.dispatchEvent(new Event('input', { bubbles: true }));
        }
      });
      await page.waitForTimeout(600);
      await page.evaluate(() => {
        const fit = document.querySelector('#zoom-reset');
        if (fit) fit.click();
      });
    });
    const click = (selector) => async (page) => {
      await page.evaluate((sel) => {
        const element = document.querySelector(sel);
        if (!element) throw new Error(`missing UI control: ${sel}`);
        element.click();
      }, selector);
    };
    await capture(browser, modulesHtml, path.join(outDir, 'modules.png'), options, click('[data-view=modules]'));
    await capture(browser, reviewHtml, path.join(outDir, 'review.png'), options, async (page) => {
      await page.evaluate(() => {
        const reviewMode = document.querySelector('[data-mode=review]');
        if (!reviewMode) throw new Error('missing review mode control');
        reviewMode.click();
      });
      await page.waitForTimeout(400);
      await page.evaluate(() => {
        const change = document.querySelector('[data-change]');
        if (change) change.click();
      });
    });
  } finally {
    await browser.close();
  }
  log(`done: ${path.relative(ROOT, outDir)}`);
}

main().catch((error) => {
  console.error(`[render-doc-screenshots] FAILED: ${error && error.message ? error.message : error}`);
  process.exitCode = 1;
});
