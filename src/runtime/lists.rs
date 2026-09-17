/// 创建容器并移交 GC 管理；返回地址稳定的共享句柄，不在分配内部触发回收。
#[unsafe(no_mangle)]
pub extern "C" fn list_create(elem_kind: u8) -> *mut MsList {
    let ptr = Box::into_raw(Box::new(MsList {
        elem_kind,
        len: 0,
        capacity: 0,
        data: std::ptr::null_mut(),
    }));
    gc_track(ptr as usize, GcObject::List(ptr));
    ptr
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
        if strings_equal(
            unsafe { read_value::<MsString>(bytes, index as usize) },
            target,
        ) {
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
pub extern "C" fn list_pop_at_string(
    list: *mut MsList,
    index: u64,
    out_len: *mut u64,
) -> *const u8 {
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
            values.sort_by(|left, right| {
                left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
            });
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
        if strings_equal(
            unsafe { read_value::<MsString>(bytes, index as usize) },
            target,
        ) {
            return index as i64;
        }
    }
    abort_runtime("列表中不存在目标元素");
}

#[unsafe(no_mangle)]
pub extern "C" fn list_contains_int(list: *const MsList, value: i64) -> u8 {
    u8::from(
        (0..unsafe { (*list).len })
            .any(|index| unsafe { read_value::<i64>(list_bytes(list), index as usize) } == value),
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn list_contains_float(list: *const MsList, value: f64) -> u8 {
    u8::from(
        (0..unsafe { (*list).len })
            .any(|index| unsafe { read_value::<f64>(list_bytes(list), index as usize) } == value),
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn list_contains_bool(list: *const MsList, value: u8) -> u8 {
    u8::from(
        (0..unsafe { (*list).len })
            .any(|index| unsafe { read_value::<u8>(list_bytes(list), index as usize) } == value),
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn list_contains_string(list: *const MsList, ptr: *const u8, len: u64) -> u8 {
    let target = MsString { ptr, len };
    u8::from((0..unsafe { (*list).len }).any(|index| {
        strings_equal(
            unsafe { read_value::<MsString>(list_bytes(list), index as usize) },
            target,
        )
    }))
}
