# Mas lang

Mas lang 的 LLVM 编译器、Fama 包管理器和语言服务器共用一个 Cargo workspace。

| 目录 | 职责 |
| --- | --- |
| `crates/maslang` | LLVM 19.1 编译器、编译 API 和运行时 |
| `crates/fama` | 项目脚手架、本地依赖安装、构建和运行 |
| `crates/mas-lang-lsp` | 复用编译器前端的 LSP 服务端 |
| `stdlib/time` | Mas lang 标准库包及原生分发文件 |
| `editors/vscode` | VS Code 扩展，使用 pnpm |
| `website` | Docusaurus 文档站，使用 pnpm |
| `specs`、`examples` | 语言规范、回归样例和示例工程 |

Fama 默认调用 `maslang` 的库 API，LSP 调用同一包的前端。编译器可通过 `maslang` 命令单独运行。标准库由 Fama 按 Mas lang 包安装。

## 构建和运行

安装支持 Rust 2024 的 Rust 工具链和 LLVM 19.1。Windows 当前使用 MSVC 工具链，必要时设置 `LLVM_SYS_191_PREFIX` 指向包含 `bin/llvm-config.exe` 的 LLVM 安装目录。

从 workspace 根目录执行以下命令。

```powershell
cargo build --workspace --locked
cargo test --workspace --locked
cargo run -p maslang --locked -- run examples/regression/compiler-stress/branch-return-matrix.ms
cargo run -p fama --locked -- install examples/projects/time-api
cargo run -p fama --locked -- run examples/projects/time-api
```

若 Fama 的全局设置指定了旧目录的 `[compiler].binary`，运行示例前可用 `$env:FAMA_HOME = Join-Path (Get-Location) 'target/fama-home'` 创建隔离配置。日常使用可将原配置中的路径指向新构建的 `target/debug/maslang.exe`。

VS Code 服务端的生成和复制见 [扩展说明](editors/vscode/README.md)。文档站见 [website/README.md](website/README.md)，本地预览在 `website` 内执行 `pnpm start`。

## 存储

- 根目录只有一份 `Cargo.lock`，所有 Rust 包共用 `target/`。如需把构建产物放到其他磁盘，可设置 `CARGO_TARGET_DIR`。使用自定义目录时，通过 `MAS_LANG_LSP_EXE` 指定扩展要复制的服务端文件。
- LLVM 和 Cargo 下载缓存沿用本机安装。Fama 的全局配置沿用 `~/.fama/setting.toml`，也可用 `FAMA_HOME` 指定目录。
- Mas lang 项目的 `build/` 和 `mod_module/` 是本地构建输出及安装缓存。`stdlib/time/native/artifacts/` 是带目标平台和 ABI 清单的分发输入，应随源码保存。

## 迁移来源

本目录基于旧项目各组件的工作树，包含当时的未提交源码。六个迁入组件的原始 Git 提交保留在本仓库的 `archive/*` 分支，并汇入 `main` 的迁移提交。旧目录仍保存原仓库、历史编译器、LuaJIT 对照环境、基准脚本与结果、备份和临时审查文件。构建产物、下载缓存和已安装的依赖可在新目录按需生成。

## 项目配置与入口

项目名、版本、构建选项和依赖统一写在项目根目录的 `settings.toml`。从旧工程迁移时，把 `mod.yml` 中的本地依赖搬到 TOML 的 `[dependencies]`，随后删除旧清单。已有的 `[build]`、`[native]` 以及 `mod-lock.yml` 可继续使用。

```toml
[project]
name = "hello"
version = "0.1.0"

[build]
entry = "src/main.ms"
output = "build"

[dependencies]
time = { path = "../mas-lang/stdlib/time", version = "0.1.0" }
```

可执行程序从无参数的 `func main()` 进入，`print` 放在函数体内。库包可通过 `src/index.ms` 导出符号。
