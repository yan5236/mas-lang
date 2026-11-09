#include "time_host.h"

#ifndef _WIN32

#include <errno.h>

/**
 * 使用 `timespec_get(TIME_UTC)` 读取当前 UTC 墙上时钟；读取失败只返回错误码，不终止进程。
 */
int mas_time_platform_read_utc(struct timespec* output) {
    if (output == NULL) {
        return MAS_TIME_ERROR_CLOCK_READ;
    }
    if (timespec_get(output, TIME_UTC) != TIME_UTC) {
        return MAS_TIME_ERROR_CLOCK_READ;
    }
    return MAS_TIME_OK;
}

/**
 * 使用 POSIX 单调时钟返回毫秒计数值，保证同进程差值计算不会倒退。
 */
int mas_time_platform_monotonic_millis(int64_t* output) {
    struct timespec monotonic_time;

    if (output == NULL) {
        return MAS_TIME_ERROR_MONOTONIC_READ;
    }
    if (clock_gettime(CLOCK_MONOTONIC, &monotonic_time) != 0) {
        return MAS_TIME_ERROR_MONOTONIC_READ;
    }

    *output = ((int64_t)monotonic_time.tv_sec * 1000LL)
        + ((int64_t)monotonic_time.tv_nsec / 1000000LL);
    return MAS_TIME_OK;
}

/**
 * 使用 `nanosleep` 实现至少等待指定毫秒数。
 * 被 `EINTR` 中断后继续等待剩余时间；其他 errno 失败返回 `code = 6`，由调用方决定如何处理。
 */
int mas_time_platform_wait_ms(int64_t ms) {
    struct timespec request;
    struct timespec remaining;

    if (ms <= 0) {
        return MAS_TIME_OK;
    }

    request.tv_sec = (time_t)(ms / 1000LL);
    request.tv_nsec = (long)((ms % 1000LL) * 1000000L);
    while (nanosleep(&request, &remaining) != 0) {
        if (errno != EINTR) {
            return MAS_TIME_ERROR_SLEEP_FAILED;
        }
        request = remaining;
    }
    return MAS_TIME_OK;
}

#endif
