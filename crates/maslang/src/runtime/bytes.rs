// 表示与原生代码共享的字节缓冲 ABI：只读指针加长度，内容归 GC 所有。
// 该结构在 C 侧对应 `MasBytes`，字段顺序固定为 `ptr`、`len`，不允许重排。
//
// 注意：返回值是 16 字节结构体时，Windows x64 的 C ABI 走隐藏返回指针（sret），
// 编译器 IR 里的 `declare { ptr, i64 }` 与 Rust 生成的函数签名并不一致。因此凡是要被
// Mas 编译产物直接调用的入口，一律用 `out: *mut MasBytes` 写回，不返回结构体；
// 只有仅供 C 代码调用的 `mas_runtime_bytes_clone` 保留结构体返回值。
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MasBytes {
    ptr: *const u8,
    len: u64,
}

/// 返回静态非空的空字节指针，长度为零，可安全作为有效空值。
const EMPTY_BYTES: MasBytes = MasBytes {
    ptr: b"".as_ptr(),
    len: 0,
};

/// 把 `MasBytes` 写入调用方提供的输出槽；输出指针为空属于契约违例。
unsafe fn store_bytes(out: *mut MasBytes, value: MasBytes) {
    if out.is_null() {
        abort_runtime("字节结果输出指针不能为空");
    }
    *out = value;
}

/// 按字节区间借用调用方传入的原始内存；长度为零时不构造切片，避免空指针参与指针运算。
fn borrowed_bytes<'a>(ptr: *const u8, len: u64) -> &'a [u8] {
    if len == 0 {
        return &[];
    }
    unsafe { std::slice::from_raw_parts(ptr, len as usize) }
}

/// 复制原生字节区间到 GC 托管字节对象；原生输入只在调用期间借用，返回值不能由原生代码释放。
/// 空区间使用静态非空指针表示合法空值；非空区间但指针为 NULL 属于契约违例。
#[unsafe(no_mangle)]
pub extern "C" fn mas_runtime_bytes_clone(ptr: *const u8, len: u64) -> MasBytes {
    if len == 0 {
        return EMPTY_BYTES;
    }
    if ptr.is_null() {
        abort_runtime("原生字节指针不能为空");
    }
    let data = unsafe { std::slice::from_raw_parts(ptr, len as usize) }
        .to_vec()
        .into_boxed_slice();
    let len = data.len() as u64;
    MasBytes {
        ptr: gc_allocate_bytes(data),
        len,
    }
}

/// 把合法的空字节值写回输出槽，供编译器为零长度结果和失败占位复用，不触发回收。
#[unsafe(no_mangle)]
pub extern "C" fn mas_runtime_bytes_empty(out: *mut MasBytes) {
    unsafe { store_bytes(out, EMPTY_BYTES) }
}

/// 把 `list<int>` 按值转换为字节对象；任一元素超出 0..=255 时把 `valid` 置零并写出空值。
/// 调用方必须传入运行时整数列表；其他元素种类属于编译器契约违例。
#[unsafe(no_mangle)]
pub extern "C" fn mas_runtime_bytes_from_list(
    list: *const MsList,
    valid: *mut u8,
    out: *mut MasBytes,
) {
    if list.is_null() || valid.is_null() || out.is_null() {
        abort_runtime("字节构造的参数不能为空");
    }
    unsafe {
        let list = &*list;
        if list.elem_kind != KIND_INT {
            abort_runtime("字节构造只接受整数列表");
        }
        let mut output = Vec::new();
        if list.len > 0 {
            output.reserve(list.len as usize);
            for index in 0..list.len as usize {
                let value = std::ptr::read_unaligned(list.data.add(index * 8).cast::<i64>());
                if !(0..=255).contains(&value) {
                    *valid = 0;
                    store_bytes(out, EMPTY_BYTES);
                    return;
                }
                output.push(value as u8);
            }
        }
        let output = output.into_boxed_slice();
        let len = output.len() as u64;
        *valid = 1;
        store_bytes(
            out,
            MasBytes {
                ptr: gc_allocate_bytes(output),
                len,
            },
        );
    }
}

/// 把合法 UTF-8 文本编码为字节对象；原生构造的非法 UTF-8 字符串把 `valid` 置零。
#[unsafe(no_mangle)]
pub extern "C" fn mas_runtime_bytes_encode_utf8(
    ptr: *const u8,
    len: u64,
    valid: *mut u8,
    out: *mut MasBytes,
) {
    if valid.is_null() || out.is_null() {
        abort_runtime("字节编码的输出指针不能为空");
    }
    let bytes = borrowed_bytes(ptr, len);
    match std::str::from_utf8(bytes) {
        Ok(text) => {
            let data = text.as_bytes().to_vec().into_boxed_slice();
            let length = data.len() as u64;
            unsafe {
                *valid = 1;
                store_bytes(
                    out,
                    MasBytes {
                        ptr: gc_allocate_bytes(data),
                        len: length,
                    },
                );
            }
        }
        Err(_) => unsafe {
            *valid = 0;
            store_bytes(out, EMPTY_BYTES);
        },
    }
}

/// 严格校验字节内容并解码为托管字符串；非法编码把 `valid` 置零并写出空串，不做损失性替换。
#[unsafe(no_mangle)]
pub extern "C" fn mas_runtime_bytes_decode_utf8(
    ptr: *const u8,
    len: u64,
    valid: *mut u8,
    out: *mut MsString,
) {
    if valid.is_null() || out.is_null() {
        abort_runtime("字节解码的输出指针不能为空");
    }
    let bytes = borrowed_bytes(ptr, len);
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => {
            unsafe {
                *valid = 1;
            }
            mas_runtime_string_clone(text.as_ptr(), text.len() as u64)
        }
        Err(_) => {
            unsafe {
                *valid = 0;
            }
            mas_runtime_string_clone(b"".as_ptr(), 0)
        }
    };
    unsafe { *out = text }
}

/// 读取单个字节的数值；调用方已检查下标范围，这里仅做防御性校验。
#[unsafe(no_mangle)]
pub extern "C" fn mas_runtime_bytes_at(ptr: *const u8, len: u64, index: i64) -> i64 {
    if index < 0 || (index as u64) >= len {
        abort_runtime("字节下标越界");
    }
    unsafe { (*ptr.add(index as usize)) as i64 }
}

/// 截取半开区间 `[start, end)` 的字节副本；负数、倒序和越界把 `valid` 置零并写出空值。
#[unsafe(no_mangle)]
pub extern "C" fn mas_runtime_bytes_slice(
    ptr: *const u8,
    len: u64,
    start: i64,
    end: i64,
    valid: *mut u8,
    out: *mut MasBytes,
) {
    if valid.is_null() || out.is_null() {
        abort_runtime("字节切片的输出指针不能为空");
    }
    let in_range = start >= 0 && end >= start && (end as u64) <= len;
    if !in_range {
        unsafe {
            *valid = 0;
            store_bytes(out, EMPTY_BYTES);
        }
        return;
    }
    let bytes = borrowed_bytes(ptr, len);
    let data = bytes[start as usize..end as usize]
        .to_vec()
        .into_boxed_slice();
    let length = data.len() as u64;
    unsafe {
        *valid = 1;
        store_bytes(
            out,
            MasBytes {
                ptr: gc_allocate_bytes(data),
                len: length,
            },
        );
    }
}

/// 按内容逐字节比较两个字节对象；长度不同直接判定为不相等。
#[unsafe(no_mangle)]
pub extern "C" fn mas_runtime_bytes_equals(
    left_ptr: *const u8,
    left_len: u64,
    right_ptr: *const u8,
    right_len: u64,
) -> u8 {
    if left_len != right_len {
        return 0;
    }
    let left = borrowed_bytes(left_ptr, left_len);
    let right = borrowed_bytes(right_ptr, right_len);
    u8::from(left == right)
}

/// 返回字节对象的字节数，与 `len(string)` 的标量计数口径区分；指针参数只为与字符串入口保持对称。
#[unsafe(no_mangle)]
pub extern "C" fn mas_runtime_bytes_len(_ptr: *const u8, len: u64) -> i64 {
    len as i64
}
