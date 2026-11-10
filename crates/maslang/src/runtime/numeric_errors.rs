/// 将数值或下标安全检查失败转换为带源码定位的语言错误；不允许返回到危险指令。
#[unsafe(no_mangle)]
pub extern "C" fn mas_numeric_error(
    code: u8,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> ! {
    let message = match code {
        1 => "整数除零",
        2 => "整数溢出",
        3 => "数组下标越界",
        5 => "不能读取 null 的值；请先用 != null 判断，再访问 .value",
        4 => "浮点数无法转换为整数：数值非有限或超出 i64 范围",
        6 => "安全结果失败时没有有效的 .value；请先检查 .ok，再读取 .value",
        7 => "字节下标越界",
        _ => "未知数值错误",
    };
    abort_runtime_at(
        message,
        make_runtime_error_site(
            path_ptr, path_len, source_ptr, source_len, span_start, span_end,
        ),
    )
}

/// 精确比较 i64 与 f64，返回小于 -1、等于 0、大于 1，或遇到 NaN 时返回 2。
/// 整数转换为浮点后若恰好相等，再用整数比较找回转换时舍入掉的差值。
#[unsafe(no_mangle)]
pub extern "C" fn mas_compare_int_float(integer: i64, float: f64) -> i8 {
    use std::cmp::Ordering;
    match (integer as f64).partial_cmp(&float) {
        Some(Ordering::Equal) if float >= -(i64::MIN as f64) => -1,
        Some(Ordering::Equal) => integer.cmp(&(float as i64)) as i8,
        Some(order) => order as i8,
        None => 2,
    }
}
