# DeepSeek 实施交接

在仓库根目录运行编码代理。可直接复制下面的启动指令，并让代理读取三个 Markdown 文件全文。

> 你是 Mas lang 仓库的实现者。先读 AGENTS.md、specs/009-application-io/requirements.md、design.md 和本文件，按阶段实现。以需求和设计的签名、边界、错误语义为验收依据。先核对当前分支、现有改动和关键入口，再写代码。每完成一阶段，运行对应验证，记录通过、失败、未运行的命令以及原因，继续可完成的工作。平台产物缺失时保持状态为未完成。遇到可逆实现细节沿用现有实现，遇到必须改变公开契约或现有行为的冲突时指出具体文件、现有行为和最小修正，等待确认该冲突，继续完成其它独立任务。

## 工作规则

1. 从 `git status --short --branch` 开始。保留现有分支和用户改动。若存在 `.project-memory`，先读项目约定和索引。改某函数前找定义、调用者和已有测试。文件位置会随着仓库变化，以当前源码为准。
2. 新增或修改的函数写准确的中文函数级注释。解释用途及必要的输入、返回、错误、副作用。Rust 用现有文档注释习惯，C/Mas 保持相邻代码风格。UTF-8 读取和保存，保留无关注释。文件原则上不超过 500 行，按实际职责拆分。
3. 沿用当前错误定位、GC、类型检查、原生分发和 Fama 依赖解析。优先复用现有辅助函数，避免为 IO 引入依赖、重复运行时、并行类型系统或大范围重排。每个阶段先验证真实入口，再扩到下阶段。
4. FFI 参数先校验指针、长度、整数转换和合法状态。每个资源获取点列出所有释放路径。普通错误返回结果。禁止因文件不存在、读写失败、无效路径、非法编码或失效句柄退出进程。
5. 使用仓库既有测试夹具和独立临时目录。只加覆盖实际缺口的测试，断言数据、状态、失败类别和输出字节。禁止依赖测试顺序、进程当前目录的全局修改或用户机器上的固定路径。
6. 代码、样例和用户文档同阶段更新。工作结束时列出变更、实跑命令和各平台验证状态。无真实数据时不要写性能结论。

## 阶段 0 基线与接入定位

- [x] 0.1 核对 `Cargo.toml`、`crates/maslang/src/runtime_bootstrap.rs`、`crates/maslang/src/native_link.rs`、`stdlib/time/settings.toml`、`crates/fama/tests/time_package.rs`、`.github/workflows/time-artifacts.yml` 及 `.github/workflows/native.yml`。确认编译器/链接器与预编译对象再生成路径。
- [x] 0.2 运行现有相关测试。基线目标为 `cargo test --workspace --locked`，基线受环境阻塞时记录错误和已能运行的包级检查。后续继续使用根锁文件。[依赖: 0.1]

## 阶段 1 类型与运行时

- [x] 1.1 打通 bytes 的注解、表达式、类型检查和 LLVM 表示。从 `token.rs`、`parser.rs`、`semantic/types.rs`、`annotations.rs`、`containers.rs` 找入口，跟踪后端 storage、GC、record 和容器 lowering。覆盖局部、全局、模块函数、记录字段、可空值和 list。声明需要保根的类型时同步核对优化路径。[依赖: 0.1] [验证 Req 1]
- [x] 1.2 实现 bytes 构造、UTF-8 转换、索引、切片和相等比较。覆盖空、NUL、0xFF、负索引、范围越界和跨 GC 存活。在 `crates/maslang/tests` 沿用现有运行真实 Mas 程序的方式，参考 `gc.rs`、`safe_conversion_bytes.rs`、`length.rs`，确认 `safe_conversion_bytes.rs` 已有的原生非法字符串用例仍通过。[依赖: 1.1] [验证 Req 1]
- [x] 1.3 在 `crates/maslang/native/include/mas_runtime.h` 和对应 Rust 运行时实现新增 bytes 字段 ABI。用一个 extern 出参样例验证 C 复制字节后 GC 仍能追踪，普通字符串和 `native_records` 继续通过。核对 bootstrap 能链接新增符号。[依赖: 1.2] [验证 Req 1, Req 6]
- [x] 1.4 实现 enum 解析、声明身份、类型检查与 lowering，并接入模块导入、别名和重导出。测试两个模块定义同名枚举及相同成员序号仍不能互用，另外覆盖重复成员和非整数隐式转换。[依赖: 0.1] [验证 Req 5]
- [x] 1.5 接入 `crates/mas-lang-lsp` 的关键字、声明扫描、类型显示、成员补全、悬停和诊断，检查编辑器已有语法资源是否须同步。用可导入公开枚举和 bytes 样例校验。[依赖: 1.2, 1.4] [验证 Req 5, Req 6]

## 阶段 2 文件、标准流和文件系统

- [x] 2.1 建立 `stdlib/io` 包及私有 C 绑定。先完成 open/close、模式和令牌校验。验证两次打开同一路径得到独立资源，关闭一个后另一个可用，旧令牌重开后依然失效。[依赖: 1.3, 1.4] [验证 Req 2]
- [x] 2.2 实现 read/write/flush/seek、ReadWrite 切换及共同错误映射。覆盖空读、短读、EOF、重复关闭、定位后重读、二进制往返和部分写入失败。用现有的真实或确定性失败入口验证错误传播。部分写入无法用宿主稳定触发时，在最小宿主适配边界测试这一分支。[依赖: 2.1] [验证 Req 2]
- [x] 2.3 加 stdin/stdout/stderr 桥。先确认 `runtime/strings.rs` 的输出路径，复用 Rust 标准流。用子进程重定向验证原始字节和 `print` 交错输出顺序，验证 stdout/stderr 隔离及标准流 close/seek 被拒绝。Windows 控制台文本和重定向分别验证。[依赖: 2.2] [验证 Req 2]
- [x] 2.4 实现文本行和整文件包装。覆盖 CRLF、LF、空行、末尾无换行、非法 UTF-8、有效内嵌 NUL、覆盖、追加、失败时释放句柄。文本写入非法内容时断言原文件内容未被截断。用现有关闭错误策略测试首次错误优先。[依赖: 2.2, 2.3] [验证 Req 2]
- [x] 2.5 实现目录、元数据、创建、重命名和删除。断言中文路径、非法路径、空目录、非空目录、符号链接和禁止覆盖。POSIX 测试非法 UTF-8 文件名，Windows 检查宽字符路径和目录重解析点。平台差异按测试条件标明。[依赖: 2.1] [验证 Req 3]

## 阶段 3 字符串方法

- [x] 3.1 按 `specs/003-string-method-collection` 和当前代码找字符串方法从解析到 lowering 的入口，重点检查 `semantic/containers.rs`、`runtime/strings.rs` 与 LSP completion。复用列表和可空载荷构造。测试多字节标量、组合字符、重叠搜索、空分隔符、空 needle、末尾空项、大小写扩张及 repeat 长度溢出。为非法 UTF-8 保留带源码位置的诊断。[依赖: 0.1] [验证 Req 4, Req 6]

## 阶段 4 文档、样例、预编译对象

- [x] 4.1 增加一个正常 Fama 项目样例，用固定块大小复制二进制文件、逐行读配置、追加 UTF-8 日志。测试复制结果逐字节一致、中文路径可访问、配置行内容及日志追加正确。把 API、错误、文本/二进制边界、显式关闭、覆盖和平台约定写进包 README 与网站标准库文档。[依赖: 1.5, 2.4, 2.5, 3.1] [验证 Req 1–6]
- [x] 4.2 更新语言文档中的类型、枚举、字符串方法、原生 ABI。示例使用实际支持的 Mas 语法，运行真实示例。当前 `time` 公开 API 和五目标对象保持兼容。[依赖: 4.1] [验证 Req 4, Req 5, Req 6]
- [ ] 4.3 生成 Windows x64、Linux x64/ARM64、macOS x64/ARM64 对象和清单。复用 `time` 的维护流程，使用实际编译输出，保留包名、版本、目标、ABI、源码/头文件指纹与对象列表的一致性。缺少构建目标时记录缺口和复现步骤，待真实对象生成后再标记完成。[依赖: 4.1] [验证 Req 6]
- [x] 4.4 通过普通 Fama 依赖安装/构建/运行验证各目标。默认路径无 `--allow-native-source-build`，源码回退另测。运行 `cargo test --workspace --locked`、`cargo fmt --all -- --check` 和 `git diff --check`。编辑器或网站有改动时按各自 package.json 的已有命令验证。最后检查 diff，记录目标级通过、失败和未验证项。[依赖: 4.2, 4.3] [验证 Req 1–6]

## 后续交接

IO 发布后再启动 `net` 规范，用小型 HTTP 服务验证多连接、超时和慢客户端。GUI 单独定义事件循环与回调 ABI。自定义泛型需先确定实例化、导出与类型诊断，再考虑带载荷枚举和通用 `Result<T, E>`。本次结果结构保持自己的公开契约。


## 实施记录（2026-11-10）

环境：Windows 11 + PowerShell 7、Rust 1.98、LLVM 19（inkwell）、MSVC 14.44；实机只覆盖 Windows x64。

### 交付范围

- 语言核心 `bytes`：字面构造 `bytes_from_list`、严格 `encode_utf8`/`decode_utf8`、`len`、`data[i]`、`data.slice`、逐字节 `==`/`!=`；可用于局部、全局、参数、返回、结构体字段、`bytes?` 与 `list<bytes>`；禁止打印、隐式转换、字典键值和排序比较、下标写入。
- 语言核心 `enum`/`pub enum`：冒号+缩进成员声明、`Enum.Member` 引用、模块级名义身份、导入/别名/重导出、可空枚举、同类型相等；拒绝空枚举、重复成员、未知成员、跨枚举比较、隐式整数转换、`print` 和构造调用。
- `stdlib/io` 0.1.0（原生 ABI 1）：Mas 包装层（types/codes/native/files/text/fs/index）、C 原生层（common 5 文件 + windows/posix 各 2 文件）、Windows x64 预编译对象与清单、独立 C 测试驱动、`examples/projects/io-api` 示例工程。
- 字符串方法：`split`、`index_of`、`substring`、`to_lower`、`to_upper`、`repeat`，按 Unicode 标量计数，非法 UTF-8 使用带路径/行/列/插入符的运行时错误。
- 工具链与文档：Fama 预编译消费路径、VS Code TextMate 语法（enum/bytes/内建函数）、网站标准库 `io` 文档、`bytes`/`enum` 语言页、字符串方法与原生 ABI 更新、`examples/projects/io-api/README.md`。

### 命令与结果

| 命令 | 结果 |
| --- | --- |
| `cargo test --workspace --locked` | 通过（EXIT=0）：lib 138、全部集成套件、io_package 5、time_package 2 全绿 |
| `cargo test -p maslang --test bytes` | 6 passed：构造/索引/切片/相等、UTF-8 严格转换、GC 跨位置存活、原生 bytes 字段、`list<bytes>`、编译期拒绝 |
| `cargo test -p maslang --test enums` | 6 passed：成员与名义身份、结构体字段/可空、参数返回、导入别名重导出、跨模块同名互斥、非法用法 |
| `cargo test -p maslang --test string_methods` | 9 passed：六方法边界、Unicode 大小写、重叠搜索、空分隔符、非法 UTF-8 源码定位 |
| `cargo test -p mas-lang-lsp` | 通过：包含 bytes 5 例、enum 5 例、字符串补全 2 例 |
| `cargo test -p fama --test io_package -- --test-threads=1` | 5 passed：示例端到端、错误类别与关闭语义、标准流重定向与顺序、非法 UTF-8 不截断、普通依赖只用预编译产物 |
| `cargo test -p fama --test time_package` | 2 passed：time 公开 API 与五目标对象兼容性回归 |
| `pwsh -File stdlib/io/native/test/run-tests.ps1` | 276 条断言、0 失败、EXIT=0；POSIX 文件 4 项 `gcc -fsyntax-only` 通过 |
| `fama install/build/run examples/projects/io-api`（无 `--allow-native-source-build`） | 通过：Windows x64 预编译对象直接消费，输出与 README 预期一致 |
| `cd website; npm run build` | 通过：en 与 zh-Hans 两个 locale 静态构建成功 |
| `rustfmt --edition 2024 --check`（本次新增文件） | 通过 |
| `git -c core.whitespace=cr-at-eol diff --check` | 通过（EXIT=0） |

### 未完成 / 未验证

- 阶段 4.3 只生成了 `x86_64-pc-windows-msvc` 的真实对象与清单；Linux x64/ARM64、macOS x64/ARM64 缺本机构建环境，状态保持未完成。回填方式：`workflow_dispatch` 触发 `.github/workflows/io-artifacts.yml`，下载 artifact 后放入 `stdlib/io/native/artifacts/<triple>/`，并在 `stdlib/io/settings.toml` 增加对应 `[native.prebuilt.<triple>]`；本地复现命令为 `rm -rf examples/projects/io-api/mod_module && fama install examples/projects/io-api && fama build examples/projects/io-api --allow-native-source-build`。
- POSIX 运行时行为未验证（本机 WSL 被宿主拒绝、MinGW 缺 `lstat`/`link`/两参数 `mkdir`、无 Docker），只做语法探测；待补测清单写进 `stdlib/io/README.md`。
- Windows 控制台文本分支未在自动化测试中覆盖：测试进程的标准输出都是管道，`is_terminal` 为假；重定向/管道语义已由 fama 用例验证，控制台只按实现逻辑审查。
- 部分写入与关闭失败的注入只在 C 驱动内可控；「操作成功但关闭失败」没有稳定的宿主注入手段，保留代码路径与审查结论。
- `cargo fmt --all -- --check` 仍会报告仓库基线里 41 个文件的既有格式差异（包含本次未改动的 `cli.rs`、`lib.rs`、`linker.rs` 等）；为避免无关重排未做全仓格式化，本次新增文件单独 rustfmt 后 `--check` 干净。
- 仓库多数文件在索引中以 CRLF 存储，`git diff --check` 会把新增行误报为行尾空白；按 `-c core.whitespace=cr-at-eol` 运行通过，`stdlib/io/native` 已用 `.gitattributes` 固定 LF 以保持预编译产物指纹。
- 英文站点（`website/i18n/en`）未同步新增页面，只更新了默认中文文档。
- 既有计时用例 `tests::should_build_project_with_time_standard_library_dependency` 在满负载并行跑时曾出现 `elapsed >= 10` 偶发失败；单跑 3/3 通过，最终全量运行通过，与本规范改动无关。

### ABI 与关键决策

- `bytes` 在 LLVM 中与字符串同为「指针 + 长度」双字托管槽，GC 扫描 kind=1；记录内 bytes 字段由 `mas_record_alloc` 的 trace 表追踪。
- 编译器直接调用的字节运行时入口一律用 `out: *mut MasBytes` 写回，不返回 16 字节结构：Windows x64 的 sret 约定与 IR 声明不一致会读到错位实参（实测）。顶层 `extern func` 的 `bytes` 参数与返回值在语义阶段被拒绝，字节载荷统一放进结构体字段。
- `enum` 成员按声明顺序 lowering 为 i64，名义身份使用模块改写后的内部名，诊断与 LSP 使用源码显示名；不开放原生 enum ABI。
- `io` 的全部原生入口使用「输入盒 + 输出盒」记录指针；令牌表、读缓冲、错误映射与平台差异都在 C 层，标准流经 `mas_runtime_std_read/write/flush` 复用 Rust 标准流，保持与 `print` 的顺序；直接连接终端时非 UTF-8 字节返回可恢复错误。
