include!("strings.rs");
include!("containers.rs");
include!("lists.rs");
include!("maps.rs");
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
