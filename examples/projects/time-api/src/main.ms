import time { monotonic_millis, now_utc, now_email_date, now_http_date, unix_seconds, unix_millis, sleep }

// 展示 time 标准库的全部最小公开 API，便于人工验收与回归验证。
func main():
    print("unix=" + string(unix_seconds()))
    print("unix_millis=" + string(unix_millis()))
    print("iso8601=" + now_utc())
    print("rfc2822=" + now_email_date())
    print("rfc7231=" + now_http_date())

    var started = monotonic_millis()
    sleep(20)
    var elapsed = monotonic_millis() - started
    print("elapsed_ms=" + string(elapsed))
