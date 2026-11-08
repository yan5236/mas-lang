/// 将 i64 按标准 Display 写入调用方提供的至少 20 字节缓冲，返回有效长度；不分配、不保留指针。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn int_to_string_into(value: i64, buffer: *mut u8) -> u64 {
    let bytes = unsafe { std::slice::from_raw_parts_mut(buffer, 20) };
    let mut cursor = std::io::Cursor::new(bytes);
    std::io::Write::write_fmt(&mut cursor, format_args!("{value}"))
        .expect("i64 text fits in 20 bytes");
    cursor.position()
}

/// 更新时仅查找一次并保留原键；新插入才复制借用字节，复用普通写入的索引、顺序和 GC 预算逻辑。
/// 内部不触发安全点，复制后的半构造键在插入前由现有 GC pending 机制保护。
#[inline(always)]
unsafe fn map_set_borrowed_string(map: *mut MsMap, key: MsString, value: ScalarCell) {
    if let Some(index) = unsafe { map_find_index(map, ScalarCell::String(key)) } {
        let (_, stored) = unsafe { (&mut (*map).entries)[index].as_mut() }
            .unwrap_or_else(|| abort_runtime("内部字典索引失效"));
        *stored = value;
        return;
    }
    let owned = MsString {
        ptr: gc_allocate_string(string_bytes(key).into()),
        len: key.len,
    };
    unsafe { map_set_cell(map, ScalarCell::String(owned), value) };
}

macro_rules! define_borrowed_string_set {
    ($name:ident, $variant:ident, $ty:ty) => {
        /// 写入借用字符串键及指定标量值；仅新键复制，调用结束后不保留输入键缓冲地址。
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(map: *mut MsMap, ptr: *const u8, len: u64, value: $ty) {
            unsafe {
                map_set_borrowed_string(map, MsString { ptr, len }, ScalarCell::$variant(value))
            };
        }
    };
}
define_borrowed_string_set!(map_set_string_int_borrowed, Int, i64);
define_borrowed_string_set!(map_set_string_float_borrowed, Float, f64);
define_borrowed_string_set!(map_set_string_bool_borrowed, Bool, u8);

/// 借用字符串键与普通托管字符串值写入；值的可达性沿用字典扫描，只复制首次插入的键。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn map_set_string_string_borrowed(
    map: *mut MsMap,
    key_ptr: *const u8,
    key_len: u64,
    ptr: *const u8,
    len: u64,
) {
    unsafe {
        map_set_borrowed_string(
            map,
            MsString {
                ptr: key_ptr,
                len: key_len,
            },
            ScalarCell::String(MsString { ptr, len }),
        )
    };
}
