# 功能需求：mas-lang LSP 语言服务器

## 需求 1：建立可运行的 LSP 服务入口

**用户故事**：作为 mas-lang 编辑器使用者，我想要编辑器能够启动 mas-lang 语言服务器，以便在 `.ms` 文件中获得诊断、补全和符号能力。

#### 验收标准 (EARS格式)

1. WHEN 编辑器通过标准输入输出启动 `mascript-lsp` THEN 系统 SHALL 按 Language Server Protocol 完成 `initialize`、`initialized` 与 `shutdown` 生命周期。
2. WHEN 客户端发送初始化请求 THEN 系统 SHALL 返回与已实现能力一致的服务端能力声明，至少包含文本同步、诊断、补全、悬停和文档符号能力。
3. IF 客户端未提供工作区根目录 THEN 系统 SHALL 仍能以单文件模式处理已打开的 `.ms` 文档。
4. WHERE Windows 终端或 VS Code 扩展启动服务 WHEN LSP 进入运行状态 THEN 系统 SHALL 不向 stdout 输出非 JSON-RPC 日志。

## 需求 2：维护打开文档与增量分析状态

**用户故事**：作为正在编辑 mas-lang 源码的开发者，我想要语言服务器跟随打开、修改和关闭事件维护文档状态，以便诊断和智能提示总是基于当前编辑器内容。

#### 验收标准 (EARS格式)

1. WHEN 客户端发送 `textDocument/didOpen` THEN 系统 SHALL 缓存文档 URI、版本号、路径和完整文本。
2. WHEN 客户端发送 `textDocument/didChange` AND 变更事件包含完整文本 THEN 系统 SHALL 用新文本替换缓存并触发重新分析。
3. WHEN 客户端发送 `textDocument/didClose` THEN 系统 SHALL 删除对应缓存并清空该文档已发布诊断。
4. IF 客户端发送的文档语言或扩展名不属于 mas-lang THEN 系统 SHALL 忽略该文档的语言分析请求。

## 需求 3：复用主力编译器前端发布诊断

**用户故事**：作为 mas-lang 开发者，我想要编辑器诊断与 `maslang` 编译结果一致，以便编辑期和构建期不会出现两套互相冲突的语言规则。

#### 验收标准 (EARS格式)

1. WHEN 已打开文档完成词法、语法或语义分析 THEN 系统 SHALL 复用 `maslang` 的前端能力生成诊断，而不是在 LSP 中重新实现语言规则。
2. WHEN 编译器返回 `CompilerError` THEN 系统 SHALL 把错误阶段、中文消息和源码跨度转换为 LSP `Diagnostic`。
3. WHEN 源码无错误 THEN 系统 SHALL 对该 URI 发布空诊断列表。
4. IF 模块导入需要读取工作区文件 THEN 系统 SHALL 使用当前工作区根目录和文档路径解析本地模块。
5. IF 分析过程中遇到文件系统或模块装载错误 THEN 系统 SHALL 发布明确诊断，而不是导致语言服务器进程退出。

## 需求 4：提供基础补全与悬停信息

**用户故事**：作为 mas-lang 开发者，我想要编辑器提供关键字、内建函数、当前文件符号和类型提示，以便减少记忆语法和手写标识符的成本。

#### 验收标准 (EARS格式)

1. WHEN 客户端请求 `textDocument/completion` THEN 系统 SHALL 返回 mas-lang 关键字、基础类型、内建函数和当前文件可见符号的补全项。
2. WHEN 补全位置处于字符串字面量或注释范围内 THEN 系统 SHALL 不返回语法关键字补全项。
3. WHEN 客户端请求 `textDocument/hover` AND 光标位于可解析符号上 THEN 系统 SHALL 返回该符号的声明种类、类型或函数签名。
4. IF 当前文件存在语法错误但词法结果可用 THEN 系统 SHALL 继续提供基于词法和局部符号表的降级补全。

## 需求 5：提供文档符号与后续导航基础

**用户故事**：作为维护中大型 mas-lang 文件的开发者，我想要编辑器展示当前文件结构，以便快速定位函数、变量和导入声明。

#### 验收标准 (EARS格式)

1. WHEN 客户端请求 `textDocument/documentSymbol` THEN 系统 SHALL 返回当前文件中的函数、变量、常量、导入和导出符号。
2. WHEN 符号拥有源码跨度 THEN 系统 SHALL 返回准确的 LSP range 与 selectionRange。
3. IF 当前文件存在语法错误 THEN 系统 SHALL 返回已成功解析或词法扫描可识别的符号子集。
4. WHERE 后续实现定义跳转、引用查找或重命名 WHEN 需要符号索引 THEN 系统 SHALL 复用本功能建立的文档符号和工作区索引模型。

## 需求 6：建立测试与编辑器集成验证边界

**用户故事**：作为 mas-lang LSP 维护者，我想要有可重复的协议级测试和手工编辑器验证步骤，以便后续扩展不会破坏基础语言服务。

#### 验收标准 (EARS格式)

1. WHEN 运行 `cargo test` 于 `mascript-lsp` THEN 系统 SHALL 覆盖文档缓存、诊断转换、补全、悬停和文档符号的核心场景。
2. WHEN 测试诊断转换 THEN 系统 SHALL 使用包含中文错误消息和多行源码跨度的样例。
3. WHEN 手工验证 VS Code 或其他 LSP 客户端集成 THEN 系统 SHALL 能打开 `.ms` 文件并看到诊断、补全、悬停和符号列表。
4. IF 某项 LSP 能力尚未实现 THEN 系统 SHALL 不在 `initialize` 能力声明中暴露该能力。

## 技术约束 (非功能需求)

1. 平台支持：首批支持 Windows 本地开发环境，并使用标准输入输出作为 LSP 传输通道。
2. 技术栈：语言服务器使用 Rust 2024；LSP 协议层优先采用 `tower-lsp`；语言分析优先依赖 `maslang`。
3. 代码边界：LSP 不复制 lexer、parser 或 semantic 的语言规则；确需新增共享能力时优先在 `maslang` 暴露可复用接口。
4. 编码规范：新增 Rust 函数必须有详细中文函数级注释，文件应保持模块化，单文件默认不超过 500 行。
5. 兼容性：LSP stdout 仅用于 JSON-RPC；调试日志必须通过 stderr、客户端日志接口或可配置文件输出。
6. 性能指标：单个 2000 行 `.ms` 文件的完整文档重分析在普通开发机上 SHALL 控制在 500ms 内；超过该指标时必须记录为后续增量分析优化任务。
