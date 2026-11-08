const path=require('node:path'),fs=require('node:fs');
const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'C:/Users/zhao/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');
const root=path.resolve(__dirname,'..'),out=path.join(root,'output/demo');fs.mkdirSync(out,{recursive:true});
(async()=>{const browser=await chromium.launch({headless:true,executablePath:process.env.CHROMIUM_PATH||'C:/Users/zhao/AppData/Local/ms-playwright/chromium-1194/chrome-win/chrome.exe'}),page=await browser.newPage({viewport:{width:1600,height:1200}});let index=0;const frame=async()=>{await page.screenshot({path:path.join(out,`frame-${String(index++).padStart(3,'0')}.png`)});};
 await page.goto('file:///'+path.join(root,'output/research-atlas/frontend-preview.html').replaceAll('\\','/'));await page.waitForSelector('.graph-node');await frame();await frame();
 await page.locator('#search').fill('pBtx');await frame();await page.keyboard.press('ArrowDown');await page.keyboard.press('Enter');for(let i=0;i<6;i++){await frame();await page.waitForTimeout(65);}await page.locator('#pin-current').click();await frame();
 await page.locator('#search').fill('');await page.locator('[data-view=modules]').click();await frame();await frame();await page.locator('[data-mode=memory]').click();await frame();await frame();
 await page.goto('file:///'+path.join(root,'output/research-atlas/backend-preview.html').replaceAll('\\','/'));await page.waitForSelector('.graph-node');await frame();await frame();await page.locator('.signature-context summary').click();await frame();await frame();await browser.close();console.log(JSON.stringify({frames:index,out}));
})().catch(e=>{console.error(e);process.exitCode=1;});
