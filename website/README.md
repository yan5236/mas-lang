# 文档站

本目录是 Mas lang 的 [Docusaurus](https://docusaurus.io/) 文档站。

## 本地开发

依赖使用 pnpm 安装：

```powershell
pnpm install
pnpm start
```

启动后打开 <http://localhost:3000/mas-lang/>，修改文档会热更新。

## 构建

```powershell
pnpm build
pnpm serve
```

静态产物生成在 `build/`。

## 注意事项

- `start` 与 `build` 不要同时运行，两者共用 `.docusaurus/` 缓存目录，并发会互相覆盖路由导致页面 404。
- 配置里显式关闭了 Docusaurus Faster（`future.faster: false`）。Faster 依赖的 `@swc/core`、`@swc/html` 原生绑定会校验 Windows 缓存目录及其祖先权限；在带 DSH 沙箱授权项的机器上（例如 `C:\` 根目录含 `CodexSandboxUsers` 条目）校验必然失败，启动会报 `Failed to load native binding`。关闭后回退到 webpack，文档功能不受影响，只是构建稍慢。
- 如需重新启用 Faster，先确认本机能正常执行 `node -e "require('@swc/core')"`，再移除或调整 `docusaurus.config.ts` 中的 `faster: false`。

## 部署

使用 SSH：

```powershell
$env:USE_SSH = "true"; pnpm deploy
```

不使用 SSH：

```powershell
$env:GIT_USER = "<你的 GitHub 用户名>"; pnpm deploy
```

站点按 GitHub Pages 的 `gh-pages` 分支发布。
