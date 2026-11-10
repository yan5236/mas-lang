// 统一导出 io 标准库的全部公开 API，保证调用方只需要 `import io { ... }`。
// 结果结构、枚举与库句柄一起导出，调用方无需再单独导入私有绑定层。

// 公开枚举与文件句柄：模式、定位起点、元数据类别与错误类别。
export import local.types {
    File,
    FileKind,
    FileMode,
    IOErrorKind,
    SeekFrom,
}
// 结果结构：所有入口共用 `ok` 与枚举错误类别，其余字段按操作补充载荷。
export import local.native {
    BytesResult,
    DirResult,
    IOStatus,
    LineResult,
    MetadataResult,
    OpenResult,
    ReadResult,
    SeekResult,
    TextResult,
    WriteResult,
}
// 文件与标准流：打开、读取、写入、刷新、关闭与定位。
export import local.files {
    close,
    flush,
    open,
    read,
    seek,
    stderr,
    stdin,
    stdout,
    write,
    write_text_to,
}
// 文本与整文件入口：按行读取、整文件读、整文件写与追加。
export import local.text {
    append_file,
    append_text,
    read_file,
    read_line,
    read_text,
    write_file,
    write_text,
}
// 文件系统：元数据、目录枚举、建目录、重命名与删除。
export import local.fs {
    list_dir,
    make_dir,
    metadata,
    remove_dir,
    remove_file,
    rename,
}
