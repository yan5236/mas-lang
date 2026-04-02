/// 返回构建阶段写入临时文件的最小 Rust 引导源码。
pub fn bootstrap_source() -> &'static str {
    r#"#![allow(unsafe_op_in_unsafe_fn)]

use std::collections::HashMap;

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

#[derive(Clone, Eq, Hash, PartialEq)]
enum MapKey {
    Int(i64),
    Float(u64),
    Bool(u8),
    String(Vec<u8>),
}

#[derive(Clone, Copy)]
struct RuntimeErrorSite {
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
    let ptr = Box::into_raw(boxed) as *const u8;
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

/// 把运行时创建的 UTF-8 文本缓冲区转换成返回给 LLVM 后端的稳定字符串 ABI。
fn boxed_string_into_raw(text: String, out_len: *mut u64) -> *const u8 {
    if out_len.is_null() {
        abort_runtime("字符串返回长度输出指针不能为空");
    }
    let boxed = text.into_bytes().into_boxed_slice();
    let len = boxed.len() as u64;
    let ptr = Box::into_raw(boxed) as *const u8;
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

/// 把整数值格式化成新的运行时字符串，供 `string(int)` 内建函数直接返回。
#[unsafe(no_mangle)]
pub extern "C" fn int_to_string(value: i64, out_len: *mut u64) -> *const u8 {
    boxed_string_into_raw(value.to_string(), out_len)
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

/// 以统一的运行时错误格式输出消息，并立刻终止当前进程。
fn abort_runtime(message: &str) -> ! {
    eprintln!("运行时错误: {}", message);
    std::process::exit(1);
}

/// 以带源码定位的统一运行时错误格式输出消息，并立刻终止当前进程。
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
    source.lines().nth(target_line.saturating_sub(1)).unwrap_or("")
}

/// 把运行时可复制的标量键转换为可放入哈希表的稳定键对象。
fn scalar_to_map_key(value: ScalarCell) -> MapKey {
    match value {
        ScalarCell::Int(value) => MapKey::Int(value),
        ScalarCell::Float(value) => MapKey::Float(value.to_bits()),
        ScalarCell::Bool(value) => MapKey::Bool(value),
        ScalarCell::String(value) => MapKey::String(string_bytes(value).to_vec()),
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

fn elem_size(elem_kind: u8) -> usize {
    match elem_kind {
        KIND_INT => std::mem::size_of::<i64>(),
        KIND_FLOAT => std::mem::size_of::<f64>(),
        KIND_BOOL => std::mem::size_of::<u8>(),
        KIND_STRING => std::mem::size_of::<MsString>(),
        _ => abort_runtime("未知的列表元素类型"),
    }
}

unsafe fn bytes_of<T: Copy>(value: &T) -> &[u8] {
    std::slice::from_raw_parts((value as *const T).cast::<u8>(), std::mem::size_of::<T>())
}

unsafe fn read_value<T: Copy>(bytes: &[u8], index: usize) -> T {
    let size = std::mem::size_of::<T>();
    let offset = index * size;
    std::ptr::read_unaligned(bytes[offset..offset + size].as_ptr().cast::<T>())
}

unsafe fn write_values<T: Copy>(buffer: &mut Vec<u8>, values: &[T]) {
    buffer.clear();
    buffer.reserve(values.len() * std::mem::size_of::<T>());
    for value in values {
        buffer.extend_from_slice(bytes_of(value));
    }
}

unsafe fn with_list_bytes_mut<R>(list: *mut MsList, callback: impl FnOnce(&mut MsList, &mut Vec<u8>) -> R) -> R {
    let list = &mut *list;
    let size = elem_size(list.elem_kind);
    let len_bytes = list.len as usize * size;
    let capacity_bytes = list.capacity as usize * size;
    let mut buffer = if list.data.is_null() || capacity_bytes == 0 {
        Vec::new()
    } else {
        Vec::from_raw_parts(list.data, len_bytes, capacity_bytes)
    };
    let result = callback(list, &mut buffer);
    list.data = if buffer.capacity() == 0 {
        std::ptr::null_mut()
    } else {
        buffer.as_mut_ptr()
    };
    list.capacity = if size == 0 { 0 } else { (buffer.capacity() / size) as u64 };
    std::mem::forget(buffer);
    result
}

unsafe fn list_bytes<'a>(list: *const MsList) -> &'a [u8] {
    let list = &*list;
    let size = elem_size(list.elem_kind);
    if list.data.is_null() || list.len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(list.data, list.len as usize * size)
    }
}

unsafe fn push_value<T: Copy>(list: *mut MsList, value: T) {
    with_list_bytes_mut(list, |list, buffer| {
        buffer.extend_from_slice(bytes_of(&value));
        list.len += 1;
    });
}

/// 从列表里按索引读取单个元素，越界时直接抛出统一运行时错误。
unsafe fn get_value<T: Copy>(list: *const MsList, index: u64) -> T {
    let list_ref = &*list;
    if index >= list_ref.len {
        abort_runtime("列表索引越界");
    }
    read_value::<T>(list_bytes(list), index as usize)
}

/// 把字符串值拆成 ABI 稳定的指针返回值与长度输出参数。
unsafe fn write_string_result(value: MsString, out_len: *mut u64) -> *const u8 {
    if out_len.is_null() {
        abort_runtime("字符串返回长度输出指针不能为空");
    }
    *out_len = value.len;
    value.ptr
}

/// 从列表中删除指定位置的元素，并同步更新长度信息。
unsafe fn remove_at(list: *mut MsList, index: u64) {
    with_list_bytes_mut(list, |list, buffer| {
        if index >= list.len {
            abort_runtime("列表索引越界");
        }
        let size = elem_size(list.elem_kind);
        let start = index as usize * size;
        let end = start + size;
        buffer.drain(start..end);
        list.len -= 1;
    });
}

fn strings_equal(left: MsString, right: MsString) -> bool {
    string_bytes(left) == string_bytes(right)
}

fn cmp_strings(left: MsString, right: MsString) -> std::cmp::Ordering {
    string_bytes(left).cmp(string_bytes(right))
}

/// 使用哈希索引查询字典槽位下标，避免每次读取都线性扫描整个字典。
unsafe fn map_find_index(map: *const MsMap, key: ScalarCell) -> Option<usize> {
    (*map).indexes.get(&scalar_to_map_key(key)).copied()
}

/// 写入或更新字典元素，同时维护插入顺序数组与哈希索引的一致性。
unsafe fn map_set_cell(map: *mut MsMap, key: ScalarCell, value: ScalarCell) {
    let map = &mut *map;
    if let Some(index) = map_find_index(map, key) {
        if let Some((_, entry_value)) = map.entries.get_mut(index).and_then(Option::as_mut) {
            *entry_value = value;
        } else {
            abort_runtime("内部字典索引失效");
        }
    } else {
        let index = map.entries.len();
        map.entries.push(Some((key, value)));
        map.indexes.insert(scalar_to_map_key(key), index);
        map.len += 1;
    }
}

/// 读取字典中的值；若键不存在则输出带源码定位的运行时错误。
unsafe fn map_get_cell(map: *const MsMap, key: ScalarCell, site: RuntimeErrorSite) -> ScalarCell {
    let index = map_find_index(map, key).unwrap_or_else(|| abort_runtime_at("字典中不存在目标键", site));
    (&(*map).entries)
        .get(index)
        .and_then(|entry| *entry)
        .map(|(_, value)| value)
        .unwrap_or_else(|| abort_runtime("内部字典索引失效"))
}

/// 删除字典元素时仅清空槽位并移除哈希索引，避免为保持顺序而整体搬移数据。
unsafe fn map_remove_cell(map: *mut MsMap, key: ScalarCell, site: RuntimeErrorSite) {
    let map = &mut *map;
    let map_key = scalar_to_map_key(key);
    let index = map
        .indexes
        .remove(&map_key)
        .unwrap_or_else(|| abort_runtime_at("字典中不存在要删除的键", site));
    let removed = map.entries.get_mut(index).and_then(Option::take);
    if removed.is_none() {
        abort_runtime("内部字典索引失效");
    }
    map.len -= 1;
}

/// 判断目标键是否存在于字典中，直接复用哈希索引结果。
unsafe fn map_contains_cell(map: *const MsMap, key: ScalarCell) -> u8 {
    u8::from(map_find_index(map, key).is_some())
}

#[unsafe(no_mangle)]
pub extern "C" fn list_create(elem_kind: u8) -> *mut MsList {
    Box::into_raw(Box::new(MsList {
        elem_kind,
        len: 0,
        capacity: 0,
        data: std::ptr::null_mut(),
    }))
}

#[unsafe(no_mangle)]
pub extern "C" fn list_len(list: *const MsList) -> u64 {
    unsafe { (*list).len }
}

#[unsafe(no_mangle)]
pub extern "C" fn list_slice(list: *const MsList, start: u64, end: u64) -> *mut MsList {
    let list_ref = unsafe { &*list };
    if start > end || end > list_ref.len {
        abort_runtime("列表切片范围越界");
    }
    let size = elem_size(list_ref.elem_kind);
    let bytes = unsafe { list_bytes(list) };
    let start_byte = start as usize * size;
    let end_byte = end as usize * size;
    let target = list_create(list_ref.elem_kind);
    unsafe {
        with_list_bytes_mut(target, |target, buffer| {
            buffer.extend_from_slice(&bytes[start_byte..end_byte]);
            target.len = end - start;
        });
    }
    target
}

#[unsafe(no_mangle)]
pub extern "C" fn list_reverse(list: *mut MsList) {
    unsafe {
        with_list_bytes_mut(list, |list, buffer| {
            let size = elem_size(list.elem_kind);
            let original = buffer.clone();
            for index in 0..list.len as usize {
                let src = (list.len as usize - 1 - index) * size;
                let dst = index * size;
                buffer[dst..dst + size].copy_from_slice(&original[src..src + size]);
            }
        });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn list_append_int(list: *mut MsList, value: i64) {
    unsafe { push_value(list, value) }
}

#[unsafe(no_mangle)]
pub extern "C" fn list_append_float(list: *mut MsList, value: f64) {
    unsafe { push_value(list, value) }
}

#[unsafe(no_mangle)]
pub extern "C" fn list_append_bool(list: *mut MsList, value: u8) {
    unsafe { push_value(list, value) }
}

#[unsafe(no_mangle)]
pub extern "C" fn list_append_string(list: *mut MsList, ptr: *const u8, len: u64) {
    unsafe { push_value(list, MsString { ptr, len }) }
}

#[unsafe(no_mangle)]
pub extern "C" fn list_get_int(list: *const MsList, index: u64) -> i64 {
    unsafe { get_value::<i64>(list, index) }
}

#[unsafe(no_mangle)]
pub extern "C" fn list_get_float(list: *const MsList, index: u64) -> f64 {
    unsafe { get_value::<f64>(list, index) }
}

#[unsafe(no_mangle)]
pub extern "C" fn list_get_bool(list: *const MsList, index: u64) -> u8 {
    unsafe { get_value::<u8>(list, index) }
}

#[unsafe(no_mangle)]
pub extern "C" fn list_get_string(list: *const MsList, index: u64, out_len: *mut u64) -> *const u8 {
    let value = unsafe { get_value::<MsString>(list, index) };
    unsafe { write_string_result(value, out_len) }
}

#[unsafe(no_mangle)]
pub extern "C" fn list_remove_int(list: *mut MsList, value: i64) {
    let bytes = unsafe { list_bytes(list) };
    let len = unsafe { (*list).len };
    for index in 0..len {
        if unsafe { read_value::<i64>(bytes, index as usize) } == value {
            unsafe { remove_at(list, index) };
            return;
        }
    }
    abort_runtime("列表中不存在要删除的元素");
}

#[unsafe(no_mangle)]
pub extern "C" fn list_remove_float(list: *mut MsList, value: f64) {
    let bytes = unsafe { list_bytes(list) };
    let len = unsafe { (*list).len };
    for index in 0..len {
        if unsafe { read_value::<f64>(bytes, index as usize) } == value {
            unsafe { remove_at(list, index) };
            return;
        }
    }
    abort_runtime("列表中不存在要删除的元素");
}

#[unsafe(no_mangle)]
pub extern "C" fn list_remove_bool(list: *mut MsList, value: u8) {
    let bytes = unsafe { list_bytes(list) };
    let len = unsafe { (*list).len };
    for index in 0..len {
        if unsafe { read_value::<u8>(bytes, index as usize) } == value {
            unsafe { remove_at(list, index) };
            return;
        }
    }
    abort_runtime("列表中不存在要删除的元素");
}

#[unsafe(no_mangle)]
pub extern "C" fn list_remove_string(list: *mut MsList, ptr: *const u8, len: u64) {
    let bytes = unsafe { list_bytes(list) };
    let len_items = unsafe { (*list).len };
    let target = MsString { ptr, len };
    for index in 0..len_items {
        if strings_equal(unsafe { read_value::<MsString>(bytes, index as usize) }, target) {
            unsafe { remove_at(list, index) };
            return;
        }
    }
    abort_runtime("列表中不存在要删除的元素");
}

#[unsafe(no_mangle)]
pub extern "C" fn list_pop_int(list: *mut MsList) -> i64 {
    list_pop_at_int(list, unsafe { (*list).len.saturating_sub(1) })
}

#[unsafe(no_mangle)]
pub extern "C" fn list_pop_float(list: *mut MsList) -> f64 {
    list_pop_at_float(list, unsafe { (*list).len.saturating_sub(1) })
}

#[unsafe(no_mangle)]
pub extern "C" fn list_pop_bool(list: *mut MsList) -> u8 {
    list_pop_at_bool(list, unsafe { (*list).len.saturating_sub(1) })
}

#[unsafe(no_mangle)]
pub extern "C" fn list_pop_string(list: *mut MsList, out_len: *mut u64) -> *const u8 {
    list_pop_at_string(list, unsafe { (*list).len.saturating_sub(1) }, out_len)
}

#[unsafe(no_mangle)]
pub extern "C" fn list_pop_at_int(list: *mut MsList, index: u64) -> i64 {
    let value = unsafe { get_value::<i64>(list, index) };
    unsafe { remove_at(list, index) };
    value
}

#[unsafe(no_mangle)]
pub extern "C" fn list_pop_at_float(list: *mut MsList, index: u64) -> f64 {
    let value = unsafe { get_value::<f64>(list, index) };
    unsafe { remove_at(list, index) };
    value
}

#[unsafe(no_mangle)]
pub extern "C" fn list_pop_at_bool(list: *mut MsList, index: u64) -> u8 {
    let value = unsafe { get_value::<u8>(list, index) };
    unsafe { remove_at(list, index) };
    value
}

#[unsafe(no_mangle)]
pub extern "C" fn list_pop_at_string(list: *mut MsList, index: u64, out_len: *mut u64) -> *const u8 {
    let value = unsafe { get_value::<MsString>(list, index) };
    unsafe { remove_at(list, index) };
    unsafe { write_string_result(value, out_len) }
}

#[unsafe(no_mangle)]
pub extern "C" fn list_sort_int(list: *mut MsList) {
    unsafe {
        with_list_bytes_mut(list, |list, buffer| {
            let mut values = (0..list.len as usize)
                .map(|index| read_value::<i64>(buffer, index))
                .collect::<Vec<_>>();
            values.sort();
            write_values(buffer, &values);
        });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn list_sort_float(list: *mut MsList) {
    unsafe {
        with_list_bytes_mut(list, |list, buffer| {
            let mut values = (0..list.len as usize)
                .map(|index| read_value::<f64>(buffer, index))
                .collect::<Vec<_>>();
            values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
            write_values(buffer, &values);
        });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn list_sort_bool(list: *mut MsList) {
    unsafe {
        with_list_bytes_mut(list, |list, buffer| {
            let mut values = (0..list.len as usize)
                .map(|index| read_value::<u8>(buffer, index))
                .collect::<Vec<_>>();
            values.sort();
            write_values(buffer, &values);
        });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn list_sort_string(list: *mut MsList) {
    unsafe {
        with_list_bytes_mut(list, |list, buffer| {
            let mut values = (0..list.len as usize)
                .map(|index| read_value::<MsString>(buffer, index))
                .collect::<Vec<_>>();
            values.sort_by(|left, right| cmp_strings(*left, *right));
            write_values(buffer, &values);
        });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn list_index_int(list: *const MsList, value: i64) -> i64 {
    let bytes = unsafe { list_bytes(list) };
    let len = unsafe { (*list).len };
    for index in 0..len {
        if unsafe { read_value::<i64>(bytes, index as usize) } == value {
            return index as i64;
        }
    }
    abort_runtime("列表中不存在目标元素");
}

#[unsafe(no_mangle)]
pub extern "C" fn list_index_float(list: *const MsList, value: f64) -> i64 {
    let bytes = unsafe { list_bytes(list) };
    let len = unsafe { (*list).len };
    for index in 0..len {
        if unsafe { read_value::<f64>(bytes, index as usize) } == value {
            return index as i64;
        }
    }
    abort_runtime("列表中不存在目标元素");
}

#[unsafe(no_mangle)]
pub extern "C" fn list_index_bool(list: *const MsList, value: u8) -> i64 {
    let bytes = unsafe { list_bytes(list) };
    let len = unsafe { (*list).len };
    for index in 0..len {
        if unsafe { read_value::<u8>(bytes, index as usize) } == value {
            return index as i64;
        }
    }
    abort_runtime("列表中不存在目标元素");
}

#[unsafe(no_mangle)]
pub extern "C" fn list_index_string(list: *const MsList, ptr: *const u8, len: u64) -> i64 {
    let bytes = unsafe { list_bytes(list) };
    let len_items = unsafe { (*list).len };
    let target = MsString { ptr, len };
    for index in 0..len_items {
        if strings_equal(unsafe { read_value::<MsString>(bytes, index as usize) }, target) {
            return index as i64;
        }
    }
    abort_runtime("列表中不存在目标元素");
}

#[unsafe(no_mangle)]
pub extern "C" fn list_contains_int(list: *const MsList, value: i64) -> u8 {
    u8::from((0..unsafe { (*list).len }).any(|index| unsafe { read_value::<i64>(list_bytes(list), index as usize) } == value))
}

#[unsafe(no_mangle)]
pub extern "C" fn list_contains_float(list: *const MsList, value: f64) -> u8 {
    u8::from((0..unsafe { (*list).len }).any(|index| unsafe { read_value::<f64>(list_bytes(list), index as usize) } == value))
}

#[unsafe(no_mangle)]
pub extern "C" fn list_contains_bool(list: *const MsList, value: u8) -> u8 {
    u8::from((0..unsafe { (*list).len }).any(|index| unsafe { read_value::<u8>(list_bytes(list), index as usize) } == value))
}

#[unsafe(no_mangle)]
pub extern "C" fn list_contains_string(list: *const MsList, ptr: *const u8, len: u64) -> u8 {
    let target = MsString { ptr, len };
    u8::from((0..unsafe { (*list).len }).any(|index| strings_equal(unsafe { read_value::<MsString>(list_bytes(list), index as usize) }, target)))
}

#[unsafe(no_mangle)]
pub extern "C" fn map_create(key_kind: u8, value_kind: u8) -> *mut MsMap {
    Box::into_raw(Box::new(MsMap {
        key_kind,
        value_kind,
        len: 0,
        entries: Vec::new(),
        indexes: HashMap::new(),
    }))
}

#[unsafe(no_mangle)]
pub extern "C" fn map_len(map: *const MsMap) -> u64 {
    unsafe { (*map).len }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_keys(map: *const MsMap) -> *mut MsList {
    let list = unsafe { list_create((*map).key_kind) };
    for entry in unsafe { &(*map).entries } {
        if let Some((key, _)) = entry {
            unsafe {
                match key {
                    ScalarCell::Int(value) => push_value(list, *value),
                    ScalarCell::Float(value) => push_value(list, *value),
                    ScalarCell::Bool(value) => push_value(list, *value),
                    ScalarCell::String(value) => push_value(list, *value),
                }
            }
        }
    }
    list
}

#[unsafe(no_mangle)]
pub extern "C" fn map_values(map: *const MsMap) -> *mut MsList {
    let list = unsafe { list_create((*map).value_kind) };
    for entry in unsafe { &(*map).entries } {
        if let Some((_, value)) = entry {
            unsafe {
                match value {
                    ScalarCell::Int(value) => push_value(list, *value),
                    ScalarCell::Float(value) => push_value(list, *value),
                    ScalarCell::Bool(value) => push_value(list, *value),
                    ScalarCell::String(value) => push_value(list, *value),
                }
            }
        }
    }
    list
}

macro_rules! define_map_remove {
    ($name:ident, $variant:ident, $ty:ty) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(
            map: *mut MsMap,
            key: $ty,
            path_ptr: *const u8,
            path_len: u64,
            source_ptr: *const u8,
            source_len: u64,
            span_start: u64,
            span_end: u64,
        ) {
            let site = make_runtime_error_site(
                path_ptr, path_len, source_ptr, source_len, span_start, span_end,
            );
            unsafe { map_remove_cell(map, ScalarCell::$variant(key), site) }
        }
    };
}

macro_rules! define_map_contains {
    ($name:ident, $variant:ident, $ty:ty) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(map: *const MsMap, key: $ty) -> u8 {
            unsafe { map_contains_cell(map, ScalarCell::$variant(key)) }
        }
    };
}

macro_rules! define_map_set {
    ($name:ident, $key_variant:ident, $key_ty:ty, $value_variant:ident, $value_ty:ty) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(map: *mut MsMap, key: $key_ty, value: $value_ty) {
            unsafe { map_set_cell(map, ScalarCell::$key_variant(key), ScalarCell::$value_variant(value)) }
        }
    };
}

macro_rules! define_map_get {
    ($name:ident, $key_variant:ident, $key_ty:ty, $value_variant:ident, $value_ty:ty) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(
            map: *const MsMap,
            key: $key_ty,
            path_ptr: *const u8,
            path_len: u64,
            source_ptr: *const u8,
            source_len: u64,
            span_start: u64,
            span_end: u64,
        ) -> $value_ty {
            let site = make_runtime_error_site(
                path_ptr, path_len, source_ptr, source_len, span_start, span_end,
            );
            let value = unsafe { map_get_cell(map, ScalarCell::$key_variant(key), site) };
            match value {
                ScalarCell::$value_variant(value) => value,
                _ => abort_runtime("内部字典值类型不匹配"),
            }
        }
    };
}

define_map_remove!(map_remove_int, Int, i64);
define_map_remove!(map_remove_float, Float, f64);
define_map_remove!(map_remove_bool, Bool, u8);

define_map_contains!(map_contains_int, Int, i64);
define_map_contains!(map_contains_float, Float, f64);
define_map_contains!(map_contains_bool, Bool, u8);

define_map_set!(map_set_int_int, Int, i64, Int, i64);
define_map_set!(map_set_int_float, Int, i64, Float, f64);
define_map_set!(map_set_int_bool, Int, i64, Bool, u8);
define_map_set!(map_set_float_int, Float, f64, Int, i64);
define_map_set!(map_set_float_float, Float, f64, Float, f64);
define_map_set!(map_set_float_bool, Float, f64, Bool, u8);
define_map_set!(map_set_bool_int, Bool, u8, Int, i64);
define_map_set!(map_set_bool_float, Bool, u8, Float, f64);
define_map_set!(map_set_bool_bool, Bool, u8, Bool, u8);

define_map_get!(map_get_int_int, Int, i64, Int, i64);
define_map_get!(map_get_int_float, Int, i64, Float, f64);
define_map_get!(map_get_int_bool, Int, i64, Bool, u8);
define_map_get!(map_get_float_int, Float, f64, Int, i64);
define_map_get!(map_get_float_float, Float, f64, Float, f64);
define_map_get!(map_get_float_bool, Float, f64, Bool, u8);
define_map_get!(map_get_bool_int, Bool, u8, Int, i64);
define_map_get!(map_get_bool_float, Bool, u8, Float, f64);
define_map_get!(map_get_bool_bool, Bool, u8, Bool, u8);
#[unsafe(no_mangle)]
pub extern "C" fn map_remove_string(
    map: *mut MsMap,
    ptr: *const u8,
    len: u64,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) {
    let site = make_runtime_error_site(path_ptr, path_len, source_ptr, source_len, span_start, span_end);
    unsafe { map_remove_cell(map, ScalarCell::String(MsString { ptr, len }), site) }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_contains_string(map: *const MsMap, ptr: *const u8, len: u64) -> u8 {
    unsafe { map_contains_cell(map, ScalarCell::String(MsString { ptr, len })) }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_set_int_string(map: *mut MsMap, key: i64, ptr: *const u8, len: u64) {
    unsafe { map_set_cell(map, ScalarCell::Int(key), ScalarCell::String(MsString { ptr, len })) }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_set_float_string(map: *mut MsMap, key: f64, ptr: *const u8, len: u64) {
    unsafe { map_set_cell(map, ScalarCell::Float(key), ScalarCell::String(MsString { ptr, len })) }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_set_bool_string(map: *mut MsMap, key: u8, ptr: *const u8, len: u64) {
    unsafe { map_set_cell(map, ScalarCell::Bool(key), ScalarCell::String(MsString { ptr, len })) }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_set_string_int(map: *mut MsMap, ptr: *const u8, len: u64, value: i64) {
    unsafe { map_set_cell(map, ScalarCell::String(MsString { ptr, len }), ScalarCell::Int(value)) }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_set_string_float(map: *mut MsMap, ptr: *const u8, len: u64, value: f64) {
    unsafe { map_set_cell(map, ScalarCell::String(MsString { ptr, len }), ScalarCell::Float(value)) }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_set_string_bool(map: *mut MsMap, ptr: *const u8, len: u64, value: u8) {
    unsafe { map_set_cell(map, ScalarCell::String(MsString { ptr, len }), ScalarCell::Bool(value)) }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_set_string_string(
    map: *mut MsMap,
    key_ptr: *const u8,
    key_len: u64,
    value_ptr: *const u8,
    value_len: u64,
) {
    unsafe {
        map_set_cell(
            map,
            ScalarCell::String(MsString { ptr: key_ptr, len: key_len }),
            ScalarCell::String(MsString { ptr: value_ptr, len: value_len }),
        )
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_get_int_string(
    map: *const MsMap,
    key: i64,
    out_len: *mut u64,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> *const u8 {
    let site = make_runtime_error_site(path_ptr, path_len, source_ptr, source_len, span_start, span_end);
    let value = unsafe { map_get_cell(map, ScalarCell::Int(key), site) };
    match value {
        ScalarCell::String(value) => unsafe { write_string_result(value, out_len) },
        _ => abort_runtime("内部字典值类型不匹配"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_get_float_string(
    map: *const MsMap,
    key: f64,
    out_len: *mut u64,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> *const u8 {
    let site = make_runtime_error_site(path_ptr, path_len, source_ptr, source_len, span_start, span_end);
    let value = unsafe { map_get_cell(map, ScalarCell::Float(key), site) };
    match value {
        ScalarCell::String(value) => unsafe { write_string_result(value, out_len) },
        _ => abort_runtime("内部字典值类型不匹配"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_get_bool_string(
    map: *const MsMap,
    key: u8,
    out_len: *mut u64,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> *const u8 {
    let site = make_runtime_error_site(path_ptr, path_len, source_ptr, source_len, span_start, span_end);
    let value = unsafe { map_get_cell(map, ScalarCell::Bool(key), site) };
    match value {
        ScalarCell::String(value) => unsafe { write_string_result(value, out_len) },
        _ => abort_runtime("内部字典值类型不匹配"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_get_string_int(
    map: *const MsMap,
    ptr: *const u8,
    len: u64,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> i64 {
    let site = make_runtime_error_site(path_ptr, path_len, source_ptr, source_len, span_start, span_end);
    let value = unsafe { map_get_cell(map, ScalarCell::String(MsString { ptr, len }), site) };
    match value {
        ScalarCell::Int(value) => value,
        _ => abort_runtime("内部字典值类型不匹配"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_get_string_float(
    map: *const MsMap,
    ptr: *const u8,
    len: u64,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> f64 {
    let site = make_runtime_error_site(path_ptr, path_len, source_ptr, source_len, span_start, span_end);
    let value = unsafe { map_get_cell(map, ScalarCell::String(MsString { ptr, len }), site) };
    match value {
        ScalarCell::Float(value) => value,
        _ => abort_runtime("内部字典值类型不匹配"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_get_string_bool(
    map: *const MsMap,
    ptr: *const u8,
    len: u64,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> u8 {
    let site = make_runtime_error_site(path_ptr, path_len, source_ptr, source_len, span_start, span_end);
    let value = unsafe { map_get_cell(map, ScalarCell::String(MsString { ptr, len }), site) };
    match value {
        ScalarCell::Bool(value) => value,
        _ => abort_runtime("内部字典值类型不匹配"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_get_string_string(
    map: *const MsMap,
    ptr: *const u8,
    len: u64,
    out_len: *mut u64,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> *const u8 {
    let site = make_runtime_error_site(path_ptr, path_len, source_ptr, source_len, span_start, span_end);
    let value = unsafe { map_get_cell(map, ScalarCell::String(MsString { ptr, len }), site) };
    match value {
        ScalarCell::String(value) => unsafe { write_string_result(value, out_len) },
        _ => abort_runtime("内部字典值类型不匹配"),
    }
}

unsafe extern "C" {
    fn mascript_entry();
}

fn main() {
    unsafe {
        mascript_entry();
    }
}
"#
}

#[cfg(test)]
mod tests {
    use super::bootstrap_source;

    /// 校验引导源码以 UTF-8 无 BOM 形式提供给写盘层。
    #[test]
    fn should_not_include_utf8_bom() {
        let bytes = bootstrap_source().as_bytes();
        assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
    }
}
