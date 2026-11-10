// io 标准库的最小可运行样例：分块复制二进制文件、逐行读配置、追加 UTF-8 日志，
// 同时演示目录枚举、元数据、禁止覆盖重命名和显式关闭。所有数据都写在项目内的
// 工作目录里，样例可以重复运行，不需要管理员权限或固定机器路径。

import io {
    FileKind,
    FileMode,
    IOErrorKind,
    SeekFrom,
    append_text,
    close,
    flush,
    list_dir,
    make_dir,
    metadata,
    open,
    read,
    read_file,
    read_line,
    remove_dir,
    remove_file,
    rename,
    seek,
    write,
    write_file,
}

// 返回样例使用的工作目录；使用相对路径，保证从工程根目录运行时行为一致。
func work_dir() -> string:
    return "build/io-data/数据"

// 把旧样例遗留的工作目录清空，使重复运行得到相同结果。
// `make_dir` 只创建一级目录，因此这里按层级依次创建父目录。
func reset_work_dir(path: string):
    var listing = list_dir(path)
    if listing.ok:
        for name in listing.value:
            var child = path + "/" + name
            var info = metadata(child)
            if info.kind == FileKind.Directory:
                remove_dir(child)
            else:
                remove_file(child)
        remove_dir(path)
    var build_dir = make_dir("build")
    if not build_dir.ok and build_dir.code != IOErrorKind.AlreadyExists:
        print("reset failed")
    var data_dir = make_dir("build/io-data")
    if not data_dir.ok and data_dir.code != IOErrorKind.AlreadyExists:
        print("reset failed")
    var created = make_dir(path)
    if not created.ok and created.code != IOErrorKind.AlreadyExists:
        print("reset failed")

// 生成 0..255 的全部字节值，作为二进制复制的源数据。
func all_bytes() -> bytes:
    var values: list<int> = []
    for value in 0..256:
        values.append(value)
    return bytes_from_list(values).value

// 按固定块大小复制文件：读一块写一块，EOF 与失败分开判断。
func copy_file(source_path: string, target_path: string, chunk_size: int) -> int:
    var source = open(source_path, FileMode.Read)
    if not source.ok:
        print("source open failed")
        return -1
    var target = open(target_path, FileMode.Write)
    if not target.ok:
        close(source.value.value)
        print("target open failed")
        return -1
    var source_handle = source.value.value
    var target_handle = target.value.value
    var copied = 0
    var reading = true
    while reading:
        var chunk = read(source_handle, chunk_size)
        if not chunk.ok:
            reading = false
        elif chunk.eof:
            reading = false
        else:
            var result = write(target_handle, chunk.value)
            copied += result.written
    flush(target_handle)
    close(source_handle)
    close(target_handle)
    return copied

// 逐行读取配置文本，返回实际读到的行，供调用方检查空行和末尾行。
func read_config_lines(path: string) -> list<string>:
    var lines: list<string> = []
    var opened = open(path, FileMode.Read)
    if not opened.ok:
        return lines
    var handle = opened.value.value
    var reading = true
    while reading:
        var line = read_line(handle)
        if not line.ok:
            reading = false
        elif line.eof:
            reading = false
        else:
            lines.append(line.value)
    close(handle)
    return lines

// 把两个字节对象逐字节比较，长度不同直接判定为不同。
func bytes_equal(left: bytes, right: bytes) -> bool:
    if len(left) != len(right):
        return false
    for index in 0..len(left):
        if left[index] != right[index]:
            return false
    return true

func main():
    var base = work_dir()
    reset_work_dir(base)

    var source_path = base + "/source.bin"
    var copied_path = base + "/copied.bin"
    var payload = all_bytes()
    var source_written = write_file(source_path, payload)
    print("write source ok=" + string(source_written.ok))
    print("write source bytes=" + string(source_written.written))

    var copied = copy_file(source_path, copied_path, 32)
    print("copy bytes=" + string(copied))
    var source_data = read_file(source_path)
    var target_data = read_file(copied_path)
    print("copy identical=" + string(bytes_equal(source_data.value, target_data.value)))
    print("copy preserves NUL=" + string(target_data.value[0] == 0))
    print("copy preserves 0xFF=" + string(target_data.value[255] == 255))

    var config_path = base + "/config.txt"
    var config_written = write_file(config_path, encode_utf8("name=样例\nmode=io\n\nlast").value)
    print("write config ok=" + string(config_written.ok))
    var lines = read_config_lines(config_path)
    print("config lines=" + string(len(lines)))
    print("config first=" + lines[0])
    print("config empty=" + string(len(lines[2]) == 0))
    print("config last no newline=" + lines[3])

    var log_path = base + "/app.log"
    var log_started = write_file(log_path, encode_utf8("日志开始\n").value)
    print("log start ok=" + string(log_started.ok))
    var log_first = append_text(log_path, "第一行\n")
    var log_second = append_text(log_path, "第二行\n")
    print("log append ok=" + string(log_first.ok and log_second.ok))
    var log_lines = read_config_lines(log_path)
    print("log lines=" + string(len(log_lines)))
    print("log last=" + log_lines[2])

    var info = metadata(copied_path)
    print("metadata regular=" + string(info.kind == FileKind.Regular))
    print("metadata size=" + string(info.size.value))

    var listing = list_dir(base)
    print("listing ok=" + string(listing.ok))
    print("listing count=" + string(len(listing.value)))

    var opened = open(copied_path, FileMode.Read)
    var handle = opened.value.value
    var seeked = seek(handle, 250, SeekFrom.Start)
    print("seek ok=" + string(seeked.ok))
    print("seek position=" + string(seeked.position))
    var tail = read(handle, 16)
    print("tail bytes=" + string(len(tail.value)))
    print("tail first=" + string(tail.value[0]))
    var close_result = close(handle)
    print("close ok=" + string(close_result.ok))
    var close_again = close(handle)
    print("close again closed=" + string(close_again.code == IOErrorKind.Closed))

    var renamed = rename(copied_path, base + "/moved.bin")
    print("rename ok=" + string(renamed.ok))
    var blocked = rename(base + "/moved.bin", config_path)
    print("rename no overwrite=" + string(blocked.code == IOErrorKind.AlreadyExists))
    var removed = remove_file(base + "/moved.bin")
    print("remove ok=" + string(removed.ok))
    print("done")
