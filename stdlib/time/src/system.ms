import local.native {
    TimeIntResult,
    TimeStringResult,
    mas_time_fail,
    mas_time_read_now_iso8601,
    mas_time_read_now_rfc2822,
    mas_time_read_now_rfc7231,
    mas_time_read_now_unix,
    mas_time_read_now_unix_millis,
}

// 返回当前 UTC 时间对应的 Unix 秒级时间戳，适合日志、签名和时间窗口判断。
pub func unix_seconds() -> int:
    var result = TimeIntResult(false, 0, 0)
    mas_time_read_now_unix(result)
    if not result.ok:
        mas_time_fail(result.code)
    return result.value

// 返回当前 UTC 时间对应的 Unix 毫秒级时间戳，适合更细粒度的时间记录。
pub func unix_millis() -> int:
    var result = TimeIntResult(false, 0, 0)
    mas_time_read_now_unix_millis(result)
    if not result.ok:
        mas_time_fail(result.code)
    return result.value

// 安全读取 Unix 秒级时间戳：宿主失败时返回 `ok = false` 和错误码，调用方可以继续执行。
pub func try_unix_seconds() -> TimeIntResult:
    var result = TimeIntResult(false, 0, 0)
    mas_time_read_now_unix(result)
    return result

// 安全读取 Unix 毫秒级时间戳：宿主失败时返回 `ok = false` 和错误码，调用方可以继续执行。
pub func try_unix_millis() -> TimeIntResult:
    var result = TimeIntResult(false, 0, 0)
    mas_time_read_now_unix_millis(result)
    return result

// 返回符合 RFC 3339 / ISO 8601 的 UTC 时间字符串，固定使用 `+00:00` 偏移。
pub func now_utc() -> string:
    var result = TimeStringResult(false, 0, "")
    mas_time_read_now_iso8601(result)
    if not result.ok:
        mas_time_fail(result.code)
    return result.value

// 返回符合 RFC 2822 的 UTC 时间字符串，固定使用英文星期、月份和 `+0000` 偏移。
pub func now_email_date() -> string:
    var result = TimeStringResult(false, 0, "")
    mas_time_read_now_rfc2822(result)
    if not result.ok:
        mas_time_fail(result.code)
    return result.value

// 返回符合 RFC 7231 的 HTTP-date 字符串，固定使用 `GMT` 文本时区。
pub func now_http_date() -> string:
    var result = TimeStringResult(false, 0, "")
    mas_time_read_now_rfc7231(result)
    if not result.ok:
        mas_time_fail(result.code)
    return result.value

// 安全读取 ISO 8601 UTC 时间字符串：时钟读取或格式化失败时返回 `ok = false`。
pub func try_now_utc() -> TimeStringResult:
    var result = TimeStringResult(false, 0, "")
    mas_time_read_now_iso8601(result)
    return result

// 安全读取 RFC 2822 UTC 时间字符串：时钟读取或格式化失败时返回 `ok = false`。
pub func try_now_email_date() -> TimeStringResult:
    var result = TimeStringResult(false, 0, "")
    mas_time_read_now_rfc2822(result)
    return result

// 安全读取 RFC 7231 HTTP-date 时间字符串：时钟读取或格式化失败时返回 `ok = false`。
pub func try_now_http_date() -> TimeStringResult:
    var result = TimeStringResult(false, 0, "")
    mas_time_read_now_rfc7231(result)
    return result
