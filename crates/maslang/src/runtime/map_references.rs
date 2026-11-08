/// 写入 int/record 字典条目；对象引用共享，键按身份匹配，内部不触发回收。
#[unsafe(no_mangle)]
pub extern "C" fn map_set_int_record(map: *mut MsMap, key: i64, value: *mut u8) {
    unsafe { map_set_cell(map, ScalarCell::Int(key), ScalarCell::Reference(value)) }
}

/// 读取 int/record 字典条目；缺失键报告源码位置，返回引用由调用方保活。
#[unsafe(no_mangle)]
pub extern "C" fn map_get_int_record(
    map: *const MsMap,
    key: i64,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> *mut u8 {
    let value = unsafe {
        map_get_cell(
            map,
            ScalarCell::Int(key),
            make_runtime_error_site(
                path_ptr, path_len, source_ptr, source_len, span_start, span_end,
            ),
        )
    };
    match value {
        ScalarCell::Reference(value) => value,
        _ => abort_runtime("内部字典值类型不匹配"),
    }
}

/// 写入 float/record 字典条目；对象引用共享，键按身份匹配，内部不触发回收。
#[unsafe(no_mangle)]
pub extern "C" fn map_set_float_record(map: *mut MsMap, key: f64, value: *mut u8) {
    unsafe { map_set_cell(map, ScalarCell::Float(key), ScalarCell::Reference(value)) }
}

/// 读取 float/record 字典条目；缺失键报告源码位置，返回引用由调用方保活。
#[unsafe(no_mangle)]
pub extern "C" fn map_get_float_record(
    map: *const MsMap,
    key: f64,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> *mut u8 {
    let value = unsafe {
        map_get_cell(
            map,
            ScalarCell::Float(key),
            make_runtime_error_site(
                path_ptr, path_len, source_ptr, source_len, span_start, span_end,
            ),
        )
    };
    match value {
        ScalarCell::Reference(value) => value,
        _ => abort_runtime("内部字典值类型不匹配"),
    }
}

/// 写入 bool/record 字典条目；对象引用共享，键按身份匹配，内部不触发回收。
#[unsafe(no_mangle)]
pub extern "C" fn map_set_bool_record(map: *mut MsMap, key: u8, value: *mut u8) {
    unsafe { map_set_cell(map, ScalarCell::Bool(key), ScalarCell::Reference(value)) }
}

/// 读取 bool/record 字典条目；缺失键报告源码位置，返回引用由调用方保活。
#[unsafe(no_mangle)]
pub extern "C" fn map_get_bool_record(
    map: *const MsMap,
    key: u8,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> *mut u8 {
    let value = unsafe {
        map_get_cell(
            map,
            ScalarCell::Bool(key),
            make_runtime_error_site(
                path_ptr, path_len, source_ptr, source_len, span_start, span_end,
            ),
        )
    };
    match value {
        ScalarCell::Reference(value) => value,
        _ => abort_runtime("内部字典值类型不匹配"),
    }
}

/// 写入 string/record 字典条目；对象引用共享，键按身份匹配，内部不触发回收。
#[unsafe(no_mangle)]
pub extern "C" fn map_set_string_record(
    map: *mut MsMap,
    key_ptr: *const u8,
    key_len: u64,
    value: *mut u8,
) {
    unsafe {
        map_set_cell(
            map,
            ScalarCell::String(MsString {
                ptr: key_ptr,
                len: key_len,
            }),
            ScalarCell::Reference(value),
        )
    }
}

/// 读取 string/record 字典条目；缺失键报告源码位置，返回引用由调用方保活。
#[unsafe(no_mangle)]
pub extern "C" fn map_get_string_record(
    map: *const MsMap,
    key_ptr: *const u8,
    key_len: u64,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> *mut u8 {
    let value = unsafe {
        map_get_cell(
            map,
            ScalarCell::String(MsString {
                ptr: key_ptr,
                len: key_len,
            }),
            make_runtime_error_site(
                path_ptr, path_len, source_ptr, source_len, span_start, span_end,
            ),
        )
    };
    match value {
        ScalarCell::Reference(value) => value,
        _ => abort_runtime("内部字典值类型不匹配"),
    }
}

/// 写入 record/int 字典条目；对象引用共享，键按身份匹配，内部不触发回收。
#[unsafe(no_mangle)]
pub extern "C" fn map_set_record_int(map: *mut MsMap, key: *mut u8, value: i64) {
    unsafe { map_set_cell(map, ScalarCell::Reference(key), ScalarCell::Int(value)) }
}

/// 读取 record/int 字典条目；缺失键报告源码位置，返回引用由调用方保活。
#[unsafe(no_mangle)]
pub extern "C" fn map_get_record_int(
    map: *const MsMap,
    key: *mut u8,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> i64 {
    let value = unsafe {
        map_get_cell(
            map,
            ScalarCell::Reference(key),
            make_runtime_error_site(
                path_ptr, path_len, source_ptr, source_len, span_start, span_end,
            ),
        )
    };
    match value {
        ScalarCell::Int(value) => value,
        _ => abort_runtime("内部字典值类型不匹配"),
    }
}

/// 写入 record/float 字典条目；对象引用共享，键按身份匹配，内部不触发回收。
#[unsafe(no_mangle)]
pub extern "C" fn map_set_record_float(map: *mut MsMap, key: *mut u8, value: f64) {
    unsafe { map_set_cell(map, ScalarCell::Reference(key), ScalarCell::Float(value)) }
}

/// 读取 record/float 字典条目；缺失键报告源码位置，返回引用由调用方保活。
#[unsafe(no_mangle)]
pub extern "C" fn map_get_record_float(
    map: *const MsMap,
    key: *mut u8,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> f64 {
    let value = unsafe {
        map_get_cell(
            map,
            ScalarCell::Reference(key),
            make_runtime_error_site(
                path_ptr, path_len, source_ptr, source_len, span_start, span_end,
            ),
        )
    };
    match value {
        ScalarCell::Float(value) => value,
        _ => abort_runtime("内部字典值类型不匹配"),
    }
}

/// 写入 record/bool 字典条目；对象引用共享，键按身份匹配，内部不触发回收。
#[unsafe(no_mangle)]
pub extern "C" fn map_set_record_bool(map: *mut MsMap, key: *mut u8, value: u8) {
    unsafe { map_set_cell(map, ScalarCell::Reference(key), ScalarCell::Bool(value)) }
}

/// 读取 record/bool 字典条目；缺失键报告源码位置，返回引用由调用方保活。
#[unsafe(no_mangle)]
pub extern "C" fn map_get_record_bool(
    map: *const MsMap,
    key: *mut u8,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> u8 {
    let value = unsafe {
        map_get_cell(
            map,
            ScalarCell::Reference(key),
            make_runtime_error_site(
                path_ptr, path_len, source_ptr, source_len, span_start, span_end,
            ),
        )
    };
    match value {
        ScalarCell::Bool(value) => value,
        _ => abort_runtime("内部字典值类型不匹配"),
    }
}

/// 写入 record/string 字典条目；对象引用共享，键按身份匹配，内部不触发回收。
#[unsafe(no_mangle)]
pub extern "C" fn map_set_record_string(
    map: *mut MsMap,
    key: *mut u8,
    value_ptr: *const u8,
    value_len: u64,
) {
    unsafe {
        map_set_cell(
            map,
            ScalarCell::Reference(key),
            ScalarCell::String(MsString {
                ptr: value_ptr,
                len: value_len,
            }),
        )
    }
}

/// 读取 record/string 字典条目；缺失键报告源码位置，返回引用由调用方保活。
#[unsafe(no_mangle)]
pub extern "C" fn map_get_record_string(
    map: *const MsMap,
    key: *mut u8,
    out_len: *mut u64,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> *const u8 {
    let value = unsafe {
        map_get_cell(
            map,
            ScalarCell::Reference(key),
            make_runtime_error_site(
                path_ptr, path_len, source_ptr, source_len, span_start, span_end,
            ),
        )
    };
    match value {
        ScalarCell::String(value) => unsafe { write_string_result(value, out_len) },
        _ => abort_runtime("内部字典值类型不匹配"),
    }
}

/// 写入 record/record 字典条目；对象引用共享，键按身份匹配，内部不触发回收。
#[unsafe(no_mangle)]
pub extern "C" fn map_set_record_record(map: *mut MsMap, key: *mut u8, value: *mut u8) {
    unsafe {
        map_set_cell(
            map,
            ScalarCell::Reference(key),
            ScalarCell::Reference(value),
        )
    }
}

/// 读取 record/record 字典条目；缺失键报告源码位置，返回引用由调用方保活。
#[unsafe(no_mangle)]
pub extern "C" fn map_get_record_record(
    map: *const MsMap,
    key: *mut u8,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> *mut u8 {
    let value = unsafe {
        map_get_cell(
            map,
            ScalarCell::Reference(key),
            make_runtime_error_site(
                path_ptr, path_len, source_ptr, source_len, span_start, span_end,
            ),
        )
    };
    match value {
        ScalarCell::Reference(value) => value,
        _ => abort_runtime("内部字典值类型不匹配"),
    }
}

// 结构体键复用已有的带源码诊断删除及布尔查询模板；身份不受字段修改影响。
define_map_remove!(map_remove_record, Reference, *mut u8);
define_map_contains!(map_contains_record, Reference, *mut u8);
