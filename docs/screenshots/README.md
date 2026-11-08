# README 截图

`architecture.png`、`modules.png`、`review.png` 由 `scripts/render-doc-screenshots.cjs` 生成，尺寸 1600×1100。

默认命令扫描仓库内的 `examples/config-service`，并为一个临时 Git 副本生成演示修改（`output/doc-screenshots/config-service`，不入库）后截图：

```powershell
pwsh -File scripts/build.ps1
node scripts/render-doc-screenshots.cjs
```

也可以复用已生成的 HTML（例如本机 research-atlas 预览，先用最终二进制重新渲染这些 HTML）：

```powershell
node scripts/render-doc-screenshots.cjs `
  --architecture-html output/research-atlas/frontend-preview.html `
  --modules-html output/research-atlas/backend-preview.html `
  --review-html output/research-atlas/commit-review.html
```

脚本只做开发者截图用途：需要 Playwright 与本地 Chrome/Chromium（见 `scripts/lib/browser-env.cjs`，可用 `PLAYWRIGHT_MODULE`、`CHROMIUM_PATH` 指定）。`node scripts/render-doc-screenshots.cjs --help` 列出全部参数。如果 UI 变化，请用最终构建重新生成这些 PNG。
