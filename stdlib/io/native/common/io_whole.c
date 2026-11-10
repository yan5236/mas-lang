/* 整文件入口的内部实现：一次打开、循环读写、必定关闭。
 * 公开的 read_file/read_text/write_file/append_file 在 `io_paths.c`，
 * 这里只负责「首次错误优先」和「关闭错误单独回报」的资源顺序。 */

#include "io_internal.h"

#include <stdlib.h>

/**
 * 整段读取的内部实现：打开、循环读取、关闭。
 * 读取失败优先于关闭失败；操作成功但关闭失败时返回关闭错误且不交付数据。
 */
int64_t mas_io_read_whole_file(
    const unsigned char* path,
    size_t path_len,
    unsigned char** data,
    size_t* size,
    int64_t* close_error
) {
    unsigned char chunk[MAS_IO_READ_BUFFER_SIZE];
    unsigned char* buffer = NULL;
    size_t length = 0;
    size_t capacity = 0;
    void* file = NULL;
    int64_t close_code;
    int64_t code;

    if (data == NULL || size == NULL || close_error == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    *data = NULL;
    *size = 0;
    *close_error = 0;

    code = mas_io_platform_open(path, path_len, MAS_IO_MODE_READ, &file);
    if (code != MAS_IO_OK) {
        return code;
    }

    for (;;) {
        size_t got = 0;

        code = mas_io_platform_read(file, chunk, sizeof(chunk), &got);
        if (code != MAS_IO_OK) {
            break;
        }
        if (got == 0) {
            break;
        }
        if (!mas_io_buffer_append(&buffer, &length, &capacity, chunk, got)) {
            code = MAS_IO_ERROR_OTHER;
            break;
        }
    }

    close_code = mas_io_platform_close(file);
    *close_error = close_code;
    if (code != MAS_IO_OK) {
        free(buffer);
        return code;
    }
    if (close_code != MAS_IO_OK) {
        free(buffer);
        return close_code;
    }

    *data = buffer;
    *size = length;
    return MAS_IO_OK;
}

/**
 * 整段写入的内部实现：打开、循环写入、关闭。
 * 写入失败优先于关闭失败；关闭失败时仍回报宿主已接受的字节数。
 */
int64_t mas_io_write_whole_file(
    const unsigned char* path,
    size_t path_len,
    int64_t mode,
    const unsigned char* data,
    size_t len,
    int64_t* written,
    int64_t* close_error
) {
    size_t total = 0;
    void* file = NULL;
    int64_t close_code;
    int64_t code;

    if (written == NULL || close_error == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    if (mode != MAS_IO_MODE_WRITE && mode != MAS_IO_MODE_APPEND) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    if (data == NULL && len > 0) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    *written = 0;
    *close_error = 0;

    code = mas_io_platform_open(path, path_len, mode, &file);
    if (code != MAS_IO_OK) {
        return code;
    }

    if (mode == MAS_IO_MODE_APPEND) {
        /* 追加语义要求每次都从文件末尾开始写；打开后先定位一次。 */
        int64_t end_position = 0;

        code = mas_io_platform_seek(file, 0, MAS_IO_SEEK_END, &end_position);
        if (code != MAS_IO_OK) {
            (void)mas_io_platform_close(file);
            return code;
        }
    }

    while (total < len) {
        size_t wrote = 0;

        code = mas_io_platform_write(file, data + total, len - total, &wrote);
        if (code != MAS_IO_OK) {
            break;
        }
        if (wrote == 0) {
            code = MAS_IO_ERROR_OTHER;
            break;
        }
        total += wrote;
    }

    close_code = mas_io_platform_close(file);
    *written = (int64_t)total;
    *close_error = close_code;
    if (code != MAS_IO_OK) {
        return code;
    }
    return close_code;
}
