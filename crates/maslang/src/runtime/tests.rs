include!("strings.rs");
include!("string_methods.rs");
include!("string_builder.rs");
include!("containers.rs");
include!("borrowed_keys.rs");
include!("lists.rs");
include!("list_records.rs");
include!("maps.rs");
include!("map_references.rs");
include!("gc.rs");

/// 创建独立托管字符串，用于验证精确引用而非静态字面量。
fn text(value: &str) -> MsString {
    mas_runtime_string_clone(value.as_ptr(), value.len() as u64)
}

/// 结束当前语句并强制回收，使测试不依赖生产阈值。
fn collect_boundary() {
    mas_gc_poll();
    mas_gc_collect();
}

/// 托管字符串重复取长复用缓存，切片仍按实参字节计数，GC 释放对象时清除长度缓存。
#[test]
fn caches_unicode_length_only_for_full_managed_strings() {
    mas_gc_enter();
    let value = text("你e\u{301}😀");
    assert_eq!(string_len(value.ptr, 3), 1);
    assert_eq!(string_len(value.ptr, value.len), 4);
    GC_HEAP.with(|heap| {
        assert!(matches!(
            heap.borrow().objects.get(&(value.ptr as usize)),
            Some(GcObject::String(_, Some(4)))
        ));
    });
    collect_boundary();
    assert_eq!(mas_gc_live_objects(), 0);
    mas_gc_leave();
}

/// 创建和丢弃多类对象后必须清空注册表，容器内部缓冲区随对象析构释放。
#[test]
fn reclaims_discarded_objects() {
    mas_gc_enter();
    for _ in 0..10000 {
        let value = text("discarded string");
        let list = list_create(KIND_STRING);
        list_append_string(list, value.ptr, value.len);
        let map = map_create(KIND_STRING, KIND_STRING);
        map_set_string_string(map, value.ptr, value.len, value.ptr, value.len);
        mas_gc_poll();
    }
    assert!(mas_gc_collections() > 0);
    collect_boundary();
    assert_eq!(mas_gc_live_objects(), 0);
    mas_gc_leave();
    gc_shutdown();
}

/// 集合根必须保持内部字符串和共享切片存活，移除最后的集合根后全部可回收。
#[test]
fn traces_shared_strings_in_lists_and_maps() {
    mas_gc_enter();
    let key = text("key");
    let value = text("value");
    let mut list = list_create(KIND_STRING);
    list_append_string(list, value.ptr, value.len);
    let map = map_create(KIND_STRING, KIND_STRING);
    map_set_string_string(map, key.ptr, key.len, value.ptr, value.len);
    mas_gc_root((&raw const list).cast(), 2, 1);
    mas_gc_root((&raw const map).cast(), 2, 1);
    collect_boundary();
    assert_eq!(mas_gc_live_objects(), 4);
    list = list_slice(list, 0, 1);
    collect_boundary();
    assert_eq!(mas_gc_live_objects(), 4);
    let mut len = 0;
    let ptr = list_get_string(list, 0, &mut len);
    assert_eq!(
        unsafe { std::slice::from_raw_parts(ptr, len as usize) },
        b"value"
    );
    assert_eq!(map_len(map), 1);
    mas_gc_leave();
    mas_gc_collect();
    assert_eq!(mas_gc_live_objects(), 0);
    gc_shutdown();
}

/// 调用者半构造对象不能被被调用者的安全点回收；返回后结束语句才解除保护。
#[test]
fn protects_construction_across_nested_calls() {
    mas_gc_enter();
    let list = list_create(KIND_INT);
    mas_gc_enter();
    collect_boundary();
    assert_eq!(mas_gc_live_objects(), 1);
    list_append_int(list, 42);
    assert_eq!(list_get_int(list, 0), 42);
    mas_gc_leave();
    collect_boundary();
    assert_eq!(mas_gc_live_objects(), 0);
    mas_gc_leave();
    gc_shutdown();
}

/// 字符串数组逐元素追踪，覆盖槽位后旧元素可回收；静态和空字符串不注册。
#[test]
fn traces_array_roots_and_ignores_static_strings() {
    mas_gc_enter();
    let mut array = [text("first"), text("second"), text("")];
    mas_gc_root(array.as_ptr().cast(), 1, 3);
    collect_boundary();
    assert_eq!(mas_gc_live_objects(), 2);
    array[0] = MsString {
        ptr: b"static".as_ptr(),
        len: 6,
    };
    collect_boundary();
    assert_eq!(mas_gc_live_objects(), 1);
    assert_eq!(array[0].len, 6);
    mas_gc_leave();
    gc_shutdown();
}

/// 存活字典反复删空时历史槽位必须有界，同时压缩保持剩余键的插入顺序。
#[test]
fn compacts_live_map_without_reordering() {
    mas_gc_enter();
    let map = map_create(KIND_INT, KIND_INT);
    let site = make_runtime_error_site(b"".as_ptr(), 0, b"".as_ptr(), 0, 0, 0);
    unsafe {
        map_set_cell(map, ScalarCell::Int(-1), ScalarCell::Int(7));
        for index in 0..10000 {
            map_set_cell(map, ScalarCell::Int(index), ScalarCell::Int(index));
            map_remove_cell(map, ScalarCell::Int(index), site);
        }
        assert_eq!((*map).len, 1);
        assert!((*map).entries.len() <= 32);
        assert!((*map).entries.capacity() <= 64);
        let keys = map_keys(map);
        assert_eq!(list_get_int(keys, 0), -1);
    }
    mas_gc_leave();
    gc_shutdown();
}

/// 字符串索引独立拥有字节；等值临时键回收后查询、更新、删除与墓碑压缩仍保持正确顺序。
#[test]
fn string_indexes_own_keys_and_survive_temporary_queries() {
    mas_gc_enter();
    let map = map_create(KIND_STRING, KIND_INT);
    mas_gc_root((&raw const map).cast(), 2, 1);
    let site = make_runtime_error_site(b"".as_ptr(), 0, b"".as_ptr(), 0, 0, 0);
    unsafe {
        let first = text("你好");
        map_set_cell(map, ScalarCell::String(first), ScalarCell::Int(7));
        let owned = (*map).string_indexes.keys().next().unwrap();
        assert_ne!(owned.as_ptr(), first.ptr);
        for index in 0..200 {
            let same = text("你好");
            assert_eq!(map_find_index(map, ScalarCell::String(same)), Some(0));
            map_set_cell(map, ScalarCell::String(same), ScalarCell::Int(index));
            let other = text(&index.to_string());
            map_set_cell(map, ScalarCell::String(other), ScalarCell::Int(index));
            map_remove_cell(map, ScalarCell::String(other), site);
            collect_boundary();
        }
        assert_eq!((*map).len, 1);
        assert!((*map).entries.len() <= 32);
        let ScalarCell::String(key) = (&(*map).entries)[0].unwrap().0 else {
            panic!("string key");
        };
        assert_eq!(string_bytes(key), "你好".as_bytes());
        assert_eq!((*map).string_indexes.len(), 1);
        assert!((*map).indexes.is_empty());
    }
    mas_gc_leave();
    gc_shutdown();
}

/// 紧凑数字索引保留完整整数位宽、浮点位模式与布尔键；覆盖原位更新和删除后末尾重插。
#[test]
fn numeric_indexes_preserve_bits_and_reinsertion_order() {
    mas_gc_enter();
    let site = make_runtime_error_site(b"".as_ptr(), 0, b"".as_ptr(), 0, 0, 0);
    let cases = [
        (
            KIND_INT,
            vec![
                ScalarCell::Int(i64::MIN),
                ScalarCell::Int(-1),
                ScalarCell::Int(0),
                ScalarCell::Int(i64::MAX),
            ],
        ),
        (
            KIND_FLOAT,
            vec![
                ScalarCell::Float(0.0),
                ScalarCell::Float(-0.0),
                ScalarCell::Float(f64::from_bits(0x7ff8000000000001)),
                ScalarCell::Float(f64::from_bits(0x7ff8000000000002)),
            ],
        ),
        (KIND_BOOL, vec![ScalarCell::Bool(0), ScalarCell::Bool(1)]),
    ];
    unsafe {
        for (kind, keys) in cases {
            let map = map_create(kind, KIND_INT);
            for (i, &key) in keys.iter().enumerate() {
                map_set_cell(map, key, ScalarCell::Int(i as i64));
            }
            for (i, &key) in keys.iter().enumerate() {
                let ScalarCell::Int(value) = map_get_cell(map, key, site) else {
                    panic!("integer value")
                };
                assert_eq!(value, i as i64);
                map_set_cell(map, key, ScalarCell::Int(i as i64 + 10));
                assert_eq!(map_find_index(map, key), Some(i));
            }
            assert_eq!(map_len(map), keys.len() as u64);
            map_remove_cell(map, keys[0], site);
            assert_eq!(map_contains_cell(map, keys[0]), 0);
            map_set_cell(map, keys[0], ScalarCell::Int(99));
            let values = map_values(map);
            assert_eq!(list_get_int(values, keys.len() as u64 - 1), 99);
            assert_eq!(list_get_int(values, 0), 11);
        }
    }
    mas_gc_leave();
    gc_shutdown();
}

/// 数字键字典的字符串值仍被追踪；更新释放旧值，删除最后引用后新值也能回收。
#[test]
fn numeric_map_updates_trace_only_current_string_values() {
    mas_gc_enter();
    let map = map_create(KIND_INT, KIND_STRING);
    mas_gc_root((&raw const map).cast(), 2, 1);
    let site = make_runtime_error_site(b"".as_ptr(), 0, b"".as_ptr(), 0, 0, 0);
    unsafe {
        map_set_cell(map, ScalarCell::Int(-1), ScalarCell::String(text("old")));
        collect_boundary();
        assert_eq!(mas_gc_live_objects(), 2);
        map_set_cell(map, ScalarCell::Int(-1), ScalarCell::String(text("new")));
        collect_boundary();
        assert_eq!(mas_gc_live_objects(), 2);
        let ScalarCell::String(value) = map_get_cell(map, ScalarCell::Int(-1), site) else {
            panic!("string value")
        };
        assert_eq!(string_bytes(value), b"new");
        map_remove_cell(map, ScalarCell::Int(-1), site);
        collect_boundary();
        assert_eq!(mas_gc_live_objects(), 1);
    }
    mas_gc_leave();
    gc_shutdown();
}

/// 复现词频 contains/get/set 链路并校验每个计数；每轮释放临时查询字符串后缓存仍安全。
#[test]
fn cached_word_counts_survive_query_collection() {
    mas_gc_enter();
    let map = map_create(KIND_STRING, KIND_INT);
    mas_gc_root((&raw const map).cast(), 2, 1);
    let site = make_runtime_error_site(b"".as_ptr(), 0, b"".as_ptr(), 0, 0, 0);
    unsafe {
        for i in 0..2000 {
            let key = ScalarCell::String(text(&(i % 37).to_string()));
            let count = if map_contains_cell(map, key) != 0 {
                let ScalarCell::Int(value) = map_get_cell(map, key, site) else {
                    panic!("int")
                };
                value + 1
            } else {
                1
            };
            map_set_cell(map, key, ScalarCell::Int(count));
            collect_boundary();
        }
        assert_eq!(map_len(map), 37);
        for i in 0..37 {
            let key = ScalarCell::String(text(&i.to_string()));
            let ScalarCell::Int(count) = map_get_cell(map, key, site) else {
                panic!("int")
            };
            assert_eq!(count, 2000 / 37 + i64::from(i < 2000 % 37));
        }
    }
    mas_gc_leave();
    gc_shutdown();
}

/// 删除非缓存键触发压缩后重查缓存键，并覆盖删除缓存键、等长不同内容、空串与内嵌零字节。
#[test]
fn string_lookup_cache_checks_content_and_invalidates_on_compaction() {
    mas_gc_enter();
    let map = map_create(KIND_STRING, KIND_INT);
    let site = make_runtime_error_site(b"".as_ptr(), 0, b"".as_ptr(), 0, 0, 0);
    unsafe {
        let keys: Vec<_> = (0..80)
            .map(|i| ScalarCell::String(text(&format!("key-{i:02}"))))
            .collect();
        for (i, &key) in keys.iter().enumerate() {
            map_set_cell(map, key, ScalarCell::Int(i as i64));
        }
        for key in &keys[..60] {
            assert!(map_find_index(map, keys[79]).is_some());
            map_remove_cell(map, *key, site);
            let ScalarCell::Int(value) = map_get_cell(map, keys[79], site) else {
                panic!("int")
            };
            assert_eq!(value, 79);
        }
        assert!((*map).entries.len() < 80);
        map_remove_cell(map, keys[79], site);
        assert_eq!(map_contains_cell(map, keys[79]), 0);
        map_set_cell(map, keys[79], ScalarCell::Int(179));
        for (i, name) in ["", "a\0b", "a\0c", "你好"].iter().enumerate() {
            map_set_cell(
                map,
                ScalarCell::String(text(name)),
                ScalarCell::Int(i as i64),
            );
        }
        for (i, name) in ["", "a\0b", "a\0c", "你好"].iter().enumerate() {
            let key = ScalarCell::String(text(name));
            for _ in 0..2 {
                let ScalarCell::Int(value) = map_get_cell(map, key, site) else {
                    panic!("int")
                };
                assert_eq!(value, i as i64);
            }
        }
        assert_eq!(map_contains_cell(map, ScalarCell::String(text("a\0d"))), 0);
        let values = map_values(map);
        assert_eq!(list_get_int(values, map_len(map) - 5), 179);
    }
    mas_gc_leave();
    gc_shutdown();
}

/// 用独立有序模型核对稠密/稀疏索引混用、覆盖、空洞、压缩及删除重插；大整数不触发巨大分配。
#[test]
fn integer_indexes_match_ordered_model_across_density_changes() {
    mas_gc_enter();
    let map = map_create(KIND_INT, KIND_INT);
    let site = make_runtime_error_site(b"".as_ptr(), 0, b"".as_ptr(), 0, 0, 0);
    let mut expected: Vec<(i64, i64)> = Vec::new();
    let mut seed = 17u64;
    unsafe {
        // 100 起初进入哈希索引，随后直接索引增长覆盖它的键域；它仍须只出现一次。
        for key in [100, i64::MAX, i64::MIN].into_iter().chain(0..256) {
            map_set_cell(map, ScalarCell::Int(key), ScalarCell::Int(7));
            if !expected.iter().any(|&(k, _)| k == key) {
                expected.push((key, 7));
            }
        }
        for step in 0..4000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let key = match step % 11 {
                0 => i64::MAX,
                1 => i64::MIN,
                2 => 100,
                3 => (seed % 500) as i64 * 1000000000,
                _ => (seed % 320) as i64 - 32,
            };
            let position = expected.iter().position(|&(k, _)| k == key);
            if step % 3 == 0 {
                if let Some(position) = position {
                    map_remove_cell(map, ScalarCell::Int(key), site);
                    expected.remove(position);
                }
                assert_eq!(map_contains_cell(map, ScalarCell::Int(key)), 0);
            } else {
                map_set_cell(map, ScalarCell::Int(key), ScalarCell::Int(step));
                if let Some(position) = position {
                    expected[position].1 = step;
                } else {
                    expected.push((key, step));
                }
            }
            assert_eq!(map_len(map), expected.len() as u64);
        }
        // 大批删除触发压缩，随后重插稠密键并验证最终完整遍历顺序和值。
        for &(key, _) in &expected[..expected.len() - 10] {
            map_remove_cell(map, ScalarCell::Int(key), site);
        }
        expected.drain(..expected.len() - 10);
        for key in 0..256 {
            map_set_cell(map, ScalarCell::Int(key), ScalarCell::Int(key));
            if let Some(entry) = expected.iter_mut().find(|(k, _)| *k == key) {
                entry.1 = key;
            } else {
                expected.push((key, key));
            }
        }
        let keys = map_keys(map);
        let values = map_values(map);
        assert_eq!(map_len(map), expected.len() as u64);
        for (index, &(key, value)) in expected.iter().enumerate() {
            assert_eq!(list_get_int(keys, index as u64), key);
            assert_eq!(list_get_int(values, index as u64), value);
            let ScalarCell::Int(actual) = map_get_cell(map, ScalarCell::Int(key), site) else {
                panic!("int");
            };
            assert_eq!(actual, value);
        }
        assert!((*map).integer_indexes.capacity() < 4096);
    }
    mas_gc_leave();
    gc_shutdown();
}

/// 读取列表中的字符串元素，供字符串方法运行时测试断言内容。
fn list_text(list: *const MsList, index: u64) -> String {
    let mut len = 0u64;
    let ptr = list_get_string(list, index, &mut len);
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    String::from_utf8(bytes.to_vec()).expect("运行时的字符串结果必须是合法 UTF-8")
}

/// 读取「返回指针 + 输出长度」的运行时字符串结果，供截取、大小写和重复测试复用。
fn take_text(ptr: *const u8, len: u64) -> String {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    String::from_utf8(bytes.to_vec()).expect("运行时的字符串结果必须是合法 UTF-8")
}

/// 运行时单元测试不校验源码位置：统一补空路径、空源码与零 span。
fn split_at(text: MsString, separator: MsString) -> *mut MsList {
    string_split(
        text.ptr,
        text.len,
        separator.ptr,
        separator.len,
        b"".as_ptr(),
        0,
        b"".as_ptr(),
        0,
        0,
        0,
    )
}

/// 使用空定位参数调用查找入口，单元测试只校验返回的标量位置。
fn index_of_at(text: MsString, needle: MsString, start: i64) -> i64 {
    string_index_of(
        text.ptr,
        text.len,
        needle.ptr,
        needle.len,
        start,
        b"".as_ptr(),
        0,
        b"".as_ptr(),
        0,
        0,
        0,
    )
}

/// 使用空定位参数调用截取入口，输出参数语义与生产调用完全一致。
fn substring_at(
    text: MsString,
    start: i64,
    end: i64,
    out_len: *mut u64,
    out_valid: *mut u8,
) -> *const u8 {
    string_substring(
        text.ptr,
        text.len,
        start,
        end,
        out_len,
        out_valid,
        b"".as_ptr(),
        0,
        b"".as_ptr(),
        0,
        0,
        0,
    )
}

/// 使用空定位参数调用重复拼接入口。
fn repeat_at(text: MsString, count: i64, out_len: *mut u64, out_valid: *mut u8) -> *const u8 {
    string_repeat(
        text.ptr,
        text.len,
        count,
        out_len,
        out_valid,
        b"".as_ptr(),
        0,
        b"".as_ptr(),
        0,
        0,
        0,
    )
}

/// 使用空定位参数调用转小写入口。
fn lower_at(text: MsString, out_len: *mut u64) -> *const u8 {
    string_to_lower(
        text.ptr,
        text.len,
        out_len,
        b"".as_ptr(),
        0,
        b"".as_ptr(),
        0,
        0,
        0,
    )
}

/// 使用空定位参数调用转大写入口。
fn upper_at(text: MsString, out_len: *mut u64) -> *const u8 {
    string_to_upper(
        text.ptr,
        text.len,
        out_len,
        b"".as_ptr(),
        0,
        b"".as_ptr(),
        0,
        0,
        0,
    )
}

/// 空 needle 的测试字面量，避免在每处重复构造运行时字符串结构。
fn empty_text() -> MsString {
    MsString {
        ptr: b"".as_ptr(),
        len: 0,
    }
}

/// 切分保留头尾与连续分隔符产生的空项，多字节分隔符和空输入都按字面规则处理。
#[test]
fn splits_keep_empty_items_and_trailing_separator() {
    mas_gc_enter();
    let text_value = text("a,b,,c,");
    let separator = text(",");
    let list = split_at(text_value, separator);
    assert_eq!(list_len(list), 5);
    let items = (0..5)
        .map(|index| list_text(list, index))
        .collect::<Vec<_>>();
    assert_eq!(items, ["a", "b", "", "c", ""]);

    let empty = text("");
    let list = split_at(empty, separator);
    assert_eq!(list_len(list), 1);
    assert_eq!(list_text(list, 0), "");

    let unicode = text("你😀好");
    let emoji = text("😀");
    let list = split_at(unicode, emoji);
    assert_eq!(list_len(list), 2);
    assert_eq!(list_text(list, 0), "你");
    assert_eq!(list_text(list, 1), "好");

    let repeated = text("a::b::");
    let double_colon = text("::");
    let list = split_at(repeated, double_colon);
    assert_eq!(list_len(list), 3);
    assert_eq!(list_text(list, 2), "");
    mas_gc_leave();
}

/// 查找结果按 Unicode 标量计数，支持重叠搜索、空 needle 和越界起点。
#[test]
fn index_of_counts_unicode_scalars() {
    mas_gc_enter();
    let text_value = text("你好aaa");
    let needle = text("a");
    assert_eq!(index_of_at(text_value, needle, 0), 2);
    let pair = text("aa");
    assert_eq!(index_of_at(text_value, pair, 3), 3);
    let missing = text("😀");
    assert_eq!(index_of_at(text_value, missing, 0), -1);
    assert_eq!(index_of_at(text_value, empty_text(), 2), 2);
    assert_eq!(index_of_at(text_value, empty_text(), 99), -1);
    assert_eq!(index_of_at(text_value, empty_text(), -1), -1);
    mas_gc_leave();
}

/// 截取按半开区间与标量下标工作，非法范围写有效标记 0；重复拼接同样先检查长度与分配。
#[test]
fn substring_and_repeat_report_validity() {
    mas_gc_enter();
    let text_value = text("你e\u{301}😀");
    let mut len = 0u64;
    let mut valid = 0u8;
    let ptr = substring_at(text_value, 1, 3, &mut len, &mut valid);
    assert_eq!(valid, 1);
    assert_eq!(take_text(ptr, len), "e\u{301}");

    let ptr = substring_at(text_value, -1, 2, &mut len, &mut valid);
    assert_eq!((valid, ptr.is_null()), (0, true));
    let _ = substring_at(text_value, 2, 1, &mut len, &mut valid);
    assert_eq!(valid, 0);
    let _ = substring_at(text_value, 0, 5, &mut len, &mut valid);
    assert_eq!(valid, 0);

    let pair = text("ab");
    let ptr = repeat_at(pair, 3, &mut len, &mut valid);
    assert_eq!(valid, 1);
    assert_eq!(take_text(ptr, len), "ababab");
    let _ = repeat_at(pair, 0, &mut len, &mut valid);
    assert_eq!((valid, len), (1, 0));
    let ptr = repeat_at(pair, -1, &mut len, &mut valid);
    assert_eq!((valid, ptr.is_null()), (0, true));
    // 极大次数在分配前被拒绝，测试进程不会真的申请巨量内存。
    let _ = repeat_at(pair, i64::MAX, &mut len, &mut valid);
    assert_eq!(valid, 0);
    mas_gc_leave();
}

/// 大小写转换使用 Rust 标准库的 Unicode 默认规则，允许长度变化。
#[test]
fn case_conversion_allows_length_changes() {
    mas_gc_enter();
    let mut len = 0u64;
    let sharp = text("ß");
    let ptr = upper_at(sharp, &mut len);
    assert_eq!(take_text(ptr, len), "SS");
    let ptr = lower_at(sharp, &mut len);
    assert_eq!(take_text(ptr, len), "ß");

    let dotted = text("İ");
    let ptr = lower_at(dotted, &mut len);
    assert_eq!(take_text(ptr, len), "i\u{307}");
    let turkish = text("中文AbC");
    let ptr = upper_at(turkish, &mut len);
    assert_eq!(take_text(ptr, len), "中文ABC");
    mas_gc_leave();
}
