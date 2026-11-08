# CodeTrail 0.4.0 本地交付验证

日期：2026-11-08。发布者：`wwwh7`。交付为本地 HTML、Skill 与 Windows x64 VSIX，未执行 Marketplace 发布。

## 实际验证结果

| 检查 | 结果 | 可核对证据 |
| --- | --- | --- |
| Rust 单元与工作流 | 32 项通过（15 + 3 + 14） | output/rust-tests.log |
| API / 复核状态测试 | 3 项通过；真实本机 HTTP mock 分别测试 OpenAI-compatible 与 Anthropic | output/extension-unit.log |
| VS Code Extension Host | 10 组通过；本机 VS Code 1.145.2 | output/vscode-test/extension-host-evidence.json |
| 浏览器回归 | 15 组通过，页面 JavaScript 错误 0 | output/browser-test/evidence.json |
| VSIX 安装 | 独立用户/扩展目录成功安装 wwwh7.code-trail@0.4.0 | output/vsix-install.log |
| 打包核对 | 内置二进制与宿主 JS 与本次源码构建一致，排除 node_modules 与测试文件 | output/package-evidence.json |
| Skill 元数据 | quick_validate.py 通过 | 本地 skill-creator 校验 |
| MCP | 实际子进程 stdio initialize、tools/list、search、neighbors 通过 | output/native-test/evidence.json |
| 无 Key 的 Skill 说明 | 当前会话读取源码，分三批校验写入 27 个 Go 函数说明 | output/research-atlas/backend-source-batch-*.json / backend-annotations-*.json |

VS Code 场景实际覆盖：真实项目扫描、增量缓存、Git 修改、内置 JS/TS Call Hierarchy、SecretStorage、API/记忆入口、编辑器定位函数、原生 diff、未保存文档标记过期、保存后自动增量刷新。刷新测试检查 Webview 代数不变，保留现有页面上下文。修复了 diff 可能替换图面板的问题，diff 在第一编辑列打开。

浏览器覆盖：缩写与键盘搜索、连续 10 次邻居点击、旧坐标保留、共享节点去重、固定/收起/前进后退、快速切换与减少动态效果、层级增减、模块下钻、Markdown 阅读/编辑/确认导出、API 表单、独立 SVG、1200/960/600 px 布局、Go 回调与类型字段关系、源码刷新身份映射、删除函数基线和复核标记。

## 真实验证项目

- 目录：`E:/work/research-atlas/app`。
- Git HEAD：`e3e777bf416937fe2ab799b1dbfb284490531b27`。
- 索引快照：`8f2555c82f272e4ee88c087f3c4edde19c9bd716001b37489dd5e25eb2e91639`。
- 151 个已索引文件，2470 个函数，2569 条已解析调用，9991 处未解析调用，7 条文件诊断。
- 最近一次缓存扫描：98 ms；缓存命中 151 文件，新解析 0 文件。单次本机测量，非跨机器性能承诺。
- `HEAD → 工作区`：0 个函数变更。既有 verification 资源修改仍在文件队列中，未改项目业务源码。
- `HEAD~1 → HEAD`：已导出真实提交审查，保留新增、修改、删除以及非函数文件；这些数目不是风险评分。
- 前端示例默认从 parseBibliography 展开。Go 后端示例默认从 Answer 展开，模块图按 HTTP、检索、存储、模型适配等实际文件分组。

文件诊断包括一份超出大小限制的生成脚本，以及六处解析恢复/未支持语法提示。它们不能被当作源代码编译错误结论。静态图中未解析数量也不能当作准确率。

## 搜索与关系样例

30 条本项目内预设查询（15 条标识符缩写 + 15 条中文职责）中，29 条目标进入前五，比例 96.7%。未进入前五的是 `pRis → parseRis`（当前全项目原生查询中第六）；界面可补充路径/职责词缩小范围。

10,000 个合成符号：浏览器 30 次输入到下一帧的 p95 为 **43.6 ms**。另测原生 CLI 20 次启动、读图和搜索，p95 为 **108.0 ms**，包含进程和 JSON 读取开销。这两个指标的范围不同。样例由真实符号复制构建，本机无头 Chromium 测量；不代表所有项目或设备。

九个人工设定关系回归样例包含 TS 本地与显式导入、Svelte、Python、Go 同包与结构字段，以及遮蔽/动态接收者反例。本样例 TP=6、FP=0、FN=0（本样例 precision/recall=1），样本很小，不能外推整个真实仓库的解析准确率。

## 记忆与说明边界

删除/基线函数的审查记录保留 before:: 身份、旧签名和基线调用证据，有专门回归用例。

原生稳定层使用 cl100k tokenizer 严格限制 1,500 tokens；测试连续 12 次稳定层保存、超限拒绝、确认要求、快照失效、记录查询与修剪。离线 HTML 保守限制为 1,500 UTF-8 字节。原生记忆在用户核对后通过确认框写入；测试使用独立临时存储，没有代替用户确认真实架构共识。

Skill 与 BYOK 都使用 snapshot/id/content_hash/title/summary 契约，并由 Rust 校验器拒绝未知字段、过期哈希、未知/重复符号和拓扑字段。BYOK 缓存包含模型/地址/提示词版本/源码哈希；配置变更使旧模型说明失效。说明不修改调用边。

## 待外部验收

1. 没有调用用户实际付费 API。本轮网络协议、模型列表、认证失败、错误摘要、重复/未知函数、快照/哈希和额外字段由本机 HTTP mock 验证。真实地址、Key、Model 的联调需由对应服务配置支持。
2. 实际 VS Code JS/TS Call Hierarchy 已通过。Go 和 Python 语言服务补强使用相同 provider 接口，但本机没有单独验收 gopls/Python 调用层级。Go 示例来自已测试的 Rust 语法和类型字段索引；接口分派、跨包类型关系和动态路由仍可能缺边。
3. 动画用 requestAnimationFrame 与非线性 ease-out 实现，验证了中断、稳定坐标与减少动态效果。没有给出跨设备 60 fps 保证。
4. 真人任务对照、约 30% 时间改善、七日留存、BYOK 完成率与付费意愿尚未测量。使用 USER_TEST_PROTOCOL.md 收集真实参与者数据。
5. Marketplace 保持未发布。没有使用发布凭据，没有创建远程仓库或执行发布脚本。

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
```

实际项目测试目录、VS Code、Playwright 与 Chromium 路径可按 README/脚本环境变量调整。浏览器回归的 review.html 由 verify-native.mjs 提前生成。快照图和 HTML 含源码片段，仅供用户请求的本地评审。

VSIX SHA-256：`c9873450d30fab907d0deff92e8d7aa40e6f70bf0412417b236f6758f58826f7`。
