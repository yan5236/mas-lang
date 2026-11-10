// 公开的文件与标准流操作：每个入口只做参数校验、令牌取出和结果透传，
// 资源状态、缓冲与错误分类全部由原生层负责。

import local.codes { mode_code, seek_code }
import local.native {
    IOStatus,
    OpenResult,
    ReadResult,
    SeekResult,
    WriteResult,
    raw_close,
    raw_flush,
    raw_open,
    raw_read,
    raw_seek,
    raw_std,
    raw_write,
}
import local.types { File, FileMode, IOErrorKind, SeekFrom }

// 打开文件：Read 要求存在，Write 创建或截断，Append 创建并每次追加，ReadWrite 要求存在且不截断。
pub func open(path: string, mode: FileMode) -> OpenResult:
    return raw_open(path, mode_code(mode))

// 借用标准输入；宿主异常无法借用时返回无效句柄，后续操作会报 Closed。
pub func stdin() -> File:
    var opened = raw_std(0)
    if not opened.ok:
        return File(-1)
    return opened.value.value

// 借用标准输出；与 `print` 共用同一输出流，调用顺序保持一致。
pub func stdout() -> File:
    var opened = raw_std(1)
    if not opened.ok:
        return File(-1)
    return opened.value.value

// 借用标准错误。
pub func stderr() -> File:
    var opened = raw_std(2)
    if not opened.ok:
        return File(-1)
    return opened.value.value

// 最多读取 count 字节；正数请求允许短读，只有没有返回数据且已到末尾时 eof 才为真。
pub func read(file: File, count: int) -> ReadResult:
    return raw_read(file.token, count)

// 把整段字节写到当前逻辑位置；失败时 written 保留宿主已接受字节数。
pub func write(file: File, data: bytes) -> WriteResult:
    return raw_write(file.token, data)

// 严格验证 UTF-8 后写入文本，不添加换行；非法文本返回 InvalidUtf8 且不触碰文件。
pub func write_text_to(file: File, text: string) -> WriteResult:
    var encoded = encode_utf8(text)
    if encoded == null:
        return WriteResult(false, IOErrorKind.InvalidUtf8, 0)
    return raw_write(file.token, encoded.value)

// 刷新库缓冲；标准流会同时刷新宿主缓冲。
pub func flush(file: File) -> IOStatus:
    return raw_flush(file.token)

// 关闭常规文件；即使返回失败，令牌也已失效，其他文件保持可用。
// 标准流是借用句柄：返回 NotSupported，且句柄仍然可用。
pub func close(file: File) -> IOStatus:
    return raw_close(file.token)

// 按 Start/Current/End 计算字节位置，成功时返回从开头计数的绝对位置。
pub func seek(file: File, offset: int, origin: SeekFrom) -> SeekResult:
    return raw_seek(file.token, offset, seek_code(origin))
