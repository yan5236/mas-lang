#include "time_host.h"

#include <stdio.h>
#include <string.h>

/**
 * 保存 RFC 与 HTTP-date 格式化复用的英文星期缩写，避免受宿主本地化环境影响。
 */
static const char* MAS_TIME_WEEKDAY_NAMES[7] = {
    "Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"
};

/**
 * 保存 RFC 与 HTTP-date 格式化复用的英文月份缩写，保证跨平台输出稳定一致。
 */
static const char* MAS_TIME_MONTH_NAMES[12] = {
    "Jan", "Feb", "Mar", "Apr", "May", "Jun",
    "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"
};

/**
 * 把普通字节缓冲区复制成运行时字符串，供所有字符串返回路径统一复用。
 */
MsString mas_time_make_string(const char* text, size_t len) {
    return mas_runtime_string_clone((const unsigned char*)text, (unsigned long long)len);
}

/**
 * 把秒级时间戳转换为 UTC 时间结构体，屏蔽 Windows 与 POSIX 上的线程安全 API 差异。
 */
int mas_time_fill_utc_tm(time_t seconds, struct tm* output) {
#ifdef _WIN32
    __time64_t wide_seconds = (__time64_t)seconds;
    return _gmtime64_s(output, &wide_seconds) == 0;
#else
    return gmtime_r(&seconds, output) != NULL;
#endif
}

/**
 * 返回当前 UTC 秒级时间戳，若宿主无法读取墙上时钟则直接输出中文运行时错误。
 */
long long mas_time_now_unix(void) {
    struct timespec current = mas_time_now_utc();
    return (long long)current.tv_sec;
}

/**
 * 返回当前 UTC 毫秒级时间戳，纳秒部分会按向下取整换算为毫秒。
 */
long long mas_time_now_unix_millis(void) {
    struct timespec current = mas_time_now_utc();
    return ((long long)current.tv_sec * 1000LL) + ((long long)current.tv_nsec / 1000000LL);
}

/**
 * 把内部字符串对象转换成原生 ABI 需要的 `ptr + out_len` 结果，避免 Windows 结构体返回兼容性问题。
 */
static const unsigned char* mas_time_string_result(MsString value, unsigned long long* out_len) {
    if (out_len == NULL) {
        mas_runtime_abort("原生字符串返回长度输出指针不能为空");
    }
    *out_len = value.len;
    return value.ptr;
}

/**
 * 返回当前 UTC 时间的 ISO 8601 / RFC 3339 字符串，固定使用 `+00:00` 偏移表示。
 */
const unsigned char* mas_time_now_iso8601(unsigned long long* out_len) {
    struct timespec current = mas_time_now_utc();
    struct tm utc_time;
    char buffer[32];
    int written;

    if (!mas_time_fill_utc_tm((time_t)current.tv_sec, &utc_time)) {
        mas_runtime_abort("无法读取当前系统时间：UTC 时间拆解失败");
    }

    written = snprintf(
        buffer,
        sizeof(buffer),
        "%04d-%02d-%02dT%02d:%02d:%02d+00:00",
        utc_time.tm_year + 1900,
        utc_time.tm_mon + 1,
        utc_time.tm_mday,
        utc_time.tm_hour,
        utc_time.tm_min,
        utc_time.tm_sec
    );
    if (written <= 0 || (size_t)written >= sizeof(buffer)) {
        mas_runtime_abort("无法格式化 ISO 8601 时间字符串");
    }

    return mas_time_string_result(mas_time_make_string(buffer, (size_t)written), out_len);
}

/**
 * 返回当前 UTC 时间的 RFC 2822 字符串，固定使用英文星期、月份与 `+0000` 偏移。
 */
const unsigned char* mas_time_now_rfc2822(unsigned long long* out_len) {
    struct timespec current = mas_time_now_utc();
    struct tm utc_time;
    char buffer[40];
    int written;

    if (!mas_time_fill_utc_tm((time_t)current.tv_sec, &utc_time)) {
        mas_runtime_abort("无法读取当前系统时间：UTC 时间拆解失败");
    }

    written = snprintf(
        buffer,
        sizeof(buffer),
        "%s, %02d %s %04d %02d:%02d:%02d +0000",
        MAS_TIME_WEEKDAY_NAMES[utc_time.tm_wday],
        utc_time.tm_mday,
        MAS_TIME_MONTH_NAMES[utc_time.tm_mon],
        utc_time.tm_year + 1900,
        utc_time.tm_hour,
        utc_time.tm_min,
        utc_time.tm_sec
    );
    if (written <= 0 || (size_t)written >= sizeof(buffer)) {
        mas_runtime_abort("无法格式化 RFC 2822 时间字符串");
    }

    return mas_time_string_result(mas_time_make_string(buffer, (size_t)written), out_len);
}

/**
 * 返回当前 UTC 时间的 RFC 7231 HTTP-date 字符串，固定使用 `GMT` 文本时区。
 */
const unsigned char* mas_time_now_rfc7231(unsigned long long* out_len) {
    struct timespec current = mas_time_now_utc();
    struct tm utc_time;
    char buffer[40];
    int written;

    if (!mas_time_fill_utc_tm((time_t)current.tv_sec, &utc_time)) {
        mas_runtime_abort("无法读取当前系统时间：UTC 时间拆解失败");
    }

    written = snprintf(
        buffer,
        sizeof(buffer),
        "%s, %02d %s %04d %02d:%02d:%02d GMT",
        MAS_TIME_WEEKDAY_NAMES[utc_time.tm_wday],
        utc_time.tm_mday,
        MAS_TIME_MONTH_NAMES[utc_time.tm_mon],
        utc_time.tm_year + 1900,
        utc_time.tm_hour,
        utc_time.tm_min,
        utc_time.tm_sec
    );
    if (written <= 0 || (size_t)written >= sizeof(buffer)) {
        mas_runtime_abort("无法格式化 RFC 7231 时间字符串");
    }

    return mas_time_string_result(mas_time_make_string(buffer, (size_t)written), out_len);
}

/**
 * 返回当前进程的单调毫秒计数值，具体时间源由平台适配层负责提供。
 */
long long mas_time_monotonic_millis(void) {
    return mas_time_platform_monotonic_millis();
}

/**
 * 阻塞当前线程至少指定毫秒数，平台差异由专属适配层处理。
 */
void mas_time_sleep(long long ms) {
    mas_time_platform_sleep(ms);
}
