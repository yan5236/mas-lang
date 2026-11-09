import time {
    monotonic_millis,
    now_utc,
    now_email_date,
    now_http_date,
    unix_seconds,
    unix_millis,
    sleep,
    try_sleep,
    try_unix_seconds,
    try_unix_millis,
    try_monotonic_millis,
    try_now_utc,
    try_now_email_date,
    try_now_http_date,
}

// 展示 time 标准库的全部最小公开 API，便于人工验收与回归验证。
// 严格接口保持“中文错误 + 退出码 1”的旧语义，可恢复入口把失败写进结果结构。
func main():
    // 严格接口：成功路径必须与 0.1.0 完全一致。
    print("unix=" + string(unix_seconds()))
    print("unix_millis=" + string(unix_millis()))
    print("iso8601=" + now_utc())
    print("rfc2822=" + now_email_date())
    print("rfc7231=" + now_http_date())

    var started = monotonic_millis()
    sleep(20)
    var elapsed = monotonic_millis() - started
    print("elapsed_ms=" + string(elapsed))

    // 负等待走错误分支：只写回 ok/code，程序继续执行后续语句。
    var negative = try_sleep(-1)
    print("sleep(-1) -> " + string(negative.ok))
    print("sleep(-1) code=" + string(negative.code))
    if negative.ok:
        print("sleep(-1) unreachable")
    print("after sleep(-1)")

    // 零等待立即成功，不做任何阻塞。
    var zero = try_sleep(0)
    print("sleep(0) -> " + string(zero.ok))
    print("sleep(0) code=" + string(zero.code))

    // 正常等待：ok 为 true，实际耗时略大于请求值。
    var waited = try_sleep(20)
    print("sleep(20) -> " + string(waited.ok))

    // 时间读取的可恢复入口：断言成功标记与基本取值范围。
    var seconds = try_unix_seconds()
    print("try_unix_seconds ok=" + string(seconds.ok))
    print("try_unix_seconds positive=" + string(seconds.value > 0))

    var millis = try_unix_millis()
    print("try_unix_millis ok=" + string(millis.ok))
    print("try_unix_millis covers_seconds=" + string(millis.value >= seconds.value * 1000))

    var monotonic = try_monotonic_millis()
    print("try_monotonic_millis ok=" + string(monotonic.ok))
    print("try_monotonic_millis non_negative=" + string(monotonic.value >= 0))

    var iso = try_now_utc()
    print("try_now_utc ok=" + string(iso.ok))
    print("try_now_utc has_date=" + string(iso.value.contains("-")))
    print("try_now_utc utc_offset=" + string(iso.value.ends_with("+00:00")))

    var email = try_now_email_date()
    print("try_now_email_date ok=" + string(email.ok))
    print("try_now_email_date has_weekday=" + string(email.value.contains(", ")))
    print("try_now_email_date utc_offset=" + string(email.value.ends_with("+0000")))

    var http = try_now_http_date()
    print("try_now_http_date ok=" + string(http.ok))
    print("try_now_http_date gmt=" + string(http.value.ends_with("GMT")))

    print("done")
