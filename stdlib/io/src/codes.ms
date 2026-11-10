// 集中维护枚举与宿主整数编码之间的唯一映射。
// 原生层只使用整数错误码与模式码，公开 API 只暴露枚举，因此转换只在这里发生。

import local.types { FileKind, FileMode, IOErrorKind, SeekFrom }

// 返回合法的空字符串，供失败结果与空行等占位值复用。
pub func empty_text() -> string:
    return ""

// 返回合法的空字节值；空字节的指针是非空静态地址，可以安全跨 GC 存活。
pub func empty_bytes() -> bytes:
    var empty: list<int> = []
    return bytes_from_list(empty).value

// 把打开模式转换为原生层使用的模式码：0=Read 1=Write 2=Append 3=ReadWrite。
pub func mode_code(mode: FileMode) -> int:
    if mode == FileMode.Write:
        return 1
    if mode == FileMode.Append:
        return 2
    if mode == FileMode.ReadWrite:
        return 3
    return 0

// 把定位起点转换为原生层使用的起点码：0=Start 1=Current 2=End。
pub func seek_code(origin: SeekFrom) -> int:
    if origin == SeekFrom.Current:
        return 1
    if origin == SeekFrom.End:
        return 2
    return 0

// 把元数据类别码转换为公开枚举：0=Regular 1=Directory 其他为 Other。
pub func file_kind(code: int) -> FileKind:
    if code == 0:
        return FileKind.Regular
    if code == 1:
        return FileKind.Directory
    return FileKind.Other

// 把宿主整数错误码转换为公开枚举；未知错误统一归为 Other，不泄露具体错误号。
pub func error_kind(code: int) -> IOErrorKind:
    if code == 0:
        return IOErrorKind.None
    if code == 1:
        return IOErrorKind.NotFound
    if code == 2:
        return IOErrorKind.PermissionDenied
    if code == 3:
        return IOErrorKind.AlreadyExists
    if code == 4:
        return IOErrorKind.InvalidArgument
    if code == 5:
        return IOErrorKind.InvalidUtf8
    if code == 6:
        return IOErrorKind.Closed
    if code == 7:
        return IOErrorKind.WrongMode
    if code == 8:
        return IOErrorKind.NotSupported
    if code == 9:
        return IOErrorKind.DirectoryNotEmpty
    if code == 10:
        return IOErrorKind.TooLarge
    return IOErrorKind.Other
