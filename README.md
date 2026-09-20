# mascript-lang

`mascript-lang` 为 VS Code 提供 Mascript / mas-lang 语言支持。

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

## 更新内置 server

LSP 通过静态链接复用编译器前端。编译器语法或语义变更后，必须重新构建 LSP 并复制内置 server；只构建编译器不会更新编辑器诊断。已安装扩展使用安装目录的独立副本，更新源码目录后还需要重新打包、安装扩展并执行 `Developer: Reload Window`。

在本地构建 `mas-lang-lsp`：

```powershell
cd ..\mas-lang-lsp
cargo build
```

复制到 VS Code 扩展目录：

```powershell
cd ..\mas-lang-vscode
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
