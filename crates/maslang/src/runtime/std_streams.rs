/// 标准流桥：把 stdin/stdout/stderr 的实际读写交给运行时，保证与 `print` 共用
/// 同一把锁和同一份缓冲，从而在重定向与管道场景下保持输出顺序。
///
/// 约定：读入口返回 `>= 0` 表示字节数（0 表示 EOF），负值表示 `-IOErrorKind` 序号。
/// 写入口有新旧两版符号：ABI 1 的 `mas_runtime_std_write` 返回字节数或负值错误码，
/// ABI 2 的 `mas_runtime_std_write_v2` 返回状态码并通过出参回报已接受字节数。
/// 原生层只负责翻译返回值，不直接接触 Rust 标准流。
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

/// 循环写入整段数据并刷新；`written` 记录宿主已接受的字节数，返回 `0` 或 `-错误码`。
///
/// 不能直接用 `write_all`：它在失败时不报告已经写出的长度，调用方会误以为一个字都没写。
/// 这里用 UFCS 调用 `Write` 的方法，避免与引导运行时的其他模块重复导入该 trait。
fn write_tracking(handle: &mut impl std::io::Write, bytes: &[u8], written: &mut usize) -> i64 {
    while *written < bytes.len() {
        match std::io::Write::write(handle, &bytes[*written..]) {
            Ok(0) => return -11,
            Ok(count) => *written += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return -std_error_code(&error),
        }
    }
    match std::io::Write::flush(handle) {
        Ok(()) => 0,
        Err(error) => -std_error_code(&error),
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
/// 这是 ABI 2 的写桥：返回 `0` 表示全部写入成功，负值为 `-错误码`；无论成功与否，
/// `accepted` 都会写入宿主已接受的字节数，断管后调用方仍知道前缀已经发出。
///
/// 重定向与管道按原始字节写出；直接连接终端时受宿主文本限制，无法表示为文本的
/// 任意二进制返回可恢复的 `InvalidArgument`（-4），不把终端行为套到重定向目标上。
#[unsafe(no_mangle)]
pub extern "C" fn mas_runtime_std_write_v2(
    which: u64,
    ptr: *const u8,
    len: u64,
    accepted: *mut u64,
) -> i64 {
    if accepted.is_null() {
        return -4;
    }
    let slot = unsafe { &mut *accepted };

    std_write(which, ptr, len, slot)
}

/// ABI 1 的写桥：成功时返回宿主接受的字节数，失败时返回负值错误码。
/// 保留原符号与原返回语义，使按 ABI 1 编译的旧包在新运行时上继续正确工作。
#[unsafe(no_mangle)]
pub extern "C" fn mas_runtime_std_write(which: u64, ptr: *const u8, len: u64) -> i64 {
    let mut accepted = 0u64;
    let result = std_write(which, ptr, len, &mut accepted);

    if result < 0 { result } else { accepted as i64 }
}

/// 两个写桥共用的实现：把 `written` 进度写回 `accepted`，返回 `0` 或 `-错误码`。
fn std_write(which: u64, ptr: *const u8, len: u64, accepted: &mut u64) -> i64 {
    *accepted = 0;
    if len == 0 {
        return 0;
    }
    if ptr.is_null() {
        return -4;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let mut written = 0usize;
    let result = match which {
        1 => {
            let stdout = std::io::stdout();
            if stdout.is_terminal() && std::str::from_utf8(bytes).is_err() {
                return -4;
            }
            write_tracking(&mut stdout.lock(), bytes, &mut written)
        }
        2 => {
            let stderr = std::io::stderr();
            if stderr.is_terminal() && std::str::from_utf8(bytes).is_err() {
                return -4;
            }
            write_tracking(&mut stderr.lock(), bytes, &mut written)
        }
        _ => return -4,
    };
    *accepted = written as u64;
    result
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
