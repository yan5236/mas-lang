/// 创建编译器私有的追加缓冲区，复制初值；字节地址不暴露给语言层，返回 GC 托管句柄。
#[unsafe(no_mangle)]
pub extern "C" fn mas_string_builder_create(ptr: *const u8, len: u64) -> *mut Vec<u8> {
    let mut bytes = Box::new(string_bytes(MsString { ptr, len }).to_vec());
    let handle = &mut *bytes as *mut Vec<u8>;
    gc_track(handle as usize, GcObject::StringBuilder(bytes));
    handle
}

/// 向仍被根槽保护的私有句柄追加不可变字符串；Vec 几何扩容，增量容量计入 GC 阈值。
#[unsafe(no_mangle)]
pub extern "C" fn mas_string_builder_append(handle: *mut Vec<u8>, ptr: *const u8, len: u64) {
    // 句柄由 create 创建且由编译器注册根；外部字符串不能引用私有缓冲区。
    let bytes = unsafe { &mut *handle };
    let capacity = bytes.capacity();
    bytes.extend_from_slice(string_bytes(MsString { ptr, len }));
    gc_charge(bytes.capacity() - capacity);
}

/// 循环退出后移交缓冲区所有权为普通不可变字符串；句柄变空，旧字符串别名保持不变。
#[unsafe(no_mangle)]
pub extern "C" fn mas_string_builder_finish(handle: *mut Vec<u8>, out_len: *mut u64) -> *const u8 {
    let bytes = std::mem::take(unsafe { &mut *handle }).into_boxed_slice();
    unsafe { *out_len = bytes.len() as u64; }
    gc_allocate_string(bytes)
}
