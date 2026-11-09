import local.native {
    TimeIntResult,
    TimeStatus,
    mas_time_fail,
    mas_time_read_monotonic_millis,
    mas_time_read_sleep,
}

// 返回当前进程内可用于耗时差值计算的单调毫秒计数值，不和 Unix 时间戳混用。
pub func monotonic_millis() -> int:
    var result = TimeIntResult(false, 0, 0)
    mas_time_read_monotonic_millis(result)
    if not result.ok:
        mas_time_fail(result.code)
    return result.value

// 安全读取单调毫秒计数值：单调时钟读取失败时返回 `ok = false`，调用方可以继续执行。
pub func try_monotonic_millis() -> TimeIntResult:
    var result = TimeIntResult(false, 0, 0)
    mas_time_read_monotonic_millis(result)
    return result

// 阻塞当前线程至少指定毫秒数，负数会由宿主运行时给出明确中文错误。
pub func sleep(ms:int):
    var result = TimeStatus(false, 0)
    mas_time_read_sleep(ms, result)
    if not result.ok:
        mas_time_fail(result.code)

// 安全阻塞等待：负数或等待失败时返回 `ok = false` 和错误码，调用方可以继续执行。
pub func try_sleep(ms:int) -> TimeStatus:
    var result = TimeStatus(false, 0)
    mas_time_read_sleep(ms, result)
    return result
