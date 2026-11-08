// Capture the demo animation frames used for output/code-trail-demo.gif.
// Playwright and Chrome/Chromium are resolved by scripts/lib/browser-env.cjs;
// override PLAYWRIGHT_MODULE/CHROMIUM_PATH if needed.
const path=require('node:path'),fs=require('node:fs');
const {launch}=require('./lib/browser-env.cjs');
const root=path.resolve(__dirname,'..'),out=path.join(root,'output/demo');fs.mkdirSync(out,{recursive:true});
(async()=>{const browser=await launch();try{const page=await browser.newPage({viewport:{width:1600,height:1200}});let index=0;const frame=async()=>{await page.screenshot({path:path.join(out,`frame-${String(index++).padStart(3,'0')}.png`)});};
 await page.goto('file:///'+path.join(root,'output/research-atlas/frontend-preview.html').replaceAll('\\','/'));await page.waitForSelector('.graph-node');await frame();await frame();
 await page.locator('#search').fill('pBtx');await frame();await page.keyboard.press('ArrowDown');await page.keyboard.press('Enter');for(let i=0;i<6;i++){await frame();await page.waitForTimeout(65);}await page.locator('#pin-current').click();await frame();
 await page.locator('#search').fill('');await page.locator('[data-view=modules]').click();await frame();await frame();await page.locator('[data-mode=memory]').click();await frame();await frame();
 await page.goto('file:///'+path.join(root,'output/research-atlas/backend-preview.html').replaceAll('\\','/'));await page.waitForSelector('.graph-node');await frame();await frame();await page.locator('.signature-context summary').click();await frame();await frame();console.log(JSON.stringify({frames:index,out}));
 }finally{await browser.close();}}
)().catch(e=>{console.error(e);process.exitCode=1;});
