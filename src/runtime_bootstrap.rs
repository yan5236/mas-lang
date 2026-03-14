/// 返回构建阶段写入临时文件的最小 Rust 引导源码。
pub fn bootstrap_source() -> &'static str {
    r#"#![allow(unsafe_op_in_unsafe_fn)]

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

fn panic_runtime(message: &str) -> ! {
    panic!("{}", message);
}

fn elem_size(elem_kind: u8) -> usize {
    match elem_kind {
        KIND_INT => std::mem::size_of::<i64>(),
        KIND_FLOAT => std::mem::size_of::<f64>(),
        KIND_BOOL => std::mem::size_of::<u8>(),
        KIND_STRING => std::mem::size_of::<MsString>(),
        _ => panic_runtime("未知的列表元素类型"),
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

unsafe fn get_value<T: Copy>(list: *const MsList, index: u64) -> T {
    let list_ref = &*list;
    if index >= list_ref.len {
        panic_runtime("列表索引越界");
    }
    read_value::<T>(list_bytes(list), index as usize)
}

unsafe fn remove_at(list: *mut MsList, index: u64) {
    with_list_bytes_mut(list, |list, buffer| {
        if index >= list.len {
            panic_runtime("列表索引越界");
        }
        let size = elem_size(list.elem_kind);
        let start = index as usize * size;
        let end = start + size;
        buffer.drain(start..end);
        list.len -= 1;
    });
}

fn strings_equal(left: MsString, right: MsString) -> bool {
    let left_bytes = unsafe { std::slice::from_raw_parts(left.ptr, left.len as usize) };
    let right_bytes = unsafe { std::slice::from_raw_parts(right.ptr, right.len as usize) };
    left_bytes == right_bytes
}

fn cmp_strings(left: MsString, right: MsString) -> std::cmp::Ordering {
    let left_bytes = unsafe { std::slice::from_raw_parts(left.ptr, left.len as usize) };
    let right_bytes = unsafe { std::slice::from_raw_parts(right.ptr, right.len as usize) };
    left_bytes.cmp(right_bytes)
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
        panic_runtime("列表切片范围越界");
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
pub extern "C" fn list_get_string(list: *const MsList, index: u64) -> MsString {
    unsafe { get_value::<MsString>(list, index) }
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
    panic_runtime("列表中不存在要删除的元素");
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
    panic_runtime("列表中不存在要删除的元素");
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
    panic_runtime("列表中不存在要删除的元素");
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
    panic_runtime("列表中不存在要删除的元素");
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
pub extern "C" fn list_pop_string(list: *mut MsList) -> MsString {
    list_pop_at_string(list, unsafe { (*list).len.saturating_sub(1) })
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
pub extern "C" fn list_pop_at_string(list: *mut MsList, index: u64) -> MsString {
    let value = unsafe { get_value::<MsString>(list, index) };
    unsafe { remove_at(list, index) };
    value
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
    panic_runtime("列表中不存在目标元素");
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
    panic_runtime("列表中不存在目标元素");
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
    panic_runtime("列表中不存在目标元素");
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
    panic_runtime("列表中不存在目标元素");
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
