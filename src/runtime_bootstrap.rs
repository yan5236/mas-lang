/// 返回构建阶段写入临时文件的最小 Rust 引导源码。
pub fn bootstrap_source() -> &'static str {
    r#"#[repr(C)]
pub struct MsString {
    ptr: *const u8,
    len: u64,
}

#[unsafe(no_mangle)]
pub extern "C" fn print_int(value: i64) {
    println!("{}", value);
}

#[unsafe(no_mangle)]
pub extern "C" fn print_float(value: f64) {
    println!("{}", value);
}

#[unsafe(no_mangle)]
pub extern "C" fn print_bool(value: u8) {
    println!("{}", value != 0);
}

#[unsafe(no_mangle)]
pub extern "C" fn print_string(ptr: *const u8, len: u64) {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let text = std::str::from_utf8(bytes).unwrap_or("<invalid utf8>");
    println!("{}", text);
}

#[unsafe(no_mangle)]
pub extern "C" fn string_concat(
    left_ptr: *const u8,
    left_len: u64,
    right_ptr: *const u8,
    right_len: u64,
    out_len: *mut u64,
) -> *const u8 {
    let left_bytes = unsafe { std::slice::from_raw_parts(left_ptr, left_len as usize) };
    let right_bytes = unsafe { std::slice::from_raw_parts(right_ptr, right_len as usize) };
    let mut buffer = Vec::with_capacity(left_bytes.len() + right_bytes.len());
    buffer.extend_from_slice(left_bytes);
    buffer.extend_from_slice(right_bytes);
    let boxed = buffer.into_boxed_slice();
    let len = boxed.len() as u64;
    let ptr = Box::into_raw(boxed) as *const u8;
    unsafe {
        *out_len = len;
    }
    ptr
}

#[unsafe(no_mangle)]
pub extern "C" fn string_equals(
    left_ptr: *const u8,
    left_len: u64,
    right_ptr: *const u8,
    right_len: u64,
) -> u8 {
    let left_bytes = unsafe { std::slice::from_raw_parts(left_ptr, left_len as usize) };
    let right_bytes = unsafe { std::slice::from_raw_parts(right_ptr, right_len as usize) };
    u8::from(left_bytes == right_bytes)
}

unsafe extern "C" {
    fn mascript_entry();
}

fn main() {
    unsafe {
        mascript_entry();
    }
}
"#
}

#[cfg(test)]
mod tests {
    use super::bootstrap_source;

    /// 校验引导源码以 UTF-8 无 BOM 形式提供给写盘层。
    #[test]
    fn should_not_include_utf8_bom() {
        let bytes = bootstrap_source().as_bytes();
        assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
    }
}
