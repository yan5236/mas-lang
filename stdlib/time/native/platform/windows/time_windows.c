#include "time_host.h"

#include <stdio.h>

/**
 * 读取当前 UTC 墙上时钟，优先使用 Windows 系统时间 API 构造统一的 `timespec`。
 */
struct timespec mas_time_now_utc(void) {
    FILETIME file_time;
    ULARGE_INTEGER file_time_value;
    const unsigned long long epoch_offset = 116444736000000000ULL;
    const unsigned long long ticks_per_second = 10000000ULL;
    const unsigned long long ticks_per_nanosecond = 100ULL;
    unsigned long long unix_ticks;
    struct timespec result;

    GetSystemTimeAsFileTime(&file_time);
    file_time_value.LowPart = file_time.dwLowDateTime;
    file_time_value.HighPart = file_time.dwHighDateTime;

    if (file_time_value.QuadPart < epoch_offset) {
        mas_runtime_abort("无法读取当前系统时间：宿主时钟早于 Unix 纪元");
    }

    unix_ticks = file_time_value.QuadPart - epoch_offset;
    result.tv_sec = (time_t)(unix_ticks / ticks_per_second);
    result.tv_nsec = (long)((unix_ticks % ticks_per_second) * ticks_per_nanosecond);
    return result;
}

/**
 * 使用 `GetTickCount64` 提供单调毫秒计数值，避免把墙上时钟误用为耗时基准。
 */
long long mas_time_platform_monotonic_millis(void) {
    return (long long)GetTickCount64();
}

/**
 * 阻塞当前线程至少 ms 毫秒。按 Win32 的最大有限等待值分段，
 * 用高精度单调计数器补足提前唤醒的时间。负数或计时失败时终止程序并报告错误。
 */
void mas_time_platform_sleep(long long ms) {
    if (ms < 0) {
        mas_runtime_abort("sleep(ms) 只接受大于等于 0 的毫秒整数");
    }
    if (ms == 0) {
        return;
    }

    LARGE_INTEGER frequency;
    if (!QueryPerformanceFrequency(&frequency)) {
        mas_runtime_abort("无法读取单调时钟频率");
    }
    const DWORD max_slice = INFINITE - 1;
    while (ms > 0) {
        const DWORD slice = ms > (long long)max_slice ? max_slice : (DWORD)ms;
        LARGE_INTEGER started;
        LARGE_INTEGER now;
        if (!QueryPerformanceCounter(&started)) {
            mas_runtime_abort("无法读取单调时钟");
        }
        double remaining = slice;
        do {
            DWORD delay = (DWORD)remaining;
            if (delay < remaining) {
                ++delay;
            }
            Sleep(delay);
            if (!QueryPerformanceCounter(&now)) {
                mas_runtime_abort("无法读取单调时钟");
            }
            double elapsed = (double)(now.QuadPart - started.QuadPart) * 1000.0
                / (double)frequency.QuadPart;
            remaining = (double)slice - elapsed;
        } while (remaining > 0);
        ms -= (long long)slice;
    }
}
