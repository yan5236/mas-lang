# time 标准库原生产物说明

当前包版本 `0.2.0`，原生 ABI 版本 `2`。通过 `src/index.ms` 导入公开 API。

## 公开 API

严格接口保持 0.1.0 的名称与返回类型，内部改为调用新的 `mas_time_read_*` 入口，失败时仍输出中文 `运行时错误: ...` 并以状态码 `1` 退出。诊断文本按错误码归并（见下方错误码表），不再与 0.1.0 的按格式/按计时器细分文案逐字一致：

- `unix_seconds()`、`unix_millis()` 返回当前 Unix 时间戳。
- `now_utc()`、`now_email_date()`、`now_http_date()` 返回 UTC 日期时间、邮件日期和 HTTP 日期字符串。
- `monotonic_millis()` 用于计算耗时，`sleep(ms)` 按毫秒暂停执行。
- 以上接口在宿主失败时仍调用 `mas_time_fail(code)`：输出中文 `运行时错误: ...` 并以状态码 `1` 退出。

可恢复接口把失败写进结果结构，调用方可以继续执行：

| 接口 | 返回 | 失败含义 |
| --- | --- | --- |
| `try_unix_seconds()` | `TimeIntResult` | 宿主时钟读取失败（`code = 1`）或早于 Unix 纪元（`code = 2`） |
| `try_unix_millis()` | `TimeIntResult` | 同上 |
| `try_monotonic_millis()` | `TimeIntResult` | 单调时钟读取失败（`code = 7`） |
| `try_now_utc()` | `TimeStringResult` | 时钟读取、UTC 拆解或格式化失败 |
| `try_now_email_date()` | `TimeStringResult` | 同上 |
| `try_now_http_date()` | `TimeStringResult` | 同上 |
| `try_sleep(ms)` | `TimeStatus` | 负等待（`code = 5`）或阻塞等待失败（`code = 6`） |

三个结果结构从 `index.ms` 一起导出，字段名与仓库统一的 `try_result` 约定一致：

```mas-lang
import time { try_sleep, TimeStatus }

func main():
    var status = try_sleep(-1)
    if not status.ok:
        print("等待失败，错误码 " + string(status.code))
```

- `TimeStatus`：`ok: bool`、`code: int`
- `TimeIntResult`：`ok: bool`、`code: int`、`value: int`
- `TimeStringResult`：`ok: bool`、`code: int`、`value: string`

失败时 `.value` 固定是无害占位值（`0` 或空串），调用方必须先判断 `.ok`。

## 错误码

| 错误码 | 含义 | 可能出现的入口 |
| --- | --- | --- |
| `0` | 成功 | 全部 |
| `1` | 宿主时钟读取失败 | `try_unix_seconds`、`try_unix_millis`、`try_now_*` |
| `2` | 宿主时钟早于 Unix 纪元（仅 Windows） | 同上 |
| `3` | UTC 时间拆解失败 | `try_now_*` |
| `4` | 时间格式化失败 | `try_now_*` |
| `5` | 负等待参数 | `try_sleep` |
| `6` | 阻塞等待失败 | `try_sleep` |
| `7` | 单调时钟读取失败 | `try_monotonic_millis` |

契约要点：

- Windows 上“宿主时钟早于 1970-01-01 UTC”必须返回 `ok = false, code = 2`，**不能**把负时间戳当作失败哨兵：负时间戳本身仍是合法取值。
- POSIX 保留 `nanosleep` 被 `EINTR` 中断后继续等待剩余时间的语义，`try_sleep` 只有在遇到其他 `errno` 时才报 `code = 6`。
- 严格接口的诊断文本按错误码归并：0.1.0 中三种日期格式的专用文案合并为“无法格式化时间字符串”，Windows 计时器失败的专用文案合并为“阻塞等待失败”。这是 0.2.0 记录在案的兼容性变化。
- 零等待（`sleep(0)` / `try_sleep(0)`）立即成功，不做任何阻塞。
- Windows 宿主层的 `GetTickCount64` 不会失败，因此 `code = 7` 目前只在 POSIX 上实际出现。

## 支持矩阵

`settings.toml` 声明五个预编译目标。**0.2.0 把原生 ABI 提升到 2，五个目标的产物必须随源码重建**：

| 目标三元组 | 产物目录 | 当前状态 |
| --- | --- | --- |
| `x86_64-pc-windows-msvc` | `native/artifacts/x86_64-pc-windows-msvc/` | 已按 0.2.0 / ABI 2 刷新（本机 MSVC 工具链生成） |
| `x86_64-unknown-linux-gnu` | `native/artifacts/x86_64-unknown-linux-gnu/` | 已按 0.2.0 / ABI 2 刷新（Actions `ubuntu-24.04` 生成） |
| `aarch64-unknown-linux-gnu` | `native/artifacts/aarch64-unknown-linux-gnu/` | 已按 0.2.0 / ABI 2 刷新（Actions `ubuntu-24.04-arm` 生成） |
| `x86_64-apple-darwin` | `native/artifacts/x86_64-apple-darwin/` | 已按 0.2.0 / ABI 2 刷新（Actions `macos-15-intel` 生成） |
| `aarch64-apple-darwin` | `native/artifacts/aarch64-apple-darwin/` | 已按 0.2.0 / ABI 2 刷新（Actions `macos-15` 生成） |

五个目标的产物均已随 0.2.0 源码重建；四个非 Windows 目标由 `.github/workflows/time-artifacts.yml` 在对应原生 runner 上生成后回填。后续如果 C 源码、头文件或 `native.abi` 再次变化，必须重新触发该工作流，否则链接器会因清单指纹或 ABI 不匹配用中文诊断拒绝，而不是把新旧对象混着链接。

普通调用方构建 `import time { ... }` 时，编译器读取 `settings.toml` 中对应的 `native.prebuilt.<target>` 条目，直接链接该目录下的对象文件，不需要本机 C 编译器。

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

其中 `manifest.toml` 至少记录包名、包版本、目标三元组、ABI 版本、源码与头文件指纹、对象文件清单。0.2.0 的目标值：

```toml
package = "time"
package_version = "0.2.0"
target = "x86_64-pc-windows-msvc"
abi = 2
```

`fama install` 会保留 `native/artifacts/`，因此本地路径依赖安装到 `mod_module/time@0.2.0/` 后仍然可以直接消费。Linux 和 macOS 目录采用相同结构，对象文件名为 `time_common.o` 和 `time_posix.o`。

## 刷新流程

### Windows（本机，维护者链路）

```powershell
cd E:\xiangmu\mas-lang\examples\projects\time-api
Remove-Item -Recurse -Force mod_module
cargo run --locked -p fama -- install .
cargo run --locked -p fama -- build . --allow-native-source-build
```

刷新结果写在 `mod_module/time@0.2.0/native/artifacts/x86_64-pc-windows-msvc/`，把它下面的 `manifest.toml`、`time_common.obj`、`time_windows.obj` 复制回 `stdlib/time/native/artifacts/x86_64-pc-windows-msvc/` 才算发布完成。

注意：如果 `~/.fama/setting.toml` 里配置了不存在的 `[compiler] binary`，`fama` 会改用外部编译器并失败；把 `FAMA_HOME` 指向空目录可以强制使用内置编译器。

### 四个非 Windows 目标

在 GitHub Actions 上手动触发 `time-artifacts` 工作流（`workflow_dispatch`），矩阵覆盖：

| runner | 产物目标 |
| --- | --- |
| `ubuntu-24.04` | `x86_64-unknown-linux-gnu` |
| `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` |
| `macos-15` | `aarch64-apple-darwin` |
| `macos-15-intel` | `x86_64-apple-darwin` |

工作流执行同样的 `install` + `build --allow-native-source-build`，按目标三元组命名上传 artifact；维护者下载后覆盖对应的 `native/artifacts/<triple>/` 目录。

## 角色分流

普通调用方：

- 直接执行 `fama build` 或 `fama run`
- 命中支持矩阵时会链接包内预编译对象
- 缺少产物、ABI 不匹配或目标不受支持时会收到中文诊断，且不会自动回退到源码编译

维护者：

- 在 `examples/projects/time-api` 中执行 `fama build --allow-native-source-build` 刷新已安装的 time 包
- 编译器会在校验失败或产物缺失时，使用 `native.sources` 的 C 源文件重新生成对象文件和 `manifest.toml`
- 刷新后的产物先写到依赖缓存的 `native/artifacts/<target>/`，发布前需复制回标准库包

## 发布检查

当以下任一内容变更时，必须先刷新全部支持目标的预编译产物，再发布新版本：

- `native/common/*.c`
- `native/platform/**/*.c`
- `native/include/**/*.h`
- `settings.toml` 中的 `native.abi`

发布前至少检查：

1. `native/artifacts/<target>/manifest.toml` 中的 `package_version`、`abi`、`source_hash`、`header_hash` 已更新。
2. `settings.toml` 中的 `native.prebuilt.<target>` 路径与目录内对象文件一致。
3. 普通 `fama build` 在不加 `--allow-native-source-build` 的情况下可以直接消费 `time`。

## 版本控制边界

`build/`、`src/build/` 和 `mod_module/` 是本地构建输出或依赖缓存，由 `.gitignore` 排除。`native/artifacts/` 是调用方默认链接所需的发布内容，继续跟踪其中的清单及 `.obj`、`.o` 文件。
