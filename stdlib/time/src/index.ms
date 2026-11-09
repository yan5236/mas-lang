// 统一导出 time 标准库的全部公开 API，保证调用方只需要 `import time { ... }`。
// 结果结构与 `try_*` 入口一起导出，调用方无需再单独导入私有绑定层。
export import local.native { TimeStatus, TimeIntResult, TimeStringResult }
// 墙上时钟读取与格式化：严格接口与可恢复 `try_*` 接口同源，失败契约由结果结构表达。
export import local.system {
    unix_seconds,
    unix_millis,
    now_utc,
    now_email_date,
    now_http_date,
    try_unix_seconds,
    try_unix_millis,
    try_now_utc,
    try_now_email_date,
    try_now_http_date,
}
// 单独导出计时与阻塞等待能力，保持墙上时钟与单调时钟职责分离。
export import local.timer { monotonic_millis, sleep, try_monotonic_millis, try_sleep }
