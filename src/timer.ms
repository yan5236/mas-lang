// 返回当前进程内可用于耗时差值计算的单调毫秒计数值，不和 Unix 时间戳混用。
pub func monotonic_millis() -> int:
    return __time_monotonic_millis()

// 阻塞当前线程至少指定毫秒数，负数会由宿主运行时给出明确中文错误。
pub func sleep(ms:int):
    __time_sleep(ms)
