# mas-lang LSP Server

此目录保存 VS Code 扩展随包分发的 mas-lang LSP server。

## 更新方式

在本地构建 `mas-lang-lsp` 后，在 `mas-lang-vscode` 目录运行：

```powershell
pnpm run copy-lsp-server
```

默认会从相邻开发仓库的 `../mas-lang-lsp/target/debug/mas-lang-lsp.exe` 复制。
如果 LSP 构建产物位于其他位置，可以设置 `MAS_LANG_LSP_EXE`：

```powershell
$env:MAS_LANG_LSP_EXE="D:\tools\mas-lang-lsp.exe"
pnpm run copy-lsp-server
```
