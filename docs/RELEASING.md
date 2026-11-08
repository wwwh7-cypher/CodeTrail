# 发布与分发

CodeTrail 通过两个渠道分发：

- **VS Code Marketplace**（publisher `wwwh7`，验证页面 <https://marketplace.visualstudio.com/items?itemName=wwwh7.code-trail>）：面向 VS Code 用户的 VSIX 安装渠道。
- **GitHub Releases**（<https://github.com/wwwh7-cypher/CodeTrail/releases>）：VSIX、Skill zip 与源码 zip，也是 Skill 用户的下载来源。

两个渠道使用同一个版本号；版本号来自 `Cargo.toml` 的 `[package] version`（`extension/package.json` 需同步，发布前检查两者一致）。

## 仓库约定

- 不提交生成产物：`bin/`、`extension/bin/`、`extension/LICENSE` 由构建/打包脚本生成，`.gitignore` 已忽略。
- `output/` 是本地构建与演示目录（VSIX、Skill/Source zip、截图中间文件），同样被忽略。
- `docs/screenshots/*.png` 会提交，README 直接引用它们。
- CI（`.github/workflows/checks.yml`）在 Linux/Windows/macOS 上构建并打包 VSIX，作为 workflow artifact 上传；**不会**自动创建 Release 或发布 Marketplace。发布动作只由维护者本机执行。
- 发布凭据（`VSCE_PAT`、Azure DevOps PAT）只放在本机环境变量中，不写入仓库、脚本、日志或发布说明。

## 发布步骤（版本号记为 `<version>`，如 0.5.0）

1. 确认版本号一致：`Cargo.toml`、`Cargo.lock`、`extension/package.json`、`extension/package-lock.json` 均为 `<version>`；更新 `extension/CHANGELOG.md` 与 README 中的版本引用。

2. 构建与验证（Windows x64 预编译包）：

   ```powershell
   pwsh -File scripts/build.ps1
   cargo +stable-x86_64-pc-windows-gnu test --locked
   npm --prefix extension ci
   npm --prefix extension run test:unit
   npm --prefix extension run test:vscode   # 需要 CODETRAIL_REAL / CODETRAIL_VSCODE
   npm --prefix extension run package       # output/code-trail-<version>-win32-x64.vsix
   python scripts/package-skill.py          # output/code-trail-<version>-skill.zip 与 -source.zip（版本号自动读取 Cargo.toml）
   ```

   全新克隆没有 `extension/bin/`（已 gitignore）。在运行 `test:vscode` 前先执行 `npm --prefix extension run package`（会复制二进制并生成 `extension/LICENSE`、`extension/media/viewer.html`），或手动复制 `bin/code-trail.exe` 到 `extension/bin/`。若 shell 设置了 `ELECTRON_RUN_AS_NODE=1`（部分 Electron 工具链），需要先移除该变量再运行 `test:vscode`。

   `package-skill.py` 在 `bin/code-trail.exe` 不存在时会以“先运行 scripts/build.ps1”的明确错误退出。

3. 用最终二进制重新生成 README 截图：

   ```powershell
   node scripts/render-doc-screenshots.cjs
   ```

   如需复用本机已有的 research-atlas 预览，先重新渲染这些 HTML，再用 `--architecture-html/--modules-html/--review-html` 出图，详见 [docs/screenshots/README.md](screenshots/README.md)。

4. 检查 VSIX 内容（应包含 `bin/code-trail.exe`、`LICENSE`、`README.md`、`CHANGELOG.md`，且不含 `node_modules` 与测试文件）：

   ```powershell
   Push-Location extension; node node_modules/@vscode/vsce/vsce ls; Pop-Location
   ```

5. 提交需要跟踪的变更（README、文档、`docs/screenshots/*.png`、`extension/CHANGELOG.md`、脚本等）。确认 `git status` 中没有 `bin/`、VSIX 或 `output/` 文件，也确认提交中没有 PAT 或密钥。

6. 创建 GitHub Release 并上传资产（示例用 GitHub CLI，也可在网页操作）：

   ```powershell
   gh release create v<version> `
     output/code-trail-<version>-win32-x64.vsix `
     output/code-trail-<version>-skill.zip `
     output/code-trail-<version>-source.zip `
     --title "CodeTrail <version>" `
     --notes-file <发布说明.md>
   ```

   建议在发布说明中列出：平台限制（预编译仅 Windows x64）、VSIX/Marketplace 安装方式、Skill 用法、`VERIFICATION.md` 链接与 `CHANGELOG.md` 摘要。

7. 发布到 VS Code Marketplace（见下一节）。

8. 更新 `VERIFICATION.md` 中的最终结果、VSIX SHA-256、字节数与证据位置；不要保留上一轮构建的过时数值。

## VS Code Marketplace 发布

### 前置条件

- **publisher**：Marketplace 上已存在 publisher `wwwh7`，且执行发布的账号是其成员/所有者（在 <https://marketplace.visualstudio.com/manage> 管理）。
- **PAT（Personal Access Token）**：在 Azure DevOps 创建 PAT（<https://dev.azure.com> → User settings → Personal access tokens），要求：
  - 账号在 `wwwh7` publisher 的组织/成员范围内；
  - scope 勾选 **Marketplace → Manage**（只读 scope 不足以发布）；
  - 设置合理的过期时间，发布完成后可轮换或删除。
- 只在当前 PowerShell 会话设置 PAT，不写入任何仓库文件：

  ```powershell
  $env:VSCE_PAT = '<粘贴 PAT；不要提交、不要回显到日志>'   # 仅本机会话
  ```

  可选：先把 PAT 存入本机凭据管理器（`npx @vscode/vsce login wwwh7`），或使用 CI secret；本仓库默认手动发布，不把 token 写入脚本。

### 发布命令

推荐复用已构建好的 VSIX（与最终验证的产物一致，不重新打包）：

```powershell
npx vsce publish --packagePath output/code-trail-<version>-win32-x64.vsix
```

- 如果 `vsce` 不在 PATH，可用本地 devDependency：`npx @vscode/vsce publish --packagePath output/code-trail-<version>-win32-x64.vsix`。
- 本仓库另有确认门控辅助脚本 `npm --prefix extension run publish:marketplace -- --confirmed`（在 `extension/` 内重新打包并发布，需要 `--confirmed` 且同样读取 `VSCE_PAT`）。
- VSIX 是**平台限定**包（本仓库为 `win32-x64`）；Marketplace 会把它标记为 Windows x64。其他平台用户需从源码构建，不能使用该包。
- 发布后核对页面：<https://marketplace.visualstudio.com/items?itemName=wwwh7.code-trail>。首次发布后状态可能有延迟。

### 注意事项

- **发布是公开且接近不可逆的**：版本一旦发布，对所有用户可见，同一版本号不能再次发布；下架/修改能力有限。发布前务必核对版本号、package 内容、`CHANGELOG.md` 与 README。
- 不要提交、打印或粘贴 PAT；不要把它写进发布说明、issue 或日志。发布结束后可撤销/轮换该 PAT。
- 同一版本的多平台包需分别以对应 `--target` 打包后发布；当前只发布 Windows x64。
- Marketplace 上的 README/CHANGELOG 来自 VSIX 内的 `extension/README.md` 与 `extension/CHANGELOG.md`，发布前确认已更新。

## 资产说明

| 资产 | 内容 |
| --- | --- |
| `code-trail-<version>-win32-x64.vsix` | Windows x64 VS Code 插件，内含原生二进制与界面资源；同时发布到 Marketplace 与 GitHub Releases |
| `code-trail-<version>-skill.zip` | 可直接放入 Agent `skills/code-trail` 的 Skill，含 `bin/code-trail.exe`、文档与源码；仅 GitHub Releases |
| `code-trail-<version>-source.zip` | 源码与构建脚本，供其他平台自行 `cargo build --release --locked`；仅 GitHub Releases |
