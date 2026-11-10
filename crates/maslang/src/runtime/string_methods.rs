// 字符串方法集合的运行时实现：切分、查找、截取、大小写转换与重复拼接。
//
// 所有入口都按 Unicode 标量处理字符串，绝不把 UTF-8 字节偏移暴露给 Mas 层；
// 原生构造的非法 UTF-8 统一按带源码位置的运行时错误终止，语义参数错误同样带位置。

/// 把字符串参数解析为 UTF-8 文本；非法编码报告带源码位置的运行时错误。
/// 空长度容忍空指针，避免为零字节切片构造无效引用。
fn parse_runtime_text_at<'a>(ptr: *const u8, len: u64, site: RuntimeErrorSite) -> &'a str {
    let bytes = if len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(ptr, len as usize) }
    };
    std::str::from_utf8(bytes)
        .unwrap_or_else(|_| abort_runtime_at("字符串不是合法的 UTF-8 文本", site))
}

/// 把 ABI 透传的路径、源码和 span 参数组装成统一的运行时错误定位对象。
fn method_error_site(
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> RuntimeErrorSite {
    make_runtime_error_site(
        path_ptr, path_len, source_ptr, source_len, span_start, span_end,
    )
}

/// 把文本复制成 GC 托管字符串并返回 ABI 片段；空文本复用静态空串，调用方负责保活。
fn managed_text(text: &str) -> (*const u8, u64) {
    let bytes = text.as_bytes().to_vec().into_boxed_slice();
    let len = bytes.len() as u64;
    (gc_allocate_string(bytes), len)
}

/// 把 Unicode 标量下标换算为字节偏移；下标等于标量数时返回文本长度。
fn scalar_byte_offset(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(offset, _)| offset)
}

/// 把可选结果的输出参数写成失败状态：长度清零，有效标记置 0。
fn mark_optional_absent(out_len: *mut u64, out_valid: *mut u8) {
    unsafe {
        *out_len = 0;
        *out_valid = 0;
    }
}

/// 按字面分隔符切分字符串，保留头尾与连续分隔符产生的空项。
/// 空分隔符是带源码位置的参数错误；空输入配非空分隔符得到一个空串元素。
#[unsafe(no_mangle)]
pub extern "C" fn string_split(
    ptr: *const u8,
    len: u64,
    sep_ptr: *const u8,
    sep_len: u64,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> *mut MsList {
    let site = method_error_site(
        path_ptr, path_len, source_ptr, source_len, span_start, span_end,
    );
    let text = parse_runtime_text_at(ptr, len, site);
    let separator = parse_runtime_text_at(sep_ptr, sep_len, site);
    if separator.is_empty() {
        abort_runtime_at("字符串方法 `split` 的分隔符不能为空", site);
    }

    let list = list_create(KIND_STRING);
    for part in text.split(separator) {
        let (part_ptr, part_len) = managed_text(part);
        list_append_string(list, part_ptr, part_len);
    }
    list
}

/// 返回首个匹配的 Unicode 标量位置；`start` 是标量下标，未命中或起点越界返回 -1。
/// 空 needle 在合法起点直接返回该起点，因此 `0..=len` 的起点都算合法。
#[unsafe(no_mangle)]
pub extern "C" fn string_index_of(
    ptr: *const u8,
    len: u64,
    needle_ptr: *const u8,
    needle_len: u64,
    start: i64,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> i64 {
    let site = method_error_site(
        path_ptr, path_len, source_ptr, source_len, span_start, span_end,
    );
    let text = parse_runtime_text_at(ptr, len, site);
    let needle = parse_runtime_text_at(needle_ptr, needle_len, site);
    let total = text.chars().count();
    let Ok(start) = usize::try_from(start) else {
        return -1;
    };
    if start > total {
        return -1;
    }
    if needle.is_empty() {
        return start as i64;
    }

    let byte_start = scalar_byte_offset(text, start);
    match text[byte_start..].find(needle) {
        Some(offset) => (start + text[byte_start..byte_start + offset].chars().count()) as i64,
        None => -1,
    }
}

/// 截取半开区间 `[start, end)`；负数、倒序或越界时写失败标记并返回空指针。
/// 合法区间即使为空也返回有效空串，因此调用方以有效标记而不是指针判断成功。
#[unsafe(no_mangle)]
pub extern "C" fn string_substring(
    ptr: *const u8,
    len: u64,
    start: i64,
    end: i64,
    out_len: *mut u64,
    out_valid: *mut u8,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> *const u8 {
    if out_len.is_null() || out_valid.is_null() {
        abort_runtime("字符串可选结果输出指针不能为空");
    }
    let site = method_error_site(
        path_ptr, path_len, source_ptr, source_len, span_start, span_end,
    );
    let text = parse_runtime_text_at(ptr, len, site);
    let total = text.chars().count();
    let (Ok(start_index), Ok(end_index)) = (usize::try_from(start), usize::try_from(end)) else {
        mark_optional_absent(out_len, out_valid);
        return std::ptr::null();
    };
    if start_index > end_index || end_index > total {
        mark_optional_absent(out_len, out_valid);
        return std::ptr::null();
    }

    let start_byte = scalar_byte_offset(text, start_index);
    let end_byte = scalar_byte_offset(text, end_index);
    let (part_ptr, part_len) = managed_text(&text[start_byte..end_byte]);
    unsafe {
        *out_len = part_len;
        *out_valid = 1;
    }
    part_ptr
}

/// 使用 Rust 标准库的 Unicode 默认规则转小写，允许长度变化（如 `İ`、`ß` 等）。
#[unsafe(no_mangle)]
pub extern "C" fn string_to_lower(
    ptr: *const u8,
    len: u64,
    out_len: *mut u64,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> *const u8 {
    let site = method_error_site(
        path_ptr, path_len, source_ptr, source_len, span_start, span_end,
    );
    let text = parse_runtime_text_at(ptr, len, site);
    boxed_string_into_raw(text.to_lowercase(), out_len)
}

/// 使用 Rust 标准库的 Unicode 默认规则转大写，允许长度变化（如 `ß` 变为 `SS`）。
#[unsafe(no_mangle)]
pub extern "C" fn string_to_upper(
    ptr: *const u8,
    len: u64,
    out_len: *mut u64,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> *const u8 {
    let site = method_error_site(
        path_ptr, path_len, source_ptr, source_len, span_start, span_end,
    );
    let text = parse_runtime_text_at(ptr, len, site);
    boxed_string_into_raw(text.to_uppercase(), out_len)
}

/// 重复拼接字符串 `count` 次；负数或无法分配时写失败标记并返回空指针，零次返回有效空串。
/// 先检查长度乘法与分配容量，避免把不可分配的长度交给分配器。
#[unsafe(no_mangle)]
pub extern "C" fn string_repeat(
    ptr: *const u8,
    len: u64,
    count: i64,
    out_len: *mut u64,
    out_valid: *mut u8,
    path_ptr: *const u8,
    path_len: u64,
    source_ptr: *const u8,
    source_len: u64,
    span_start: u64,
    span_end: u64,
) -> *const u8 {
    if out_len.is_null() || out_valid.is_null() {
        abort_runtime("字符串可选结果输出指针不能为空");
    }
    let site = method_error_site(
        path_ptr, path_len, source_ptr, source_len, span_start, span_end,
    );
    let text = parse_runtime_text_at(ptr, len, site);
    let Ok(count) = usize::try_from(count) else {
        mark_optional_absent(out_len, out_valid);
        return std::ptr::null();
    };
    let Some(total) = text.len().checked_mul(count) else {
        mark_optional_absent(out_len, out_valid);
        return std::ptr::null();
    };

    let mut buffer: Vec<u8> = Vec::new();
    if buffer.try_reserve_exact(total).is_err() {
        mark_optional_absent(out_len, out_valid);
        return std::ptr::null();
    }
    for _ in 0..count {
        buffer.extend_from_slice(text.as_bytes());
    }

    let boxed = buffer.into_boxed_slice();
    let result_len = boxed.len() as u64;
    let result_ptr = gc_allocate_string(boxed);
    unsafe {
        *out_len = result_len;
        *out_valid = 1;
    }
    result_ptr
}
