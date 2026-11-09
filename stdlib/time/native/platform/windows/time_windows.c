#include "time_host.h"

/**
 * 读取当前 UTC 墙上时钟，使用 Windows 系统时间 API 构造统一的 `timespec`。
 * 宿主时钟早于 Unix 纪元时返回 `code = 2`：这里必须给出明确的失败状态，
 * 而不是把负时间戳当作失败哨兵（负时间戳本身仍是合法取值）。
 */
int mas_time_platform_read_utc(struct timespec* output) {
    FILETIME file_time;
    ULARGE_INTEGER file_time_value;
    const unsigned long long epoch_offset = 116444736000000000ULL;
    const unsigned long long ticks_per_second = 10000000ULL;
    const unsigned long long ticks_per_nanosecond = 100ULL;
    unsigned long long unix_ticks;

    if (output == NULL) {
        return MAS_TIME_ERROR_CLOCK_READ;
    }

    GetSystemTimeAsFileTime(&file_time);
    file_time_value.LowPart = file_time.dwLowDateTime;
    file_time_value.HighPart = file_time.dwHighDateTime;

    if (file_time_value.QuadPart < epoch_offset) {
        return MAS_TIME_ERROR_BEFORE_EPOCH;
    }

    unix_ticks = file_time_value.QuadPart - epoch_offset;
    output->tv_sec = (time_t)(unix_ticks / ticks_per_second);
    output->tv_nsec = (long)((unix_ticks % ticks_per_second) * ticks_per_nanosecond);
    return MAS_TIME_OK;
}

/**
 * 使用 `GetTickCount64` 提供单调毫秒计数值，避免把墙上时钟误用为耗时基准。
 * 该 API 不会失败，因此这里始终返回成功；`code = 7` 由 POSIX 适配层实际使用。
 */
int mas_time_platform_monotonic_millis(int64_t* output) {
    if (output == NULL) {
        return MAS_TIME_ERROR_MONOTONIC_READ;
    }
    *output = (int64_t)GetTickCount64();
    return MAS_TIME_OK;
}

/**
 * 阻塞当前线程至少 ms 毫秒。按 Win32 的最大有限等待值分段，
 * 用高精度单调计数器补足提前唤醒的时间；计时失败返回 `code = 6`，不再终止进程。
 */
int mas_time_platform_wait_ms(int64_t ms) {
    LARGE_INTEGER frequency;
    const DWORD max_slice = INFINITE - 1;

    if (ms <= 0) {
        return MAS_TIME_OK;
    }

    if (!QueryPerformanceFrequency(&frequency) || frequency.QuadPart == 0) {
        return MAS_TIME_ERROR_SLEEP_FAILED;
    }

    while (ms > 0) {
        const DWORD slice = ms > (int64_t)max_slice ? max_slice : (DWORD)ms;
        LARGE_INTEGER started;
        LARGE_INTEGER now;
        double remaining;
        if (!QueryPerformanceCounter(&started)) {
            return MAS_TIME_ERROR_SLEEP_FAILED;
        }
        remaining = (double)slice;
        do {
            DWORD delay = (DWORD)remaining;
            double elapsed;
            if ((double)delay < remaining) {
                ++delay;
            }
            Sleep(delay);
            if (!QueryPerformanceCounter(&now)) {
                return MAS_TIME_ERROR_SLEEP_FAILED;
            }
            elapsed = (double)(now.QuadPart - started.QuadPart) * 1000.0
                / (double)frequency.QuadPart;
            remaining = (double)slice - elapsed;
        } while (remaining > 0);
        ms -= (int64_t)slice;
    }
    return MAS_TIME_OK;
}
