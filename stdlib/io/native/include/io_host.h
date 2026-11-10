#ifndef MAS_IO_HOST_H
#define MAS_IO_HOST_H

#include <stddef.h>
#include <stdint.h>

/**
 * io 包的原生 ABI 版本，必须与 `settings.toml` 的 `native.abi` 以及各目标
 * `manifest.toml` 中的 `abi` 字段保持一致。
 *
 * 版本 2 只改变标准流写桥的签名：新增 `mas_runtime_std_write_v2`（返回状态码 +
 * 已接受字节数出参）。ABI 1 的三参数 `mas_runtime_std_write` 保留原符号与原语义，
 * 旧包可以继续在新运行时上链接运行。
 */
#define MAS_IO_ABI_VERSION 2

/**
 * 错误码：序号与 Mas 侧 `IOErrorKind` 的成员顺序严格一致，是两侧唯一的映射约定。
 * 原生层只向结果结构写这些整数，公开枚举由 Mas 包装层按同一序号显式转换。
 */
#define MAS_IO_OK 0
#define MAS_IO_ERROR_NOT_FOUND 1
#define MAS_IO_ERROR_PERMISSION_DENIED 2
#define MAS_IO_ERROR_ALREADY_EXISTS 3
#define MAS_IO_ERROR_INVALID_ARGUMENT 4
#define MAS_IO_ERROR_INVALID_UTF8 5
#define MAS_IO_ERROR_CLOSED 6
#define MAS_IO_ERROR_WRONG_MODE 7
#define MAS_IO_ERROR_NOT_SUPPORTED 8
#define MAS_IO_ERROR_DIRECTORY_NOT_EMPTY 9
#define MAS_IO_ERROR_TOO_LARGE 10
#define MAS_IO_ERROR_OTHER 11

/** `FileMode` 的整数序号，与 Mas 侧枚举声明顺序一致。 */
#define MAS_IO_MODE_READ 0
#define MAS_IO_MODE_WRITE 1
#define MAS_IO_MODE_APPEND 2
#define MAS_IO_MODE_READ_WRITE 3

/** `SeekFrom` 的整数序号。 */
#define MAS_IO_SEEK_START 0
#define MAS_IO_SEEK_CURRENT 1
#define MAS_IO_SEEK_END 2

/** `FileKind` 的整数序号。 */
#define MAS_IO_KIND_REGULAR 0
#define MAS_IO_KIND_DIRECTORY 1
#define MAS_IO_KIND_OTHER 2

/** 标准流编号，用于 `MasIoStdArg.which` 与 `mas_runtime_std_*` 桥。 */
#define MAS_IO_STDIN 0
#define MAS_IO_STDOUT 1
#define MAS_IO_STDERR 2

/**
 * 表示 Mas lang 运行时与原生模块共享的字符串 ABI，使用 `ptr + len` 传递 UTF-8 文本。
 * 该结构与运行时 `mas_runtime.h` 里的 `MasString` 同构，包内因此不必重复引入运行时头文件。
 */
typedef struct MsString {
    const unsigned char* ptr;
    unsigned long long len;
} MasString;

/**
 * 表示共享的字节 ABI，与运行时 `mas_runtime_bytes_clone` 返回的托管字节同构。
 * 长度以字节计数，允许 NUL 和任意非 UTF-8 数据。
 */
typedef struct MasBytes {
    const unsigned char* ptr;
    unsigned long long len;
} MasBytes;

/**
 * 记录统一使用目标平台的自然对齐，禁止 `#pragma pack`，否则会破坏两侧的共享布局。
 * 字段顺序即内存偏移顺序：`ok` 占 1 字节，`code` 从 8 字节偏移开始。
 */
typedef struct MasIoStatus {
    uint8_t ok;
    int64_t code;
} MasIoStatus;

/** 原生文件表项的公开句柄载荷，对应 Mas 侧 `pub struct File { token: int }`。 */
typedef struct MasIoFile {
    int64_t token;
} MasIoFile;

/** `open`/`std` 的结果；成功时 `value` 指向新分配的托管句柄，失败为 NULL。 */
typedef struct MasIoOpenResult {
    uint8_t ok;
    int64_t code;
    MasIoFile* value;
} MasIoOpenResult;

/** 整文件读取的字节结果；失败时 `value` 是有效空字节。 */
typedef struct MasIoBytesResult {
    uint8_t ok;
    int64_t code;
    MasBytes value;
} MasIoBytesResult;

/** 整文件读取的文本结果；失败时 `value` 是有效空串。 */
typedef struct MasIoTextResult {
    uint8_t ok;
    int64_t code;
    MasString value;
} MasIoTextResult;

/** 分块读取结果：`eof` 只在没有任何数据交付且宿主报告末尾时为 1。 */
typedef struct MasIoReadResult {
    uint8_t ok;
    int64_t code;
    MasBytes value;
    uint8_t eof;
} MasIoReadResult;

/** 读行结果：`eof` 只在没有更多行时为 1，最后一段无换行文本仍按普通行交付。 */
typedef struct MasIoLineResult {
    uint8_t ok;
    int64_t code;
    MasString value;
    uint8_t eof;
} MasIoLineResult;

/** 写入结果：失败时 `written` 保留宿主已接受的字节数。 */
typedef struct MasIoWriteResult {
    uint8_t ok;
    int64_t code;
    int64_t written;
} MasIoWriteResult;

/** 定位结果：成功时 `position` 为从文件开头计数的绝对位置。 */
typedef struct MasIoSeekResult {
    uint8_t ok;
    int64_t code;
    int64_t position;
} MasIoSeekResult;

/** 元数据结果：`size` 只对普通文件非 NULL，指向托管 `int64` 盒子。 */
typedef struct MasIoMetadataResult {
    uint8_t ok;
    int64_t code;
    int64_t kind;
    int64_t* size;
} MasIoMetadataResult;

/** 目录迭代器打开结果；`handle` 是进程内令牌，0 表示无效。 */
typedef struct MasIoDirOpenResult {
    uint8_t ok;
    int64_t code;
    int64_t handle;
} MasIoDirOpenResult;

/** 目录迭代结果：`done` 为 1 表示正常结束且名字为空串。 */
typedef struct MasIoDirEntryResult {
    uint8_t ok;
    int64_t code;
    uint8_t done;
    MasString name;
} MasIoDirEntryResult;

/*
 * 输入盒：Mas 侧用 `pub struct` 声明同样的字段顺序，按指针传入。
 * 常见标量按值传递的 extern 参数在 Win64 上不可靠，因此所有入口统一使用记录指针。
 */

/** `open(path, mode)` 的输入盒。 */
typedef struct MasIoOpenArg {
    MasString path;
    int64_t mode;
} MasIoOpenArg;

/** `mas_io_std(which)` 的输入盒，`which` 取 `MAS_IO_STD*`。 */
typedef struct MasIoStdArg {
    int64_t which;
} MasIoStdArg;

/** 只携带文件令牌的输入盒，用于 `read_line`、`flush`、`close`。 */
typedef struct MasIoTokenArg {
    int64_t token;
} MasIoTokenArg;

/** 只携带目录迭代器句柄的输入盒。 */
typedef struct MasIoHandleArg {
    int64_t handle;
} MasIoHandleArg;

/** `read(token, count)` 的输入盒。 */
typedef struct MasIoReadArg {
    int64_t token;
    int64_t count;
} MasIoReadArg;

/** `write(token, data)` 的输入盒。 */
typedef struct MasIoWriteArg {
    int64_t token;
    MasBytes data;
} MasIoWriteArg;

/** `seek(token, offset, origin)` 的输入盒，`origin` 取 `MAS_IO_SEEK_*`。 */
typedef struct MasIoSeekArg {
    int64_t token;
    int64_t offset;
    int64_t origin;
} MasIoSeekArg;

/** 只携带路径的输入盒。 */
typedef struct MasIoPathArg {
    MasString path;
} MasIoPathArg;

/** 携带路径与字节数据的输入盒，用于整文件写入。 */
typedef struct MasIoPathDataArg {
    MasString path;
    MasBytes data;
} MasIoPathDataArg;

/** 携带源与目标路径的输入盒，用于 `rename`。 */
typedef struct MasIoRenameArg {
    MasString source;
    MasString destination;
} MasIoRenameArg;

/*
 * 运行时导出：由 `crates/maslang` 的引导运行时提供，包内只声明不实现。
 */

/** 复制一段 UTF-8 字节并返回新的托管字符串。 */
MasString mas_runtime_string_clone(const unsigned char* ptr, unsigned long long len);

/** 复制一段任意字节并返回新的托管字节。 */
MasBytes mas_runtime_bytes_clone(const unsigned char* ptr, unsigned long long len);

/**
 * 分配八字节对齐的零初始化托管记录；`offsets` 列出载荷中每个托管指针的字节偏移，
 * `count` 为零时 `offsets` 可为 `NULL`。
 */
void* mas_record_alloc(unsigned long long size, const unsigned long long* offsets, unsigned long long count);

/** 按统一的中文运行时错误格式终止进程；`message` 必须是以零结尾的字符串。 */
void mas_runtime_abort(const char* message);

/**
 * 标准流桥：由运行时实现，原生层只调用。
 *
 * 读：返回 `>= 0` 为字节数，`< 0` 为 `-错误码`（错误码取值与 `MAS_IO_ERROR_*` 一致）。
 * 写（ABI 1，保留兼容）：`mas_runtime_std_write` 成功时返回已接受字节数，失败返回负值错误码。
 * 写（ABI 2）：`mas_runtime_std_write_v2` 返回 `0` 或负值错误码，并通过 `accepted`
 * 回报宿主已接受的字节数——即使失败也可能已经写出前缀，调用方必须使用这个值。
 */
long long mas_runtime_std_read(unsigned char* ptr, unsigned long long capacity);
long long mas_runtime_std_write(unsigned long long which, const unsigned char* ptr, unsigned long long len);
long long mas_runtime_std_write_v2(
    unsigned long long which,
    const unsigned char* ptr,
    unsigned long long len,
    unsigned long long* accepted
);
long long mas_runtime_std_flush(unsigned long long which);

/*
 * 公开原生入口：全部先校验指针，失败只写结果结构，绝不终止进程。
 */

/** 打开常规文件并登记令牌。 */
void mas_io_open(MasIoOpenArg* in, MasIoOpenResult* out);
/** 借用标准流句柄（令牌 1..3），不登记新表项。 */
void mas_io_std(MasIoStdArg* in, MasIoOpenResult* out);
/** 读取最多 `count` 字节，允许短读。 */
void mas_io_read(MasIoReadArg* in, MasIoReadResult* out);
/** 读取一行文本，与 `mas_io_read` 共用同一逻辑位置和缓冲。 */
void mas_io_read_line(MasIoTokenArg* in, MasIoLineResult* out);
/** 循环写入完整数据，失败时回报已接受字节数。 */
void mas_io_write(MasIoWriteArg* in, MasIoWriteResult* out);
/** 刷新输出缓冲；标准流走运行时桥。 */
void mas_io_flush(MasIoTokenArg* in, MasIoStatus* out);
/** 关闭句柄；即使失败也使令牌失效。 */
void mas_io_close(MasIoTokenArg* in, MasIoStatus* out);
/** 按起点、当前位置或末尾定位并返回绝对位置。 */
void mas_io_seek(MasIoSeekArg* in, MasIoSeekResult* out);
/** 读取完整文件的字节内容。 */
void mas_io_read_file(MasIoPathArg* in, MasIoBytesResult* out);
/** 读取完整文件并严格校验 UTF-8。 */
void mas_io_read_text(MasIoPathArg* in, MasIoTextResult* out);
/** 创建或覆盖文件后写入整段数据。 */
void mas_io_write_file(MasIoPathDataArg* in, MasIoWriteResult* out);
/** 创建或追加文件后写入整段数据。 */
void mas_io_append_file(MasIoPathDataArg* in, MasIoWriteResult* out);
/** 跟随符号链接查询类型与普通文件大小。 */
void mas_io_metadata(MasIoPathArg* in, MasIoMetadataResult* out);
/** 打开目录迭代器。 */
void mas_io_dir_open(MasIoPathArg* in, MasIoDirOpenResult* out);
/** 取下一个直接子项名称。 */
void mas_io_dir_next(MasIoHandleArg* in, MasIoDirEntryResult* out);
/** 关闭目录迭代器。 */
void mas_io_dir_close(MasIoHandleArg* in, MasIoStatus* out);
/** 创建一级目录。 */
void mas_io_make_dir(MasIoPathArg* in, MasIoStatus* out);
/** 同一文件系统内移动且禁止覆盖。 */
void mas_io_rename(MasIoRenameArg* in, MasIoStatus* out);
/** 删除文件或指向文件的符号链接。 */
void mas_io_remove_file(MasIoPathArg* in, MasIoStatus* out);
/** 删除空目录，拒绝符号链接与 Windows 目录重解析点。 */
void mas_io_remove_dir(MasIoPathArg* in, MasIoStatus* out);

#endif
