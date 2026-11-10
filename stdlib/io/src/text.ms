// 公开的文本与整文件操作：行读取共用句柄缓冲，整文件入口在打开前做严格编码校验。

import local.native {
    BytesResult,
    LineResult,
    TextResult,
    WriteResult,
    raw_append_file,
    raw_read_file,
    raw_read_text,
    raw_read_line,
    raw_write_file,
}
import local.types { File, IOErrorKind }

// 读取一行文本：读至 LF 或文件末尾，剥除 LF 和它前面紧邻的 CR，保留其他字符。
// 空行返回空串且 eof 为假；最后一段没有换行的文本作为一行返回。
pub func read_line(file: File) -> LineResult:
    return raw_read_line(file.token)

// 读取整个文件为字节；空文件成功，失败时返回空字节载荷。
pub func read_file(path: string) -> BytesResult:
    return raw_read_file(path)

// 读取整个文件为文本；严格校验 UTF-8，非法编码返回 InvalidUtf8 而不是替换字符。
pub func read_text(path: string) -> TextResult:
    return raw_read_text(path)

// 创建或覆盖整个文件。
pub func write_file(path: string, data: bytes) -> WriteResult:
    return raw_write_file(path, data)

// 创建或追加整个文件。
pub func append_file(path: string, data: bytes) -> WriteResult:
    return raw_append_file(path, data)

// 严格验证 UTF-8 后创建或覆盖文本文件；非法文本在打开文件前返回失败，原文件不被截断。
pub func write_text(path: string, text: string) -> WriteResult:
    var encoded = encode_utf8(text)
    if encoded == null:
        return WriteResult(false, IOErrorKind.InvalidUtf8, 0)
    return raw_write_file(path, encoded.value)

// 严格验证 UTF-8 后创建或追加文本文件；非法文本在打开文件前返回失败。
pub func append_text(path: string, text: string) -> WriteResult:
    var encoded = encode_utf8(text)
    if encoded == null:
        return WriteResult(false, IOErrorKind.InvalidUtf8, 0)
    return raw_append_file(path, encoded.value)
