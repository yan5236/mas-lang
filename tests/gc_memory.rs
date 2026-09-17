use mascript_compiler_llvm::runtime_bootstrap::bootstrap_source;
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
unsafe impl GlobalAlloc for Counted {
    /// 委托系统分配并记录成功分配的请求字节。
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() { LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed); }
        ptr
    }
    /// 委托系统释放并扣除对应请求字节。
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        System.dealloc(ptr, layout);
    }
    /// 仅在重新分配成功后更新新旧容量差额。
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
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

/// 对比十倍累计工作量后的存活请求字节，容许 GC 哈希表有限容量波动。
fn main() {
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
