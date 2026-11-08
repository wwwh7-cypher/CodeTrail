# 发布与分发

CodeTrail 目前只通过 GitHub Releases 分发预编译产物，不发布 VS Code Marketplace。

## 仓库约定

- 不提交生成产物：`bin/`、`extension/bin/`、`extension/LICENSE` 由构建/打包脚本生成，`.gitignore` 已忽略。
- `output/` 是本地构建与演示目录（VSIX、Skill/Source zip、截图中间文件），同样被忽略。
- `docs/screenshots/*.png` 会提交，README 直接引用它们。
- CI（`.github/workflows/checks.yml`）在 Linux/Windows/macOS 上构建并打包 VSIX，作为 workflow artifact 上传；不会自动创建 Release。

## 发布步骤

1. 构建与验证（Windows x64 预编译包）：

   ```powershell
   pwsh -File scripts/build.ps1
   cargo +stable-x86_64-pc-windows-gnu test --locked
   npm --prefix extension ci
   npm --prefix extension run test:unit
   npm --prefix extension run test:vscode   # 需要 CODETRAIL_REAL / CODETRAIL_VSCODE
   npm --prefix extension run package       # output/code-trail-0.4.0-win32-x64.vsix
   python scripts/package-skill.py          # output/code-trail-0.4.0-skill.zip 与 -source.zip
   ```

   全新克隆没有 `extension/bin/`（已 gitignore）。在运行 `test:vscode` 前先执行 `npm --prefix extension run package`（会复制二进制并生成 `extension/LICENSE`、`extension/media/viewer.html`），或手动复制 `bin/code-trail.exe` 到 `extension/bin/`。若 shell 设置了 `ELECTRON_RUN_AS_NODE=1`（部分 Electron 工具链），需要先移除该变量再运行 `test:vscode`。

   `package-skill.py` 在 `bin/code-trail.exe` 不存在时会以“先运行 scripts/build.ps1”的明确错误退出。

2. 用最终二进制重新生成 README 截图：

   ```powershell
   node scripts/render-doc-screenshots.cjs
   ```

   如需复用本机已有的 research-atlas 预览，先重新渲染这些 HTML，再用 `--architecture-html/--modules-html/--review-html` 出图，详见 [docs/screenshots/README.md](screenshots/README.md)。

3. 提交需要跟踪的变更（README、文档、`docs/screenshots/*.png`、脚本等）。确认 `git status` 中没有 `bin/`、VSIX 或 `output/` 文件。

4. 创建 Release 并上传资产（示例用 GitHub CLI，也可在网页操作）：

   ```powershell
   gh release create v0.4.0 `
     output/code-trail-0.4.0-win32-x64.vsix `
     output/code-trail-0.4.0-skill.zip `
     output/code-trail-0.4.0-source.zip `
     --title "CodeTrail 0.4.0" `
     --notes-file <发布说明.md>
   ```

   建议在发布说明中列出：平台限制（预编译仅 Windows x64）、VSIX 安装方式、Skill 用法、`VERIFICATION.md` 链接，以及“未发布 Marketplace”的说明。

5. 更新 `VERIFICATION.md` 中的最终结果、VSIX SHA-256 与证据位置；不要保留上一轮构建的过时数值。

## 资产说明

| 资产 | 内容 |
| --- | --- |
| `code-trail-<version>-win32-x64.vsix` | Windows x64 VS Code 插件，内含原生二进制与界面资源 |
| `code-trail-<version>-skill.zip` | 可直接放入 Agent `skills/code-trail` 的 Skill，含 `bin/code-trail.exe`、文档与源码 |
| `code-trail-<version>-source.zip` | 源码与构建脚本，供其他平台自行 `cargo build --release --locked` |
