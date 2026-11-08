import local.native { mas_time_now_iso8601, mas_time_now_rfc2822, mas_time_now_rfc7231, mas_time_now_unix, mas_time_now_unix_millis }

// 返回当前 UTC 时间对应的 Unix 秒级时间戳，适合日志、签名和时间窗口判断。
pub func now_unix() -> int:
    return mas_time_now_unix()

// 返回当前 UTC 时间对应的 Unix 毫秒级时间戳，适合更细粒度的时间记录。
pub func now_unix_millis() -> int:
    return mas_time_now_unix_millis()

// 返回符合 RFC 3339 / ISO 8601 的 UTC 时间字符串，固定使用 `+00:00` 偏移。
pub func now_iso8601() -> string:
    return mas_time_now_iso8601()

// 返回符合 RFC 2822 的 UTC 时间字符串，固定使用英文星期、月份和 `+0000` 偏移。
pub func now_rfc2822() -> string:
    return mas_time_now_rfc2822()

// 返回符合 RFC 7231 的 HTTP-date 字符串，固定使用 `GMT` 文本时区。
pub func now_rfc7231() -> string:
    return mas_time_now_rfc7231()
