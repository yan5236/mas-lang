use maslang::runtime_bootstrap::bootstrap_source;
use std::process::Command;

/// 使用真实 System 分配器的计数差额确认缓冲区确实释放，而非仅从 GC 注册表删除。
#[test]
fn released_objects_return_their_heap_allocations() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("mascript-gc-memory-{unique}"));
    std::fs::create_dir_all(&root).unwrap();
    let runtime = bootstrap_source()
        .rsplit_once("unsafe extern \"C\" {")
        .unwrap()
        .0;
    let probe = r#"
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};
struct Counted;
static LIVE: AtomicIsize = AtomicIsize::new(0);
static ALLOCATIONS: AtomicIsize = AtomicIsize::new(0);
unsafe impl GlobalAlloc for Counted {
    /// 委托系统分配并记录请求次数与成功分配的请求字节。
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        let ptr = System.alloc(layout);
        if !ptr.is_null() { LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed); }
        ptr
    }
    /// 委托系统释放并扣除对应请求字节。
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        System.dealloc(ptr, layout);
    }
    /// 记录重新分配次数，并仅在成功后更新新旧容量差额。
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        let result = System.realloc(ptr, layout, size);
        if !result.is_null() { LIVE.fetch_add(size as isize - layout.size() as isize, Ordering::Relaxed); }
        result
    }
}
#[global_allocator]
static ALLOCATOR: Counted = Counted;

/// 反复分配并丢弃字符串、列表和字典，最后清除语句构造保护并强制回收。
fn churn(count: usize) {
    mas_gc_enter();
    for _ in 0..count {
        let text = mas_runtime_string_clone(b"sixteen bytes!!!".as_ptr(), 16);
        let list = list_create(KIND_STRING);
        list_append_string(list, text.ptr, text.len);
        let map = map_create(KIND_STRING, KIND_STRING);
        map_set_string_string(map, text.ptr, text.len, text.ptr, text.len);
        mas_gc_poll();
    }
    mas_gc_leave();
    mas_gc_collect();
    assert_eq!(mas_gc_live_objects(), 0);
}

/// 验证等值字符串键的查找和更新不产生临时分配，索引仍持有独立拥有的字节。
fn allocation_free_lookup() {
    mas_gc_enter();
    let key = mas_runtime_string_clone(b"key".as_ptr(), 3);
    let query = mas_runtime_string_clone(b"key".as_ptr(), 3);
    let map = map_create(KIND_STRING, KIND_INT);
    map_set_string_int(map, key.ptr, key.len, 1);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    for i in 0..1000 {
        assert_eq!(unsafe { map_find_index(map, ScalarCell::String(query)) }, Some(0));
        map_set_string_int(map, query.ptr, query.len, i);
    }
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed), before);
    mas_gc_leave();
    mas_gc_collect();
}

/// 实测十万次私有追加只需对数次扩容，强制 GC 后缓冲区及最终字符串均释放。
fn builder_allocations() {
    mas_gc_enter();
    let mut handle = mas_string_builder_create(b"".as_ptr(), 0);
    mas_gc_root((&mut handle as *mut *mut Vec<u8>).cast(), 2, 1);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    for _ in 0..100000 {
        mas_string_builder_append(handle, b"abcdefghijklmnop".as_ptr(), 16);
    }
    let allocations = ALLOCATIONS.load(Ordering::Relaxed) - before;
    assert!(allocations < 32, "append allocations: {allocations}");
    mas_gc_collect();
    let mut len = 0;
    let ptr = mas_string_builder_finish(handle, &mut len);
    assert_eq!(len, 1600000);
    assert!(string_bytes(MsString {ptr, len}).chunks_exact(16).all(|x| x == b"abcdefghijklmnop"));
    mas_gc_leave();
    mas_gc_collect();
    assert_eq!(mas_gc_live_objects(), 0);
}

/// 预热 GC 表后逐项检查整数转换仅分配一次；同时验证负数、零及 i64 极值的精确文本。
fn integer_string_allocations() {
    mas_gc_enter();
    let mut len = 0;
    int_to_string(123, &mut len);
    mas_gc_poll();
    mas_gc_collect();
    for value in [i64::MIN, -4096, -1, 0, 1, 4095, i64::MAX] {
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        let ptr = int_to_string(value, &mut len);
        assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 1);
        assert_eq!(string_bytes(MsString { ptr, len }), value.to_string().as_bytes());
        mas_gc_poll();
        mas_gc_collect();
    }
    mas_gc_leave();
    gc_shutdown();
}

/// 预热 GC 登记表后，通过真实分配器确认稠密整数索引及条目缓冲区随字典完整释放。
fn dense_index_allocations() {
    mas_gc_enter();
    map_create(KIND_INT, KIND_INT);
    mas_gc_leave();
    gc_shutdown();
    let before = LIVE.load(Ordering::Relaxed);
    mas_gc_enter();
    let map = map_create(KIND_INT, KIND_INT);
    mas_gc_root((&raw const map).cast(), 2, 1);
    for key in 0..10000 { map_set_int_int(map, key, key); }
    mas_gc_poll();
    mas_gc_collect();
    assert_eq!(map_len(map), 10000);
    assert!(LIVE.load(Ordering::Relaxed) - before > 10000 * 8);
    mas_gc_leave();
    gc_shutdown();
    assert_eq!(LIVE.load(Ordering::Relaxed), before);
}

/// 验证查询零分配，并对比十倍累计工作量后的存活请求字节，容许 GC 哈希表有限容量波动。
fn main() {
    dense_index_allocations();
    integer_string_allocations();
    builder_allocations();
    allocation_free_lookup();
    churn(10000);
    let first = LIVE.load(Ordering::Relaxed);
    churn(100000);
    let second = LIVE.load(Ordering::Relaxed);
    assert!(second - first < 65536, "heap grew: {first} -> {second}");
    gc_shutdown();
    println!("{first} {second}");
}
"#;
    let source = root.join("probe.rs");
    std::fs::write(&source, format!("{runtime}\n{probe}")).unwrap();
    let binary = root.join("probe.exe");
    let result = Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into()))
        .args(["--edition", "2024", "-O"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = Command::new(binary).output().unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    eprintln!(
        "live requested heap bytes (10k / 100k): {}",
        String::from_utf8_lossy(&output.stdout).trim()
    );
}
