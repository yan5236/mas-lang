use std::cell::RefCell;

/// 非移动托管对象；容器独占缓冲区，字符串独占字节存储。
enum GcObject {
    Record {
        words: Box<[u64]>,
        offsets: &'static [u64],
    },
    String(Box<[u8]>),
    StringBuilder(Box<Vec<u8>>),
    List(*mut MsList),
    Map(*mut MsMap),
}

impl GcObject {
    /// 返回主要堆容量，含构建缓冲区、三类索引和拥有的字符串字节，用于回收阈值；不含分配器元数据。
    fn bytes(&self) -> usize {
        match self {
            Self::Record { words, .. } => words.len() * 8,
            Self::String(bytes) => bytes.len(),
            Self::StringBuilder(bytes) => std::mem::size_of::<Vec<u8>>() + bytes.capacity(),
            Self::List(ptr) => unsafe {
                std::mem::size_of::<MsList>()
                    + (**ptr).capacity as usize * elem_size((**ptr).elem_kind)
            },
            Self::Map(ptr) => unsafe {
                let map = &**ptr;
                std::mem::size_of::<MsMap>()
                    + map_index_capacity(map)
                    + map.string_indexes.keys().map(Vec::capacity).sum::<usize>()
            },
        }
    }

    /// 枚举结构体、列表和字典的有效托管引用；列表只扫描当前长度，已删除槽位不保活对象。
    fn children(&self, output: &mut Vec<usize>) {
        match self {
            Self::Record { words, offsets } => {
                for offset in *offsets {
                    output.push(unsafe {
                        std::ptr::read_unaligned(
                            words
                                .as_ptr()
                                .cast::<u8>()
                                .add(*offset as usize)
                                .cast::<usize>(),
                        )
                    });
                }
            }
            Self::String(_) | Self::StringBuilder(_) => {}
            Self::List(ptr) => unsafe {
                let list = &**ptr;
                if (KIND_RECORD..=9).contains(&list.elem_kind) {
                    for index in 0..list.len as usize {
                        output.push(std::ptr::read_unaligned(
                            list.data
                                .add(index * std::mem::size_of::<*mut u8>())
                                .cast::<*mut u8>(),
                        ) as usize);
                    }
                }
                if list.elem_kind == KIND_STRING {
                    for index in 0..list.len as usize {
                        let text = std::ptr::read_unaligned(
                            list.data
                                .add(index * std::mem::size_of::<MsString>())
                                .cast::<MsString>(),
                        );
                        output.push(text.ptr as usize);
                    }
                }
            },
            Self::Map(ptr) => unsafe {
                if (**ptr).key_kind < KIND_STRING && (**ptr).value_kind < KIND_STRING {
                    return;
                }
                for (key, value) in (&**ptr).entries.iter().flatten() {
                    for cell in [key, value] {
                        match cell {
                            ScalarCell::String(text) => output.push(text.ptr as usize),
                            ScalarCell::Reference(pointer) => output.push(*pointer as usize),
                            _ => {}
                        }
                    }
                }
            },
        }
    }
}

impl Drop for GcObject {
    /// 释放不可达对象及其独占缓冲区；共享子对象由清扫阶段统一处理。
    fn drop(&mut self) {
        unsafe {
            match self {
                Self::Record { .. } | Self::String(_) | Self::StringBuilder(_) => {}
                Self::List(ptr) => {
                    let list = Box::from_raw(*ptr);
                    if !list.data.is_null() {
                        drop(Vec::from_raw_parts(
                            list.data,
                            list.len as usize * elem_size(list.elem_kind),
                            list.capacity as usize * elem_size(list.elem_kind),
                        ));
                    }
                }
                Self::Map(ptr) => {
                    drop(Box::from_raw(*ptr));
                }
            }
        }
    }
}

/// 编译器提供零初始化的根槽；kind=1 为字符串，2 为容器指针，count 支持字符串数组。
struct GcRoot {
    address: *const u8,
    kind: u8,
    count: usize,
}
#[derive(Default)]
struct GcFrame {
    roots: Vec<GcRoot>,
    // 当前语句的半构造对象必须在嵌套调用期间保持可达。
    pending: Vec<usize>,
}
#[derive(Default)]
struct GcHeap {
    objects: HashMap<usize, GcObject>,
    frames: Vec<GcFrame>,
    allocated: usize,
    threshold: usize,
    collections: u64,
}
thread_local! { static GC_HEAP: RefCell<GcHeap> = RefCell::new(GcHeap::default()); }
const GC_MIN_THRESHOLD: usize = 256 * 1024;

impl GcHeap {
    /// 从精确根与半构造对象出发迭代标记并清扫；调用者必须处于安全点。
    /// 收集后依据存活载荷及对象登记项重设预算，与注册时的新增分配口径一致。
    fn collect(&mut self) {
        let mut work = Vec::new();
        for frame in &self.frames {
            work.extend_from_slice(&frame.pending);
            for root in &frame.roots {
                for index in 0..root.count {
                    let stride = if root.kind == 1 {
                        std::mem::size_of::<MsString>()
                    } else {
                        std::mem::size_of::<usize>()
                    };
                    work.push(unsafe {
                        std::ptr::read_unaligned(root.address.add(index * stride).cast::<usize>())
                    });
                }
            }
        }
        let mut marked = std::collections::HashSet::new();
        while let Some(address) = work.pop() {
            if let Some(object) = self.objects.get(&address) {
                if marked.insert(address) {
                    object.children(&mut work);
                }
            }
        }
        self.objects.retain(|address, _| marked.contains(address));
        let live_bytes: usize = self.objects.values().map(GcObject::bytes).sum::<usize>()
            + self.objects.len() * std::mem::size_of::<(usize, GcObject)>();
        self.threshold = live_bytes.saturating_mul(2).max(GC_MIN_THRESHOLD);
        self.allocated = 0;
        self.collections += 1;
    }
}

/// 接管新对象并保护到当前语句结束；对象登记项与载荷共同计入预算，防止短字符串积压。
/// 分配内部不回收，避免扫描半构造对象；注册表保留容量不作为存活载荷重复记账。
fn gc_track(address: usize, object: GcObject) {
    GC_HEAP.with(|heap| {
        let mut heap = heap.borrow_mut();
        heap.allocated = heap
            .allocated
            .saturating_add(object.bytes())
            .saturating_add(std::mem::size_of::<(usize, GcObject)>());
        heap.objects.insert(address, object);
        if let Some(frame) = heap.frames.last_mut() {
            frame.pending.push(address);
        }
    });
}

/// 接管字符串并返回稳定地址；空字符串使用静态存储，无需注册和释放。
fn gc_allocate_string(bytes: Box<[u8]>) -> *const u8 {
    if bytes.is_empty() {
        return b"".as_ptr();
    }
    let ptr = bytes.as_ptr();
    gc_track(ptr as usize, GcObject::String(bytes));
    ptr
}

/// 进入语言函数；原生函数共享调用者保护帧，不允许跨线程保存托管指针。
#[unsafe(no_mangle)]
pub extern "C" fn mas_gc_enter() {
    GC_HEAP.with(|heap| heap.borrow_mut().frames.push(GcFrame::default()));
}

/// 注册当前函数的根槽；地址必须有效到 leave，注册本身不触发收集。
#[unsafe(no_mangle)]
pub extern "C" fn mas_gc_root(address: *const u8, kind: u8, count: u64) {
    GC_HEAP.with(|heap| {
        heap.borrow_mut()
            .frames
            .last_mut()
            .expect("GC frame missing")
            .roots
            .push(GcRoot {
                address,
                kind,
                count: count as usize,
            })
    });
}

/// 移除当前栈帧，不在返回值写入调用者根槽前触发回收。
#[unsafe(no_mangle)]
pub extern "C" fn mas_gc_leave() {
    GC_HEAP.with(|heap| {
        heap.borrow_mut().frames.pop().expect("GC frame missing");
    });
}

/// 语句或循环安全点结束构造保护，并按累计分配量决定是否收集。
#[unsafe(no_mangle)]
pub extern "C" fn mas_gc_poll() {
    GC_HEAP.with(|heap| {
        let mut heap = heap.borrow_mut();
        if let Some(frame) = heap.frames.last_mut() {
            frame.pending.clear();
        }
        if heap.allocated >= heap.threshold.max(GC_MIN_THRESHOLD) {
            heap.collect();
        }
    });
}

/// 强制收集用于运行时诊断；保留活跃帧和当前未完成语句的引用。
#[unsafe(no_mangle)]
pub extern "C" fn mas_gc_collect() {
    GC_HEAP.with(|heap| heap.borrow_mut().collect());
}

/// 返回托管对象数，供端到端回归检查存活对象是否随累计工作量增长。
#[unsafe(no_mangle)]
pub extern "C" fn mas_gc_live_objects() -> i64 {
    GC_HEAP.with(|heap| heap.borrow().objects.len() as i64)
}

/// 返回收集次数，供回归确认自动安全点实际触发回收。
#[unsafe(no_mangle)]
pub extern "C" fn mas_gc_collections() -> i64 {
    GC_HEAP.with(|heap| heap.borrow().collections as i64)
}

/// 正常退出时释放全部剩余对象；此时不允许继续访问托管指针。
fn gc_shutdown() {
    GC_HEAP.with(|heap| {
        let mut heap = heap.borrow_mut();
        assert!(heap.frames.is_empty(), "GC frames still active at shutdown");
        heap.objects.clear();
    });
}

/// 将实际容量增长计入分配预算；零增长是恒等操作，直接返回以避免 TLS 借用，不触发收集。
fn gc_charge(bytes: usize) {
    if bytes == 0 {
        return;
    }
    GC_HEAP.with(|heap| {
        let mut heap = heap.borrow_mut();
        heap.allocated = heap.allocated.saturating_add(bytes);
    });
}

/// 为编译器提供八字节对齐的零初始化对象；静态偏移表只列托管指针，供 GC 精确追踪。
/// 调用者保证布局与偏移表有效且表在整个进程存活；分配阶段不触发回收。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mas_record_alloc(size: u64, offsets: *const u64, count: u64) -> *mut u8 {
    let mut words = vec![0u64; (size as usize).div_ceil(8).max(1)].into_boxed_slice();
    let address = words.as_mut_ptr().cast::<u8>();
    let offsets = if count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(offsets, count as usize)
    };
    gc_track(address as usize, GcObject::Record { words, offsets });
    address
}

/// 检查原生返回对象的所有权；仅接受当前线程 GC 的对象或允许的 null，不解引用外部地址。
/// C 可返回借入的对象，或用 mas_record_alloc 分配新对象；栈、静态区及 malloc 地址不能冒充托管引用。
#[unsafe(no_mangle)]
pub extern "C" fn mas_native_require_record(pointer: *mut u8, nullable: u8) -> *mut u8 {
    if pointer.is_null() {
        if nullable != 0 {
            return pointer;
        }
        abort_runtime("原生函数为非空类型返回了 null");
    }
    let valid = GC_HEAP.with(|heap| {
        matches!(
            heap.borrow().objects.get(&(pointer as usize)),
            Some(GcObject::Record { .. })
        )
    });
    if !valid {
        abort_runtime("原生函数返回了非托管对象；请使用 mas_record_alloc 或返回借入的对象");
    }
    pointer
}
