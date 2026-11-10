#include "io_internal.h"

#include <stdlib.h>
#include <string.h>

/**
 * 标准流借用句柄固定占用令牌 1..3，不进入动态表，也不会被 `close` 移除。
 * 这样 `stdin()`、`stdout()`、`stderr()` 每次返回同一个句柄，且与其他文件互不影响。
 */
static MasIoEntry g_std_entries[3] = {
    { 1, MAS_IO_MODE_READ, 1, MAS_IO_STDIN, NULL, NULL, 0, 0, 0, 0, 0 },
    { 2, MAS_IO_MODE_WRITE, 1, MAS_IO_STDOUT, NULL, NULL, 0, 0, 0, 0, 0 },
    { 3, MAS_IO_MODE_WRITE, 1, MAS_IO_STDERR, NULL, NULL, 0, 0, 0, 0, 0 }
};

/** 动态表只保存常规文件；按开放句柄数量增长，关闭时即时移除。 */
static MasIoEntry** g_entries = NULL;
static size_t g_entry_count = 0;
static size_t g_entry_capacity = 0;

/** 令牌单调递增、永不复用；常规文件从 4 开始，1..3 保留给标准流。 */
static int64_t g_next_token = 4;

/** 按令牌查找表项；标准流令牌单独处理，未知令牌返回 NULL（对应 `Closed`）。 */
MasIoEntry* mas_io_find_entry(int64_t token) {
    size_t index;

    if (token >= 1 && token <= 3) {
        return &g_std_entries[(size_t)(token - 1)];
    }
    if (token <= 0) {
        return NULL;
    }
    for (index = 0; index < g_entry_count; index += 1) {
        if (g_entries[index]->token == token) {
            return g_entries[index];
        }
    }
    return NULL;
}

/** 判断表项是否可读：只有 `Read` 与 `ReadWrite` 带读方向。 */
int mas_io_entry_readable(const MasIoEntry* entry) {
    return entry->mode == MAS_IO_MODE_READ || entry->mode == MAS_IO_MODE_READ_WRITE;
}

/** 判断表项是否可写：`Write`、`Append` 与 `ReadWrite` 带写方向。 */
int mas_io_entry_writable(const MasIoEntry* entry) {
    return entry->mode == MAS_IO_MODE_WRITE
        || entry->mode == MAS_IO_MODE_APPEND
        || entry->mode == MAS_IO_MODE_READ_WRITE;
}

/** 缓冲区中尚未交付给调用方的字节数。 */
size_t mas_io_entry_available(const MasIoEntry* entry) {
    return entry->buffer_length - entry->buffer_offset;
}

/** 丢弃读缓冲里的预读数据，不触碰宿主游标。 */
void mas_io_entry_drop_buffer(MasIoEntry* entry) {
    entry->buffer_offset = 0;
    entry->buffer_length = 0;
}

/** 把令牌插入动态表；表增长或令牌溢出失败时返回错误码。 */
int64_t mas_io_register_entry(MasIoEntry* entry) {
    MasIoEntry** grown;
    size_t capacity;

    if (g_entry_count == g_entry_capacity) {
        capacity = g_entry_capacity == 0 ? 8 : g_entry_capacity * 2;
        if (capacity <= g_entry_capacity) {
            return MAS_IO_ERROR_OTHER;
        }
        grown = (MasIoEntry**)realloc(g_entries, capacity * sizeof(*grown));
        if (grown == NULL) {
            return MAS_IO_ERROR_OTHER;
        }
        g_entries = grown;
        g_entry_capacity = capacity;
    }
    if (g_next_token <= 0 || g_next_token == INT64_MAX) {
        return MAS_IO_ERROR_TOO_LARGE;
    }

    entry->token = g_next_token;
    g_next_token += 1;
    g_entries[g_entry_count] = entry;
    g_entry_count += 1;
    return MAS_IO_OK;
}

/** 从动态表移除表项；令牌随即失效，再次访问会返回 `Closed`。 */
void mas_io_unregister_entry(MasIoEntry* entry) {
    size_t index;

    for (index = 0; index < g_entry_count; index += 1) {
        if (g_entries[index] == entry) {
            memmove(
                &g_entries[index],
                &g_entries[index + 1],
                (g_entry_count - index - 1) * sizeof(*g_entries)
            );
            g_entry_count -= 1;
            return;
        }
    }
}

/**
 * 从宿主读取：标准流走运行时桥，常规文件走平台层。
 * 桥用负值回报 `-错误码`，这里立即收敛，绝不把负值转成无符号长度。
 */
static int64_t mas_io_host_read(MasIoEntry* entry, unsigned char* buffer, size_t capacity, size_t* read_out) {
    if (entry->is_std) {
        long long got = mas_runtime_std_read(buffer, (unsigned long long)capacity);
        if (got < 0) {
            return mas_io_normalize_code(-(int64_t)got);
        }
        if ((unsigned long long)got > (unsigned long long)capacity) {
            return MAS_IO_ERROR_OTHER;
        }
        *read_out = (size_t)got;
        return MAS_IO_OK;
    }
    return mas_io_platform_read(entry->file, buffer, capacity, read_out);
}

/** 向宿主写入一次；标准流同样走运行时桥并收敛错误码。 */
int64_t mas_io_host_write(
    MasIoEntry* entry,
    const unsigned char* data,
    size_t len,
    size_t* written_out
) {
    if (entry->is_std) {
        long long wrote = mas_runtime_std_write(
            (unsigned long long)entry->std_kind,
            data,
            (unsigned long long)len
        );
        if (wrote < 0) {
            return mas_io_normalize_code(-(int64_t)wrote);
        }
        if ((unsigned long long)wrote > (unsigned long long)len) {
            return MAS_IO_ERROR_OTHER;
        }
        *written_out = (size_t)wrote;
        return MAS_IO_OK;
    }
    return mas_io_platform_write(entry->file, data, len, written_out);
}

/**
 * 读缓冲为空时向宿主补充一块数据。宿主返回 0 字节表示到达末尾，置 `eof_reached`；
 * 读到的字节留在缓冲里，逻辑位置只在真正交付给调用方时推进。
 */
int64_t mas_io_entry_fill(MasIoEntry* entry) {
    size_t got = 0;
    int64_t code;

    if (entry->eof_reached) {
        return MAS_IO_OK;
    }
    if (entry->read_buffer == NULL) {
        entry->read_buffer = (unsigned char*)malloc((size_t)MAS_IO_READ_BUFFER_SIZE);
        if (entry->read_buffer == NULL) {
            return MAS_IO_ERROR_OTHER;
        }
    }

    code = mas_io_host_read(entry, entry->read_buffer, (size_t)MAS_IO_READ_BUFFER_SIZE, &got);
    if (code != MAS_IO_OK) {
        return code;
    }
    entry->buffer_offset = 0;
    entry->buffer_length = got;
    if (got == 0) {
        entry->eof_reached = 1;
    }
    return MAS_IO_OK;
}

/**
 * 写之前的模式切换：`Append` 每次定位到末尾；`ReadWrite` 若还有预读数据，
 * 先把宿主游标同步回逻辑位置，避免写到预读区之后。普通 `Write` 没有读缓冲。
 */
int64_t mas_io_entry_sync_before_write(MasIoEntry* entry) {
    int64_t position = 0;
    int64_t code;

    if (entry->is_std) {
        return MAS_IO_OK;
    }
    if (entry->mode == MAS_IO_MODE_APPEND) {
        code = mas_io_platform_seek(entry->file, 0, MAS_IO_SEEK_END, &position);
        if (code != MAS_IO_OK) {
            return code;
        }
        entry->position = position;
        mas_io_entry_drop_buffer(entry);
        entry->eof_reached = 0;
        return MAS_IO_OK;
    }
    if (mas_io_entry_available(entry) > 0) {
        code = mas_io_platform_seek(entry->file, entry->position, MAS_IO_SEEK_START, &position);
        if (code != MAS_IO_OK) {
            return code;
        }
        entry->position = position;
        mas_io_entry_drop_buffer(entry);
        entry->eof_reached = 0;
    }
    return MAS_IO_OK;
}

/** 打开常规文件并登记令牌；失败时清理已经获取的平台资源。 */
void mas_io_open(MasIoOpenArg* in, MasIoOpenResult* out) {
    size_t path_len = 0;
    int64_t code;
    MasIoEntry* entry;
    MasIoFile* box;
    void* file = NULL;

    if (out == NULL) {
        return;
    }
    if (in == NULL) {
        mas_io_store_open_failure(out, MAS_IO_ERROR_INVALID_ARGUMENT);
        return;
    }

    code = mas_io_validate_path(in->path.ptr, in->path.len, &path_len);
    if (code != MAS_IO_OK) {
        mas_io_store_open_failure(out, code);
        return;
    }
    if (in->mode < MAS_IO_MODE_READ || in->mode > MAS_IO_MODE_READ_WRITE) {
        mas_io_store_open_failure(out, MAS_IO_ERROR_INVALID_ARGUMENT);
        return;
    }

    entry = (MasIoEntry*)calloc(1, sizeof(*entry));
    if (entry == NULL) {
        mas_io_store_open_failure(out, MAS_IO_ERROR_OTHER);
        return;
    }

    code = mas_io_platform_open(in->path.ptr, path_len, in->mode, &file);
    if (code != MAS_IO_OK) {
        free(entry);
        mas_io_store_open_failure(out, code);
        return;
    }

    box = (MasIoFile*)mas_record_alloc((unsigned long long)sizeof(MasIoFile), NULL, 0);
    if (box == NULL) {
        (void)mas_io_platform_close(file);
        free(entry);
        mas_io_store_open_failure(out, MAS_IO_ERROR_OTHER);
        return;
    }

    entry->mode = in->mode;
    entry->file = file;
    code = mas_io_register_entry(entry);
    if (code != MAS_IO_OK) {
        (void)mas_io_platform_close(file);
        free(entry);
        mas_io_store_open_failure(out, code);
        return;
    }

    box->token = entry->token;
    out->ok = 1;
    out->code = MAS_IO_OK;
    out->value = box;
}

/** 借用进程标准流句柄：令牌固定为 `which + 1`，不登记新表项。 */
void mas_io_std(MasIoStdArg* in, MasIoOpenResult* out) {
    MasIoFile* box;

    if (out == NULL) {
        return;
    }
    if (in == NULL || in->which < MAS_IO_STDIN || in->which > MAS_IO_STDERR) {
        mas_io_store_open_failure(out, MAS_IO_ERROR_INVALID_ARGUMENT);
        return;
    }

    box = (MasIoFile*)mas_record_alloc((unsigned long long)sizeof(MasIoFile), NULL, 0);
    if (box == NULL) {
        mas_io_store_open_failure(out, MAS_IO_ERROR_OTHER);
        return;
    }
    box->token = g_std_entries[(size_t)in->which].token;
    out->ok = 1;
    out->code = MAS_IO_OK;
    out->value = box;
}

/** 关闭句柄：先把令牌移出表使其立即失效，再释放平台资源与读缓冲。 */
void mas_io_close(MasIoTokenArg* in, MasIoStatus* out) {
    MasIoEntry* entry;
    int64_t code;

    if (out == NULL) {
        return;
    }
    if (in == NULL) {
        mas_io_store_status(out, MAS_IO_ERROR_INVALID_ARGUMENT);
        return;
    }

    entry = mas_io_find_entry(in->token);
    if (entry == NULL) {
        mas_io_store_status(out, MAS_IO_ERROR_CLOSED);
        return;
    }
    if (entry->is_std) {
        mas_io_store_status(out, MAS_IO_ERROR_NOT_SUPPORTED);
        return;
    }

    mas_io_unregister_entry(entry);
    code = mas_io_platform_close(entry->file);
    free(entry->read_buffer);
    free(entry);
    mas_io_store_status(out, code);
}

/**
 * 刷新输出缓冲。实现层不保留用户态写缓冲，常规文件直接成功；
 * 标准流交给运行时桥，保证与既有 `print` 的输出顺序一致。
 * 输入方向没有可刷新的缓冲，返回 `NotSupported` 而不是假装成功。
 */
void mas_io_flush(MasIoTokenArg* in, MasIoStatus* out) {
    MasIoEntry* entry;
    int64_t code;

    if (out == NULL) {
        return;
    }
    if (in == NULL) {
        mas_io_store_status(out, MAS_IO_ERROR_INVALID_ARGUMENT);
        return;
    }

    entry = mas_io_find_entry(in->token);
    if (entry == NULL) {
        mas_io_store_status(out, MAS_IO_ERROR_CLOSED);
        return;
    }

    if (entry->is_std) {
        long long result;

        if (entry->std_kind == MAS_IO_STDIN) {
            mas_io_store_status(out, MAS_IO_ERROR_NOT_SUPPORTED);
            return;
        }
        result = mas_runtime_std_flush((unsigned long long)entry->std_kind);
        code = result < 0 ? mas_io_normalize_code(-(int64_t)result) : MAS_IO_OK;
        mas_io_store_status(out, code);
        return;
    }

    if (!mas_io_entry_writable(entry)) {
        mas_io_store_status(out, MAS_IO_ERROR_NOT_SUPPORTED);
        return;
    }
    mas_io_store_status(out, mas_io_platform_flush(entry->file));
}
