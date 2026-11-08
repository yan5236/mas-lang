# 实施任务清单：mas-lang LSP 语言服务器

## 阶段 1：协议入口与项目结构

- [x] 1.1 为 `mascript-lsp` 添加 `tower-lsp`、`tokio` 与 `maslang` path dependency，并确认 `cargo check` 能解析依赖 [验证 Req 1, Req 3]
- [x] 1.2 将 `main.rs` 收敛为启动入口，新增 `server.rs`、`documents.rs`、`analysis.rs`、`diagnostics.rs`、`symbols.rs`、`completion.rs`、`hover.rs`、`lsp_convert.rs` 模块骨架 [依赖: 1.1] [验证 Req 1]
- [x] 1.3 实现 stdio LSP server 启动流程，确保 stdout 仅输出 JSON-RPC 数据 [依赖: 1.2] [验证 Req 1]
- [x] 1.4 实现 `initialize`、`initialized` 与 `shutdown` handler，并只声明首批已规划能力 [依赖: 1.3] [验证 Req 1, Req 6]

阶段 1 实现备注：已在 `mas-lang-lsp` 中接入 `tower-lsp` stdio server，补齐一阶段模块边界、能力声明、基础补全通道和协议 smoke 测试；`examples/projects/language-server-demo/src/main.ms` 已改为手工验证用 mas-lang 样例。

## 阶段 2：文档缓存与位置转换

- [x] 2.1 实现 `OpenDocument` 与文档仓库，支持按 URI 写入、读取和删除打开文档 [依赖: 1.2] [验证 Req 2]
- [x] 2.2 实现 `didOpen`、完整文本 `didChange` 与 `didClose`，关闭文档时发布空诊断 [依赖: 2.1] [验证 Req 2]
- [x] 2.3 实现 mas-lang `Span` 与 LSP `Range` 的 UTF-16 位置转换，并覆盖 ASCII、中文和多行源码样例 [依赖: 2.1] [验证 Req 3, Req 6]
- [x] 2.4 增加非 `.ms` 文档过滤逻辑，避免对无关文件执行语言分析 [依赖: 2.2] [验证 Req 2]

阶段 2 实现备注：已在 `mas-lang-lsp` 中实现文档仓库事件接入、完整文本同步、关闭清理诊断、`.ms` 文档过滤和 `Span` 到 LSP UTF-16 `Range` 转换；`examples/projects/language-server-demo/src/main.ms` 已扩展为手工验证样例，并由真实 stdio 协议测试覆盖 `didOpen`、`didChange` 与 `didClose`。

## 阶段 3：编译器前端复用与诊断

- [x] 3.1 实现 `AnalysisService`，把打开文档转换为 `SourceFile` 并调用 `maslang::analyze_source` [依赖: 2.1, 2.3] [验证 Req 3]
- [x] 3.2 实现 `CompilerError` 到 LSP `Diagnostic` 的转换，保留阶段、中文消息、严重级别和源码范围 [依赖: 3.1] [验证 Req 3, Req 6]
- [x] 3.3 在 `didOpen` 与 `didChange` 后触发分析并发布诊断，源码无错误时发布空诊断 [依赖: 3.2] [验证 Req 3]
- [x] 3.4 处理模块装载、文件系统错误和编译器错误，确保错误只转为诊断或日志，不终止 LSP 进程 [依赖: 3.3] [验证 Req 3]
- [x] 3.5 为诊断转换和发布流程添加单元测试，覆盖词法、语法、语义与模块错误样例 [依赖: 3.4] [验证 Req 6]

阶段 3 实现备注：已在 `mas-lang-lsp` 中接入 `AnalysisService` 调用 `maslang::analyze_source`，把 `CompilerError` 转换为带中文阶段、中文消息、严重级别和 UTF-16 range 的 LSP `Diagnostic`；`didOpen` 与完整文本 `didChange` 后会发布最新诊断，源码恢复正常时发布空诊断。为保证编辑器未保存内容参与分析，`maslang` 的模块装载入口已允许入口模块使用传入的内存 `SourceFile`，依赖模块仍按磁盘装载。`examples/projects/language-server-demo/src/main.ms` 已更新为第三阶段手工验证样例，并由 stdio 协议测试覆盖打开、报错、修改恢复的诊断通知链路。

## 阶段 4：符号模型、补全与悬停

- [x] 4.1 审查 `maslang` AST 的源码跨度覆盖，列出补全文档符号所需的最小共享接口变更 [依赖: 3.1] [验证 Req 4, Req 5]
- [x] 4.2 实现 `MasSymbol` 模型，从成功分析结果中提取函数、变量、常量、导入和导出符号 [依赖: 4.1] [验证 Req 5]
- [x] 4.3 实现语法错误时的降级符号扫描，至少识别当前文件中已经完整书写的函数和声明名 [依赖: 4.2] [验证 Req 4, Req 5]
- [x] 4.4 实现 `documentSymbol` handler，返回 range 与 selectionRange 准确的文档符号 [依赖: 4.2, 4.3] [验证 Req 5]
- [x] 4.5 实现 `completion` handler，组合关键字、基础类型、内建函数和当前文件可见符号 [依赖: 4.2, 4.3] [验证 Req 4]
- [x] 4.6 实现字符串字面量和注释内的补全抑制逻辑 [依赖: 4.5] [验证 Req 4]
- [x] 4.7 实现 `hover` handler，展示符号种类、类型或函数签名 [依赖: 4.2] [验证 Req 4]
- [x] 4.8 为文档符号、补全和悬停添加单元测试，覆盖成功分析与语法错误降级场景 [依赖: 4.4, 4.5, 4.7] [验证 Req 4, Req 5, Req 6]

阶段 4 实现备注：已在 `mas-lang-lsp` 中实现 `SymbolSnapshot` 与 `MasSymbol` 共享模型，成功分析路径从 `maslang` AST 提取函数、导入、变量和常量，失败路径使用编译器 Lexer 降级扫描已完整书写的导入、函数、变量和常量声明；降级符号的 `range` 会覆盖完整声明片段，确保 `selectionRange` 始终落在 `range` 内，避免 VS Code 在错误输入时拒绝 `documentSymbol` 响应。`documentSymbol`、`completion` 与 `hover` handler 已接入打开文档的最新分析快照。补全现在组合关键字、基础类型、内建函数和当前文件符号，并在字符串字面量或行注释中抑制语法关键字；`func`、`main`、`if`、`elif`、`else`、`while`、`for`、内建函数和当前文件函数符号使用 LSP snippet 补全代码骨架或调用括号；悬停展示中文符号种类和函数签名或类型详情。`examples/projects/language-server-demo/src/main.ms` 已扩展为包含 `time` 导入、常量、函数、局部变量和函数调用的 LSP 验证样例；新增 `phase4_language_features` 与 stdio 协议测试覆盖四阶段能力。

## 阶段 5：协议级验证与编辑器接入

- [x] 5.1 增加协议级集成测试或快照测试，模拟 `initialize`、`didOpen`、`didChange`、`completion`、`hover`、`documentSymbol` 与 `shutdown` [依赖: 3.5, 4.8] [验证 Req 1, Req 2, Req 6]
- [x] 5.2 在真实 `.ms` 样例上运行 `cargo test` 与 `cargo check`，记录失败诊断和修复结果 [依赖: 5.1] [验证 Req 6]
- [x] 5.3 编写手工 VS Code 或通用 LSP 客户端验证步骤，确认诊断、补全、悬停和文档符号可见 [依赖: 5.2] [验证 Req 6]
- [x] 5.4 更新本 spec 的任务状态与实现备注，确保每项已交付能力能追溯到需求编号 [依赖: 5.3] [验证 Req 6]

阶段 5 实现备注：已在真实工作区运行 `cargo check` 与 `cargo test` 验证 `mas-lang-lsp`，协议级测试覆盖初始化、打开文档、诊断发布、补全、悬停、文档符号和关闭流程。`mas-lang-vscode` 已改为通过 `vscode-languageclient` 启动扩展内置的 `server/mas-lang-lsp.exe`，不再依赖相邻源码仓库路径；插件缺少内置 server 时会提示重新构建或安装带 server 的版本，同时保留 `.ms` 语言识别与语法高亮。VS Code 侧不注册额外本地补全 provider，snippet 补全统一由 LSP 返回；语言配置已补充冒号块 `onEnterRules`，覆盖 `func`、`if`、`elif`、`else`、`while` 和 `for` 后的回车缩进。手工验证步骤记录在 `editors/vscode/README.md`，通过 Extension Host 打开 `.ms` 文件后检查诊断、snippet 补全、回车缩进、悬停和文档符号可见性。

## 阶段 6：后续能力预留

- [ ] 6.1 基于 `MasSymbol` 设计跨文件工作区索引，为定义跳转和引用查找建立后续 spec 输入 [依赖: 4.2] [验证 Req 5]
- [ ] 6.2 评估标准库与 `fama` 依赖缓存的导出符号读取方式，为包级补全建立后续 spec 输入 [依赖: 4.5] [验证 Req 4]
- [ ] 6.3 评估格式化、重命名和代码操作所需的 AST span 与语义引用能力，拆分为独立后续规范 [依赖: 5.4] [验证 Req 6]
