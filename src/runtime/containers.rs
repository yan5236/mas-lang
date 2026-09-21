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

/// 查询键对应的条目位置；整数优先直接索引，空洞及范围外回退随机哈希，保留历史稀疏条目。
/// 字符串先按内容验证最近命中的条目，再回退到随机哈希索引。
/// 缓存仅保存位置，不延长查询字符串寿命；单线程中通过 Cell 更新，不分配或触发 GC。
/// 强制在类型固定的 ABI 包装层内展开，使未使用的 ScalarCell 分支可被优化器删除。
#[inline(always)]
unsafe fn map_find_index(map: *const MsMap, key: ScalarCell) -> Option<usize> {
    match key {
        ScalarCell::Int(value) => {
            let map = &*map;
            usize::try_from(value)
                .ok()
                .and_then(|key| map.integer_indexes.get(key).copied())
                .filter(|&index| index != usize::MAX)
                .or_else(|| map.indexes.get(&(value as u64)).copied())
        }
        ScalarCell::String(value) => {
            let map = &*map;
            let bytes = string_bytes(value);
            if let Some(index) = map.last_string_index.get() {
                if let Some(Some((ScalarCell::String(stored), _))) = map.entries.get(index) {
                    if string_bytes(*stored) == bytes {
                        return Some(index);
                    }
                }
            }
            let index = map.string_indexes.get(bytes).copied();
            map.last_string_index.set(index);
            index
        }
        _ => (*map).indexes.get(&scalar_to_map_key(key)).copied(),
    }
}

/// 写入或更新字典并保持插入顺序；命中时只覆盖值，保留原键及其字符串可达性。
/// 非负整数仅在键域足够紧凑时扩展直接索引；回退索引用 entry 合并查找与插入，避免重复哈希。
/// 新增条目才结算容量增长并计入 GC 预算，内部不收集，空洞不代表语言层存在该键。
/// 在类型固定的导出函数内展开，消除热更新路径的枚举装箱及无关类型分支。
#[inline(always)]
unsafe fn map_set_cell(map: *mut MsMap, key: ScalarCell, value: ScalarCell) {
    let map = &mut *map;
    let old_capacity = map_index_capacity(map);
    let next = map.entries.len();
    let index = match key {
        ScalarCell::String(text) => {
            if let Some(index) = map_find_index(map, key) {
                index
            } else {
                let bytes = string_bytes(text).to_vec();
                gc_charge(bytes.capacity());
                map.string_indexes.insert(bytes, next);
                map.last_string_index.set(Some(next));
                next
            }
        }
        ScalarCell::Int(number) => {
            let number_index = usize::try_from(number).ok();
            let dense_index = number_index
                .and_then(|number| map.integer_indexes.get(number).copied())
                .filter(|&index| index != usize::MAX);
            if let Some(index) = dense_index {
                index
            } else {
                let dense_key = number_index.filter(|&number| {
                    number < map.integer_indexes.len()
                        || number
                            < (map.len as usize)
                                .saturating_add(1)
                                .saturating_mul(2)
                                .max(64)
                });
                // 没有历史稀疏条目时直接创建稠密槽，避免 entry 为一个不用的空哈希表分配桶。
                let index = if dense_key.is_some() && map.indexes.is_empty() {
                    next
                } else {
                    match map.indexes.entry(number as u64) {
                        std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            if dense_key.is_none() {
                                entry.insert(next);
                            }
                            next
                        }
                    }
                };
                if index == next {
                    if let Some(number) = dense_key {
                        if number >= map.integer_indexes.len() {
                            map.integer_indexes
                                .resize((number + 1).next_power_of_two(), usize::MAX);
                        }
                        map.integer_indexes[number] = next;
                    }
                }
                index
            }
        }
        _ => *map.indexes.entry(scalar_to_map_key(key)).or_insert(next),
    };
    if index != next {
        let (_, entry_value) = map.entries[index]
            .as_mut()
            .unwrap_or_else(|| abort_runtime("内部字典索引失效"));
        *entry_value = value;
        return;
    }
    map.entries.push(Some((key, value)));
    map.len += 1;
    gc_charge(map_index_capacity(map).saturating_sub(old_capacity));
}

/// 汇总条目和三类索引的实际缓冲容量；字符串索引拥有的字节另行记账，不包含分配器元数据。
fn map_index_capacity(map: &MsMap) -> usize {
    map.entries.capacity() * std::mem::size_of::<Option<(ScalarCell, ScalarCell)>>()
        + map.indexes.capacity() * std::mem::size_of::<(MapKey, usize)>()
        + map.integer_indexes.capacity() * std::mem::size_of::<usize>()
        + map.string_indexes.capacity() * std::mem::size_of::<(Vec<u8>, usize)>()
}

/// 读取字典中的值；若键不存在则输出带源码定位的运行时错误。
#[inline(always)]
unsafe fn map_get_cell(map: *const MsMap, key: ScalarCell, site: RuntimeErrorSite) -> ScalarCell {
    let index =
        map_find_index(map, key).unwrap_or_else(|| abort_runtime_at("字典中不存在目标键", site));
    (&(*map).entries)
        .get(index)
        .and_then(|entry| *entry)
        .map(|(_, value)| value)
        .unwrap_or_else(|| abort_runtime("内部字典索引失效"))
}

/// 删除键前失效查询缓存并清空直接索引槽；缺失键报告原来的源码定位错误。
/// 墓碑过多时按原顺序压缩并更新所有索引，裁剪直接索引尾部空洞，防止旧位置被误用。
unsafe fn map_remove_cell(map: *mut MsMap, key: ScalarCell, site: RuntimeErrorSite) {
    let map = &mut *map;
    map.last_string_index.set(None);
    let index = match key {
        ScalarCell::Int(value) => usize::try_from(value)
            .ok()
            .and_then(|key| map.integer_indexes.get_mut(key))
            .filter(|slot| **slot != usize::MAX)
            .map(|slot| std::mem::replace(slot, usize::MAX))
            .or_else(|| map.indexes.remove(&(value as u64))),
        ScalarCell::String(value) => map.string_indexes.remove(string_bytes(value)),
        _ => map.indexes.remove(&scalar_to_map_key(key)),
    }
    .unwrap_or_else(|| abort_runtime_at("字典中不存在要删除的键", site));
    let removed = map.entries.get_mut(index).and_then(Option::take);
    if removed.is_none() {
        abort_runtime("内部字典索引失效");
    }
    map.len -= 1;
    if map.entries.len() > 32 && map.entries.len() > (map.len as usize).saturating_mul(2) {
        map.entries.retain(Option::is_some);
        // 更新现有索引而不分配临时键，持久字符串索引仍拥有独立字节。
        for (index, entry) in map.entries.iter().enumerate() {
            let key = entry.as_ref().expect("live entry").0;
            let slot = match key {
                ScalarCell::Int(value) => usize::try_from(value)
                    .ok()
                    .and_then(|key| map.integer_indexes.get_mut(key))
                    .filter(|slot| **slot != usize::MAX)
                    .or_else(|| map.indexes.get_mut(&(value as u64))),
                ScalarCell::String(value) => map.string_indexes.get_mut(string_bytes(value)),
                _ => map.indexes.get_mut(&scalar_to_map_key(key)),
            };
            *slot.expect("live key") = index;
        }
        let dense_len = map
            .integer_indexes
            .iter()
            .rposition(|&index| index != usize::MAX)
            .map_or(0, |index| index + 1);
        map.integer_indexes.truncate(dense_len);
        map.integer_indexes.shrink_to_fit();
        map.entries.shrink_to((map.len as usize).max(32));
        map.indexes.shrink_to((map.len as usize).max(32));
        map.string_indexes.shrink_to((map.len as usize).max(32));
    }
}

/// 判断目标键是否存在于字典中，直接复用哈希索引结果。
#[inline(always)]
unsafe fn map_contains_cell(map: *const MsMap, key: ScalarCell) -> u8 {
    u8::from(map_find_index(map, key).is_some())
}
