// 公开的文件系统操作：目录枚举在 Mas 包装层收集 `list<string>`，
// 每条路径都关闭迭代器；读取失败时丢弃已收集的部分结果并返回错误。

import local.codes { error_kind }
import local.native {
    DirResult,
    IOStatus,
    MetadataResult,
    raw_dir_close,
    raw_dir_next,
    raw_dir_open,
    raw_make_dir,
    raw_metadata,
    raw_remove_dir,
    raw_remove_file,
    raw_rename,
}
import local.types { IOErrorKind }

// 返回空名称列表，供目录枚举失败时的失败占位使用。
func empty_names() -> list<string>:
    var empty: list<string> = []
    return empty

// 查询元数据：跟随符号链接，只有普通文件带字节大小；目录与其他类型 size 为 null。
pub func metadata(path: string) -> MetadataResult:
    return raw_metadata(path)

// 枚举目录的直接子项名称，排除 "." 与 ".."，顺序未指定。
// 枚举中途失败时返回错误并丢弃已收集结果；成功与失败路径都会关闭迭代器。
pub func list_dir(path: string) -> DirResult:
    var opened = raw_dir_open(path)
    if not opened.ok:
        return DirResult(false, error_kind(opened.code), empty_names())
    var handle = opened.handle
    var names: list<string> = []
    var failed = false
    var failure = 0
    var finished = false
    while not finished:
        var entry = raw_dir_next(handle)
        if not entry.ok:
            failed = true
            failure = entry.code
            finished = true
        elif entry.done:
            finished = true
        else:
            names.append(entry.name)
    var closed = raw_dir_close(handle)
    if failed:
        return DirResult(false, error_kind(failure), empty_names())
    if not closed.ok:
        return DirResult(false, closed.code, empty_names())
    return DirResult(true, IOErrorKind.None, names)

// 创建一级目录：父目录缺失报 NotFound，目标已存在报 AlreadyExists。
pub func make_dir(path: string) -> IOStatus:
    return raw_make_dir(path)

// 同一文件系统内移动：目标存在时失败，跨文件系统或宿主不支持时返回 NotSupported。
pub func rename(source: string, destination: string) -> IOStatus:
    return raw_rename(source, destination)

// 删除文件或指向文件的符号链接。
pub func remove_file(path: string) -> IOStatus:
    return raw_remove_file(path)

// 删除空目录：非空返回 DirectoryNotEmpty，符号链接与 Windows 目录重解析点被拒绝。
pub func remove_dir(path: string) -> IOStatus:
    return raw_remove_dir(path)
