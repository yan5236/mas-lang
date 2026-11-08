/// 创建容器、空索引与空查询缓存并移交 GC；索引拥有字符串字节，空索引不分配，不在此处回收。
#[unsafe(no_mangle)]
pub extern "C" fn map_create(key_kind: u8, value_kind: u8) -> *mut MsMap {
    let ptr = Box::into_raw(Box::new(MsMap {
        key_kind,
        value_kind,
        len: 0,
        entries: Vec::new(),
        indexes: HashMap::new(),
        integer_indexes: Vec::new(),
        string_indexes: HashMap::new(),
        last_string_index: std::cell::Cell::new(None),
    }));
    gc_track(ptr as usize, GcObject::Map(ptr));
    ptr
}

#[unsafe(no_mangle)]
pub extern "C" fn map_len(map: *const MsMap) -> u64 {
    unsafe { (*map).len }
}

/// 按插入顺序复制有效键到列表；结构体键复制共享指针，快照由 GC 管理。
#[unsafe(no_mangle)]
pub extern "C" fn map_keys(map: *const MsMap) -> *mut MsList {
    let list = unsafe { list_create((*map).key_kind) };
    for entry in unsafe { &(*map).entries } {
        if let Some((key, _)) = entry {
            unsafe {
                match key {
                    ScalarCell::Reference(value) => push_value(list, *value),
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

/// 按插入顺序复制有效值到列表；保留元素标签与托管引用，不递归复制对象。
#[unsafe(no_mangle)]
pub extern "C" fn map_values(map: *const MsMap) -> *mut MsList {
    let list = unsafe { list_create((*map).value_kind) };
    for entry in unsafe { &(*map).entries } {
        if let Some((_, value)) = entry {
            unsafe {
                match value {
                    ScalarCell::Reference(value) => push_value(list, *value),
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
            unsafe {
                map_set_cell(
                    map,
                    ScalarCell::$key_variant(key),
                    ScalarCell::$value_variant(value),
                )
            }
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
    let site = make_runtime_error_site(
        path_ptr, path_len, source_ptr, source_len, span_start, span_end,
    );
    unsafe { map_remove_cell(map, ScalarCell::String(MsString { ptr, len }), site) }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_contains_string(map: *const MsMap, ptr: *const u8, len: u64) -> u8 {
    unsafe { map_contains_cell(map, ScalarCell::String(MsString { ptr, len })) }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_set_int_string(map: *mut MsMap, key: i64, ptr: *const u8, len: u64) {
    unsafe {
        map_set_cell(
            map,
            ScalarCell::Int(key),
            ScalarCell::String(MsString { ptr, len }),
        )
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_set_float_string(map: *mut MsMap, key: f64, ptr: *const u8, len: u64) {
    unsafe {
        map_set_cell(
            map,
            ScalarCell::Float(key),
            ScalarCell::String(MsString { ptr, len }),
        )
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_set_bool_string(map: *mut MsMap, key: u8, ptr: *const u8, len: u64) {
    unsafe {
        map_set_cell(
            map,
            ScalarCell::Bool(key),
            ScalarCell::String(MsString { ptr, len }),
        )
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_set_string_int(map: *mut MsMap, ptr: *const u8, len: u64, value: i64) {
    unsafe {
        map_set_cell(
            map,
            ScalarCell::String(MsString { ptr, len }),
            ScalarCell::Int(value),
        )
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_set_string_float(map: *mut MsMap, ptr: *const u8, len: u64, value: f64) {
    unsafe {
        map_set_cell(
            map,
            ScalarCell::String(MsString { ptr, len }),
            ScalarCell::Float(value),
        )
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn map_set_string_bool(map: *mut MsMap, ptr: *const u8, len: u64, value: u8) {
    unsafe {
        map_set_cell(
            map,
            ScalarCell::String(MsString { ptr, len }),
            ScalarCell::Bool(value),
        )
    }
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
            ScalarCell::String(MsString {
                ptr: key_ptr,
                len: key_len,
            }),
            ScalarCell::String(MsString {
                ptr: value_ptr,
                len: value_len,
            }),
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
    let site = make_runtime_error_site(
        path_ptr, path_len, source_ptr, source_len, span_start, span_end,
    );
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
    let site = make_runtime_error_site(
        path_ptr, path_len, source_ptr, source_len, span_start, span_end,
    );
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
    let site = make_runtime_error_site(
        path_ptr, path_len, source_ptr, source_len, span_start, span_end,
    );
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
    let site = make_runtime_error_site(
        path_ptr, path_len, source_ptr, source_len, span_start, span_end,
    );
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
    let site = make_runtime_error_site(
        path_ptr, path_len, source_ptr, source_len, span_start, span_end,
    );
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
    let site = make_runtime_error_site(
        path_ptr, path_len, source_ptr, source_len, span_start, span_end,
    );
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
    let site = make_runtime_error_site(
        path_ptr, path_len, source_ptr, source_len, span_start, span_end,
    );
    let value = unsafe { map_get_cell(map, ScalarCell::String(MsString { ptr, len }), site) };
    match value {
        ScalarCell::String(value) => unsafe { write_string_result(value, out_len) },
        _ => abort_runtime("内部字典值类型不匹配"),
    }
}

/// 整数字典读取使用常量错误上下文指针，命中不读取诊断字段；缺失时保留原源码定位与退出行为。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn map_get_int_int_site(
    map: *const MsMap,
    key: i64,
    site: *const RuntimeErrorSite,
) -> i64 {
    let index = unsafe { map_find_index(map, ScalarCell::Int(key)) }
        .unwrap_or_else(|| unsafe { abort_runtime_at("字典中不存在目标键", *site) });
    match unsafe { (&(*map).entries).get(index) } {
        Some(Some((_, ScalarCell::Int(value)))) => *value,
        Some(Some(_)) => abort_runtime("内部字典值类型不匹配"),
        _ => abort_runtime("内部字典索引失效"),
    }
}
