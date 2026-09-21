use std::collections::HashMap;
use std::ffi::{CStr, c_char};

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

#[unsafe(no_mangle)]
pub extern "C" fn print_int(value: i64) {
    println!("{}", value);
}

#[unsafe(no_mangle)]
pub extern "C" fn print_float(value: f64) {
    println!("{}", value);
}

#[unsafe(no_mangle)]
pub extern "C" fn print_bool(value: u8) {
    println!("{}", value != 0);
}

#[unsafe(no_mangle)]
pub extern "C" fn print_string(ptr: *const u8, len: u64) {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let text = std::str::from_utf8(bytes).unwrap_or("<invalid utf8>");
    println!("{}", text);
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

/// 把 Mascript 的运行时字符串解析为 UTF-8 文本切片，供转换函数统一复用。
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

/// 尝试把字符串解析为 `int`，成功时写入输出参数，失败时仅返回 `false`。
#[unsafe(no_mangle)]
pub extern "C" fn try_string_to_int(ptr: *const u8, len: u64, out_value: *mut i64) -> u8 {
    if out_value.is_null() {
        abort_runtime("安全整数转换的输出指针不能为空");
    }
    let text = parse_runtime_text(ptr, len);
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

/// 尝试把字符串解析为 `float`，成功时写入输出参数，失败时仅返回 `false`。
#[unsafe(no_mangle)]
pub extern "C" fn try_string_to_float(ptr: *const u8, len: u64, out_value: *mut f64) -> u8 {
    if out_value.is_null() {
        abort_runtime("安全浮点转换的输出指针不能为空");
    }
    let text = parse_runtime_text(ptr, len);
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

/// 尝试把字符串解析为 `bool`，仅 `true/false` 两种文本视为成功。
#[unsafe(no_mangle)]
pub extern "C" fn try_string_to_bool(ptr: *const u8, len: u64, out_value: *mut u8) -> u8 {
    if out_value.is_null() {
        abort_runtime("安全布尔转换的输出指针不能为空");
    }
    let text = parse_runtime_text(ptr, len);
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

/// 返回 UTF-8 字符串的 Unicode 标量数量；组合字符分别计数，不修改字符串或触发 GC。
#[unsafe(no_mangle)]
pub extern "C" fn string_len(ptr: *const u8, len: u64) -> i64 {
    parse_runtime_text(ptr, len).chars().count() as i64
}
