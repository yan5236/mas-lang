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
 * 返回错误码对应的既有中文错误文本；未知错误码返回 `NULL`，由调用方拼接兜底文本。
 * 文本必须与改造前的严格接口保持一致，避免调用方的错误匹配逻辑失效。
 */
static const char* mas_time_error_text(int64_t code) {
    switch (code) {
    case MAS_TIME_ERROR_CLOCK_READ:
        return "无法读取当前系统时间：宿主时钟读取失败";
    case MAS_TIME_ERROR_BEFORE_EPOCH:
        return "无法读取当前系统时间：宿主时钟早于 Unix 纪元";
    case MAS_TIME_ERROR_UTC_DECOMPOSE:
        return "无法读取当前系统时间：UTC 时间拆解失败";
    case MAS_TIME_ERROR_FORMAT:
        return "无法格式化时间字符串";
    case MAS_TIME_ERROR_NEGATIVE_SLEEP:
        return "sleep(ms) 只接受大于等于 0 的毫秒整数";
    case MAS_TIME_ERROR_SLEEP_FAILED:
        return "阻塞等待失败";
    case MAS_TIME_ERROR_MONOTONIC_READ:
        return "无法读取单调时钟";
    default:
        return NULL;
    }
}

/**
 * 严格包装的统一失败出口：按错误码输出中文错误，并以状态码 1 结束进程。
 * `mas_runtime_abort` 负责统一的 `运行时错误: ...` 格式与退出码，因此输出与改造前完全一致。
 */
void mas_time_fail(int64_t code) {
    const char* text = mas_time_error_text(code);
    char buffer[96];

    if (text != NULL) {
        mas_runtime_abort(text);
    }

    snprintf(
        buffer,
        sizeof(buffer),
        "时间操作失败：未知错误码 %lld",
        (long long)code
    );
    mas_runtime_abort(buffer);
}

/**
 * 把宿主失败写进整型结果：`ok` 清零、填写错误码，并写回无害占位值 `0`。
 */
static void mas_time_store_int_failure(MasTimeIntResult* out, int64_t code) {
    out->ok = 0;
    out->code = code;
    out->value = 0;
}

/**
 * 把宿主失败写进字符串结果：`ok` 清零、填写错误码，并写回空串占位值。
 */
static void mas_time_store_string_failure(MasTimeStringResult* out, int64_t code) {
    out->ok = 0;
    out->code = code;
    out->value = mas_time_make_string("", 0);
}

/**
 * 把已经格式化好的文本写进字符串结果，表示本次读取完全成功。
 */
static void mas_time_store_string_success(MasTimeStringResult* out, const char* text, int written) {
    out->ok = 1;
    out->code = MAS_TIME_OK;
    out->value = mas_time_make_string(text, (size_t)written);
}

/**
 * 读取一次 UTC 墙上时钟，并把平台层返回的错误码收敛到契约声明的取值范围内。
 * Windows 的“时钟早于 Unix 纪元”单独保留，其余宿主读取失败统一归为 `code = 1`。
 */
static int64_t mas_time_read_utc(struct timespec* output) {
    int code = mas_time_platform_read_utc(output);
    if (code == MAS_TIME_OK) {
        return MAS_TIME_OK;
    }
    if (code == MAS_TIME_ERROR_BEFORE_EPOCH) {
        return MAS_TIME_ERROR_BEFORE_EPOCH;
    }
    return MAS_TIME_ERROR_CLOCK_READ;
}

/**
 * 取一次当前 UTC 时间并拆解成 `struct tm`，是三个格式化入口共用的前置步骤。
 */
static int64_t mas_time_prepare_now(struct tm* utc_time) {
    struct timespec current;
    int64_t code = mas_time_read_utc(&current);

    if (code != MAS_TIME_OK) {
        return code;
    }
    if (!mas_time_fill_utc_tm((time_t)current.tv_sec, utc_time)) {
        return MAS_TIME_ERROR_UTC_DECOMPOSE;
    }
    return MAS_TIME_OK;
}

/**
 * 校验拆解出的星期与月份落在英文缩写表范围内，避免越界索引。
 */
static int mas_time_utc_names_in_range(const struct tm* utc_time) {
    return utc_time->tm_wday >= 0 && utc_time->tm_wday <= 6
        && utc_time->tm_mon >= 0 && utc_time->tm_mon <= 11;
}

/**
 * 读取当前 UTC 秒级时间戳；宿主时钟读取失败时只写回状态码。
 */
void mas_time_read_now_unix(MasTimeIntResult* out) {
    struct timespec current;
    int64_t code;

    if (out == NULL) {
        return;
    }

    code = mas_time_read_utc(&current);
    if (code != MAS_TIME_OK) {
        mas_time_store_int_failure(out, code);
        return;
    }

    out->ok = 1;
    out->code = MAS_TIME_OK;
    out->value = (int64_t)current.tv_sec;
}

/**
 * 读取当前 UTC 毫秒级时间戳，纳秒部分按向下取整换算为毫秒。
 */
void mas_time_read_now_unix_millis(MasTimeIntResult* out) {
    struct timespec current;
    int64_t code;

    if (out == NULL) {
        return;
    }

    code = mas_time_read_utc(&current);
    if (code != MAS_TIME_OK) {
        mas_time_store_int_failure(out, code);
        return;
    }

    out->ok = 1;
    out->code = MAS_TIME_OK;
    out->value = ((int64_t)current.tv_sec * 1000LL) + ((int64_t)current.tv_nsec / 1000000LL);
}

/**
 * 读取当前进程的单调毫秒计数值；平台层失败时只写回状态码。
 */
void mas_time_read_monotonic_millis(MasTimeIntResult* out) {
    int64_t value = 0;
    int code;

    if (out == NULL) {
        return;
    }

    code = mas_time_platform_monotonic_millis(&value);
    if (code != MAS_TIME_OK) {
        mas_time_store_int_failure(out, MAS_TIME_ERROR_MONOTONIC_READ);
        return;
    }

    out->ok = 1;
    out->code = MAS_TIME_OK;
    out->value = value;
}

/**
 * 返回当前 UTC 时间的 ISO 8601 / RFC 3339 字符串，固定使用 `+00:00` 偏移表示。
 */
void mas_time_read_now_iso8601(MasTimeStringResult* out) {
    struct tm utc_time;
    char buffer[32];
    int written;
    int64_t code;

    if (out == NULL) {
        return;
    }

    code = mas_time_prepare_now(&utc_time);
    if (code != MAS_TIME_OK) {
        mas_time_store_string_failure(out, code);
        return;
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
        mas_time_store_string_failure(out, MAS_TIME_ERROR_FORMAT);
        return;
    }

    mas_time_store_string_success(out, buffer, written);
}

/**
 * 返回当前 UTC 时间的 RFC 2822 字符串，固定使用英文星期、月份与 `+0000` 偏移。
 */
void mas_time_read_now_rfc2822(MasTimeStringResult* out) {
    struct tm utc_time;
    char buffer[40];
    int written;
    int64_t code;

    if (out == NULL) {
        return;
    }

    code = mas_time_prepare_now(&utc_time);
    if (code != MAS_TIME_OK) {
        mas_time_store_string_failure(out, code);
        return;
    }
    if (!mas_time_utc_names_in_range(&utc_time)) {
        mas_time_store_string_failure(out, MAS_TIME_ERROR_UTC_DECOMPOSE);
        return;
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
        mas_time_store_string_failure(out, MAS_TIME_ERROR_FORMAT);
        return;
    }

    mas_time_store_string_success(out, buffer, written);
}

/**
 * 返回当前 UTC 时间的 RFC 7231 HTTP-date 字符串，固定使用 `GMT` 文本时区。
 */
void mas_time_read_now_rfc7231(MasTimeStringResult* out) {
    struct tm utc_time;
    char buffer[40];
    int written;
    int64_t code;

    if (out == NULL) {
        return;
    }

    code = mas_time_prepare_now(&utc_time);
    if (code != MAS_TIME_OK) {
        mas_time_store_string_failure(out, code);
        return;
    }
    if (!mas_time_utc_names_in_range(&utc_time)) {
        mas_time_store_string_failure(out, MAS_TIME_ERROR_UTC_DECOMPOSE);
        return;
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
        mas_time_store_string_failure(out, MAS_TIME_ERROR_FORMAT);
        return;
    }

    mas_time_store_string_success(out, buffer, written);
}

/**
 * 阻塞当前线程至少指定毫秒数：负数写回 `code = 5`，零立即成功，其余交给平台适配层。
 */
void mas_time_read_sleep(int64_t ms, MasTimeStatus* out) {
    int code;

    if (out == NULL) {
        return;
    }

    if (ms < 0) {
        out->ok = 0;
        out->code = MAS_TIME_ERROR_NEGATIVE_SLEEP;
        return;
    }

    if (ms == 0) {
        out->ok = 1;
        out->code = MAS_TIME_OK;
        return;
    }

    code = mas_time_platform_wait_ms(ms);
    out->ok = (uint8_t)(code == MAS_TIME_OK);
    out->code = code;
}
