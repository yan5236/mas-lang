// 声明 time 标准库依赖的宿主原生符号，供公开 API 包装层统一复用。
pub extern func mas_time_now_unix() -> int

// 声明 time 标准库的毫秒级 Unix 时间读取符号，返回值固定为 `int`。
pub extern func mas_time_now_unix_millis() -> int

// 声明 ISO 8601 UTC 时间字符串读取符号，返回值固定为 `string`。
pub extern func mas_time_now_iso8601() -> string

// 声明 RFC 2822 UTC 时间字符串读取符号，返回值固定为 `string`。
pub extern func mas_time_now_rfc2822() -> string

// 声明 RFC 7231 HTTP-date 时间字符串读取符号，返回值固定为 `string`。
pub extern func mas_time_now_rfc7231() -> string

// 声明单调毫秒计时符号，供耗时差值计算场景直接复用。
pub extern func mas_time_monotonic_millis() -> int

// 声明阻塞等待符号，宿主层会自行校验负数并返回中文错误。
pub extern func mas_time_sleep(ms:int)
