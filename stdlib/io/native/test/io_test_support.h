#ifndef MAS_IO_TEST_SUPPORT_H
#define MAS_IO_TEST_SUPPORT_H

#include "io_host.h"

#include <stddef.h>

/*
 * 独立 C 测试驱动的共享声明。
 * 断言、临时目录、运行时桩和 API 包装分别实现在 `io_host_test.c` 与 `io_test_api.c`，
 * 测试用例按主题拆在 `io_host_test_stream.c`、`io_host_test_text.c`、
 * `io_host_test_std.c`、`io_host_test_fs.c` 与 `io_host_test_links.c`。
 */

/** 记录一次断言；失败时打印文件与行号并累计失败数。 */
void io_test_check(int condition, const char* file, int line, const char* message);

/** 记录一次跳过；宿主能力（例如创建符号链接的权限）不足时使用。 */
void io_test_skip(const char* file, int line, const char* message);

#define CHECK(condition, message) io_test_check((condition) ? 1 : 0, __FILE__, __LINE__, message)
#define SKIP(message) io_test_skip(__FILE__, __LINE__, message)

/** 测试临时目录下 `name` 的 UTF-8 绝对路径；缓冲区按轮转复用。 */
const char* io_test_path(const char* name);

/** 测试临时目录本身的 UTF-8 绝对路径。 */
const char* io_test_root_path(void);

/** 把一段 UTF-8 文本（通常是绝对路径）转换成宽字符，供宿主调用使用。 */
int io_test_wide_from_utf8(const char* text, wchar_t* out, size_t capacity);

/** 把测试临时目录下 
ame 拼成宽字符路径，供清理和宿主调用使用。 */
int io_test_wide_path(const char* name, wchar_t* out, size_t capacity);

/** 按 C 串长度构造 `MasString`。 */
MasString io_test_string(const char* text);

/** 比较载荷：长度一致且逐字节相等。 */
int io_test_bytes_equal(MasBytes value, const void* expected, size_t length);

/** 比较字符串载荷：长度一致且逐字节相等。 */
int io_test_string_equal(MasString value, const char* expected, size_t length);

/* 标准流桩的控制项：用于确定性触发 EOF、部分写入失败和读取失败。 */
extern unsigned long long g_stdout_bytes;
extern unsigned long long g_stderr_bytes;
extern unsigned long long g_stdout_flushes;
extern unsigned long long g_stderr_flushes;
extern int g_std_write_fail_code;
extern int g_std_write_partial_error;
extern long long g_std_write_limit;

/** 设置 stdin 桩的数据来源。 */
void io_test_stdin_set(const char* data, size_t length);

/** 让 stdin 桩在读取到 `offset` 字节之后返回可恢复错误。 */
void io_test_stdin_fail_after(size_t offset);

/**
 * 限制 stdin 桩每次宿主读取最多交付 `limit` 字节，模拟管道一次只有少量数据就绪。
 * `(size_t)-1` 表示不限制。
 */
void io_test_stdin_deliver(size_t limit);

/** 返回 stdin 桩被宿主读取的次数；用于断言拿到数据后不再请求更多。 */
unsigned long long io_test_stdin_read_calls(void);

/** 恢复标准流桩的默认状态。 */
void io_test_std_reset(void);

/* 公开 API 的薄包装，统一初始化输入盒和结果结构。 */
MasIoOpenResult io_test_open(const char* path, int64_t mode);
MasIoOpenResult io_test_open_std(int64_t which);
MasIoReadResult io_test_read(MasIoFile* file, int64_t count);
MasIoLineResult io_test_read_line(MasIoFile* file);
MasIoWriteResult io_test_write(MasIoFile* file, const void* data, size_t length);
MasIoStatus io_test_flush(MasIoFile* file);
MasIoStatus io_test_close(MasIoFile* file);
MasIoSeekResult io_test_seek(MasIoFile* file, int64_t offset, int64_t origin);

MasIoBytesResult io_test_read_file(const char* path);
MasIoTextResult io_test_read_text(const char* path);
MasIoWriteResult io_test_write_file(const char* path, const void* data, size_t length);
MasIoWriteResult io_test_append_file(const char* path, const void* data, size_t length);
MasIoMetadataResult io_test_metadata(const char* path);
MasIoDirOpenResult io_test_dir_open(const char* path);
MasIoDirEntryResult io_test_dir_next(int64_t handle);
MasIoStatus io_test_dir_close(int64_t handle);
MasIoStatus io_test_make_dir(const char* path);
MasIoStatus io_test_rename(const char* source, const char* destination);
MasIoStatus io_test_remove_file(const char* path);
MasIoStatus io_test_remove_dir(const char* path);

/** 读取并校验整文件内容，便于在多个用例里复用。 */
int io_test_file_equals(const char* path, const void* expected, size_t length);

/** 用整文件入口准备一份文本内容，返回是否成功。 */
int io_test_prepare_text(const char* path, const char* text);

/* 测试分组入口，由 `main` 依序调用。 */
void io_test_run_streams(void);
void io_test_run_std(void);
void io_test_run_text(void);
void io_test_run_fs(void);
void io_test_run_links(void);
void io_test_run_concurrency(void);

/** 子进程工作模式：向 `path` 追加 `iterations` 次、每次 `chunk` 字节；全部成功返回 0。 */
int io_test_run_append_worker(const char* path, int iterations, size_t chunk);

#endif
