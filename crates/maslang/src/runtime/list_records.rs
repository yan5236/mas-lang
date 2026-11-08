/// 追加结构体共享引用，不复制对象；调用方保证列表标签与指针布局匹配，不触发回收。
#[unsafe(no_mangle)]
pub extern "C" fn list_append_record(list: *mut MsList, value: *mut u8) {
    unsafe { push_value(list, value) }
}

/// 读取结构体引用；越界沿用列表运行时错误，不移除元素。
#[unsafe(no_mangle)]
pub extern "C" fn list_get_record(list: *const MsList, index: u64) -> *mut u8 {
    unsafe { get_value(list, index) }
}

/// 覆盖已有槽位；非法下标终止运行，旧引用不再由此槽保活，不触发回收。
#[unsafe(no_mangle)]
pub extern "C" fn list_set_record(list: *mut MsList, index: u64, value: *mut u8) {
    unsafe { set_value(list, index, value) }
}

/// 移除并返回指定位置的共享引用；越界终止运行，调用方为返回值建立 GC 根。
#[unsafe(no_mangle)]
pub extern "C" fn list_pop_at_record(list: *mut MsList, index: u64) -> *mut u8 {
    let value = unsafe { get_value(list, index) };
    unsafe { remove_at(list, index) };
    value
}

/// 移除并返回末尾引用；空列表报越界错误，不触发回收。
#[unsafe(no_mangle)]
pub extern "C" fn list_pop_record(list: *mut MsList) -> *mut u8 {
    list_pop_at_record(list, unsafe { (*list).len.saturating_sub(1) })
}

/// 查找首个匹配槽位；结构体按身份、可空基础值按内容比较，不分配或修改列表。
fn record_position(list: *const MsList, value: *mut u8) -> Option<u64> {
    (0..unsafe { (*list).len }).find(|index| {
        reference_equal(
            unsafe { (*list).elem_kind },
            unsafe { get_value::<*mut u8>(list, *index) },
            value,
        )
    })
}

/// 返回首个匹配元素的下标；未找到时报告列表运行时错误。
#[unsafe(no_mangle)]
pub extern "C" fn list_index_record(list: *const MsList, value: *mut u8) -> i64 {
    record_position(list, value).unwrap_or_else(|| abort_runtime("列表中不存在目标元素")) as i64
}

/// 返回是否含有匹配元素；结果采用单字节布尔 ABI。
#[unsafe(no_mangle)]
pub extern "C" fn list_contains_record(list: *const MsList, value: *mut u8) -> u8 {
    u8::from(record_position(list, value).is_some())
}

/// 删除首个匹配元素引用；没有匹配时终止运行，其他引用保持有效。
#[unsafe(no_mangle)]
pub extern "C" fn list_remove_record(list: *mut MsList, value: *mut u8) {
    let index =
        record_position(list, value).unwrap_or_else(|| abort_runtime("列表中不存在要删除的元素"));
    unsafe { remove_at(list, index) };
}

/// 比较托管引用；可空基础值按内容比较，空指针只等于空，其他对象按身份比较。
fn reference_equal(kind: u8, left: *mut u8, right: *mut u8) -> bool {
    if left.is_null() || right.is_null() {
        return left == right;
    }
    unsafe {
        match kind {
            6 => left.cast::<i64>().read_unaligned() == right.cast::<i64>().read_unaligned(),
            7 => left.cast::<f64>().read_unaligned() == right.cast::<f64>().read_unaligned(),
            8 => left.read() == right.read(),
            9 => strings_equal(
                left.cast::<MsString>().read_unaligned(),
                right.cast::<MsString>().read_unaligned(),
            ),
            _ => left == right,
        }
    }
}
