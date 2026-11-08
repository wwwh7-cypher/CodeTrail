# CodeTrail

在 Agent 写代码之后，用模糊搜索与交互式函数调用网络帮助开发者定位、理解和复核修改。

核心由 Rust 编写；独立 HTML 与 VS Code Webview 共用阅读和探索逻辑。当前本地交付版本为 0.4.0，发布者 `wwwh7`，本轮不发布 Marketplace。

## 交付

- `output/code-trail-0.4.0-win32-x64.vsix`：可安装的 Windows x64 VS Code 插件。
- `output/research-atlas/frontend-preview.html`：真实前端源码的交互预览。
- `output/research-atlas/backend-preview.html`：真实 Go 后端的函数与功能模块图。
- `output/research-atlas/commit-review.html`：真实 HEAD~1 → HEAD 提交的审查示例。
- `output/code-trail-demo.gif`：搜索、展开、模块、记忆与后端的演示。
- `output/code-trail-0.4.0-skill.zip` / `code-trail-0.4.0-source.zip`：Skill 与源码分发包。
- `SKILL.md`：Agent Skill 入口。
- `PRODUCT_PLAN.md`：产品计划与验收目标。
- `VERIFICATION.md`：实际执行结果与边界；目标指标不等于已证实的实用效果。

## 两种模型入口

**VS Code：**设置 API URL、Model ID、API Key；Key 保存于 SecretStorage。普通搜索、看图和 Git 审查不依赖模型。

**Agent Skill：**扫描后让当前对话模型解释所选源码，用 `prepare-annotations` 和 `annotate` 写入经过结构校验的说明，无需额外 Key。说明不会决定调用关系。

## 安装与打开

在 VS Code 扩展视图选择“从 VSIX 安装”，安装 output 中的 Windows x64 包。运行 `CodeTrail: 打开架构导览`。编辑器内可用 `Ctrl+Alt+G` 定位当前函数；API 设置页填写 URL、Model ID 和 Key。

HTML 可直接用浏览器打开；VS Code 使用启用交互脚本的 Webview 展示同一界面。Skill 压缩包解压为 Agent 的 skills/code-trail 目录后，通过 `$code-trail` 调用。

## 本地运行

```powershell
& .\bin\code-trail.exe scan 'E:\work\research-atlas\app' --cache '.\output\parse-cache.json' --out '.\output\architecture.html'
& .\bin\code-trail.exe query '.\output\architecture.graph.json' search 'pBib'
& .\bin\code-trail.exe query '.\output\architecture.graph.json' neighbors 'parseBibliography'
& .\bin\code-trail.exe query '.\output\architecture.graph.json' modules
& .\bin\code-trail.exe review 'E:\work\research-atlas\app' --base HEAD --out '.\output\review.html'
```

同名函数需使用输出中的唯一 ID 或 `symbol_key`。一级查询有 offset/limit，返回总量、调用证据与未解析项。缓存复用未变化文件的语法结果，并重新解析符号关系。

## 构建与测试

需要 Rust、可用 C 编译器，以及用于扩展构建的 Node.js。此 Windows 环境使用 GNU Rust 工具链：

```powershell
pwsh -File scripts/build.ps1
npm --prefix extension ci
npm --prefix extension run test:unit
npm --prefix extension run test:vscode
npm --prefix extension run package
```

Rust 检查：`cargo +stable-x86_64-pc-windows-gnu test --locked`。其他平台可使用默认 Rust 工具链。VS Code 测试默认使用本机 `E:/Program Files/Code/Code.exe`，可通过 `CODETRAIL_VSCODE` 指定；测试使用独立用户目录和临时修改样例，不写入验证目标项目的业务源码。

## 解释边界

支持 TS/JS/Svelte script/Python/Go 的语法索引；Go 支持同包函数、已知接收者与有明确结构字段类型的静态候选调用。接口分派、动态回调连接、外部包和框架注册可能需要语言服务或人工复核。语言服务也是静态证据，不能证明完整运行时影响。

模块边聚合已解析的跨模块函数调用。HTTP 关联需要单独证据，不会被冒充为函数调用。源码跳转、Git diff、持久化复核状态、SecretStorage 与语言服务由 VS Code 运行时提供；离线 HTML 对这些入口给出明确说明。

稳定记忆持久化硬限制为 1,500 个 cl100k_base tokens；离线 HTML 导出采用更保守的 1,500 UTF-8 字节预算。两者均需要确认。MCP 只提供读取/查询工具，不暴露写入或模型调用。

浏览器和原生交付回归：先运行 `node scripts/verify-native.mjs`，再运行 `node scripts/verify-explorer.cjs`。可用 `PLAYWRIGHT_MODULE` 与 `CHROMIUM_PATH` 指定本机浏览器测试依赖；这些依赖不进入 VSIX。真人效果验证见 `USER_TEST_PROTOCOL.md`。
