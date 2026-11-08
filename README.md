# CodeTrail

在 Agent 写代码之后，用模糊搜索与交互式函数调用网络帮助开发者定位、理解和复核修改。核心由 Rust 编写；独立 HTML 与 VS Code Webview 共用同一套阅读和探索逻辑。当前版本 0.4.0，发布者 `wwwh7`。

[下载预编译版本（GitHub Releases）](https://github.com/wwwh7-cypher/CodeTrail/releases) · [Agent Skill 入口](SKILL.md) · [验证记录与边界](VERIFICATION.md) · [发布说明](docs/RELEASING.md)

## 截图

以下截图由 `node scripts/render-doc-screenshots.cjs` 生成：架构与模块图扫描仓库内示例项目 `examples/config-service`，修改影响图来自同一示例的临时 Git 副本（含脚本生成的演示改动），因此可直接复现。

![架构导览](docs/screenshots/architecture.png)

*搜索定位函数并展开调用网络（示例展示两层）；旧节点保留坐标，可固定、收起和回退。*

![功能模块](docs/screenshots/modules.png)

*功能模块视图按目录/文件边界聚合实际已解析的调用；浅层仓库按文件分组。*

![修改影响](docs/screenshots/review.png)

*Git 修改影响：新增/修改/删除函数与调用边，函数队列与非函数文件队列分开复核。*

## 5 分钟上手

### 1. VS Code（Windows x64）

1. 从 [Releases](https://github.com/wwwh7-cypher/CodeTrail/releases) 下载 `code-trail-0.4.0-win32-x64.vsix`。
2. 在 VS Code 扩展视图右上角选择“从 VSIX 安装”，重新加载窗口。
3. 打开任意本地项目，运行命令 `CodeTrail: 打开架构导览`。
4. 输入函数缩写、文件名或已有中文职责并回车；单击节点继续展开。`Ctrl+Alt+G` 在编辑器中定位当前函数。

普通搜索、看图与 Git 审查不依赖模型；只有“解释此函数”需要自备 API（见下文两种模型入口）。

### 2. 命令行与独立 HTML

从 Releases 下载 skill 压缩包（内含 `bin/code-trail.exe`），或在本仓库构建二进制：

```powershell
pwsh -File scripts/build.ps1
& .\bin\code-trail.exe scan .\examples\config-service --out .\output\architecture.html --cache .\output\parse-cache.json
& .\bin\code-trail.exe query .\output\architecture.graph.json search 'config'
& .\bin\code-trail.exe review . --base HEAD --out .\output\review.html
Start-Process .\output\architecture.html
```

HTML 自包含、无需服务器；VS Code 版对同一份图使用启用脚本的 Webview。同名函数需使用输出中的唯一 ID 或 `symbol_key`。`review . --base HEAD` 展示当前工作区相对 HEAD 的改动；扫描其他项目时把 `.` 换成目标路径。

### 3. Agent Skill

把 Releases 中的 skill zip 解压为 Agent 的 `skills/code-trail` 目录（zip 已包含 `bin/` 原生二进制）：

```text
code-trail scan <repository> --out <output>/architecture.html --cache <output>/parse-cache.json
code-trail query <output>/architecture.graph.json search "parseConfig" --limit 20
```

由当前对话模型解释源码，再通过 `prepare-annotations` / `annotate` 写入经过 Rust 结构校验的说明，无需额外 API Key。完整工作流见 [SKILL.md](SKILL.md)。

## 两种模型入口

**VS Code（BYOK）：** 设置 Provider、API Base URL、可选 Models URL、Model ID 与 API Key；Key 保存在 SecretStorage，按协议与来源隔离。支持 OpenAI-compatible 与 Anthropic 协议。

**Agent Skill：** 使用当前对话模型，无需额外 Key，也不复制插件凭据。说明只描述函数职责，任何模型叙述都不会决定调用关系。

## 平台与分发

- **预编译产物仅 Windows x64**：VSIX 要求 VS Code `^1.100.0`；Skill zip 内含 Windows x64 原生二进制。
- **其他平台**需从源码构建：`cargo build --release --locked`，二进制位于 `target/release/code-trail`。Windows 本地可用 `scripts/build.ps1`；`.cargo/config.toml` 仅为 `x86_64-pc-windows-gnu` 目标配置链接器。
- **未发布 Marketplace**。GitHub Releases 是唯一的预编译分发渠道；`output/` 是本地构建/演示目录（已 gitignore），不进入仓库。
- 仓库不提交生成产物：`bin/`、`extension/bin/`、`extension/LICENSE` 以及 VSIX/Skill/源码 zip 均由构建或打包脚本生成。

## 构建与测试

```powershell
pwsh -File scripts/build.ps1            # Rust 发布构建，产物复制到 bin/
cargo +stable-x86_64-pc-windows-gnu test --locked
npm --prefix extension ci
npm --prefix extension run test:unit
npm --prefix extension run test:vscode
npm --prefix extension run package      # output/code-trail-0.4.0-win32-x64.vsix
python scripts/package-skill.py         # output/*.zip，需先构建 bin/
node scripts/render-doc-screenshots.cjs # 重新生成 docs/screenshots/*.png
```

- VS Code 集成测试需要真实 Git 项目与 VS Code：用 `CODETRAIL_REAL` 指定项目，用 `CODETRAIL_VSCODE` 指定 `Code.exe`。未设置且检测不到项目时会打印跳过说明并正常退出；检测不到 VS Code 时给出明确错误。若 shell 设置了 `ELECTRON_RUN_AS_NODE=1`（部分 Electron 工具链会设置），运行 `test:vscode` 前需先移除该变量。
- VSIX 内置 `extension/bin/code-trail[.exe]`；仓库不再提交该二进制。全新克隆后首次运行 `test:vscode` 前，先执行 `npm --prefix extension run package`（它会复制二进制、生成 `extension/LICENSE` 并按当前构建刷新 `extension/media/viewer.html`），或手动把 `bin/code-trail.exe` 复制到 `extension/bin/`，或在 VS Code 设置 `codeTrail.binaryPath` 指向本机 CLI。
- 浏览器回归：先运行 `node scripts/verify-native.mjs`，再运行 `node scripts/verify-explorer.cjs`。Playwright 与 Chromium 由 `scripts/lib/browser-env.cjs` 解析，可用 `PLAYWRIGHT_MODULE`、`CHROMIUM_PATH` 覆盖，也可临时安装 Playwright（这些开发者依赖不进入 VSIX）。
- 截图脚本默认扫描 `examples/config-service`（架构/模块）并生成一个带演示改动的临时 Git 副本（修改影响）；也可用 `--architecture-html`、`--modules-html`、`--review-html` 复用已生成的 HTML（例如本机的 research-atlas 预览），或用 `--project` 指定要扫描的项目。`--help` 列出全部参数。
- 发布所需资产与命令见 [docs/RELEASING.md](docs/RELEASING.md)。

## 解释边界

支持 TS/TSX/JS/JSX/Svelte script/Python/Go 的语法索引；Go 支持同包函数、已知接收者与有明确结构字段类型的静态候选调用。解析器发生语法恢复时，诊断以 `path:line:col: ...` 形式给出；它只说明该文件在此处的静态索引可能不完整，不等同于项目编译失败。接口分派、动态回调连接、外部包和框架注册可能需要语言服务或人工复核；语言服务同样只是静态证据，不能证明完整运行时影响。

模块边只聚合已解析的跨模块函数调用；HTTP 关联需要单独证据，不会被冒充为函数调用。VS Code 运行时额外提供源码跳转、原生 diff、持久化复核状态、SecretStorage 与可选语言服务补强；离线 HTML 对这些入口给出明确说明。

稳定记忆持久化硬限制为 1,500 个 cl100k_base tokens；离线 HTML 导出采用更保守的 1,500 UTF-8 字节预算，两者均需用户确认。MCP 只提供读取/查询工具，不暴露写入或模型调用。静态可达性只说明潜在影响，不证明运行时行为变化或审查充分性。

## 文档

- [VERIFICATION.md](VERIFICATION.md)：本机实际执行结果、证据位置与未覆盖边界。
- [PRODUCT_PLAN.md](PRODUCT_PLAN.md)：产品计划与验收目标；目标指标不等于已证实的实用效果。
- [USER_TEST_PROTOCOL.md](USER_TEST_PROTOCOL.md)：真人对照验收方法。
- [SKILL.md](SKILL.md)：Agent Skill 使用入口。
- [references/](references/)：分析边界与说明写回契约。
