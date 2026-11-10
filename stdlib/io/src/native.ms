// 声明 io 包的原生绑定层：这里只放私有输入/输出盒、extern 入口与一次性的结果转换。
// Mas 的 extern 参数约定是“结构体按引用传递”，所以路径、字节和文本都放进托管记录，
// 由 C 按偏移读写；C 侧绝不保存跨调用指针，结果载荷统一通过托管复制入口写回。

import local.codes { empty_bytes, empty_text, error_kind, file_kind }
import local.types {
    File,
    FileKind,
    IOErrorKind,
}

// 打开文件的原生输入盒：mode 是 `codes.mode_code` 转换后的整数模式码。
struct OpenArg:
    path: string
    mode: int

// 标准流选择输入盒：which 0=stdin 1=stdout 2=stderr。
struct StdArg:
    which: int

// 只携带令牌的输入盒：读一行、flushing、关闭都复用这一形状。
struct TokenArg:
    token: int

// 只携带目录迭代器句柄的输入盒。
struct HandleArg:
    handle: int

// 分块读取输入盒：count 为请求的最大字节数，负数由原生层报 InvalidArgument。
struct ReadArg:
    token: int
    count: int

// 写入输入盒：data 直接借用 Mas 托管字节，C 只在调用期间读取。
struct WriteArg:
    token: int
    data: bytes

// 定位输入盒：offset 为有符号偏移，origin 为 0/1/2 起点码。
struct SeekArg:
    token: int
    offset: int
    origin: int

// 只携带路径的输入盒。
struct PathArg:
    path: string

// 携带路径与字节载荷的输入盒，供整文件写入与追加复用。
struct PathDataArg:
    path: string
    data: bytes

// 重命名输入盒：同一文件系统内禁止覆盖目标。
struct RenameArg:
    source: string
    destination: string

// 原生状态结果：code 为宿主整数错误码，由包装层转换为公开枚举。
struct RawStatus:
    ok: bool
    code: int

// 原生打开结果：value 由原生层通过 `mas_record_alloc` 分配，成功后非空。
struct RawOpenResult:
    ok: bool
    code: int
    value: File?

// 原生整文件字节结果。
struct RawBytesResult:
    ok: bool
    code: int
    value: bytes

// 原生整文件文本结果。
struct RawTextResult:
    ok: bool
    code: int
    value: string

// 原生分块读取结果：eof 只在没有返回数据且宿主读到文件末尾时为真。
struct RawReadResult:
    ok: bool
    code: int
    value: bytes
    eof: bool

// 原生按行读取结果：value 已剥除换行符，其他控制字符原样保留。
struct RawLineResult:
    ok: bool
    code: int
    value: string
    eof: bool

// 原生写入结果：written 为宿主已接受的字节数，失败时保留。
struct RawWriteResult:
    ok: bool
    code: int
    written: int

// 原生定位结果：position 为从文件开头计数的绝对位置。
struct RawSeekResult:
    ok: bool
    code: int
    position: int

// 原生元数据结果：kind 为 0/1/2 类别码，size 只有普通文件非空。
struct RawMetadataResult:
    ok: bool
    code: int
    kind: int
    size: int?

// 原生目录打开结果：handle 是目录迭代器令牌，必须以 dir_close 释放。
struct RawDirOpenResult:
    ok: bool
    code: int
    handle: int

// 原生目录项结果：done 表示枚举正常结束。
struct RawDirEntryResult:
    ok: bool
    code: int
    done: bool
    name: string

// 不携带额外载荷的操作状态；成功时 code 为 None。
pub struct IOStatus:
    ok: bool
    code: IOErrorKind

// 打开结果：成功后 value 非空，失败时 value 为 null。
pub struct OpenResult:
    ok: bool
    code: IOErrorKind
    value: File?

// 整文件字节读取结果：失败时 value 为有效空字节。
pub struct BytesResult:
    ok: bool
    code: IOErrorKind
    value: bytes

// 整文件文本读取结果：失败时 value 为空字符串。
pub struct TextResult:
    ok: bool
    code: IOErrorKind
    value: string

// 分块读取结果：失败时 eof 为 false，value 为有效空字节。
pub struct ReadResult:
    ok: bool
    code: IOErrorKind
    value: bytes
    eof: bool

// 按行读取结果：失败时 eof 为 false，value 为空字符串。
pub struct LineResult:
    ok: bool
    code: IOErrorKind
    value: string
    eof: bool

// 写入结果：written 始终为宿主已接受的字节数，失败时同样保留。
pub struct WriteResult:
    ok: bool
    code: IOErrorKind
    written: int

// 定位结果：成功时 position 为绝对位置，失败时为 0。
pub struct SeekResult:
    ok: bool
    code: IOErrorKind
    position: int

// 元数据结果：失败时 kind 为 Other 且 size 为 null。
pub struct MetadataResult:
    ok: bool
    code: IOErrorKind
    kind: FileKind
    size: int?

// 目录枚举结果：失败时 value 为空列表，且迭代器已经关闭。
pub struct DirResult:
    ok: bool
    code: IOErrorKind
    value: list<string>

// 声明打开文件入口：成功时把句柄记录指针写进 out.value。
pub extern func mas_io_open(input: OpenArg, out: RawOpenResult)

// 声明标准流借用入口：which 选择 stdin/stdout/stderr。
pub extern func mas_io_std(input: StdArg, out: RawOpenResult)

// 声明分块读取入口：最多读 count 字节，短读合法。
pub extern func mas_io_read(input: ReadArg, out: RawReadResult)

// 声明按行读取入口：读至 LF 或文件末尾，剥除 LF 及其前面紧邻的 CR。
pub extern func mas_io_read_line(input: TokenArg, out: RawLineResult)

// 声明写入入口：循环写完 data，失败时回报宿主已接受字节数。
pub extern func mas_io_write(input: WriteArg, out: RawWriteResult)

// 声明刷新入口：把库缓冲刷到宿主。
pub extern func mas_io_flush(input: TokenArg, out: RawStatus)

// 声明关闭入口：即使失败也使令牌失效，其他文件保持可用。
pub extern func mas_io_close(input: TokenArg, out: RawStatus)

// 声明定位入口：成功返回从开头计数的绝对位置。
pub extern func mas_io_seek(input: SeekArg, out: RawSeekResult)

// 声明整文件字节读取入口。
pub extern func mas_io_read_file(input: PathArg, out: RawBytesResult)

// 声明整文件文本读取入口：严格校验 UTF-8，空文件成功。
pub extern func mas_io_read_text(input: PathArg, out: RawTextResult)

// 声明整文件覆盖写入入口。
pub extern func mas_io_write_file(input: PathDataArg, out: RawWriteResult)

// 声明整文件追加写入入口。
pub extern func mas_io_append_file(input: PathDataArg, out: RawWriteResult)

// 声明元数据查询入口：跟随符号链接，只有普通文件返回大小。
pub extern func mas_io_metadata(input: PathArg, out: RawMetadataResult)

// 声明目录迭代开始入口。
pub extern func mas_io_dir_open(input: PathArg, out: RawDirOpenResult)

// 声明目录迭代下一步入口：done 为真表示正常结束。
pub extern func mas_io_dir_next(input: HandleArg, out: RawDirEntryResult)

// 声明目录迭代关闭入口。
pub extern func mas_io_dir_close(input: HandleArg, out: RawStatus)

// 声明一级目录创建入口。
pub extern func mas_io_make_dir(input: PathArg, out: RawStatus)

// 声明禁止覆盖的重命名入口。
pub extern func mas_io_rename(input: RenameArg, out: RawStatus)

// 声明文件删除入口。
pub extern func mas_io_remove_file(input: PathArg, out: RawStatus)

// 声明空目录删除入口。
pub extern func mas_io_remove_dir(input: PathArg, out: RawStatus)

// 打开文件并把宿主整数错误码转换为公开枚举；路径与模式由包装层校验后传入。
pub func raw_open(path: string, mode: int) -> OpenResult:
    var out = RawOpenResult(false, 0, null)
    mas_io_open(OpenArg(path, mode), out)
    return OpenResult(out.ok, error_kind(out.code), out.value)

// 借用进程标准流；which 0=stdin 1=stdout 2=stderr。宿主失败时返回 null 句柄。
pub func raw_std(which: int) -> OpenResult:
    var out = RawOpenResult(false, 0, null)
    mas_io_std(StdArg(which), out)
    return OpenResult(out.ok, error_kind(out.code), out.value)

// 分块读取并转换错误码；失败时 value 为空字节、eof 为 false。
pub func raw_read(token: int, count: int) -> ReadResult:
    var out = RawReadResult(false, 0, empty_bytes(), false)
    mas_io_read(ReadArg(token, count), out)
    return ReadResult(out.ok, error_kind(out.code), out.value, out.eof)

// 按行读取并转换错误码；完整行照常交付。
pub func raw_read_line(token: int) -> LineResult:
    var out = RawLineResult(false, 0, empty_text(), false)
    mas_io_read_line(TokenArg(token), out)
    return LineResult(out.ok, error_kind(out.code), out.value, out.eof)

// 写入字节并转换错误码；失败时 written 保留宿主已接受字节数。
pub func raw_write(token: int, data: bytes) -> WriteResult:
    var out = RawWriteResult(false, 0, 0)
    mas_io_write(WriteArg(token, data), out)
    return WriteResult(out.ok, error_kind(out.code), out.written)

// 刷新句柄缓冲。
pub func raw_flush(token: int) -> IOStatus:
    var out = RawStatus(false, 0)
    mas_io_flush(TokenArg(token), out)
    return IOStatus(out.ok, error_kind(out.code))

// 关闭句柄；无论成功与否令牌都失效。
pub func raw_close(token: int) -> IOStatus:
    var out = RawStatus(false, 0)
    mas_io_close(TokenArg(token), out)
    return IOStatus(out.ok, error_kind(out.code))

// 定位文件位置并转换错误码。
pub func raw_seek(token: int, offset: int, origin: int) -> SeekResult:
    var out = RawSeekResult(false, 0, 0)
    mas_io_seek(SeekArg(token, offset, origin), out)
    return SeekResult(out.ok, error_kind(out.code), out.position)

// 读取整个文件为字节；失败时 value 为空字节。
pub func raw_read_file(path: string) -> BytesResult:
    var out = RawBytesResult(false, 0, empty_bytes())
    mas_io_read_file(PathArg(path), out)
    return BytesResult(out.ok, error_kind(out.code), out.value)

// 读取整个文件为文本；非法 UTF-8 与读失败都返回失败。
pub func raw_read_text(path: string) -> TextResult:
    var out = RawTextResult(false, 0, empty_text())
    mas_io_read_text(PathArg(path), out)
    return TextResult(out.ok, error_kind(out.code), out.value)

// 创建或覆盖写入整个文件。
pub func raw_write_file(path: string, data: bytes) -> WriteResult:
    var out = RawWriteResult(false, 0, 0)
    mas_io_write_file(PathDataArg(path, data), out)
    return WriteResult(out.ok, error_kind(out.code), out.written)

// 创建或追加写入整个文件。
pub func raw_append_file(path: string, data: bytes) -> WriteResult:
    var out = RawWriteResult(false, 0, 0)
    mas_io_append_file(PathDataArg(path, data), out)
    return WriteResult(out.ok, error_kind(out.code), out.written)

// 查询元数据并把类别码转换为公开枚举。
pub func raw_metadata(path: string) -> MetadataResult:
    var out = RawMetadataResult(false, 0, 2, null)
    mas_io_metadata(PathArg(path), out)
    return MetadataResult(out.ok, error_kind(out.code), file_kind(out.kind), out.size)

// 打开目录迭代器。
pub func raw_dir_open(path: string) -> RawDirOpenResult:
    var out = RawDirOpenResult(false, 0, 0)
    mas_io_dir_open(PathArg(path), out)
    return out

// 读取目录迭代器的下一个名称；done 表示枚举正常结束。
pub func raw_dir_next(handle: int) -> RawDirEntryResult:
    var out = RawDirEntryResult(false, 0, false, empty_text())
    mas_io_dir_next(HandleArg(handle), out)
    return out

// 关闭目录迭代器，返回公开状态。
pub func raw_dir_close(handle: int) -> IOStatus:
    var out = RawStatus(false, 0)
    mas_io_dir_close(HandleArg(handle), out)
    return IOStatus(out.ok, error_kind(out.code))

// 创建一级目录。
pub func raw_make_dir(path: string) -> IOStatus:
    var out = RawStatus(false, 0)
    mas_io_make_dir(PathArg(path), out)
    return IOStatus(out.ok, error_kind(out.code))

// 同一文件系统内禁止覆盖的重命名。
pub func raw_rename(source: string, destination: string) -> IOStatus:
    var out = RawStatus(false, 0)
    mas_io_rename(RenameArg(source, destination), out)
    return IOStatus(out.ok, error_kind(out.code))

// 删除文件或指向文件的符号链接。
pub func raw_remove_file(path: string) -> IOStatus:
    var out = RawStatus(false, 0)
    mas_io_remove_file(PathArg(path), out)
    return IOStatus(out.ok, error_kind(out.code))

// 删除空目录。
pub func raw_remove_dir(path: string) -> IOStatus:
    var out = RawStatus(false, 0)
    mas_io_remove_dir(PathArg(path), out)
    return IOStatus(out.ok, error_kind(out.code))

