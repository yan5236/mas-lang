import time { monotonic_millis, now_iso8601, now_rfc2822, now_rfc7231, now_unix, now_unix_millis, sleep }

// 展示 time 标准库的全部最小公开 API，便于人工验收与回归验证。
func main():
    print("unix=" + string(now_unix()))
    print("unix_millis=" + string(now_unix_millis()))
    print("iso8601=" + now_iso8601())
    print("rfc2822=" + now_rfc2822())
    print("rfc7231=" + now_rfc7231())

    var started = monotonic_millis()
    sleep(20)
    var elapsed = monotonic_millis() - started
    print("elapsed_ms=" + string(elapsed))
