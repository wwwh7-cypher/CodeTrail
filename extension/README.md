# CodeTrail — 看懂 Agent 改动，从函数开始

CodeTrail 在 VS Code 内展示可交互的函数调用网络，帮助开发者定位代码、逐步展开上下文并复核 Git 修改。Rust 负责代码索引和图查询；模型只解释函数职责。当前版本 0.5.0，发布者 `wwwh7`。

## 安装

1. 在 VS Code 扩展视图搜索 **CodeTrail**（publisher `wwwh7`），或打开 [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=wwwh7.code-trail)。Marketplace 页面在发布前可能暂时不可用。
2. 也可从 [GitHub Releases](https://github.com/wwwh7-cypher/CodeTrail/releases) 下载 `code-trail-<version>-win32-x64.vsix`，在扩展视图右上角选择 **从 VSIX 安装**。
3. 预编译包目标为 **Windows x64**；其他平台需从源码构建（见仓库根目录 README）。安装后重新加载窗口。

## 使用

1. 打开本地项目，在命令面板运行 **CodeTrail: 打开架构导览**。
2. 输入函数缩写、文件名或已生成的中文职责。用上下方向键选择，Enter 定位。
3. 单击节点展开一级调用方和被调用方。旧节点保留位置，可固定、收起、返回与前进；箭头指向被调用方。
4. 切换 **功能模块** 查看目录/包边界与跨模块调用，点击模块进入函数。
5. 运行 **CodeTrail: 审查 Git 修改影响**，选择基线；在当前/基线版本之间查看新增、修改、删除节点与调用边，打开原生 diff 并标记复核状态。

支持 TS、TSX、JS、JSX、Svelte script、Python、Go、C 与 C++ 的语法索引。C/C++ 只索引函数定义（原型不是节点）：C 解析同文件调用、本地引号 `#include` 文件与头文件对应的同目录实现文件，静态候选边标记为 `c-include-static`；C++ 额外解析 `this->method()`、同类非限定兄弟方法与唯一的 `Class::method` 候选。对象/指针/函数指针上的成员调用需要类型信息，会以明确原因保持未解析（属于静态缺口，不是源码错误）；系统头文件按外部依赖处理，未集成 clang。VS Code 内按需使用可用语言扩展的 Call Hierarchy 补强关系；动态分派、外部依赖和框架隐式连接仍可能遗漏。未解析调用数量不代表错误率。

## API URL、Model 与 API Key

通过 **CodeTrail: API URL / Model 设置** 打开设置页：

- 服务协议：OpenAI 兼容或 Anthropic Messages。
- API Base URL：例如 `https://api.openai.com/v1` 或 `https://api.anthropic.com/v1`，也可填写兼容服务地址。
- Model ID：手动填写服务支持的模型；可获取模型列表。
- 模型列表 URL：可选，默认使用 Base URL 下的 `/models`。
- API Key：由 VS Code SecretStorage 按协议与服务来源保存，不进入 HTML 或图 JSON。

搜索、看图、Git 审查无需 Key。点击“解释此函数”后，扩展会说明将发送的函数片段与服务地址，经确认后请求模型。当前不内置模型，不下载模型。

同一项目也可以作为 **Agent Skill** 使用：由当前对话模型解释源码，无需额外配置 API Key。两种模式共用 Rust 图与结构化说明格式。

## 有界架构记忆与 MCP

默认使用阅读视图展示 Markdown，需编辑时切换源码。VS Code 原生确认框通过后才将稳定层或审查记录写入本机存储。稳定层限制为 1,500 个 `cl100k_base` tokens；代码变化后旧结论显示过时，默认查询排除过时记录。

**CodeTrail: 导出只读 MCP 配置** 提供本地 stdio 连接。MCP 可查询函数、路径、潜在上游和已确认记忆，不暴露写入或模型调用工具。

## 数据与边界

- 默认分析和缓存均保留在本机。开启 BYOK 解释时，只发送用户选择的函数片段。
- 支持增量解析缓存；重新扫描仍重新解析相关符号关系。未保存编辑会将当前图标为过期。
- Git 图说明潜在静态可达性，不证明运行时行为变化、审查充分性或合并安全。
- 模块视图按实际路径分组，可配置名称与范围；跨服务 HTTP 关联不伪装为直接函数调用。
- 本机指标只记录使用与错误计数，没有远程遥测。
- C/C++ 是静态语法索引：只索引函数定义，接口分派、成员类型推断与函数指针调用可能缺边；未解析数量不代表错误率。
- 此扩展使用本机 Rust 可执行文件，需要受信任的本地工作区。预编译包为 Windows x64。

## 支持与反馈

- Marketplace：<https://marketplace.visualstudio.com/items?itemName=wwwh7.code-trail>（publisher `wwwh7`；Marketplace 条目发布前请使用 Releases 的 VSIX）。
- GitHub：<https://github.com/wwwh7-cypher/CodeTrail>；问题反馈：<https://github.com/wwwh7-cypher/CodeTrail/issues>；预编译包：<https://github.com/wwwh7-cypher/CodeTrail/releases>。
- 版本变更见仓库内 `CHANGELOG.md`。本项目未发布到其他扩展市场。
