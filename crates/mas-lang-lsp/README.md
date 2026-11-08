# mas-lang-lsp

通过 stdio 提供主力 LLVM 编译器的诊断、补全、悬停和文档符号。

## 结构体与可空类型

- 识别缩进 `struct`、用户定义类型、`T?` 和 `null`，诊断复用编译器。
- 结构体和字段出现在文档符号中；签名保留问号及原始类型名。
- 提供结构体缩进片段、null 及基础可空类型补全。
- 当前文件中 `person.` 给出字段，`person: Person?` 的 `person.` 给出 `.value`，`person.value.` 给出 Person 字段。
- 支持显式变量类型、构造推断、函数参数、函数返回值及成员链；尚未写完点号时保留补全。
- 字段悬停显示字段类型；结构体名和字段引用支持当前文件定义跳转。不同结构体的同名字段按接收者类型区分。内建 `.value` 不伪造源码定义。

成员索引限定当前内存文档；跨文件结构体成员索引、工作区引用/重命名不在当前能力内。复杂的未完成语法无法解析时，继续使用现有词法符号降级路径。编译器诊断仍分析未经修补的原始源码。

## 构建与验证

```powershell
cargo test -p mas-lang-lsp --locked --offline
cargo build -p mas-lang-lsp --release --locked --offline
```

Windows 独立产物为 `target/release/mas-lang-lsp.exe`。如需更新扩展内置服务端，参见 [VS Code 扩展](../../editors/vscode/README.md)。

`tests/records.rs` 覆盖诊断、同名字段/参数定位及未完成成员链；`tests/phase4_protocol.rs` 通过真实 stdio 验证新诊断、补全、悬停和定义请求。
