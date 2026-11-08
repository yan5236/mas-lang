#ifndef MAS_TIME_HOST_H
#define MAS_TIME_HOST_H

#include <stddef.h>
#include <stdint.h>
#include <time.h>

#ifdef _WIN32
#include <windows.h>
#endif

/**
 * 表示 Mas lang 运行时与原生模块共享的字符串 ABI，使用 `ptr + len` 传递 UTF-8 文本。
 */
typedef struct MsString {
    const unsigned char* ptr;
    unsigned long long len;
} MsString;

/**
 * 复制一段 UTF-8 字节并返回新的运行时字符串，供原生模块稳定返回 `string`。
 */
MsString mas_runtime_string_clone(const unsigned char* ptr, unsigned long long len);

/**
 * 按统一的中文运行时错误格式终止当前进程，供原生模块直接上报宿主错误。
 */
void mas_runtime_abort(const char* message);

/**
 * 把秒级时间戳转换为 UTC 的 `struct tm`，失败时返回 `0`，成功时返回 `1`。
 */
int mas_time_fill_utc_tm(time_t seconds, struct tm* output);

/**
 * 把字节缓冲区复制成运行时字符串，内部统一走 Mas lang 运行时提供的字符串分配函数。
 */
MsString mas_time_make_string(const char* text, size_t len);

/**
 * 读取当前 UTC 墙上时钟并返回 `timespec`，失败时直接给出中文运行时错误。
 */
struct timespec mas_time_now_utc(void);

/**
 * 返回当前进程的单调毫秒计数值，供 `monotonic_millis()` 公开 API 调用。
 */
long long mas_time_platform_monotonic_millis(void);

/**
 * 阻塞当前线程至少指定毫秒数，负数时必须直接给出中文运行时错误。
 */
void mas_time_platform_sleep(long long ms);

#endif
