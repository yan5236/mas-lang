# Mas lang

Mas lang 是一门静态类型编程语言。编译器用 LLVM 把 `.ms` 源码编译成本机可执行程序。仓库也包含 Fama 项目与依赖管理工具，以及供 VS Code 使用的语言服务器。

## 快速开始

需要支持 Rust 2024 的稳定版 Rust、LLVM 19.1 和本机 C 工具链。将 `LLVM_SYS_191_PREFIX` 指向 LLVM 安装目录。Linux 和 macOS 的工具链安装命令见 [CI 配置](.github/workflows/native.yml)。Windows 使用 MSVC。

```sh
cargo build --workspace --locked
cargo run --locked -p maslang -- run examples/regression/syntax/standalone-hello/main.ms
```

把 `run` 换成 `build` 可保留可执行文件。默认产物位于源码旁的 `build/` 目录，在 Linux 上是 ELF，在 macOS 上是 Mach-O，在 Windows 上带 `.exe` 后缀。

更多内容见 [语言文档](website/docs/introduction.mdx)、[VS Code 扩展](editors/vscode/README.md)、[time 标准库示例](stdlib/time/README.md)与 [io 标准库示例](stdlib/io/README.md)。
