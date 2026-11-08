# Mas lang 工作区约定

- LLVM 编译器和运行时位于 `crates/maslang`。构建、运行、模块导入和原生链接有变更时，同时检查 `crates/fama` 的工程入口。
- Fama 的依赖安装与标准库包结构联动。修改标准库时，检查 `stdlib/` 中的包清单及原生链接输入。
- 语言服务位于 `crates/mas-lang-lsp`，通过库接口使用编译器前端。扩展从 workspace 的构建产物复制服务端。
- `specs/` 保存语言规范。`examples/` 保存验证样例，产品修复应落到相应组件。
- Rust 验证从根目录执行 `cargo test --workspace --locked`，可用 `-p` 限定包。保留根锁文件和标准库声明的原生分发文件，生成的构建目录和安装缓存由本地工具管理。
