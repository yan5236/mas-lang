# io 标准库

`io` 提供应用级的**同步**文件、目录和标准流操作，以及无损的二进制读写。包版本 `0.1.0`，原生 ABI 版本 `2`。ABI 2 只改变了标准流写桥的符号与签名，ABI 1 的旧符号 `mas_runtime_std_write` 保留原语义，旧包可以继续在新运行时上运行。

它是普通的标准库包，不是编译器内建能力：用 Fama 安装后，`import io { ... }` 即可使用。文本与二进制严格分开——文本是 `string`（合法 UTF-8），二进制是 `bytes`（任意字节）；把两者混用会在编译期或运行期被拦住。

## 安装

在项目 `settings.toml` 中声明 path 依赖，指向仓库里的 `stdlib/io`：

```toml
[dependencies]
"io" = { path = "../../../stdlib/io", version = "0.1.0" }
```

然后执行 `fama install`，把包安装到项目的 `mod_module/io@0.1.0/`。完整可运行工程见 [`examples/projects/io-api`](../../examples/projects/io-api)。

```sh
fama install
fama build
fama run
```

Windows x64 使用包内预编译对象，普通构建不需要额外参数；其余四个目标等待 CI 回填产物（见[预编译产物与目标矩阵](#预编译产物与目标矩阵)）。

## 导入

`import io { ... }` 必须逐个列出用到的名字，函数、结果结构、枚举和 `File` 都从同一入口导出：

```mas-lang
import io {
    FileMode,
    IOErrorKind,
    close,
    open,
    read,
}
```

## 公开 API 一览

签名中的 `File` 是库句柄（`pub struct File: token: int`）；调用方不要手工构造它。

### `IOStatus`：只表示成功/失败

| 接口 | 说明 |
| --- | --- |
| `flush(file: File) -> IOStatus` | 刷新库缓冲；标准流同时刷新宿主缓冲。输入方向返回 `NotSupported`。 |
| `close(file: File) -> IOStatus` | 关闭常规文件。**即使返回失败，句柄也已失效**；重复关闭报 `Closed`。标准流不接受 `close`（`NotSupported`，借用句柄仍可用）。 |
| `make_dir(path: string) -> IOStatus` | 创建一级目录。父目录缺失报 `NotFound`，目标已存在报 `AlreadyExists`。 |
| `rename(source: string, destination: string) -> IOStatus` | 同一文件系统内移动，目标存在时失败，不覆盖。跨文件系统或宿主不支持时 `NotSupported`。 |
| `remove_file(path: string) -> IOStatus` | 删除文件或指向文件的符号链接。 |
| `remove_dir(path: string) -> IOStatus` | 删除空目录；非空报 `DirectoryNotEmpty`，目录链接/Windows 目录重解析点被拒绝。 |

### `OpenResult` 与 `File`

| 接口 | 说明 |
| --- | --- |
| `open(path: string, mode: FileMode) -> OpenResult` | `Read` 要求文件存在；`Write` 创建或截断；`Append` 创建且每次写到末尾；`ReadWrite` 要求存在且不截断。成功时 `value: File?` 非空。 |
| `stdin() -> File` | 借用进程标准输入。 |
| `stdout() -> File` | 借用进程标准输出，与 `print` 共用同一输出流和顺序。 |
| `stderr() -> File` | 借用进程标准错误。 |

`FileMode` 有 `Read`、`Write`、`Append`、`ReadWrite` 四个成员；`SeekFrom` 有 `Start`、`Current`、`End`。

### `ReadResult` 与 `LineResult`

| 接口 | 说明 |
| --- | --- |
| `read(file: File, count: int) -> ReadResult` | 最多读 `count` 字节。每次调用最多交付一块数据（常规文件一块为 8 KiB），因此正数请求允许短读；管道上只要拿到一段数据就立即返回，不会为凑满 `count` 继续阻塞。读到数据时 `eof = false`，下一次空读取才报告 EOF。 |
| `read_line(file: File) -> LineResult` | 读至 LF 或文件末尾，剥除 LF 和它前面紧邻的 CR，保留其他字符。 |

### `WriteResult`

| 接口 | 说明 |
| --- | --- |
| `write(file: File, data: bytes) -> WriteResult` | 循环写完整段，失败时 `written` 保留宿主已接受的字节数。 |
| `write_text_to(file: File, text: string) -> WriteResult` | 严格验证 UTF-8 后转到同一写入入口，不添加换行。 |
| `write_file(path: string, data: bytes) -> WriteResult` | 创建或**覆盖**整个文件。 |
| `append_file(path: string, data: bytes) -> WriteResult` | 创建或**追加**整个文件。 |
| `write_text(path: string, text: string) -> WriteResult` | 验证 UTF-8 后创建或覆盖文本文件。 |
| `append_text(path: string, text: string) -> WriteResult` | 验证 UTF-8 后创建或追加文本文件。 |

整文件文本入口在打开文件**之前**验证 UTF-8：非法文本返回 `InvalidUtf8`，已有文件不会被截断。

### `SeekResult`

| 接口 | 说明 |
| --- | --- |
| `seek(file: File, offset: int, origin: SeekFrom) -> SeekResult` | 按 `Start`/`Current`/`End` 计算字节位置，成功时 `position` 为从开头计数的新位置。 |

### `BytesResult` 与 `TextResult`

| 接口 | 说明 |
| --- | --- |
| `read_file(path: string) -> BytesResult` | 读取整个文件为 `bytes`。空文件成功。 |
| `read_text(path: string) -> TextResult` | 读取整个文件为 `string`，严格校验 UTF-8；非法编码返回 `InvalidUtf8`，不做替换。 |

### `MetadataResult` 与 `DirResult`

| 接口 | 说明 |
| --- | --- |
| `metadata(path: string) -> MetadataResult` | 跟随符号链接，返回 `kind: FileKind` 和普通文件的字节 `size: int?`。 |
| `list_dir(path: string) -> DirResult` | 返回直接子项名称，排除 `.` 与 `..`，顺序未指定。 |

## 结果类型与失败语义

所有结果都有 `ok: bool` 和 `code: IOErrorKind`。成功时 `ok = true`、`code = IOErrorKind.None`；失败时 `ok = false`，`code` 是下表类别。**判断成功/失败以 `ok` 为准**，不要依赖枚举内部序号。

| 类型 | 额外字段 | 失败时的占位值 |
| --- | --- | --- |
| `IOStatus` | 无 | 无载荷 |
| `OpenResult` | `value: File?` | `null` |
| `BytesResult` | `value: bytes` | 有效空字节（长度 0） |
| `TextResult` | `value: string` | 空字符串 |
| `ReadResult` | `value: bytes`、`eof: bool` | 空字节、`eof = false` |
| `LineResult` | `value: string`、`eof: bool` | 空字符串、`eof = false` |
| `WriteResult` | `written: int` | 保留宿主已接受的字节数 |
| `SeekResult` | `position: int` | `0` |
| `MetadataResult` | `kind: FileKind`、`size: int?` | `FileKind.Other`、`size = null` |
| `DirResult` | `value: list<string>` | 空列表，迭代器已关闭 |

部分失败的细节：二进制 `read` 遇到宿主部分成功时，先交付已读字节，把错误留到下一次正数读取或 `read_line`；`read_line` 在完整行之前遇到读错误时，丢弃本次消费的残行并返回错误；整文件读取遇错返回失败和空值。

## `IOErrorKind` 的 12 个成员

| 成员 | 什么时候出现 |
| --- | --- |
| `None` | 成功；`ok = true` 时使用。 |
| `NotFound` | 打开、读取、查询、删除或创建目录时，路径不存在（含父目录缺失）。 |
| `PermissionDenied` | 宿主拒绝访问：只读文件、权限不足、目录不可写等。 |
| `AlreadyExists` | 目标已存在且操作不允许覆盖：`make_dir` 已存在的目录、`rename` 目标已存在。 |
| `InvalidArgument` | 参数本身不合法：负的读取长度、空路径、内嵌 NUL 的路径、目录被当作文件打开等。 |
| `InvalidUtf8` | 文本边界遇到非法 UTF-8：`read_text`、`read_line`、文本写入，或 POSIX 目录里出现非法 UTF-8 名称。 |
| `Closed` | 句柄已关闭（包括重复关闭），或传入了未知/越界的令牌。 |
| `WrongMode` | 操作与打开模式不符，例如对只写句柄调用 `read`。 |
| `NotSupported` | 宿主或文件系统不支持：标准流的 `close`/`seek`、输入方向 `flush`、跨文件系统 `rename`、平台缺少禁止覆盖的重命名原语。 |
| `DirectoryNotEmpty` | `remove_dir` 的目标不是空目录。 |
| `TooLarge` | 位置、大小或字节计数超出有符号 64 位范围（含令牌表溢出）。 |
| `Other` | 宿主错误无法归入以上类别；未知错误统一归到这里。 |

## 文本与二进制边界

| 你需要处理的数据 | 用哪个类型 | 入口 |
| --- | --- | --- |
| 图片、压缩包、协议帧、任意文件 | `bytes` | `read`、`read_file`、`write`、`write_file`、`append_file` |
| 必须是合法 UTF-8 的文本 | `string` | `read_line`、`read_text`、`write_text_to`、`write_text`、`append_text` |

- `bytes` 原样保留 NUL 和非法 UTF-8，`len`、下标、切片都按**字节**计算。
- `string` 保证是合法 UTF-8；`len` 按 Unicode 标量计数，不是字节数。
- `read_text` 与 `read_line` 是严格入口：遇到非法编码返回 `InvalidUtf8`，不会插入替换字符。`read_line` 会消费掉出问题的那一行。
- 两种类型不能隐式互转，也不能比较；需要时用 `encode_utf8` / `decode_utf8` 显式转换。
- Windows 普通文件使用二进制模式，整文件文本读写原样保留换行。
- `bytes` 可以放进 `list<bytes>`（支持 `append`、下标读取、`contains`、`index`、`pop`、`remove`、`try_get`，元素按内容比较），但列表不能 `sort()`；`bytes` **不能**作为 `map` 的键，也**不能**作为 `map` 的值（报「首版不支持把 `bytes` 作为字典值」）。本包结果结构里的 `bytes` 字段（如 `BytesResult.value`）属于结构体字段，不受这条限制。

`bytes` 和字符串方法的细节见[字节](../../website/docs/basics/bytes.mdx)与[字符串方法](../../website/docs/data-structures/string.mdx)文档。

## 显式关闭与令牌失效

`File` 内部是进程内受检查的令牌，指向原生表项。规则如下：

- 关闭对**全部别名**生效：同一个 `File` 复制给多个变量后，任一别名 `close` 后，其余别名再操作都返回 `Closed`。
- **重复关闭报 `Closed`**；其他文件保持可用，不受影响。
- `close` 即使返回失败，常规文件令牌也已经失效；标准流走的是另一条规则（返回 `NotSupported`，借用句柄仍可用）。
- 丢失最后一个 Mas 引用**不会**自动关闭；进程退出时由宿主回收资源。请在结束使用时显式 `close`。
- 标准流是借用句柄：`stdin`/`stdout`/`stderr` 的 `close` 和 `seek` 返回 `NotSupported`；`stdout`/`stderr` 支持 `write` 和 `flush`，`stdin` 支持 `read`。

## 覆盖、追加与文件系统约定

| 操作 | 约定 |
| --- | --- |
| `write_file` / `write_text` | 创建或**覆盖**（截断）已有文件。 |
| `append_file` / `append_text` | 创建或**追加**到末尾。 |
| `rename` | 同一文件系统内移动；**目标存在时失败**（`AlreadyExists`），绝不静默覆盖。跨文件系统返回 `NotSupported`。 |
| `remove_file` | 删除文件或指向文件的符号链接，不跟随链接删除目标。 |
| `remove_dir` | 只删除**空**目录；非空返回 `DirectoryNotEmpty`；符号链接和 Windows 目录重解析点被拒绝。 |
| `make_dir` | 只创建一级目录；父目录缺失报 `NotFound`，目标已存在报 `AlreadyExists`。 |
| `list_dir` | 只返回直接子项名称，排除 `.` 与 `..`；顺序未指定。 |

路径规则：路径必须是合法 UTF-8 的 `string`，内嵌 NUL 一律 `InvalidArgument`；Unicode 路径按宿主规则访问。`metadata` 跟随符号链接；`list_dir` 失败时丢弃已收集的部分结果并关闭迭代器。

追加写入的落点由宿主保证（Windows `FILE_APPEND_DATA`、POSIX `O_APPEND`）：多个进程同时对同一文件调用 `append_file` / `append_text` 不会互相覆盖，也不会丢字节。

## 平台差异

| 行为 | Windows | POSIX |
| --- | --- | --- |
| 路径 | UTF-8 → UTF-16，使用宽字符 API | 校验后的 UTF-8 字节 + 终止 NUL |
| 目录名编码 | 宽字符天然支持 Unicode | 目录项出现非法 UTF-8 名称时返回 `InvalidUtf8`，不做损失性替换 |
| 删除目录 | 先拒绝 `FILE_ATTRIBUTE_REPARSE_POINT`（junction 等重解析点），再删空目录 | 先拒绝符号链接，再删空目录 |
| 禁止覆盖改名 | `MoveFileExW` 不带替换标志 | 优先 `renameat2(RENAME_NOREPLACE)`，否则 `link` + `unlink`；都不可用返回 `NotSupported` |
| 标准流 | 运行时桥（Rust 标准流）；控制台无法表示的任意二进制写入返回可恢复错误 | 同左 |
| 跨文件系统移动 | `NotSupported` | `NotSupported` |

平台间保持一致的语义（有测试覆盖）：目录不能当文件打开、删除目录只删空目录、改名禁止覆盖、内嵌 NUL 的路径一律 `InvalidArgument`。更底层的平台实现细节见下文[原生层实现与验证状态](#原生层实现与验证状态)。

## 预编译产物与目标矩阵

发布矩阵沿用 `time` 的五个目标：

| 目标三元组 | 现状 |
| --- | --- |
| `x86_64-pc-windows-msvc` | **已带预编译产物**（本机 MSVC 工具链实际编译），普通 `fama build` 可直接消费 |
| `x86_64-unknown-linux-gnu` | 待 CI 回填 |
| `aarch64-unknown-linux-gnu` | 待 CI 回填 |
| `x86_64-apple-darwin` | 待 CI 回填 |
| `aarch64-apple-darwin` | 待 CI 回填 |

四个待回填目标上，普通消费会因缺少预编译对象而在链接前报错，这是预期状态。维护者可以用 `--allow-native-source-build` 在本机从 C 源刷新产物：

```sh
fama build --allow-native-source-build
```

发布前再把 `native/artifacts/<target>/` 复制回标准库包。CI 工作流是 `.github/workflows/io-artifacts.yml`。

## 最小完整示例

下面的程序只用到 `io` 的公开 API：整文件写入、追加、严格文本读取、分块读取、元数据和显式关闭。

```mas-lang
import io {
    FileMode,
    IOErrorKind,
    append_text,
    close,
    metadata,
    open,
    read,
    read_text,
    write_file,
}

func main():
    var path = "demo.txt"
    var written = write_file(path, encode_utf8("你好").value)
    print("write ok=" + string(written.ok))

    var appended = append_text(path, "，世界")
    print("append ok=" + string(appended.ok))

    var text = read_text(path)
    print("text=" + text.value)

    var opened = open(path, FileMode.Read)
    if not opened.ok:
        print("open failed")
        return
    var handle = opened.value.value
    var chunk = read(handle, 6)
    print("chunk bytes=" + string(len(chunk.value)))
    print("close ok=" + string(close(handle).ok))

    var info = metadata(path)
    print("size=" + string(info.size.value))
    var again = close(handle)
    print("closed=" + string(again.code == IOErrorKind.Closed))
```

在工程根目录运行，输出：

```text
write ok=true
append ok=true
text=你好，世界
chunk bytes=6
close ok=true
size=15
closed=true
```

（该示例在 Windows x64、原生 ABI 2 上实际编译运行通过；其他目标未在当前环境验证。）

## 原生层实现与验证状态

本节面向维护者，保留 C 原生层的事实与验证口径；使用者可以跳过。

### 目录结构

```text
stdlib/io/
├── settings.toml                  # 包声明：abi = 2、include、native.sources、各目标预编译产物
├── mod-lock.yml                   # 无依赖包的空锁文件（与 time 一致）
├── src/                           # Mas 包装层（index.ms / native.ms / files.ms / text.ms / fs.ms …）
├── README.md
└── native/
    ├── include/
    │   ├── io_host.h              # 公开原生 ABI：记录布局、错误码、20 个入口
    │   └── io_internal.h          # 包内契约：平台适配、共享工具、流状态
    ├── common/
    │   ├── io_common.c            # UTF-8/路径校验、整数检查、结果写入、有界扩容
    │   ├── io_state.c             # 句柄表、读缓冲、宿主读写原语、open/std/close/flush
    │   ├── io_stream.c            # read/read_line/write/seek
    │   ├── io_whole.c             # 整段读写的内部实现（首次错误优先、关闭错误回报）
    │   └── io_paths.c             # 整文件、元数据、目录迭代、建目录/改名/删除
    ├── platform/
    │   ├── windows/io_windows.c    # 错误映射、UTF-8→UTF-16、常规文件读写
    │   ├── windows/io_windows_fs.c # 元数据、目录、建目录、MoveFileExW、删除
    │   ├── posix/io_posix.c        # 错误映射、路径字节串、常规文件读写
    │   └── posix/io_posix_fs.c     # 元数据、目录、mkdir、renameat2、删除
    ├── artifacts/
    │   └── x86_64-pc-windows-msvc/ # 本机 cl.exe 实际编译输出 + manifest.toml
    └── test/
        ├── io_host_test.c          # 测试驱动入口、运行时桩、临时目录
        ├── io_test_api.c           # 公开 API 薄包装
        ├── io_test_support.h       # 断言、路径辅助、标准流桩控制
        ├── io_host_test_stream.c   # 打开模式、短读/EOF、写入、定位
        ├── io_host_test_text.c     # 读行、追加、读写切换、边界
        ├── io_host_test_std.c      # 关闭/别名、同文件双开、标准流
        ├── io_host_test_fs.c       # 整文件、元数据、目录、建、改名、删除
        ├── io_host_test_links.c    # 符号链接与 junction 重解析点
        ├── io_host_test_concurrency.c # 多进程并发追加（同一文件 8 进程 × 30 MiB）
        ├── run-tests.ps1           # 本机 cl.exe 编译并运行
        └── posix-syntax-probe.ps1  # POSIX 源码语法探测（尽力而为）
```

### 原生层职责

- **公共层**负责计数、循环、资源状态和错误归并：模式与令牌校验、读缓冲、逻辑位置、部分写入的 `written` 语义、关闭顺序、整文件入口的清理与首次错误优先。
- **平台层**只做宿主差异：路径编码、打开标志、定位、元数据、目录枚举、独占重命名、删除限制，以及**唯一的**宿主错误映射函数（Windows `mas_io_win_map`、POSIX `mas_io_posix_map`）。
- **包内不实现运行时**：托管字符串/字节复制、`mas_record_alloc` 和标准流桥都由 `crates/maslang` 提供；`io_host.h` 只声明不实现，测试驱动用 `malloc` 桩替换。
- **深层模块是实现细节**：公开 API 只经由包根 `index.ms` 导出。`import io.native`、`import io.fs` 下的 `raw_*`、输入/输出盒属于包内契约，不承诺跨版本兼容。目录迭代的裸绑定（`mas_io_dir_*` 与 `raw_dir_*`）和唯一调用方 `list_dir` 放在同一模块且不标 `pub`，外部工程无法通过导入直接拿到返回整数错误码的入口（有编译期回归用例）。

### ABI 约定

Mas 的 `extern func` 不能可靠地按值传 16 字节结构体（Win64 下按引用传递会读到垃圾），因此**所有入口都使用「输入盒 + 输出盒」记录指针**：Mas 侧声明字段顺序完全一致的 `pub struct`，C 侧只按偏移读写，不保存跨调用指针。字段顺序见 `io_host.h`，`bool` 字段在记录里是 `uint8_t`，禁止 `#pragma pack`。

错误码序号与 `IOErrorKind` 成员顺序一一对应，是两侧唯一的映射约定：

| 码 | 含义 | 码 | 含义 |
| --- | --- | --- | --- |
| 0 | `None` | 6 | `Closed` |
| 1 | `NotFound` | 7 | `WrongMode` |
| 2 | `PermissionDenied` | 8 | `NotSupported` |
| 3 | `AlreadyExists` | 9 | `DirectoryNotEmpty` |
| 4 | `InvalidArgument` | 10 | `TooLarge` |
| 5 | `InvalidUtf8` | 11 | `Other` |

所有入口先校验指针，失败只写结果结构，**绝不终止进程**（`mas_runtime_abort` 只留给未来严格包装层使用，原生 IO 路径不调用它）。

标准流写桥使用**版本化符号**，不允许原地修改已发布签名：

| 符号 | ABI | 签名与返回语义 |
| --- | --- | --- |
| `mas_runtime_std_write` | 1（保留） | 三参数；成功返回已接受字节数，失败返回负值错误码 |
| `mas_runtime_std_write_v2` | 2 | 四参数，多一个 `accepted` 出参；返回 `0` 或负值错误码，失败时也写回已接受字节数 |

新编译的 `io` 对象只调用 v2；按 ABI 1 编译的旧包继续调用 v1，因此可以和新运行时混用。ABI 数字（`settings.toml` / `manifest.toml`）只负责包自身的清单一致性检查，不能替代符号版本化。

### 令牌与生命周期

- 常规文件令牌从 4 开始、进程内单调递增、永不复用；1..3 保留给 `stdin`/`stdout`/`stderr` 三个借用句柄。令牌溢出（实际不可达）返回 `TooLarge`。
- 动态表按开放句柄数增长，关闭即时移除；关闭对同一令牌的所有别名生效，重复关闭或未知令牌返回 `Closed`。不做 GC 自动关闭。
- 同一路径两次打开是两个独立表项。目录迭代器使用独立令牌空间（从 1 开始，0 无效），未知句柄同样返回 `Closed`。
- `close` 先把常规文件表项移出表再关闭宿主资源，因此**关闭失败也使令牌失效**；标准流不登记表项，`close` 返回 `NotSupported` 且不改变借用状态。

### 缓冲与定位

- 每个常规文件句柄一份 8 KiB 读缓冲；`read` 与 `read_line` 共用同一缓冲和逻辑位置，不会各自新建读缓冲吞掉后续数据。
- `position` 是「下一个逻辑读写字节」的绝对位置，预读数据不计入；`seek(Current)` 和读转写同步都用它，而不是宿主游标。
- `read` 的返回缓冲按实际读到的字节有界扩容，不为一个很大的 `count` 预先整块分配；每次调用最多交付一块，因此超大 `count` 在 EOF 上直接返回 EOF，不会预分配巨量内存。
- `read_line` 用独立的按需扩容行累加器；读到完整行前的错误会丢弃本次消费的残行。
- 写直接循环写宿主，保证 `written` 是宿主真正接受的字节数——包括标准流在同一次调用里先写出一部分再断管的情况。
- `Append` 的落点由宿主追加语义保证：Windows 用 `FILE_APPEND_DATA`、POSIX 用 `O_APPEND`，写入时不再「先定位到末尾再写」，因此多个进程并发追加不会互相覆盖。
- `flush` 没有用户态写缓冲可刷：常规写句柄成功，标准流走运行时桥；输入方向返回 `NotSupported`，不假装成功。

### 平台实现细节

| 行为 | Windows | POSIX |
| --- | --- | --- |
| 路径 | UTF-8 → `MultiByteToWideChar(MB_ERR_INVALID_CHARS)`，宽字符 API | 校验后的 UTF-8 字节 + 终止 NUL |
| 打开 | `CreateFileW`，目录先按属性拒绝（`InvalidArgument`） | `open` 后用 `fstat` 拒绝目录 |
| 追加 | `FILE_APPEND_DATA` + `OPEN_ALWAYS`，由宿主保证每次写入原子落在末尾 | `O_WRONLY`+`O_CREAT`+`O_APPEND`，同样由宿主保证落点 |
| 元数据 | `CreateFileW`（跟随链接）+ `GetFileInformationByHandle`，非磁盘句柄归 `Other` | `stat`（跟随链接），`S_ISREG`/`S_ISDIR` 之外归 `Other` |
| 目录枚举 | `FindFirstFileW`/`FindNextFileW`，空目录按「立即结束」处理 | `opendir`/`readdir`，非法 UTF-8 名字返回 `InvalidUtf8` |
| 禁止覆盖改名 | `MoveFileExW` 不带 `MOVEFILE_REPLACE_EXISTING` | Linux 用 `renameat2(RENAME_NOREPLACE)`；macOS 用 `renamex_np(RENAME_EXCL)`；其他 POSIX 退回 `link()`+`unlink()`（不支持目录），都不可用返回 `NotSupported` |
| 删除目录 | 先拒绝 `FILE_ATTRIBUTE_REPARSE_POINT`，再 `RemoveDirectoryW` | 先 `lstat` 拒绝符号链接，再 `rmdir` |
| 标准流 | 运行时桥（Rust 标准流），控制台文本失败以可恢复错误码返回 | 同左 |

### 记录在案的实现选择

设计文档没有逐字规定、由本层决定并已在测试中固定下来的行为：

- 读句柄 `flush`、`stdin` 的 `flush` 返回 `NotSupported`；追加句柄不可读（`WrongMode`）。
- `read_line` 只在有 LF 时剥除紧邻其前的 CR；文件末尾没有 LF 时，末尾的裸 CR 属于内容。
- 空路径、`NULL` 结果指针、越界模式/起点/标准流编号都是可恢复错误，不崩溃。
- 关闭后访问任意入口（含目录迭代器）返回 `Closed`；标准流的 `close`/`seek` 返回 `NotSupported`。
- Windows 设备名（如 `NUL`）按宿主规则打开为设备而不是普通文件；普通文件接口对设备不承诺可读写，元数据把它们归为 `Other`。

### 验证状态

| 项目 | 状态 |
| --- | --- |
| Windows x64 原生层 | **已实跑**：`native/test/run-tests.ps1` 在本机 MSVC 下编译并以 `/W4 /WX` 通过，**300 条断言全绿，退出码 0**（含并发追加、定位失败、断管部分写入、符号链接与 junction 用例） |
| Windows x64 预编译产物 | 已按 0.1.0 / ABI 2 重新生成并回填审核修复后的源码指纹（`native/artifacts/x86_64-pc-windows-msvc/`，本机 cl.exe 输出）；默认消费路径已实跑 |
| ABI 1 兼容 | **已实跑**：用 `12fd9c7` 的旧包（abi = 1、旧三参数写桥）配当前编译器默认构建，`write_text_to` 返回 `ok=true`；端到端用例 `io_legacy_std_write_symbol_keeps_abi1_semantics` 断言旧符号返回 7 字节 |
| Linux x64/ARM64、macOS x64/ARM64 | **未运行**：本机无可用 POSIX 工具链。只用 `posix-syntax-probe.ps1` 做了 `gcc -fsyntax-only` 语法探测，**6 项探测通过**（每个文件 3 个分支）；这只证明语法与局部类型自洽，不证明 POSIX 上的实际行为 |
| 跨平台 CI（Linux/macOS） | 预编译产物未交付的目标自动退回源码构建运行行为用例；默认消费用例显式跳过并打印原因，不再把「产物未交付」误报成产品缺陷 |
| Mas 包装层（`src/`） | 已补齐；C 侧 ABI 与其 `native.ms` 声明逐字段核对一致（字段顺序、错误码序号与入口名） |

`posix-syntax-probe.ps1` 会把 POSIX 源复制到临时目录，把两参数 `mkdir` 调用替换成 MinGW 的 `_mkdir`，并补齐 `lstat`/`link`/`renamex_np`/`S_ISLNK` 声明，然后分别对 `io_posix.c` 与 `io_posix_fs.c` 走 `link`/`unlink` 回退分支、`-D__linux__` 的 `renameat2` 分支和 `-D__APPLE__` 的 `renamex_np` 分支，共 6 次 `gcc -fsyntax-only`。**通过只证明语法与局部类型自洽，不证明 POSIX 上的实际行为。**

POSIX 侧需要在真实 Linux/macOS 上补测的清单（按风险排序）：

1. `renameat2(RENAME_NOREPLACE)`（Linux）与 `renamex_np(RENAME_EXCL)`（macOS）：目标已存在、跨文件系统、目录移动、旧内核/文件系统不支持时的错误映射。
2. 目录名非法 UTF-8 → `InvalidUtf8`（需要能构造任意字节文件名的文件系统）。
3. 符号链接：`remove_file` 删链接不删目标、`remove_dir` 拒绝对目录的链接。
4. 权限不足（`EACCES`）与只读文件系统（`EROFS`）的映射。
5. `EINTR` 重试路径（读/写）与等价于 `CloseHandle` 的关闭失败路径。
6. 大文件偏移（>2 GiB）的 `seek` 与 `_FILE_OFFSET_BITS=64` 生效情况。
7. POSIX 断管：向已关闭的管道写入默认会收到 `SIGPIPE`，当前运行时未接管该信号，断管是否返回可恢复错误而不是终止进程**未验证**（Windows 已有真实断管端到端用例）。
8. 非 Apple、非 Linux 的其他 POSIX 目标走 `link()`+`unlink()` 回退，目录改名返回 `NotSupported`（当前没有这类发布目标）。

### 测试驱动

```powershell
# 在 stdlib/io 下执行；脚本自己探测 vcvars64.bat 与 cl.exe
pwsh -File native/test/run-tests.ps1
pwsh -File native/test/run-tests.ps1 -SkipPosixProbe
pwsh -File native/test/run-tests.ps1 -VcVars 'C:\...\VC\Auxiliary\Build\vcvars64.bat'
```

- 退出码：`0` 全部断言通过，`1` 有失败断言，`90` 找不到或调用 `vcvars64.bat` 失败，`91` 编译失败。
- 测试在 `%TEMP%\mas-io-native-test-<pid>-<tick>` 下建真实文件，结束时递归删除；不修改进程当前目录，也不依赖用户机器上的固定路径。
- 并发追加用例用同一可执行文件的 `--append-worker` 模式派生 8 个子进程，各向同一文件追加 30 MiB；最终大小必须正好等于 240 MiB，用来卡住「先定位再写」造成的覆盖。
- 运行时符号（字符串/字节复制、记录分配、标准流桥）由测试驱动的 `malloc` 桩实现；标准流桩支持注入数据、模拟 EOF、单次只交付少量字节（管道短读）、部分写入失败和读取错误，用于确定性覆盖错误分支。
- 文件用 UTF-8 无 BOM 保存（与 `time` 一致），编译时加 `/utf-8`，避免 MSVC 按系统代码页解析中文注释与字符串。

### 版本控制边界

`build/`、`src/build/`、`mod_module/` 是本地构建输出或依赖缓存，由 `.gitignore` 排除；`native/artifacts/` 是发布内容，继续跟踪清单与 `.obj`/`.o` 文件。
