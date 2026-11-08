# Mas lang

Mas lang 扩展为 VS Code 提供 Mas lang 语言支持。

当前扩展通过内置的 `server/mas-lang-lsp.exe` 启动 mas-lang LSP server，不依赖相邻源码仓库路径。安装带有 `server` 目录的扩展后，`.ms` 文件可以直接获得编辑器语言能力。

## 支持内容

- `.ms` 文件识别
- TextMate 语法高亮
- 基础语言配置
  - `//` 行注释
  - `()`、`[]`、`{}` 自动闭合
  - `func`、`if`、`elif`、`else`、`while`、`for` 冒号块回车缩进
- LSP 语言能力
  - 编译器同源诊断
  - 关键字、类型、内建函数和当前文件符号补全，`func`、`main`、`if` 等代码骨架由 LSP snippet 提供
  - 符号悬停信息
  - 文档符号列表，语法错误时会使用降级符号扫描保持 Outline 可用

## 补全与缩进

插件不注册额外的本地补全 provider，补全候选统一来自内置 LSP server，避免 VS Code 侧和 LSP 侧维护两套规则。输入 `func` 后接受补全会插入函数骨架，输入 `main` 会插入 `func main() :` 代码块；函数名和 `print` 等内建函数会补齐调用括号。

语言配置会在冒号结尾的块头后自动缩进，例如：

```text
func main() :
    print("hello")
```

## 内置 LSP server

扩展启动时会从自身安装目录读取：

```text
server/mas-lang-lsp.exe
```

如果该文件缺失，扩展会提示当前版本缺少 mas-lang LSP server，但 `.ms` 文件识别和语法高亮仍可继续使用。

## 本地构建与调试（F5）

用 VS Code 直接打开 `editors\vscode` 目录（不要打开仓库根目录），首次执行一次 `pnpm install`，然后按 F5 并选择 `Run Extension`。调试配置会自动执行 `.vscode/tasks.json` 里的 `build` 任务：

1. `build-lsp`：在仓库根目录执行 `cargo build -p mas-lang-lsp --locked`
2. `compile`：执行 `pnpm run compile`（类型检查 + ESLint + esbuild）
3. `copy-lsp-server`：把 `target\debug\mas-lang-lsp.exe` 复制到 `server\`

也可以从 `Terminal > Run Task` 单独运行这些任务。等价的命令行步骤：

```powershell
cargo build -p mas-lang-lsp --locked
cd editors\vscode
pnpm install
pnpm run compile
pnpm run copy-lsp-server
```

`watch` 任务仍然保留，用于持续监听 TypeScript/esbuild；它是不会结束的后台任务，适合长时间开发，不要作为 F5 的预启动任务。

## 更新内置 server

LSP 通过静态链接复用编译器前端。编译器语法或语义变更后，必须重新构建 LSP 并复制内置 server；只构建编译器不会更新编辑器诊断。已安装扩展使用安装目录的独立副本，更新源码目录后还需要重新打包、安装扩展并执行 `Developer: Reload Window`。

在 workspace 根目录构建 `mas-lang-lsp`：

```powershell
cargo build -p mas-lang-lsp --locked
```

复制到 VS Code 扩展目录：

```powershell
cd editors\vscode
pnpm run copy-lsp-server
```

如果 LSP 构建产物不在默认位置，可以指定：

```powershell
$env:MAS_LANG_LSP_EXE="D:\tools\mas-lang-lsp.exe"
pnpm run copy-lsp-server
```

## 验证步骤

```powershell
pnpm install
pnpm run check-types
pnpm run lint
pnpm run compile
```

然后在 VS Code 中启动 Extension Host，打开 `.ms` 文件，检查诊断、snippet 补全、冒号块回车缩进、悬停和文档符号是否可见；可临时输入不完整函数声明，确认右下角不再持续弹出 `textDocument/documentSymbol` 请求失败。
