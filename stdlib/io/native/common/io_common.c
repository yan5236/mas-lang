#include "io_internal.h"

#include <stdlib.h>
#include <string.h>

/**
 * 校验一段 UTF-8：拒绝孤立续字节、过长编码、UTF-16 代理区以及超过 U+10FFFF 的值。
 * 空串合法。该实现按字节推进，不依赖宿主 locale，也不做有损替换。
 */
int mas_io_utf8_is_valid(const unsigned char* ptr, size_t len) {
    size_t index = 0;

    if (ptr == NULL) {
        return len == 0;
    }

    while (index < len) {
        unsigned char lead = ptr[index];
        unsigned int codepoint;
        unsigned int minimum;
        size_t continuation;
        size_t offset;

        if (lead < 0x80) {
            index += 1;
            continue;
        }
        if ((lead & 0xE0) == 0xC0) {
            codepoint = (unsigned int)(lead & 0x1F);
            minimum = 0x80U;
            continuation = 1;
        } else if ((lead & 0xF0) == 0xE0) {
            codepoint = (unsigned int)(lead & 0x0F);
            minimum = 0x800U;
            continuation = 2;
        } else if ((lead & 0xF8) == 0xF0) {
            codepoint = (unsigned int)(lead & 0x07);
            minimum = 0x10000U;
            continuation = 3;
        } else {
            return 0;
        }

        /* 剩余字节必须足够容纳全部续字节。 */
        if (len - index <= continuation) {
            return 0;
        }
        for (offset = 1; offset <= continuation; offset += 1) {
            unsigned char next = ptr[index + offset];
            if ((next & 0xC0) != 0x80) {
                return 0;
            }
            codepoint = (codepoint << 6) | (unsigned int)(next & 0x3F);
        }

        if (codepoint < minimum || codepoint > 0x10FFFFU) {
            return 0;
        }
        if (codepoint >= 0xD800U && codepoint <= 0xDFFFU) {
            return 0;
        }
        index += continuation + 1;
    }

    return 1;
}

/**
 * 校验路径：非空、不含内嵌 NUL、是合法 UTF-8。
 * 长度超出 `size_t` 能表达的范围时返回 `TooLarge`，避免后续转换截断。
 */
int64_t mas_io_validate_path(const unsigned char* ptr, unsigned long long len, size_t* out_len) {
    if (out_len == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    if (ptr == NULL || len == 0) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    if (len > (unsigned long long)((size_t)-1)) {
        return MAS_IO_ERROR_TOO_LARGE;
    }
    if (memchr(ptr, 0, (size_t)len) != NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    if (!mas_io_utf8_is_valid(ptr, (size_t)len)) {
        return MAS_IO_ERROR_INVALID_UTF8;
    }
    *out_len = (size_t)len;
    return MAS_IO_OK;
}

/** 把字节复制成托管字符串；空内容统一借助静态空指针，避免把 NULL 交给运行时。 */
MasString mas_io_make_string(const unsigned char* ptr, size_t len) {
    static const unsigned char empty = 0;

    if (ptr == NULL || len == 0) {
        ptr = &empty;
        len = 0;
    }
    return mas_runtime_string_clone(ptr, (unsigned long long)len);
}

/** 把字节复制成托管字节；空内容同样返回有效空值。 */
MasBytes mas_io_make_bytes(const unsigned char* ptr, size_t len) {
    static const unsigned char empty = 0;

    if (ptr == NULL || len == 0) {
        ptr = &empty;
        len = 0;
    }
    return mas_runtime_bytes_clone(ptr, (unsigned long long)len);
}

/** 在托管记录中分配一个 `int64` 盒子；没有指针载荷，因此偏移表传 NULL。 */
int64_t* mas_io_make_i64(int64_t value) {
    int64_t* box = (int64_t*)mas_record_alloc((unsigned long long)sizeof(int64_t), NULL, 0);

    if (box == NULL) {
        return NULL;
    }
    *box = value;
    return box;
}

/** 带溢出检查的 `int64` 加法，避免有符号溢出这种未定义行为。 */
int mas_io_add_i64(int64_t left, int64_t right, int64_t* out) {
    if (out == NULL) {
        return 0;
    }
    if (right > 0 && left > INT64_MAX - right) {
        return 0;
    }
    if (right < 0 && left < INT64_MIN - right) {
        return 0;
    }
    *out = left + right;
    return 1;
}

/**
 * 有界扩容追加：容量从 8 KiB 起倍增，直到刚好覆盖需要的长度；
 * 长度相加溢出或分配失败时不改变 `*data` 与 `*length`。
 */
int mas_io_buffer_append(
    unsigned char** data,
    size_t* length,
    size_t* capacity,
    const unsigned char* extra,
    size_t extra_len
) {
    size_t needed;
    size_t grown_capacity;
    unsigned char* grown;

    if (data == NULL || length == NULL || capacity == NULL) {
        return 0;
    }
    if (extra_len == 0) {
        return 1;
    }
    if (extra == NULL || *length > (size_t)-1 - extra_len) {
        return 0;
    }

    needed = *length + extra_len;
    if (needed > *capacity) {
        grown_capacity = *capacity == 0 ? (size_t)MAS_IO_READ_BUFFER_SIZE : *capacity;
        while (grown_capacity < needed) {
            if (grown_capacity > (size_t)-1 / 2) {
                grown_capacity = needed;
                break;
            }
            grown_capacity *= 2;
        }
        grown = (unsigned char*)realloc(*data, grown_capacity);
        if (grown == NULL) {
            return 0;
        }
        *data = grown;
        *capacity = grown_capacity;
    }

    memcpy(*data + *length, extra, extra_len);
    *length = needed;
    return 1;
}

/** 把运行时桥的负值错误码收敛到已知类别，未知一律归 `Other`。 */
int64_t mas_io_normalize_code(int64_t code) {
    if (code >= MAS_IO_ERROR_NOT_FOUND && code <= MAS_IO_ERROR_OTHER) {
        return code;
    }
    return MAS_IO_ERROR_OTHER;
}

/** 只写状态字段：成功与否完全由错误码决定，避免两处维护同一判断。 */
void mas_io_store_status(MasIoStatus* out, int64_t code) {
    if (out == NULL) {
        return;
    }
    out->ok = (uint8_t)(code == MAS_IO_OK ? 1 : 0);
    out->code = code;
}

/** 打开失败：句柄载荷写 NULL，调用方必须先看 `ok`。 */
void mas_io_store_open_failure(MasIoOpenResult* out, int64_t code) {
    if (out == NULL) {
        return;
    }
    out->ok = 0;
    out->code = code;
    out->value = NULL;
}

/** 字节结果：失败时写有效空字节，避免把悬空或 NULL 载荷交给 Mas。 */
void mas_io_store_bytes_result(MasIoBytesResult* out, int64_t code, const unsigned char* data, size_t len) {
    if (out == NULL) {
        return;
    }
    out->ok = (uint8_t)(code == MAS_IO_OK ? 1 : 0);
    out->code = code;
    out->value = code == MAS_IO_OK ? mas_io_make_bytes(data, len) : mas_io_make_bytes(NULL, 0);
}

/** 文本结果：失败时写有效空串。 */
void mas_io_store_text_result(MasIoTextResult* out, int64_t code, const unsigned char* data, size_t len) {
    if (out == NULL) {
        return;
    }
    out->ok = (uint8_t)(code == MAS_IO_OK ? 1 : 0);
    out->code = code;
    out->value = code == MAS_IO_OK ? mas_io_make_string(data, len) : mas_io_make_string(NULL, 0);
}

/** 分块读取结果：失败时 eof 必须为 0，成功时按调用方判断是否到达末尾。 */
void mas_io_store_read_result(
    MasIoReadResult* out,
    int64_t code,
    const unsigned char* data,
    size_t len,
    int eof
) {
    if (out == NULL) {
        return;
    }
    out->ok = (uint8_t)(code == MAS_IO_OK ? 1 : 0);
    out->code = code;
    out->value = code == MAS_IO_OK ? mas_io_make_bytes(data, len) : mas_io_make_bytes(NULL, 0);
    out->eof = (uint8_t)(code == MAS_IO_OK && eof != 0 ? 1 : 0);
}

/** 读行结果：失败时 eof 为 0，空行是成功且 eof 为 0 的独立情形。 */
void mas_io_store_line_result(
    MasIoLineResult* out,
    int64_t code,
    const unsigned char* data,
    size_t len,
    int eof
) {
    if (out == NULL) {
        return;
    }
    out->ok = (uint8_t)(code == MAS_IO_OK ? 1 : 0);
    out->code = code;
    out->value = code == MAS_IO_OK ? mas_io_make_string(data, len) : mas_io_make_string(NULL, 0);
    out->eof = (uint8_t)(code == MAS_IO_OK && eof != 0 ? 1 : 0);
}

/** 写入结果：失败时保留宿主已接受的字节数，未写入时为 0。 */
void mas_io_store_write_result(MasIoWriteResult* out, int64_t code, int64_t written) {
    if (out == NULL) {
        return;
    }
    out->ok = (uint8_t)(code == MAS_IO_OK ? 1 : 0);
    out->code = code;
    out->written = written < 0 ? 0 : written;
}

/** 定位结果：失败时位置写 0，与设计的无害占位值一致。 */
void mas_io_store_seek_result(MasIoSeekResult* out, int64_t code, int64_t position) {
    if (out == NULL) {
        return;
    }
    out->ok = (uint8_t)(code == MAS_IO_OK ? 1 : 0);
    out->code = code;
    out->position = code == MAS_IO_OK ? position : 0;
}

/** 元数据结果：失败时类型为 `Other` 且大小为 NULL；`size_box` 由调用方预先分配。 */
void mas_io_store_metadata_result(MasIoMetadataResult* out, int64_t code, int64_t kind, int64_t* size_box) {
    if (out == NULL) {
        return;
    }
    out->ok = (uint8_t)(code == MAS_IO_OK ? 1 : 0);
    out->code = code;
    if (code == MAS_IO_OK) {
        out->kind = kind;
        out->size = size_box;
        return;
    }
    out->kind = MAS_IO_KIND_OTHER;
    out->size = NULL;
}

/** 目录打开结果：失败时句柄写 0，令牌 0 永远是无效值。 */
void mas_io_store_dir_open_result(MasIoDirOpenResult* out, int64_t code, int64_t handle) {
    if (out == NULL) {
        return;
    }
    out->ok = (uint8_t)(code == MAS_IO_OK ? 1 : 0);
    out->code = code;
    out->handle = code == MAS_IO_OK ? handle : 0;
}

/** 目录迭代结果：正常结束时 `done` 为 1 且名字为空串。 */
void mas_io_store_dir_entry_result(
    MasIoDirEntryResult* out,
    int64_t code,
    int done,
    const unsigned char* name,
    size_t len
) {
    if (out == NULL) {
        return;
    }
    out->ok = (uint8_t)(code == MAS_IO_OK ? 1 : 0);
    out->code = code;
    out->done = (uint8_t)(code == MAS_IO_OK && done != 0 ? 1 : 0);
    out->name = code == MAS_IO_OK && done == 0 ? mas_io_make_string(name, len) : mas_io_make_string(NULL, 0);
}
