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

/// 临时接回列表缓冲区所有权，更新长度容量并计入 GC 分配预算；回调内不触发收集。
unsafe fn with_list_bytes_mut<R>(
    list: *mut MsList,
    callback: impl FnOnce(&mut MsList, &mut Vec<u8>) -> R,
) -> R {
    let list = &mut *list;
    let size = elem_size(list.elem_kind);
    let old_capacity = list.capacity as usize * size;
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
    list.capacity = if size == 0 {
        0
    } else {
        (buffer.capacity() / size) as u64
    };
    gc_charge(buffer.capacity().saturating_sub(old_capacity));
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

/// 写入或更新字典并保持插入顺序；新增键和容量扩张计入 GC 分配预算。
unsafe fn map_set_cell(map: *mut MsMap, key: ScalarCell, value: ScalarCell) {
    let map = &mut *map;
    let old_capacity = map.entries.capacity()
        * std::mem::size_of::<Option<(ScalarCell, ScalarCell)>>()
        + map.indexes.capacity() * std::mem::size_of::<(MapKey, usize)>();
    if let Some(index) = map_find_index(map, key) {
        if let Some((_, entry_value)) = map.entries.get_mut(index).and_then(Option::as_mut) {
            *entry_value = value;
        } else {
            abort_runtime("内部字典索引失效");
        }
    } else {
        let index = map.entries.len();
        map.entries.push(Some((key, value)));
        let owned_key = scalar_to_map_key(key);
        if let MapKey::String(bytes) = &owned_key {
            gc_charge(bytes.capacity());
        }
        map.indexes.insert(owned_key, index);
        map.len += 1;
    }
    let capacity = map.entries.capacity() * std::mem::size_of::<Option<(ScalarCell, ScalarCell)>>()
        + map.indexes.capacity() * std::mem::size_of::<(MapKey, usize)>();
    gc_charge(capacity.saturating_sub(old_capacity));
}

/// 读取字典中的值；若键不存在则输出带源码定位的运行时错误。
unsafe fn map_get_cell(map: *const MsMap, key: ScalarCell, site: RuntimeErrorSite) -> ScalarCell {
    let index =
        map_find_index(map, key).unwrap_or_else(|| abort_runtime_at("字典中不存在目标键", site));
    (&(*map).entries)
        .get(index)
        .and_then(|entry| *entry)
        .map(|(_, value)| value)
        .unwrap_or_else(|| abort_runtime("内部字典索引失效"))
}

/// 删除键并在墓碑过多时按原顺序压缩存储，避免仍存活的空字典随历史操作无限增长。
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
    if map.entries.len() > 32 && map.entries.len() > (map.len as usize).saturating_mul(2) {
        map.entries.retain(Option::is_some);
        // 更新现有索引值，保持剩余元素原有顺序；临时字符串查找键用后即释放。
        for (index, entry) in map.entries.iter().enumerate() {
            let key = scalar_to_map_key(entry.as_ref().expect("live entry").0);
            *map.indexes.get_mut(&key).expect("live key") = index;
        }
        map.entries.shrink_to((map.len as usize).max(32));
        map.indexes.shrink_to((map.len as usize).max(32));
    }
}

/// 判断目标键是否存在于字典中，直接复用哈希索引结果。
unsafe fn map_contains_cell(map: *const MsMap, key: ScalarCell) -> u8 {
    u8::from(map_find_index(map, key).is_some())
}
