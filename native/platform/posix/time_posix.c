#include "time_host.h"

#ifndef _WIN32

#include <errno.h>
#include <string.h>

/**
 * 使用 `timespec_get(TIME_UTC)` 读取当前 UTC 墙上时钟，失败时直接报告中文错误。
 */
struct timespec mas_time_now_utc(void) {
    struct timespec result;
    if (timespec_get(&result, TIME_UTC) != TIME_UTC) {
        mas_runtime_abort("无法读取当前系统时间：宿主时钟读取失败");
    }
    return result;
}

/**
 * 使用 POSIX 单调时钟返回毫秒计数值，保证同进程差值计算不会倒退。
 */
long long mas_time_platform_monotonic_millis(void) {
    struct timespec monotonic_time;
    if (clock_gettime(CLOCK_MONOTONIC, &monotonic_time) != 0) {
        mas_runtime_abort("无法读取单调时钟");
    }
    return ((long long)monotonic_time.tv_sec * 1000LL)
        + ((long long)monotonic_time.tv_nsec / 1000000LL);
}

/**
 * 使用 `nanosleep` 实现至少等待指定毫秒数，并在中断后继续补足剩余时间。
 */
void mas_time_platform_sleep(long long ms) {
    struct timespec request;
    struct timespec remaining;

    if (ms < 0) {
        mas_runtime_abort("sleep(ms) 只接受大于等于 0 的毫秒整数");
    }

    request.tv_sec = (time_t)(ms / 1000LL);
    request.tv_nsec = (long)((ms % 1000LL) * 1000000L);
    while (nanosleep(&request, &remaining) != 0) {
        if (errno != EINTR) {
            mas_runtime_abort("阻塞等待失败");
        }
        request = remaining;
    }
}

#endif
