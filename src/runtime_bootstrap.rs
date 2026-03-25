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

#[derive(Clone, Copy)]
enum ScalarCell {
    Int(i64),
    Float(f64),
    Bool(u8),
    String(MsString),
}

#[repr(C)]
pub struct MsMap {
    key_kind: u8,
    value_kind: u8,
    entries: Vec<(ScalarCell, ScalarCell)>,
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

/// 把字符串值拆成 ABI 稳定的指针返回值与长度输出参数。
unsafe fn write_string_result(value: MsString, out_len: *mut u64) -> *const u8 {
    if out_len.is_null() {
        panic_runtime("字符串返回长度输出指针不能为空");
    }
    *out_len = value.len;
    value.ptr
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

fn scalar_equals(left: ScalarCell, right: ScalarCell) -> bool {
    match (left, right) {
        (ScalarCell::Int(left), ScalarCell::Int(right)) => left == right,
        (ScalarCell::Float(left), ScalarCell::Float(right)) => left == right,
        (ScalarCell::Bool(left), ScalarCell::Bool(right)) => left == right,
        (ScalarCell::String(left), ScalarCell::String(right)) => strings_equal(left, right),
        _ => false,
    }
}

unsafe fn map_find_index(map: *const MsMap, key: ScalarCell) -> Option<usize> {
    (*map)
        .entries
        .iter()
        .position(|(entry_key, _)| scalar_equals(*entry_key, key))
}

unsafe fn map_set_cell(map: *mut MsMap, key: ScalarCell, value: ScalarCell) {
    let map = &mut *map;
    if let Some(index) = map.entries.iter().position(|(entry_key, _)| scalar_equals(*entry_key, key)) {
        map.entries[index].1 = value;
    } else {
        map.entries.push((key, value));
    }
}

unsafe fn map_get_cell(map: *const MsMap, key: ScalarCell) -> ScalarCell {
    let index = map_find_index(map, key).unwrap_or_else(|| panic_runtime("字典中不存在目标键"));
    (&(*map).entries)[index].1
}

unsafe fn map_remove_cell(map: *mut MsMap, key: ScalarCell) {
    let index = map_find_index(map, key).unwrap_or_else(|| panic_runtime("字典中不存在要删除的键"));
    (*map).entries.remove(index);
}

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

#[unsafe(no_mangle)]
pub extern "C" fn map_create(key_kind: u8, value_kind: u8) -> *mut MsMap {
    Box::into_raw(Box::new(MsMap {
        key_kind,
        value_kind,
        entries: Vec::new(),
    }))
}

#[unsafe(no_mangle)]
pub extern "C" fn map_len(map: *const MsMap) -> u64 {
    unsafe { (*map).entries.len() as u64 }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_keys(map: *const MsMap) -> *mut MsList {
    let list = unsafe { list_create((*map).key_kind) };
    for (key, _) in unsafe { &(*map).entries } {
        unsafe {
            match key {
                ScalarCell::Int(value) => push_value(list, *value),
                ScalarCell::Float(value) => push_value(list, *value),
                ScalarCell::Bool(value) => push_value(list, *value),
                ScalarCell::String(value) => push_value(list, *value),
            }
        }
    }
    list
}

#[unsafe(no_mangle)]
pub extern "C" fn map_values(map: *const MsMap) -> *mut MsList {
    let list = unsafe { list_create((*map).value_kind) };
    for (_, value) in unsafe { &(*map).entries } {
        unsafe {
            match value {
                ScalarCell::Int(value) => push_value(list, *value),
                ScalarCell::Float(value) => push_value(list, *value),
                ScalarCell::Bool(value) => push_value(list, *value),
                ScalarCell::String(value) => push_value(list, *value),
            }
        }
    }
    list
}

macro_rules! define_map_remove {
    ($name:ident, $variant:ident, $ty:ty) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(map: *mut MsMap, key: $ty) {
            unsafe { map_remove_cell(map, ScalarCell::$variant(key)) }
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
        pub extern "C" fn $name(map: *const MsMap, key: $key_ty) -> $value_ty {
            let value = unsafe { map_get_cell(map, ScalarCell::$key_variant(key)) };
            match value {
                ScalarCell::$value_variant(value) => value,
                _ => panic_runtime("内部字典值类型不匹配"),
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
pub extern "C" fn map_remove_string(map: *mut MsMap, ptr: *const u8, len: u64) {
    unsafe { map_remove_cell(map, ScalarCell::String(MsString { ptr, len })) }
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
pub extern "C" fn map_get_int_string(map: *const MsMap, key: i64, out_len: *mut u64) -> *const u8 {
    let value = unsafe { map_get_cell(map, ScalarCell::Int(key)) };
    match value {
        ScalarCell::String(value) => unsafe { write_string_result(value, out_len) },
        _ => panic_runtime("内部字典值类型不匹配"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_get_float_string(map: *const MsMap, key: f64, out_len: *mut u64) -> *const u8 {
    let value = unsafe { map_get_cell(map, ScalarCell::Float(key)) };
    match value {
        ScalarCell::String(value) => unsafe { write_string_result(value, out_len) },
        _ => panic_runtime("内部字典值类型不匹配"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_get_bool_string(map: *const MsMap, key: u8, out_len: *mut u64) -> *const u8 {
    let value = unsafe { map_get_cell(map, ScalarCell::Bool(key)) };
    match value {
        ScalarCell::String(value) => unsafe { write_string_result(value, out_len) },
        _ => panic_runtime("内部字典值类型不匹配"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_get_string_int(map: *const MsMap, ptr: *const u8, len: u64) -> i64 {
    let value = unsafe { map_get_cell(map, ScalarCell::String(MsString { ptr, len })) };
    match value {
        ScalarCell::Int(value) => value,
        _ => panic_runtime("内部字典值类型不匹配"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_get_string_float(map: *const MsMap, ptr: *const u8, len: u64) -> f64 {
    let value = unsafe { map_get_cell(map, ScalarCell::String(MsString { ptr, len })) };
    match value {
        ScalarCell::Float(value) => value,
        _ => panic_runtime("内部字典值类型不匹配"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_get_string_bool(map: *const MsMap, ptr: *const u8, len: u64) -> u8 {
    let value = unsafe { map_get_cell(map, ScalarCell::String(MsString { ptr, len })) };
    match value {
        ScalarCell::Bool(value) => value,
        _ => panic_runtime("内部字典值类型不匹配"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_get_string_string(
    map: *const MsMap,
    ptr: *const u8,
    len: u64,
    out_len: *mut u64,
) -> *const u8 {
    let value = unsafe { map_get_cell(map, ScalarCell::String(MsString { ptr, len })) };
    match value {
        ScalarCell::String(value) => unsafe { write_string_result(value, out_len) },
        _ => panic_runtime("内部字典值类型不匹配"),
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
