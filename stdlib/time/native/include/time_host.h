#ifndef MAS_TIME_HOST_H
#define MAS_TIME_HOST_H

#include <stddef.h>
#include <stdint.h>
#include <time.h>

#ifdef _WIN32
#include <windows.h>
#endif

/**
 * time 包的原生 ABI 版本，必须与 `settings.toml` 的 `native.abi` 以及各目标
 * `manifest.toml` 中的 `abi` 字段保持一致。
 */
#define MAS_TIME_ABI_VERSION 2

/** 成功状态码：所有 `mas_time_read_*` 入口在成功时都填写该值。 */
#define MAS_TIME_OK 0
/** 宿主时钟读取失败，例如 POSIX 的 `timespec_get` 返回非 `TIME_UTC`。 */
#define MAS_TIME_ERROR_CLOCK_READ 1
/** 宿主时钟早于 Unix 纪元；只有 Windows 会返回，不能用负时间戳充当该状态。 */
#define MAS_TIME_ERROR_BEFORE_EPOCH 2
/** UTC 时间拆解失败，例如 `gmtime_r` / `_gmtime64_s` 返回错误。 */
#define MAS_TIME_ERROR_UTC_DECOMPOSE 3
/** 时间字符串格式化失败，例如 `snprintf` 返回值超出缓冲区容量。 */
#define MAS_TIME_ERROR_FORMAT 4
/** 负等待参数：`sleep(ms)` 只接受大于等于 0 的毫秒整数。 */
#define MAS_TIME_ERROR_NEGATIVE_SLEEP 5
/** 阻塞等待失败，例如 POSIX `nanosleep` 返回的不是 `EINTR`。 */
#define MAS_TIME_ERROR_SLEEP_FAILED 6
/** 单调时钟读取失败，例如 POSIX `clock_gettime(CLOCK_MONOTONIC)` 返回错误。 */
#define MAS_TIME_ERROR_MONOTONIC_READ 7

/**
 * 表示 Mas lang 运行时与原生模块共享的字符串 ABI，使用 `ptr + len` 传递 UTF-8 文本。
 * 该结构与运行时 `mas_runtime.h` 里的 `MasString` 同构，包内因此不必重复引入运行时头文件。
 */
typedef struct MsString {
    const unsigned char* ptr;
    unsigned long long len;
} MsString;

/** 实现契约中使用的字符串类型名，与 `MsString` 共用同一内存布局。 */
typedef MsString MasString;

/**
 * 只表达成功/失败的操作结果，对应 Mas 侧的 `pub struct TimeStatus { ok: bool; code: int }`。
 * 字段使用目标平台的自然对齐，不允许 `#pragma pack`，否则会破坏两侧的共享布局。
 */
typedef struct MasTimeStatus {
    uint8_t ok;
    int64_t code;
} MasTimeStatus;

/**
 * 携带整数值的结果，对应 Mas 侧的 `pub struct TimeIntResult`。
 * 失败时 `ok` 为 `0`，`code` 填写具体错误码，`value` 固定写回占位值 `0`。
 */
typedef struct MasTimeIntResult {
    uint8_t ok;
    int64_t code;
    int64_t value;
} MasTimeIntResult;

/**
 * 携带托管字符串的结果，对应 Mas 侧的 `pub struct TimeStringResult`。
 * 失败时 `ok` 为 `0`，`code` 填写具体错误码，`value` 固定写回空串。
 */
typedef struct MasTimeStringResult {
    uint8_t ok;
    int64_t code;
    MasString value;
} MasTimeStringResult;

/**
 * 复制一段 UTF-8 字节并返回新的运行时字符串，供原生模块稳定返回 `string`。
 */
MsString mas_runtime_string_clone(const unsigned char* ptr, unsigned long long len);

/**
 * 按统一的中文运行时错误格式终止当前进程，供原生模块直接上报宿主错误。
 */
void mas_runtime_abort(const char* message);

/**
 * 严格包装的失败出口：按错误码输出既有中文错误并以状态码 1 结束进程。
 * 只应由 `stdlib/time/src/*.ms` 的严格接口在 `ok == 0` 时调用。
 */
void mas_time_fail(int64_t code);

/**
 * 把秒级时间戳转换为 UTC 的 `struct tm`，失败时返回 `0`，成功时返回 `1`。
 */
int mas_time_fill_utc_tm(time_t seconds, struct tm* output);

/**
 * 把字节缓冲区复制成运行时字符串，内部统一走 Mas lang 运行时提供的字符串分配函数。
 */
MsString mas_time_make_string(const char* text, size_t len);

/**
 * 读取当前 UTC 秒级时间戳并填写结果结构；可预期的宿主失败只写状态码，不终止进程。
 */
void mas_time_read_now_unix(MasTimeIntResult* out);

/**
 * 读取当前 UTC 毫秒级时间戳并填写结果结构，纳秒部分按向下取整换算。
 */
void mas_time_read_now_unix_millis(MasTimeIntResult* out);

/**
 * 读取当前进程的单调毫秒计数值并填写结果结构，供 `monotonic_millis()` 与 `try_monotonic_millis()` 复用。
 */
void mas_time_read_monotonic_millis(MasTimeIntResult* out);

/**
 * 读取当前 UTC 时间并填写 ISO 8601 / RFC 3339 字符串结果，固定使用 `+00:00` 偏移。
 */
void mas_time_read_now_iso8601(MasTimeStringResult* out);

/**
 * 读取当前 UTC 时间并填写 RFC 2822 字符串结果，固定使用英文星期、月份与 `+0000` 偏移。
 */
void mas_time_read_now_rfc2822(MasTimeStringResult* out);

/**
 * 读取当前 UTC 时间并填写 RFC 7231 HTTP-date 字符串结果，固定使用 `GMT` 文本时区。
 */
void mas_time_read_now_rfc7231(MasTimeStringResult* out);

/**
 * 阻塞当前线程至少指定毫秒数并填写状态结果：负数写回 `code = 5`，零立即成功，其余由平台适配层等待。
 */
void mas_time_read_sleep(int64_t ms, MasTimeStatus* out);

/**
 * 平台适配层：读取当前 UTC 墙上时钟，成功返回 `MAS_TIME_OK`，否则返回对应错误码。
 */
int mas_time_platform_read_utc(struct timespec* output);

/**
 * 平台适配层：读取单调毫秒计数，成功返回 `MAS_TIME_OK`，否则返回对应错误码。
 */
int mas_time_platform_monotonic_millis(int64_t* output);

/**
 * 平台适配层：阻塞等待指定毫秒数，成功返回 `MAS_TIME_OK`，否则返回对应错误码。
 */
int mas_time_platform_wait_ms(int64_t ms);

#endif
