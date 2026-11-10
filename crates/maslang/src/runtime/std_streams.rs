/// 标准流桥：把 stdin/stdout/stderr 的实际读写交给运行时，保证与 `print` 共用
/// 同一把锁和同一份缓冲，从而在重定向与管道场景下保持输出顺序。
///
/// 约定：返回值 `>= 0` 表示读写字节数（读为 0 表示 EOF），负值表示 `-IOErrorKind` 序号。
/// 原生层只负责把返回值翻译成公开错误类别，不直接接触 Rust 标准流。
use std::io::{IsTerminal, Read};

/// 把标准库 IO 错误归类为 io 包的公开错误码：权限拒绝、无效参数与未知错误分开返回。
fn std_error_code(error: &std::io::Error) -> i64 {
    use std::io::ErrorKind;
    match error.kind() {
        ErrorKind::PermissionDenied => 2,
        ErrorKind::NotFound => 1,
        ErrorKind::InvalidInput | ErrorKind::InvalidData => 4,
        ErrorKind::BrokenPipe | ErrorKind::UnexpectedEof => 8,
        _ => 11,
    }
}

/// 从标准输入读取最多 `capacity` 字节；返回 0 表示 EOF，负值表示错误码。
/// 被信号中断时重试，避免把 `Interrupted` 当成真正的读错误上报给调用方。
#[unsafe(no_mangle)]
pub extern "C" fn mas_runtime_std_read(ptr: *mut u8, capacity: u64) -> i64 {
    if capacity == 0 {
        return 0;
    }
    if ptr.is_null() {
        return -4;
    }
    let buffer = unsafe { std::slice::from_raw_parts_mut(ptr, capacity as usize) };
    let stdin = std::io::stdin();
    loop {
        let mut handle = stdin.lock();
        match handle.read(buffer) {
            Ok(read) => return read as i64,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return -std_error_code(&error),
        }
    }
}

/// 把整段字节写入标准输出或标准错误，并立即刷新，使顺序与 `print` 保持一致。
/// `which` 使用 1=stdout、2=stderr；其他取值返回无效参数错误码。
///
/// 重定向与管道按原始字节写出；直接连接终端时受宿主文本限制，无法表示为文本的
/// 任意二进制返回可恢复的 `InvalidArgument`（-4），不把终端行为套到重定向目标上。
#[unsafe(no_mangle)]
pub extern "C" fn mas_runtime_std_write(which: u64, ptr: *const u8, len: u64) -> i64 {
    if len == 0 {
        return 0;
    }
    if ptr.is_null() {
        return -4;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let written = match which {
        1 => {
            let stdout = std::io::stdout();
            if stdout.is_terminal() && std::str::from_utf8(bytes).is_err() {
                return -4;
            }
            let mut handle = stdout.lock();
            handle.write_all(bytes).and_then(|()| handle.flush())
        }
        2 => {
            let stderr = std::io::stderr();
            if stderr.is_terminal() && std::str::from_utf8(bytes).is_err() {
                return -4;
            }
            let mut handle = stderr.lock();
            handle.write_all(bytes).and_then(|()| handle.flush())
        }
        _ => return -4,
    };
    match written {
        Ok(()) => len as i64,
        Err(error) => -std_error_code(&error),
    }
}

/// 刷新标准输出或标准错误缓冲；`which` 使用 1=stdout、2=stderr。
#[unsafe(no_mangle)]
pub extern "C" fn mas_runtime_std_flush(which: u64) -> i64 {
    let flushed = match which {
        1 => std::io::stdout().lock().flush(),
        2 => std::io::stderr().lock().flush(),
        _ => return -4,
    };
    match flushed {
        Ok(()) => 0,
        Err(error) => -std_error_code(&error),
    }
}
