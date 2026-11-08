# CodeTrail 0.4.0 本地交付验证

日期：2026-11-08。发布者：`wwwh7`。交付通过 GitHub Releases 分发 Windows x64 VSIX、Skill 与源码包；未发布 VS Code Marketplace。

本文件记录开发机上的实际执行结果。表中 `output/...` 证据是本机验证产物，已被 gitignore、不随仓库分发；预编译 VSIX、Skill 与源码包通过 [GitHub Releases](https://github.com/wwwh7-cypher/CodeTrail/releases) 分发。所有结果均来自下方“复现”命令与最终构建。

## 实际验证结果

| 检查 | 结果 | 可核对证据 |
| --- | --- | --- |
| Rust 单元与工作流 | 41 项通过（23 + 4 + 14），0 警告 | output/eval/final-cargo-test.log |
| API / 复核状态测试 | 3 项通过；真实本机 HTTP mock 分别测试 OpenAI-compatible 与 Anthropic | output/extension-unit.log |
| VS Code Extension Host | 10 组集成场景通过；本机 VS Code 1.145.2（最终构建重跑） | output/vscode-test/extension-host-evidence.json |
| 浏览器回归 | 15 组通过，页面 JavaScript 错误 0；另做诊断横幅/匿名回调折叠/模块兜底的针对性检查 | output/browser-test/evidence.json、output/eval/shots/ui-* |
| VSIX 安装 | 独立用户/扩展目录成功安装 wwwh7.code-trail@0.4.0（最终构建重跑） | output/vsix-smoke、output/vsix-install.log |
| 打包核对 | 内置二进制与宿主 JS 与本次源码构建一致，排除 node_modules 与测试文件 | output/package-evidence.json |
| Skill 元数据 | quick_validate.py 通过 | 本地 skill-creator 校验 |
| MCP | 实际子进程 stdio initialize、tools/list、search、neighbors 通过 | output/native-test/evidence.json |
| 无 Key 的 Skill 说明 | 当前会话读取源码，分三批校验写入 27 个 Go 函数说明 | output/research-atlas/backend-source-batch-*.json / backend-annotations-*.json |

VS Code 场景实际覆盖：真实项目扫描、增量缓存、Git 修改、内置 JS/TS Call Hierarchy、SecretStorage、API/记忆入口、编辑器定位函数、原生 diff、未保存文档标记过期、保存后自动增量刷新。刷新测试检查 Webview 代数不变，保留现有页面上下文。修复了 diff 可能替换图面板的问题，diff 在第一编辑列打开。

浏览器覆盖：缩写与键盘搜索、连续 10 次邻居点击、旧坐标保留、共享节点去重、固定/收起/前进后退、快速切换与减少动态效果、层级增减、模块下钻、Markdown 阅读/编辑/确认导出、API 表单、独立 SVG、1200/960/600 px 布局、Go 回调与类型字段关系、源码刷新身份映射、删除函数基线和复核标记。

## 真实验证项目

- 目录：`E:/work/research-atlas/app`（本机验证路径；复现时用 `CODETRAIL_REAL` 指向任意真实 Git 项目，缺失时集成测试会打印跳过说明）。
- Git HEAD：`e3e777bf416937fe2ab799b1dbfb284490531b27`。
- 索引快照：`8f2555c82f272e4ee88c087f3c4edde19c9bd716001b37489dd5e25eb2e91639`。
- 151 个已索引文件，2470 个函数，2569 条已解析调用，9991 处未解析调用。修复 `abstract` 误报后文件诊断由 7 条降为 1 条（仅剩超 2 MB 生成脚本），图快照保持 `8f2555c8…` 不变。
- 最近一次缓存扫描：analysis 93 ms（wall 188 ms）；缓存命中 151 文件，新解析 0 文件。单次本机测量，非跨机器性能承诺。
- `HEAD → 工作区`：0 个函数变更。既有 verification 资源修改仍在文件队列中，未改项目业务源码。
- `HEAD~1 → HEAD`：已导出真实提交审查，保留新增、修改、删除以及非函数文件；这些数目不是风险评分。
- 前端示例默认从 parseBibliography 展开。Go 后端示例默认从 Answer 展开，模块图按 HTTP、检索、存储、模型适配等实际文件分组。

文件诊断当前只有一份超出大小限制的生成脚本；此前六处解析恢复提示是 tree-sitter-typescript 对 `abstract` 属性名的误报，已在本轮修复（见“本轮修复与清理”）。诊断不能被当作源代码编译错误结论；诊断以 `path:line:col: ...` 形式标出发生语法恢复的位置。静态图中未解析数量也不能当作准确率。

## 搜索与关系样例

30 条本项目内预设查询（15 条标识符缩写 + 15 条中文职责）中，29 条目标进入前五，比例 96.7%。未进入前五的是 `pRis → parseRis`（当前全项目原生查询中第六）；界面可补充路径/职责词缩小范围。

10,000 个合成符号：浏览器 30 次输入到下一帧的 p95，本轮最终构建重跑为 **56.6 ms**（上一轮为 43.6 ms；同一台机器不同运行存在波动，不构成性能承诺）。另测原生 CLI 20 次启动、读图和搜索，p95 为 **106.8 ms**，包含进程和 JSON 读取开销。这两个指标的范围不同。样例由真实符号复制构建，本机无头 Chromium 测量；不代表所有项目或设备。

九个人工设定关系回归样例包含 TS 本地与显式导入、Svelte、Python、Go 同包与结构字段，以及遮蔽/动态接收者反例。本样例 TP=6、FP=0、FN=0（本样例 precision/recall=1），样本很小，不能外推整个真实仓库的解析准确率。

## 本轮修复与清理（2026-11-08 第二轮，版本仍为 0.4.0）

### TypeScript `abstract` 属性误报（P0）

- 根因：`tree-sitter-typescript 0.23.2`（crates.io 最新版）把 `abstract` 当作关键字，且该关键字不在语法允许作为标识符的 `_reserved_identifier` 列表中。因此 `interface X { abstract?: string }`、`abstract(): void`、`class { abstract = 1 }`、类型字面量等合法 TypeScript 代码会触发解析恢复，被误报为“未支持语法”；只有类型声明的文件（如 `types.ts`）不会产出符号。
- 修复：在 `src/analysis.rs` 的解析流程中，仅当首次解析出现错误且文件类型为 ts/tsx/svelte 时，用等长掩码重试：只把属性位置的 `abstract` 替换为等长标识符 `abs_trac`，重试无错误才采用该语法树；所有名称、签名、正文与哈希仍从原始源码读取（字节偏移一致）。`abstract class` / `abstract new` 等修饰符位置不受影响。
- 效果：research-atlas 验证项目文件诊断从 7 条降为 1 条（仅剩超 2 MB 生成脚本），索引快照仍为 `8f2555c8…`，文件/函数/边数量不变；新增 8 个回归用例（接口属性、方法、类字段、类型字面量、Svelte、混杂真实错误等）。
- 诊断文本同时增加首个错误/缺失节点的 1-based `path:line:col:`。
- 残留：若 `abstract` 与后续 token 之间夹注释（如 `abstract /*x*/: string`）仍不会触发掩码；Svelte `<script>` 之外的标记本就不解析。

### 界面（P0）

- 诊断可见：统计区新增“分析边界”按钮，展示 `N 个文件不完整`；点击该按钮、顶部“分析边界”或统计第 4 项均可打开边界弹窗，列出文件诊断、未纳入图调用的按原因聚合（12 类）与下一步建议。
- 审核队列降噪：匿名回调 `<callback@…>` 默认折叠为“显示 N 个匿名回调变更”，位于具名变更之前；展开状态跨重渲染保留。示例：代码仓初始导入的 review 页为 185 个具名函数变更 + 486 个匿名回调 + 71 个文件/非函数修改。
- 无说明函数：不再统一显示“处理函数数据”，未知名称回退为真实函数名并去重；检查器明确显示“未生成职责说明”，只保留结构提示与来源。
- 统计文案：第 4 项改为“未纳入图的调用”，取消橙色强调，点击查看原因分布。
- 扁平仓库模块兜底：只有单一目录分组时按文件分组（`examples/config-service` 由 1 个 `(root)` 变为 5 个文件模块），并支持进入文件层级与动态范围选项。

### CLI 与工程（P1）

- `--help` 为 10 个顶层子命令、7 个 query 操作、7 个 memory 操作补齐中文 about/long_about（24 处），新增帮助文本回归测试。
- 去掉脚本/测试中的个人绝对路径：浏览器工具统一由 `scripts/lib/browser-env.cjs` 解析（`PLAYWRIGHT_MODULE` / `CHROMIUM_PATH` 可覆盖）；VS Code 集成测试改用 `CODETRAIL_REAL` / `CODETRAIL_VSCODE`，缺失时打印跳过说明或明确错误。
- 不再跟踪构建产物：`bin/code-trail.exe`、`extension/bin/code-trail.exe`、`extension/LICENSE` 移出版本库（`.gitignore` 忽略，由构建/打包脚本生成）。仓库跟踪文件 71 → 68，工作区跟踪体积约 24 MB → 2.9 MB。
- 新增 `docs/RELEASING.md` 与 `scripts/render-doc-screenshots.cjs`；README 增加截图、5 分钟上手、平台限制与 Releases 说明。
- 自检注意：运行 `test:vscode` 的环境若设置了 `ELECTRON_RUN_AS_NODE=1`（部分 Electron 工具链会设置），VS Code 会被当作 Node 执行参数；需先 `Remove-Item Env:\ELECTRON_RUN_AS_NODE` 再运行。

## 记忆与说明边界

删除/基线函数的审查记录保留 before:: 身份、旧签名和基线调用证据，有专门回归用例。

原生稳定层使用 cl100k tokenizer 严格限制 1,500 tokens；测试连续 12 次稳定层保存、超限拒绝、确认要求、快照失效、记录查询与修剪。离线 HTML 保守限制为 1,500 UTF-8 字节。原生记忆在用户核对后通过确认框写入；测试使用独立临时存储，没有代替用户确认真实架构共识。

Skill 与 BYOK 都使用 snapshot/id/content_hash/title/summary 契约，并由 Rust 校验器拒绝未知字段、过期哈希、未知/重复符号和拓扑字段。BYOK 缓存包含模型/地址/提示词版本/源码哈希；配置变更使旧模型说明失效。说明不修改调用边。

## 待外部验收

1. 没有调用用户实际付费 API。本轮网络协议、模型列表、认证失败、错误摘要、重复/未知函数、快照/哈希和额外字段由本机 HTTP mock 验证。真实地址、Key、Model 的联调需由对应服务配置支持。
2. 实际 VS Code JS/TS Call Hierarchy 已通过。Go 和 Python 语言服务补强使用相同 provider 接口，但本机没有单独验收 gopls/Python 调用层级。Go 示例来自已测试的 Rust 语法和类型字段索引；接口分派、跨包类型关系和动态路由仍可能缺边。
3. 动画用 requestAnimationFrame 与非线性 ease-out 实现，验证了中断、稳定坐标与减少动态效果。没有给出跨设备 60 fps 保证。
4. 真人任务对照、约 30% 时间改善、七日留存、BYOK 完成率与付费意愿尚未测量。使用 USER_TEST_PROTOCOL.md 收集真实参与者数据。
5. Marketplace 保持未发布，也没有使用 Marketplace 发布凭据。仓库与 GitHub Releases 是唯一分发渠道，Release 资产由维护者手动上传。

## 复现

```powershell
cargo +stable-x86_64-pc-windows-gnu test --locked
pwsh -File scripts/build.ps1
npm --prefix extension ci
npm --prefix extension run test:unit
npm --prefix extension run package
npm --prefix extension run test:vscode
node scripts/render-research-preview.mjs bin/code-trail.exe
node scripts/render-backend-preview.mjs
node scripts/verify-native.mjs
node scripts/verify-explorer.cjs
python scripts/package-skill.py
node scripts/render-doc-screenshots.cjs
```

实际项目测试目录、VS Code 可执行文件、Playwright 模块与 Chromium 路径可用 `CODETRAIL_REAL`、`CODETRAIL_VSCODE`、`PLAYWRIGHT_MODULE`、`CHROMIUM_PATH` 覆盖（见 README 开发章节与 `scripts/lib/browser-env.cjs`）。`bin/code-trail.exe` 与 `extension/bin/code-trail.exe` 都是构建产物：先运行 `pwsh -File scripts/build.ps1`；全新克隆在 `npm --prefix extension run test:vscode` 前还需执行 `npm --prefix extension run package`（或手动复制二进制到 `extension/bin/`），因为该目录不再提交到仓库。若 shell 设置了 `ELECTRON_RUN_AS_NODE=1`，运行 `test:vscode` 前需要先移除。浏览器回归的 review.html 由 verify-native.mjs 提前生成。快照图和 HTML 含源码片段，仅供用户请求的本地评审。

## 发布资产（GitHub Release `v0.4.0`）

- 下载：<https://github.com/wwwh7-cypher/CodeTrail/releases/tag/v0.4.0>
- `code-trail-0.4.0-win32-x64.vsix`（4,503,305 字节）：SHA-256 `CCA75DB0C66295DAA05CCA8FCC497288C94AC3DFD6A2A103844EB234FD79A5DA`
- `code-trail-0.4.0-skill.zip`（4,490,701 字节）：SHA-256 `FD1FA3111C557C86BBF2E06C5F7A4681A1C1DCDA84013293BD9DDDFD82459E7C`
- `code-trail-0.4.0-source.zip`（9,365,520 字节）：SHA-256 `637381B695932CAFA085AFA639A310394D376B87315F20963823EA7306AE3099`
