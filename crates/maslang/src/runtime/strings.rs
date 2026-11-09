use std::collections::HashMap;
use std::ffi::{CStr, c_char};
use std::io::Write;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MsString {
    ptr: *const u8,
    len: u64,
}

#[repr(C)]
pub struct MsList {
    elem_kind: u8,
    len: u64,
    capacity: u64,
    data: *mut u8,
}

#[derive(Clone, Copy)]
enum ScalarCell {
    Int(i64),
    Float(f64),
    Bool(u8),
    String(MsString),
    Reference(*mut u8),
}

// 字典键类型由 key_kind 固定；索引只存位模式，避免每个桶重复存储类型标签。
type MapKey = u64;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RuntimeErrorSite {
    path: MsString,
    source: MsString,
    span_start: u64,
    span_end: u64,
}

#[repr(C)]
pub struct MsMap {
    key_kind: u8,
    value_kind: u8,
    len: u64,
    entries: Vec<Option<(ScalarCell, ScalarCell)>>,
    indexes: HashMap<MapKey, usize>,
    // 非负稠密整数键直接定位条目；usize::MAX 表示空槽，其余键仍使用随机哈希。
    integer_indexes: Vec<usize>,
    string_indexes: HashMap<Vec<u8>, usize>,
    // 仅缓存条目位置，不持有查询字符串；每次命中仍比较字节，删除时失效。
    last_string_index: std::cell::Cell<Option<usize>>,
}

const KIND_INT: u8 = 1;
const KIND_FLOAT: u8 = 2;
const KIND_BOOL: u8 = 3;
const KIND_STRING: u8 = 4;
const KIND_RECORD: u8 = 5;

/// 把一行 UTF-8 文本写入标准输出并返回状态：`1` 成功，`0` 表示写入或刷新失败。
/// 统一使用返回 `io::Result` 的写入路径，不调用会 panic 的 `println!` 宏。
fn write_stdout_line(bytes: &[u8]) -> u8 {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    let written = handle
        .write_all(bytes)
        .and_then(|()| handle.write_all(b"\n"))
        .and_then(|()| handle.flush());
    u8::from(written.is_ok())
}

/// 打印整数并返回写入状态；失败由调用方决定是继续还是报告语言错误。
#[unsafe(no_mangle)]
pub extern "C" fn print_int(value: i64) -> u8 {
    write_stdout_line(value.to_string().as_bytes())
}

/// 打印浮点数并返回写入状态，文本格式与原有 `Display` 输出保持一致。
#[unsafe(no_mangle)]
pub extern "C" fn print_float(value: f64) -> u8 {
    write_stdout_line(value.to_string().as_bytes())
}

/// 打印布尔值并返回写入状态，输出固定为 `true` 或 `false`。
#[unsafe(no_mangle)]
pub extern "C" fn print_bool(value: u8) -> u8 {
    write_stdout_line((value != 0).to_string().as_bytes())
}

/// 打印字符串并返回写入状态；非法 UTF-8 沿用原有占位文本，不在这里终止进程。
#[unsafe(no_mangle)]
pub extern "C" fn print_string(ptr: *const u8, len: u64) -> u8 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let text = std::str::from_utf8(bytes).unwrap_or("<invalid utf8>");
    write_stdout_line(text.as_bytes())
}

/// 严格 `print` 的失败分支：标准输出不可写时按统一中文运行时错误退出。
/// 需要继续执行的调用方应改用 `try_print` 获取布尔状态。
#[unsafe(no_mangle)]
pub extern "C" fn mas_output_error() -> ! {
    abort_runtime("无法写入标准输出：输出流已关闭或不可用")
}

/// 拼接两个有效 UTF-8 字节区间，返回 GC 托管的稳定地址并写出结果长度。
#[unsafe(no_mangle)]
pub extern "C" fn string_concat(
    left_ptr: *const u8,
    left_len: u64,
    right_ptr: *const u8,
    right_len: u64,
    out_len: *mut u64,
) -> *const u8 {
    let left_bytes = unsafe { std::slice::from_raw_parts(left_ptr, left_len as usize) };
    let right_bytes = unsafe { std::slice::from_raw_parts(right_ptr, right_len as usize) };
    let mut buffer = Vec::with_capacity(left_bytes.len() + right_bytes.len());
    buffer.extend_from_slice(left_bytes);
    buffer.extend_from_slice(right_bytes);
    let boxed = buffer.into_boxed_slice();
    let len = boxed.len() as u64;
    let ptr = gc_allocate_string(boxed);
    unsafe {
        *out_len = len;
    }
    ptr
}

#[unsafe(no_mangle)]
pub extern "C" fn string_equals(
    left_ptr: *const u8,
    left_len: u64,
    right_ptr: *const u8,
    right_len: u64,
) -> u8 {
    let left_bytes = unsafe { std::slice::from_raw_parts(left_ptr, left_len as usize) };
    let right_bytes = unsafe { std::slice::from_raw_parts(right_ptr, right_len as usize) };
    u8::from(left_bytes == right_bytes)
}

/// 去除字符串首尾空白字符，并把结果重新封装成运行时字符串 ABI。
#[unsafe(no_mangle)]
pub extern "C" fn string_trim(ptr: *const u8, len: u64, out_len: *mut u64) -> *const u8 {
    let text = parse_runtime_text(ptr, len);
    boxed_string_into_raw(text.trim().to_string(), out_len)
}

/// 判断目标字符串是否包含指定子串，结果继续使用 `u8` 表示布尔值。
#[unsafe(no_mangle)]
pub extern "C" fn string_contains(
    ptr: *const u8,
    len: u64,
    needle_ptr: *const u8,
    needle_len: u64,
) -> u8 {
    let text = parse_runtime_text(ptr, len);
    let needle = parse_runtime_text(needle_ptr, needle_len);
    u8::from(text.contains(needle))
}

/// 判断目标字符串是否以给定前缀开头，并保持 UTF-8 文本语义。
#[unsafe(no_mangle)]
pub extern "C" fn string_starts_with(
    ptr: *const u8,
    len: u64,
    prefix_ptr: *const u8,
    prefix_len: u64,
) -> u8 {
    let text = parse_runtime_text(ptr, len);
    let prefix = parse_runtime_text(prefix_ptr, prefix_len);
    u8::from(text.starts_with(prefix))
}

/// 判断目标字符串是否以给定后缀结尾，并保持 UTF-8 文本语义。
#[unsafe(no_mangle)]
pub extern "C" fn string_ends_with(
    ptr: *const u8,
    len: u64,
    suffix_ptr: *const u8,
    suffix_len: u64,
) -> u8 {
    let text = parse_runtime_text(ptr, len);
    let suffix = parse_runtime_text(suffix_ptr, suffix_len);
    u8::from(text.ends_with(suffix))
}

/// 按从左到右的非重叠匹配规则替换子串，并在空模式时直接返回原字符串。
#[unsafe(no_mangle)]
pub extern "C" fn string_replace(
    ptr: *const u8,
    len: u64,
    from_ptr: *const u8,
    from_len: u64,
    to_ptr: *const u8,
    to_len: u64,
    out_len: *mut u64,
) -> *const u8 {
    let text = parse_runtime_text(ptr, len);
    let from = parse_runtime_text(from_ptr, from_len);
    if from.is_empty() {
        return boxed_string_into_raw(text.to_string(), out_len);
    }
    let to = parse_runtime_text(to_ptr, to_len);
    boxed_string_into_raw(text.replace(from, to), out_len)
}

/// 将 UTF-8 缓冲区移交 GC 并返回稳定字符串地址；输出长度指针为空时报告运行时错误。
fn boxed_string_into_raw(text: String, out_len: *mut u64) -> *const u8 {
    if out_len.is_null() {
        abort_runtime("字符串返回长度输出指针不能为空");
    }
    let boxed = text.into_bytes().into_boxed_slice();
    let len = boxed.len() as u64;
    let ptr = gc_allocate_string(boxed);
    unsafe {
        *out_len = len;
    }
    ptr
}

/// 把 Mas lang 的运行时字符串解析为 UTF-8 文本切片，供转换函数统一复用。
fn parse_runtime_text<'a>(ptr: *const u8, len: u64) -> &'a str {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    std::str::from_utf8(bytes).unwrap_or_else(|_| abort_runtime("字符串不是合法的 UTF-8 文本"))
}

/// 把字符串按十进制规则解析为 `int`，失败时报告明确的运行时类型转换错误。
#[unsafe(no_mangle)]
pub extern "C" fn string_to_int(ptr: *const u8, len: u64) -> i64 {
    let text = parse_runtime_text(ptr, len);
    text.parse::<i64>()
        .unwrap_or_else(|_| abort_runtime("字符串无法转换为 int"))
}

/// 把字符串按浮点规则解析为 `float`，失败时报告明确的运行时类型转换错误。
#[unsafe(no_mangle)]
pub extern "C" fn string_to_float(ptr: *const u8, len: u64) -> f64 {
    let text = parse_runtime_text(ptr, len);
    text.parse::<f64>()
        .unwrap_or_else(|_| abort_runtime("字符串无法转换为 float"))
}

/// 尝试把字符串解析为 `int`，成功时写入输出参数；非法 UTF-8 或解析失败都只返回 `false`。
/// 原生模块可以构造任意字节，因此安全入口不得经由会终止进程的文本解析路径。
#[unsafe(no_mangle)]
pub extern "C" fn try_string_to_int(ptr: *const u8, len: u64, out_value: *mut i64) -> u8 {
    if out_value.is_null() {
        abort_runtime("安全整数转换的输出指针不能为空");
    }
    let Some(text) = runtime_text_checked(ptr, len) else {
        return 0;
    };
    match text.parse::<i64>() {
        Ok(value) => {
            unsafe {
                *out_value = value;
            }
            1
        }
        Err(_) => 0,
    }
}

/// 尝试把字符串解析为 `float`，成功时写入输出参数；非法 UTF-8 或解析失败都只返回 `false`。
#[unsafe(no_mangle)]
pub extern "C" fn try_string_to_float(ptr: *const u8, len: u64, out_value: *mut f64) -> u8 {
    if out_value.is_null() {
        abort_runtime("安全浮点转换的输出指针不能为空");
    }
    let Some(text) = runtime_text_checked(ptr, len) else {
        return 0;
    };
    match text.parse::<f64>() {
        Ok(value) => {
            unsafe {
                *out_value = value;
            }
            1
        }
        Err(_) => 0,
    }
}

/// 尝试把字符串解析为 `bool`，仅 `true/false` 两种文本视为成功；
/// 非法 UTF-8 与不匹配文本一样返回 `false`。
#[unsafe(no_mangle)]
pub extern "C" fn try_string_to_bool(ptr: *const u8, len: u64, out_value: *mut u8) -> u8 {
    if out_value.is_null() {
        abort_runtime("安全布尔转换的输出指针不能为空");
    }
    let Some(text) = runtime_text_checked(ptr, len) else {
        return 0;
    };
    let parsed = match text {
        "true" => Some(1),
        "false" => Some(0),
        _ => None,
    };
    if let Some(value) = parsed {
        unsafe {
            *out_value = value;
        }
        1
    } else {
        0
    }
}

/// 把运行时字符串解析为 UTF-8 文本；空长度容忍空指针，非法 UTF-8 返回 `None` 而不终止进程。
fn runtime_text_checked<'a>(ptr: *const u8, len: u64) -> Option<&'a str> {
    let bytes = if len == 0 {
        &[][..]
    } else {
        if ptr.is_null() {
            return None;
        }
        unsafe { std::slice::from_raw_parts(ptr, len as usize) }
    };
    std::str::from_utf8(bytes).ok()
}

/// 在足够容纳 i64（含负号）的栈缓冲区中按标准 Display 格式化，再一次分配精确长度的托管字节。
/// 写出结果长度并返回 GC 管理地址；避免 String 转 Box 时额外缩容，不改变整数极值或负数文本。
#[unsafe(no_mangle)]
pub extern "C" fn int_to_string(value: i64, out_len: *mut u64) -> *const u8 {
    let mut buffer = [0u8; 20];
    let mut cursor = std::io::Cursor::new(buffer.as_mut_slice());
    std::io::Write::write_fmt(&mut cursor, format_args!("{value}"))
        .expect("i64 text fits in 20 bytes");
    let len = cursor.position() as usize;
    let ptr = gc_allocate_string(buffer[..len].into());
    unsafe {
        *out_len = len as u64;
    }
    ptr
}

/// 把浮点值格式化成新的运行时字符串，保持和 Rust `Display` 一致的输出格式。
#[unsafe(no_mangle)]
pub extern "C" fn float_to_string(value: f64, out_len: *mut u64) -> *const u8 {
    boxed_string_into_raw(value.to_string(), out_len)
}

/// 把布尔值格式化成 `true/false` 文本，供 `string(bool)` 内建函数返回。
#[unsafe(no_mangle)]
pub extern "C" fn bool_to_string(value: u8, out_len: *mut u64) -> *const u8 {
    boxed_string_into_raw((value != 0).to_string(), out_len)
}

/// 克隆原生字节区间到 GC 托管字符串；原生输入只在调用期间借用，返回值不能由原生代码释放。
#[unsafe(no_mangle)]
pub extern "C" fn mas_runtime_string_clone(ptr: *const u8, len: u64) -> MsString {
    if ptr.is_null() {
        abort_runtime("原生字符串指针不能为空");
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let boxed = bytes.to_vec().into_boxed_slice();
    let len = boxed.len() as u64;
    let ptr = gc_allocate_string(boxed);
    MsString { ptr, len }
}

/// 接收 C 风格错误消息并按统一中文运行时格式终止当前进程，供原生模块直接复用。
#[unsafe(no_mangle)]
pub extern "C" fn mas_runtime_abort(message: *const c_char) -> ! {
    if message.is_null() {
        abort_runtime("原生模块返回了空错误消息");
    }
    let text = unsafe { CStr::from_ptr(message) }
        .to_str()
        .unwrap_or("原生模块返回了非 UTF-8 错误消息");
    abort_runtime(text)
}

/// 以统一的运行时错误格式输出消息，并立刻终止当前进程。
#[cold]
#[inline(never)]
fn abort_runtime(message: &str) -> ! {
    eprintln!("运行时错误: {}", message);
    std::process::exit(1);
}

/// 以带源码定位的统一运行时错误格式输出消息，并立刻终止当前进程。
#[cold]
#[inline(never)]
fn abort_runtime_at(message: &str, site: RuntimeErrorSite) -> ! {
    let path = std::str::from_utf8(string_bytes(site.path)).unwrap_or("<invalid utf8>");
    let source = std::str::from_utf8(string_bytes(site.source)).unwrap_or("");
    let (line, column) = runtime_line_col(source, site.span_start as usize);
    let line_text = runtime_line_text(source, line);
    let caret_width = (site.span_end as usize)
        .saturating_sub(site.span_start as usize)
        .max(1);
    let caret_padding = " ".repeat(column.saturating_sub(1));
    let line_number = line.to_string();
    let line_number_padding = " ".repeat(line_number.len());
    let carets = "^".repeat(caret_width);
    eprintln!(
        "运行时错误: {message}\n--> {path}:{line}:{column}\n{line_number} | {line_text}\n{line_number_padding} | {caret_padding}{carets}"
    );
    std::process::exit(1);
}

/// 返回指定运行时字符串对应的字节切片，供比较、排序和哈希索引复用。
fn string_bytes<'a>(value: MsString) -> &'a [u8] {
    unsafe { std::slice::from_raw_parts(value.ptr, value.len as usize) }
}

/// 把字节偏移转换为 1 基的行号和列号，保持与编译期诊断一致的定位规则。
fn runtime_line_col(source: &str, position: usize) -> (usize, usize) {
    let clamped = position.min(source.len());
    let mut line = 1usize;
    let mut column = 1usize;

    for character in source[..clamped].chars() {
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }

    (line, column)
}

/// 返回指定行的源码文本，行号按 1 开始计算。
fn runtime_line_text<'a>(source: &'a str, target_line: usize) -> &'a str {
    source
        .lines()
        .nth(target_line.saturating_sub(1))
        .unwrap_or("")
}

/// 将同一静态键类型转换为无损位模式，保留浮点正负零及 NaN 位模式的既有区分。
/// 字符串使用独立字节索引，误入此路径时终止为运行时错误。
fn scalar_to_map_key(value: ScalarCell) -> MapKey {
    match value {
        ScalarCell::Reference(value) => value as usize as u64,
        ScalarCell::Int(value) => value as u64,
        ScalarCell::Float(value) => value.to_bits(),
        ScalarCell::Bool(value) => value as u64,
        ScalarCell::String(_) => abort_runtime("字符串键必须使用字节索引"),
    }
}

/// 把 ABI 透传的路径、源码和 span 参数组装成统一的运行时错误定位对象。
fn make_runtime_error_site(
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> RuntimeErrorSite {
    RuntimeErrorSite {
        path: MsString {
            ptr: path_ptr,
            len: path_len,
        },
        source: MsString {
            ptr: source_ptr,
            len: source_len,
        },
        span_start,
        span_end,
    }
}

/// 全局声明执行前的访问统一终止为语言错误，六个参数指定源码文本和访问范围。
#[unsafe(no_mangle)]
pub extern "C" fn mas_global_uninitialized(
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> ! {
    abort_runtime_at(
        "全局变量尚未初始化",
        make_runtime_error_site(
            path_ptr, path_len, source_ptr, source_len, span_start, span_end,
        ),
    )
}

/// 返回 UTF-8 字符串的 Unicode 标量数，组合字符分别计数；复用字面量和托管缓存，其他输入仍校验 UTF-8。
#[unsafe(no_mangle)]
pub extern "C" fn string_len(ptr: *const u8, len: u64) -> i64 {
    gc_string_scalar_count(ptr, len)
        .unwrap_or_else(|| parse_runtime_text(ptr, len).chars().count()) as i64
}
