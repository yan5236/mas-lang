// 表示只有成功/失败的时间操作结果，字段与仓库统一的 try_result 约定保持一致。
pub struct TimeStatus:
    ok: bool
    code: int

// 表示携带整数值的时间读取结果，失败时 `value` 固定为无害占位值 0。
pub struct TimeIntResult:
    ok: bool
    code: int
    value: int

// 表示携带托管字符串的时间读取结果，失败时 `value` 固定为空串。
pub struct TimeStringResult:
    ok: bool
    code: int
    value: string

// 声明 UTC 秒级时间读取入口；C 侧通过指针写回结果结构，失败只填写状态码。
pub extern func mas_time_read_now_unix(out: TimeIntResult)

// 声明 UTC 毫秒级时间读取入口，纳秒部分按向下取整换算为毫秒。
pub extern func mas_time_read_now_unix_millis(out: TimeIntResult)

// 声明单调毫秒计时读取入口，供耗时差值计算场景直接复用。
pub extern func mas_time_read_monotonic_millis(out: TimeIntResult)

// 声明 ISO 8601 UTC 时间字符串读取入口，成功时写回托管字符串。
pub extern func mas_time_read_now_iso8601(out: TimeStringResult)

// 声明 RFC 2822 UTC 时间字符串读取入口，成功时写回托管字符串。
pub extern func mas_time_read_now_rfc2822(out: TimeStringResult)

// 声明 RFC 7231 HTTP-date 字符串读取入口，成功时写回托管字符串。
pub extern func mas_time_read_now_rfc7231(out: TimeStringResult)

// 声明阻塞等待入口：负数写回 code=5，零立即成功，其余交给平台适配层。
pub extern func mas_time_read_sleep(ms:int, out: TimeStatus)

// 声明严格包装的失败出口：按错误码输出中文错误并以状态码 1 结束进程。
pub extern func mas_time_fail(code:int)
