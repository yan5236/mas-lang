// 定义 io 包的公开枚举与库句柄类型。
// 所有枚举序号都属于实现细节：调用方只按成员名使用，序号与宿主错误码的对应关系
// 由 `codes.ms` 与原生层共同维护，不对外暴露。

// 文件打开模式：Read 要求存在，Write 创建或截断，Append 创建或每次写到末尾，
// ReadWrite 要求存在且不截断。
pub enum FileMode:
    Read
    Write
    Append
    ReadWrite

// 定位起点：Start 从开头计数，Current 从当前逻辑位置计数，End 从文件末尾计数。
pub enum SeekFrom:
    Start
    Current
    End

// 元数据类别：只有普通文件带字节大小，目录与设备等其他类型不带。
pub enum FileKind:
    Regular
    Directory
    Other

// 公开错误类别；与操作系统错误号、Win32 错误码无关，未知宿主错误统一归为 Other。
pub enum IOErrorKind:
    None
    NotFound
    PermissionDenied
    AlreadyExists
    InvalidArgument
    InvalidUtf8
    Closed
    WrongMode
    NotSupported
    DirectoryNotEmpty
    TooLarge
    Other

// 表示已打开文件或标准流的库句柄；token 只是资源标识，不是权限凭证。
// 调用方不要手工构造该记录；原生层会校验未知、已关闭和越界的令牌。
pub struct File:
    token: int
