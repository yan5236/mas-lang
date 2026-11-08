# time 标准库原生产物说明

通过 `src/index.ms` 导入公开函数：

- `unix_seconds()`、`unix_millis()` 返回当前 Unix 时间戳。
- `now_utc()`、`now_email_date()`、`now_http_date()` 返回 UTC 日期时间、邮件日期和 HTTP 日期字符串。
- `monotonic_millis()` 用于计算耗时，`sleep(ms)` 按毫秒暂停执行。

## 支持矩阵

当前 `time` 包默认提供以下预编译原生产物：

| 目标三元组 | 产物目录 | 状态 |
| --- | --- | --- |
| `x86_64-pc-windows-msvc` | `native/artifacts/x86_64-pc-windows-msvc/` | 已支持 |

普通调用方构建 `import time { ... }` 时，编译器会优先读取 `settings.toml` 中声明的 `native.prebuilt.<target>` 条目，并直接链接该目录下的对象文件。默认消费路径不会自动回退到源码编译，因此调用方机器不需要安装 `gcc`、`clang` 或 `cl.exe`。

## 目录协议

预编译产物统一放在包内稳定路径：

```text
native/
└── artifacts/
    └── x86_64-pc-windows-msvc/
        ├── manifest.toml
        ├── time_common.obj
        └── time_windows.obj
```

其中 `manifest.toml` 至少记录以下信息：

- 包名与包版本
- 目标三元组
- ABI 版本
- 源码与头文件指纹
- 对象文件清单

`fama install` 会保留 `native/artifacts/` 目录，因此本地路径依赖安装到 `mod_module/time@0.1.0/` 后仍然可以直接被消费。

在 Linux 和 macOS 上，使用本机 C 编译器和 `fama run --allow-native-source-build examples/projects/time-api` 运行示例。该选项显式允许从包内 POSIX 源码构建原生对象。

## 角色分流

普通调用方：

- 直接执行 `fama build` 或 `fama run`
- 命中支持矩阵时会链接包内预编译对象
- 缺少产物、ABI 不匹配或目标不受支持时会收到中文诊断

维护者：

- 在 `examples/projects/time-api` 中执行 `fama build --allow-native-source-build` 刷新已安装的 time 包
- 编译器会在校验失败或产物缺失时，使用 `native.sources` 的 C 源文件重新生成对象文件和 `manifest.toml`
- 刷新后的产物会写到依赖缓存的 `native/artifacts/<target>/`，发布前需复制回标准库包

## 发布检查

当以下任一内容变更时，必须先刷新预编译产物，再发布新版本：

- `native/common/*.c`
- `native/platform/**/*.c`
- `native/include/**/*.h`
- `settings.toml` 中的 `native.abi`

发布前至少检查：

1. `native/artifacts/<target>/manifest.toml` 中的 `package_version`、`abi`、`source_hash`、`header_hash` 已更新。
2. `settings.toml` 中的 `native.prebuilt.<target>` 路径与目录内对象文件一致。
3. 普通 `fama build` 在不加 `--allow-native-source-build` 的情况下可以直接消费 `time`。

## 版本控制边界

`build/`、`src/build/` 和 `mod_module/` 是本地构建输出或依赖缓存，由 `.gitignore` 排除。`native/artifacts/` 是调用方默认链接所需的发布内容，继续跟踪其中的清单和 OBJ 文件；不要全局忽略所有 `*.obj`。
