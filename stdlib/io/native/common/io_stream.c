/* 流式数据路径：分块读取、读行、写入与定位。
 * 句柄表、读缓冲和宿主读写原语在 `io_state.c`；整文件入口在 `io_whole.c`。 */

#include "io_internal.h"

#include <stdlib.h>
#include <string.h>

/**
 * 分块读取：一次调用最多交付一块数据。缓冲区里已有预读数据就先交付它，
 * 否则向宿主补充一块；拿到数据后立即返回，绝不为凑满 `count` 再读宿主，
 * 因此管道上「上游只写了一部分并保持打开」时不会阻塞。`count` 为 0 时不触达宿主，
 * 返回空值且 `eof` 为 0。
 */
void mas_io_read(MasIoReadArg* in, MasIoReadResult* out) {
    MasIoEntry* entry;
    unsigned char* data = NULL;
    size_t want;
    size_t total = 0;
    size_t capacity = 0;
    int64_t code = MAS_IO_OK;

    if (out == NULL) {
        return;
    }
    if (in == NULL) {
        mas_io_store_read_result(out, MAS_IO_ERROR_INVALID_ARGUMENT, NULL, 0, 0);
        return;
    }

    entry = mas_io_find_entry(in->token);
    if (entry == NULL) {
        mas_io_store_read_result(out, MAS_IO_ERROR_CLOSED, NULL, 0, 0);
        return;
    }
    if (!mas_io_entry_readable(entry)) {
        mas_io_store_read_result(out, MAS_IO_ERROR_WRONG_MODE, NULL, 0, 0);
        return;
    }
    if (in->count < 0) {
        mas_io_store_read_result(out, MAS_IO_ERROR_INVALID_ARGUMENT, NULL, 0, 0);
        return;
    }
    if (in->count == 0) {
        mas_io_store_read_result(out, MAS_IO_OK, NULL, 0, 0);
        return;
    }
    if ((unsigned long long)in->count > (unsigned long long)((size_t)-1)) {
        mas_io_store_read_result(out, MAS_IO_ERROR_TOO_LARGE, NULL, 0, 0);
        return;
    }
    if (entry->pending_error != 0) {
        code = entry->pending_error;
        entry->pending_error = 0;
        mas_io_store_read_result(out, code, NULL, 0, 0);
        return;
    }

    want = (size_t)in->count;

    /* 每调用一次最多交付一块：有预读就先交付，没有就补充一块。
     * 拿到数据后立即返回，避免为凑满 count 再次阻塞在管道上。 */
    {
        size_t available = mas_io_entry_available(entry);
        size_t chunk;

        if (available == 0) {
            if (!entry->eof_reached) {
                code = mas_io_entry_fill(entry);
            }
            if (code == MAS_IO_OK) {
                available = mas_io_entry_available(entry);
            }
        }

        if (code == MAS_IO_OK && available > 0) {
            chunk = available < want ? available : want;
            if (!mas_io_buffer_append(
                    &data,
                    &total,
                    &capacity,
                    entry->read_buffer + entry->buffer_offset,
                    chunk
                )) {
                code = MAS_IO_ERROR_OTHER;
            } else {
                entry->buffer_offset += chunk;
                if (!entry->is_std) {
                    entry->position += (int64_t)chunk;
                }
            }
        }
    }

    if (code != MAS_IO_OK) {
        if (total == 0) {
            free(data);
            mas_io_store_read_result(out, code, NULL, 0, 0);
            return;
        }
        /* 宿主部分成功后继续交付已读字节，错误留到下一次正数读取或 read_line。 */
        entry->pending_error = code;
    }

    mas_io_store_read_result(out, MAS_IO_OK, data, total, total == 0 && entry->eof_reached);
    free(data);
}

/**
 * 读一行：读至 LF 或文件末尾，剥除 LF 和紧邻其前的 CR，保留其他字节（含内嵌 NUL）。
 * 与 `read` 共用同一缓冲和逻辑位置；读到完整行之前的错误会丢弃本次消费的残行。
 */
void mas_io_read_line(MasIoTokenArg* in, MasIoLineResult* out) {
    MasIoEntry* entry;
    unsigned char* line = NULL;
    size_t line_length = 0;
    size_t line_capacity = 0;
    int terminated = 0;
    int64_t code = MAS_IO_OK;

    if (out == NULL) {
        return;
    }
    if (in == NULL) {
        mas_io_store_line_result(out, MAS_IO_ERROR_INVALID_ARGUMENT, NULL, 0, 0);
        return;
    }

    entry = mas_io_find_entry(in->token);
    if (entry == NULL) {
        mas_io_store_line_result(out, MAS_IO_ERROR_CLOSED, NULL, 0, 0);
        return;
    }
    if (!mas_io_entry_readable(entry)) {
        mas_io_store_line_result(out, MAS_IO_ERROR_WRONG_MODE, NULL, 0, 0);
        return;
    }
    if (entry->pending_error != 0) {
        code = entry->pending_error;
        entry->pending_error = 0;
        mas_io_store_line_result(out, code, NULL, 0, 0);
        return;
    }

    for (;;) {
        size_t available = mas_io_entry_available(entry);
        const unsigned char* start;
        const unsigned char* newline;
        size_t chunk;

        if (available == 0) {
            if (entry->eof_reached) {
                break;
            }
            code = mas_io_entry_fill(entry);
            if (code != MAS_IO_OK) {
                break;
            }
            continue;
        }

        start = entry->read_buffer + entry->buffer_offset;
        newline = (const unsigned char*)memchr(start, '\n', available);
        chunk = newline != NULL ? (size_t)(newline - start) : available;
        if (!mas_io_buffer_append(&line, &line_length, &line_capacity, start, chunk)) {
            code = MAS_IO_ERROR_OTHER;
            break;
        }
        entry->buffer_offset += chunk;
        if (!entry->is_std) {
            entry->position += (int64_t)chunk;
        }
        if (newline != NULL) {
            entry->buffer_offset += 1;
            if (!entry->is_std) {
                entry->position += 1;
            }
            terminated = 1;
            break;
        }
    }

    if (code != MAS_IO_OK) {
        free(line);
        mas_io_store_line_result(out, code, NULL, 0, 0);
        return;
    }

    if (!terminated && line_length == 0 && entry->eof_reached) {
        free(line);
        mas_io_store_line_result(out, MAS_IO_OK, NULL, 0, 1);
        return;
    }

    if (terminated && line_length > 0 && line[line_length - 1] == '\r') {
        line_length -= 1;
    }
    if (!mas_io_utf8_is_valid(line, line_length)) {
        free(line);
        mas_io_store_line_result(out, MAS_IO_ERROR_INVALID_UTF8, NULL, 0, 0);
        return;
    }

    mas_io_store_line_result(out, MAS_IO_OK, line, line_length, 0);
    free(line);
}

/** 循环写入完整数据；失败时回报宿主已接受的字节数。空数据直接成功。 */
void mas_io_write(MasIoWriteArg* in, MasIoWriteResult* out) {
    MasIoEntry* entry;
    size_t total = 0;
    size_t length;
    int64_t code = MAS_IO_OK;

    if (out == NULL) {
        return;
    }
    if (in == NULL) {
        mas_io_store_write_result(out, MAS_IO_ERROR_INVALID_ARGUMENT, 0);
        return;
    }

    entry = mas_io_find_entry(in->token);
    if (entry == NULL) {
        mas_io_store_write_result(out, MAS_IO_ERROR_CLOSED, 0);
        return;
    }
    if (!mas_io_entry_writable(entry)) {
        mas_io_store_write_result(out, MAS_IO_ERROR_WRONG_MODE, 0);
        return;
    }
    if (in->data.ptr == NULL && in->data.len > 0) {
        mas_io_store_write_result(out, MAS_IO_ERROR_INVALID_ARGUMENT, 0);
        return;
    }
    if (in->data.len > (unsigned long long)((size_t)-1)
        || in->data.len > (unsigned long long)INT64_MAX) {
        mas_io_store_write_result(out, MAS_IO_ERROR_TOO_LARGE, 0);
        return;
    }
    length = (size_t)in->data.len;
    if (length == 0) {
        mas_io_store_write_result(out, MAS_IO_OK, 0);
        return;
    }

    code = mas_io_entry_sync_before_write(entry);
    if (code != MAS_IO_OK) {
        mas_io_store_write_result(out, code, 0);
        return;
    }

    while (total < length) {
        size_t wrote = 0;

        code = mas_io_host_write(entry, in->data.ptr + total, length - total, &wrote);
        if (code != MAS_IO_OK) {
            /* 宿主可能已经接受了一部分（标准流断管、部分写入）：先计入 written 再返回错误。 */
            total += wrote;
            if (!entry->is_std) {
                entry->position += (int64_t)wrote;
            }
            break;
        }
        if (wrote == 0) {
            /* 宿主没有任何进展时不能继续循环，按可恢复错误回报。 */
            code = MAS_IO_ERROR_OTHER;
            break;
        }
        total += wrote;
        if (!entry->is_std) {
            entry->position += (int64_t)wrote;
        }
    }

    if (code != MAS_IO_OK) {
        mas_io_store_write_result(out, code, (int64_t)total);
        return;
    }
    if (!entry->is_std) {
        entry->eof_reached = 0;
    }
    mas_io_store_write_result(out, MAS_IO_OK, (int64_t)total);
}

/**
 * 定位：成功丢弃旧读缓冲、清除 EOF 与待报错误，返回从开头计数的绝对位置。
 * End 起点用文件大小换算，不先移动宿主游标；目标非法或定位失败时读缓冲保持不变，
 * 因此宿主游标与库内缓冲不会失步，后续读取不会静默漏数据。
 * 存在未报告错误时先返回该错误并清除它，下一次调用可以重试定位。
 */
void mas_io_seek(MasIoSeekArg* in, MasIoSeekResult* out) {
    MasIoEntry* entry;
    int64_t target;
    int64_t position = 0;
    int64_t code;

    if (out == NULL) {
        return;
    }
    if (in == NULL) {
        mas_io_store_seek_result(out, MAS_IO_ERROR_INVALID_ARGUMENT, 0);
        return;
    }

    entry = mas_io_find_entry(in->token);
    if (entry == NULL) {
        mas_io_store_seek_result(out, MAS_IO_ERROR_CLOSED, 0);
        return;
    }
    if (entry->is_std) {
        mas_io_store_seek_result(out, MAS_IO_ERROR_NOT_SUPPORTED, 0);
        return;
    }
    if (in->origin < MAS_IO_SEEK_START || in->origin > MAS_IO_SEEK_END) {
        mas_io_store_seek_result(out, MAS_IO_ERROR_INVALID_ARGUMENT, 0);
        return;
    }
    if (entry->pending_error != 0) {
        code = entry->pending_error;
        entry->pending_error = 0;
        mas_io_store_seek_result(out, code, 0);
        return;
    }

    if (in->origin == MAS_IO_SEEK_START) {
        target = in->offset;
    } else if (in->origin == MAS_IO_SEEK_CURRENT) {
        if (!mas_io_add_i64(entry->position, in->offset, &target)) {
            mas_io_store_seek_result(out, MAS_IO_ERROR_TOO_LARGE, 0);
            return;
        }
    } else {
        int64_t end_position = 0;

        /* 只查询大小，不把宿主游标移到末尾：否则目标非法时缓冲与游标会失步。 */
        code = mas_io_platform_size(entry->file, &end_position);
        if (code != MAS_IO_OK) {
            mas_io_store_seek_result(out, code, 0);
            return;
        }
        if (!mas_io_add_i64(end_position, in->offset, &target)) {
            mas_io_store_seek_result(out, MAS_IO_ERROR_TOO_LARGE, 0);
            return;
        }
    }

    if (target < 0) {
        mas_io_store_seek_result(out, MAS_IO_ERROR_INVALID_ARGUMENT, 0);
        return;
    }

    code = mas_io_platform_seek(entry->file, target, MAS_IO_SEEK_START, &position);
    if (code != MAS_IO_OK) {
        mas_io_store_seek_result(out, code, 0);
        return;
    }

    mas_io_entry_drop_buffer(entry);
    entry->eof_reached = 0;
    entry->pending_error = 0;
    entry->position = position;
    mas_io_store_seek_result(out, MAS_IO_OK, position);
}
