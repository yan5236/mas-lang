#ifndef MAS_IO_INTERNAL_H
#define MAS_IO_INTERNAL_H

#include "io_host.h"

/**
 * 读缓冲的初始容量：每个常规文件句柄一份，按块读取宿主数据。
 * 行读取只在需要时扩容本行累加器，不改变这个块大小。
 */
#define MAS_IO_READ_BUFFER_SIZE 8192

/*
 * 流状态（`io_state.c` 定义，`io_stream.c` 使用）。
 * 这部分不属于公开 ABI：Mas 侧只持有令牌，表项布局随时可以调整。
 */

/**
 * 表示一个受检查的文件表项：常规文件持有平台句柄和读缓冲，标准流只持有编号。
 * `position` 是下一个逻辑读/写字节的绝对位置，读缓冲里的预读数据不计入其中，
 * 因此读转写和 `seek(Current)` 都能回到调用方看到的位置。
 */
typedef struct MasIoEntry {
    int64_t token;
    int64_t mode;
    int is_std;
    int64_t std_kind;
    void* file;
    unsigned char* read_buffer;
    size_t buffer_offset;
    size_t buffer_length;
    int64_t position;
    int eof_reached;
    int64_t pending_error;
} MasIoEntry;

/** 按令牌查找表项；标准流令牌单独处理，未知令牌返回 NULL（对应 `Closed`）。 */
MasIoEntry* mas_io_find_entry(int64_t token);

/** 判断表项是否可读：只有 `Read` 与 `ReadWrite` 带读方向。 */
int mas_io_entry_readable(const MasIoEntry* entry);

/** 判断表项是否可写：`Write`、`Append` 与 `ReadWrite` 带写方向。 */
int mas_io_entry_writable(const MasIoEntry* entry);

/** 缓冲区中尚未交付给调用方的字节数。 */
size_t mas_io_entry_available(const MasIoEntry* entry);

/** 丢弃读缓冲里的预读数据，不触碰宿主游标。 */
void mas_io_entry_drop_buffer(MasIoEntry* entry);

/** 把令牌插入动态表；表增长或令牌溢出失败时返回错误码。 */
int64_t mas_io_register_entry(MasIoEntry* entry);

/** 从动态表移除表项；令牌随即失效，再次访问会返回 `Closed`。 */
void mas_io_unregister_entry(MasIoEntry* entry);

/** 读缓冲为空时向宿主补充一块数据；宿主返回 0 字节表示到达末尾。 */
int64_t mas_io_entry_fill(MasIoEntry* entry);

/** 写之前的模式切换：追加落点由宿主保证，读写模式把底层游标同步到逻辑位置。 */
int64_t mas_io_entry_sync_before_write(MasIoEntry* entry);

/** 向宿主写入一次；标准流走运行时桥，常规文件走平台层。 */
int64_t mas_io_host_write(
    MasIoEntry* entry,
    const unsigned char* data,
    size_t len,
    size_t* written_out
);

/*
 * 平台层基础工具：每个目标平台各有一份实现，平台内的文件系统与流式文件复用。
 * Windows 与 POSIX 的声明都在这里，实际构建只为当前目标链接其中一份。
 */

/** Windows：宿主 Win32 错误 → `MAS_IO_ERROR_*`，整个 Windows 层唯一映射点。 */
int64_t mas_io_win_map(unsigned long error);

/** Windows：UTF-8 路径 → 以零结尾的宽字符路径。 */
int64_t mas_io_win_wide(const unsigned char* path, size_t path_len, wchar_t** out);

/** Windows：宽字符文件名 → UTF-8，长度不含终止零。 */
int64_t mas_io_win_utf8(const wchar_t* text, unsigned char** out, size_t* out_len);

/** POSIX：宿主 errno → `MAS_IO_ERROR_*`，整个 POSIX 层唯一映射点。 */
int64_t mas_io_posix_map(int error);

/** POSIX：UTF-8 路径 → 以零结尾的字节串。 */
int64_t mas_io_posix_path(const unsigned char* path, size_t path_len, char** out);

/*
 * 平台适配层内部契约。
 * 所有函数：成功返回 `MAS_IO_OK`，失败返回 `MAS_IO_ERROR_*`，绝不终止进程。
 * 路径参数已经过 `mas_io_validate_path` 校验（非空、无内嵌 NUL、合法 UTF-8），
 * 平台层仍需按宿主规则完成自己的转换与错误映射。
 */

/** 打开常规文件，成功时把平台句柄写入 `*file`。 */
int64_t mas_io_platform_open(const unsigned char* path, size_t path_len, int64_t mode, void** file);

/** 从当前位置读取最多 `capacity` 字节；`*read_out` 为 0 表示宿主到达末尾。 */
int64_t mas_io_platform_read(void* file, unsigned char* buffer, size_t capacity, size_t* read_out);

/** 从当前位置写入，`*written_out` 为宿主本次已接受的字节数（可能小于 `len`）。 */
int64_t mas_io_platform_write(void* file, const unsigned char* data, size_t len, size_t* written_out);

/** 把实现层的写缓冲刷到宿主；没有用户态写缓冲时直接成功。 */
int64_t mas_io_platform_flush(void* file);

/** 按 `origin`（0=Start 1=Current 2=End）定位并返回从开头计数的绝对位置。 */
int64_t mas_io_platform_seek(void* file, int64_t offset, int64_t origin, int64_t* position_out);

/**
 * 查询文件当前字节大小，不移动宿主游标（用于 End 起点定位，避免失败时游标失步）。
 * 超出有符号 64 位范围返回 `TooLarge`。
 */
int64_t mas_io_platform_size(void* file, int64_t* size_out);

/** 关闭并释放平台句柄；即使返回错误，句柄也不可再用。 */
int64_t mas_io_platform_close(void* file);

/** 跟随符号链接查询元数据；`*has_size` 只对普通文件为 1。 */
int64_t mas_io_platform_metadata(
    const unsigned char* path,
    size_t path_len,
    int64_t* kind,
    int64_t* size,
    int* has_size
);

/** 打开目录迭代器，平台句柄写入 `*dir`。 */
int64_t mas_io_platform_dir_open(const unsigned char* path, size_t path_len, void** dir);

/**
 * 取下一个直接子项名称。`*done` 为 1 表示正常结束；
 * 否则 `*name` 由 `malloc` 分配、调用方负责 `free`，`*name_len` 为字节数。
 */
int64_t mas_io_platform_dir_next(void* dir, int* done, unsigned char** name, size_t* name_len);

/** 关闭并释放目录迭代器。 */
int64_t mas_io_platform_dir_close(void* dir);

/** 创建一级目录。 */
int64_t mas_io_platform_make_dir(const unsigned char* path, size_t path_len);

/** 同一文件系统内移动且禁止覆盖。 */
int64_t mas_io_platform_rename(
    const unsigned char* source,
    size_t source_len,
    const unsigned char* destination,
    size_t destination_len
);

/** 删除文件或指向文件的符号链接。 */
int64_t mas_io_platform_remove_file(const unsigned char* path, size_t path_len);

/** 删除空目录，拒绝符号链接与 Windows 目录重解析点。 */
int64_t mas_io_platform_remove_dir(const unsigned char* path, size_t path_len);

/*
 * 包内共享工具，由 `io_common.c` 实现，流层、文件系统层与平台层共用。
 */

/** 严格校验 UTF-8（拒绝过长编码、代理区与超出 U+10FFFF 的值），合法返回 1。 */
int mas_io_utf8_is_valid(const unsigned char* ptr, size_t len);

/**
 * 校验输入路径：非空、无内嵌 NUL、合法 UTF-8。
 * 成功返回 `MAS_IO_OK` 并写入字节长度；失败返回对应错误码。
 */
int64_t mas_io_validate_path(const unsigned char* ptr, unsigned long long len, size_t* out_len);

/** 把字节复制成托管字符串；`ptr` 为 NULL 且 `len` 为 0 时返回有效空串。 */
MasString mas_io_make_string(const unsigned char* ptr, size_t len);

/** 把字节复制成托管字节；`ptr` 为 NULL 且 `len` 为 0 时返回有效空值。 */
MasBytes mas_io_make_bytes(const unsigned char* ptr, size_t len);

/** 在托管记录中分配一个 `int64` 盒子并写入值；分配失败返回 NULL。 */
int64_t* mas_io_make_i64(int64_t value);

/** 带溢出检查的 `int64` 加法；溢出返回 0，成功写入 `*out` 并返回 1。 */
int mas_io_add_i64(int64_t left, int64_t right, int64_t* out);

/**
 * 有界扩容追加：把 `extra_len` 字节追加到 `*data`，容量按倍增收敛到实际需要。
 * 长度溢出或分配失败返回 0，`*data`/`*length` 保持调用前状态。
 */
int mas_io_buffer_append(
    unsigned char** data,
    size_t* length,
    size_t* capacity,
    const unsigned char* extra,
    size_t extra_len
);

/** 把运行时桥返回的负值错误码收敛到 `MAS_IO_ERROR_*` 范围，未知归 `Other`。 */
int64_t mas_io_normalize_code(int64_t code);

/*
 * 结果结构写入：集中维护 ABI 字段，失败时写入契约规定的无害占位值。
 */

/** 只写状态：`ok` 由错误码是否为 0 决定。 */
void mas_io_store_status(MasIoStatus* out, int64_t code);

/** 打开失败：`value` 为 NULL。 */
void mas_io_store_open_failure(MasIoOpenResult* out, int64_t code);

/** 字节结果：失败时写有效空字节。 */
void mas_io_store_bytes_result(MasIoBytesResult* out, int64_t code, const unsigned char* data, size_t len);

/** 文本结果：失败时写有效空串。 */
void mas_io_store_text_result(MasIoTextResult* out, int64_t code, const unsigned char* data, size_t len);

/** 分块读取结果：失败时 `eof` 为 0。 */
void mas_io_store_read_result(
    MasIoReadResult* out,
    int64_t code,
    const unsigned char* data,
    size_t len,
    int eof
);

/** 读行结果：失败时 `eof` 为 0。 */
void mas_io_store_line_result(
    MasIoLineResult* out,
    int64_t code,
    const unsigned char* data,
    size_t len,
    int eof
);

/** 写入结果：保留已接受字节数。 */
void mas_io_store_write_result(MasIoWriteResult* out, int64_t code, int64_t written);

/** 定位结果：失败时位置为 0。 */
void mas_io_store_seek_result(MasIoSeekResult* out, int64_t code, int64_t position);

/** 元数据结果：失败时类型为 `Other` 且大小为 NULL。 */
void mas_io_store_metadata_result(MasIoMetadataResult* out, int64_t code, int64_t kind, int64_t* size_box);

/** 目录打开结果：失败时句柄为 0。 */
void mas_io_store_dir_open_result(MasIoDirOpenResult* out, int64_t code, int64_t handle);

/** 目录迭代结果：`done` 为 1 时名字为空串。 */
void mas_io_store_dir_entry_result(
    MasIoDirEntryResult* out,
    int64_t code,
    int done,
    const unsigned char* name,
    size_t len
);

/*
 * 整段入口的内部实现（`io_stream.c`），供整文件包装复用同一套打开/写入/关闭规则。
 */

/**
 * 打开并读完全部内容。成功时 `*data` 由 `malloc` 分配（调用方 `free`），
 * 返回首次错误（读取失败优先于关闭失败）；关闭错误单独写入 `*close_error`。
 */
int64_t mas_io_read_whole_file(
    const unsigned char* path,
    size_t path_len,
    unsigned char** data,
    size_t* size,
    int64_t* close_error
);

/**
 * 打开、写入并关闭。`mode` 取 `MAS_IO_MODE_WRITE` 或 `MAS_IO_MODE_APPEND`。
 * `*written` 为宿主已接受的字节数，`*close_error` 单独回报关闭错误；
 * 返回首次错误（写入失败优先于关闭失败）。
 */
int64_t mas_io_write_whole_file(
    const unsigned char* path,
    size_t path_len,
    int64_t mode,
    const unsigned char* data,
    size_t len,
    int64_t* written,
    int64_t* close_error
);

#endif
