// 统一导出 time 标准库的全部公开 API，保证调用方只需要 `import time { ... }`。
export import local.system { unix_seconds, unix_millis, now_utc, now_email_date, now_http_date }
// 单独导出计时与阻塞等待能力，保持墙上时钟与单调时钟职责分离。
export import local.timer { monotonic_millis, sleep }
