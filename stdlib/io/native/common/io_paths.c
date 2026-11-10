#include "io_internal.h"

#include <stdlib.h>
#include <string.h>

/**
 * 表示一个目录迭代器表项。公开 `DirResult` 只交付名字列表，
 * 但内部仍按 open/next/close 管理，保证任何失败路径都能关闭宿主迭代器。
 */
typedef struct MasIoDirIterator {
    int64_t token;
    void* dir;
} MasIoDirIterator;

/** 目录迭代器使用独立的令牌空间，0 始终无效。 */
static MasIoDirIterator** g_dirs = NULL;
static size_t g_dir_count = 0;
static size_t g_dir_capacity = 0;
static int64_t g_next_dir_token = 1;

/** 按令牌查找目录迭代器；未知令牌返回 NULL。 */
static MasIoDirIterator* mas_io_find_dir(int64_t token) {
    size_t index;

    if (token <= 0) {
        return NULL;
    }
    for (index = 0; index < g_dir_count; index += 1) {
        if (g_dirs[index]->token == token) {
            return g_dirs[index];
        }
    }
    return NULL;
}

/** 登记目录迭代器并分配单调令牌，令牌永不复用。 */
static int64_t mas_io_register_dir(MasIoDirIterator* iterator) {
    MasIoDirIterator** grown;
    size_t capacity;

    if (g_dir_count == g_dir_capacity) {
        capacity = g_dir_capacity == 0 ? 4 : g_dir_capacity * 2;
        if (capacity <= g_dir_capacity) {
            return MAS_IO_ERROR_OTHER;
        }
        grown = (MasIoDirIterator**)realloc(g_dirs, capacity * sizeof(*grown));
        if (grown == NULL) {
            return MAS_IO_ERROR_OTHER;
        }
        g_dirs = grown;
        g_dir_capacity = capacity;
    }
    if (g_next_dir_token <= 0 || g_next_dir_token == INT64_MAX) {
        return MAS_IO_ERROR_TOO_LARGE;
    }

    iterator->token = g_next_dir_token;
    g_next_dir_token += 1;
    g_dirs[g_dir_count] = iterator;
    g_dir_count += 1;
    return MAS_IO_OK;
}

/** 从目录表移除迭代器，令牌立即失效。 */
static void mas_io_unregister_dir(MasIoDirIterator* iterator) {
    size_t index;

    for (index = 0; index < g_dir_count; index += 1) {
        if (g_dirs[index] == iterator) {
            memmove(
                &g_dirs[index],
                &g_dirs[index + 1],
                (g_dir_count - index - 1) * sizeof(*g_dirs)
            );
            g_dir_count -= 1;
            return;
        }
    }
}

/** 校验整文件输入的路径与数据长度，避免在别处重复同一组检查。 */
static int64_t mas_io_check_path_data(
    const MasIoPathDataArg* in,
    size_t* path_len,
    size_t* data_len
) {
    int64_t code;

    if (in == NULL || path_len == NULL || data_len == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    code = mas_io_validate_path(in->path.ptr, in->path.len, path_len);
    if (code != MAS_IO_OK) {
        return code;
    }
    if (in->data.ptr == NULL && in->data.len > 0) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    if (in->data.len > (unsigned long long)((size_t)-1)
        || in->data.len > (unsigned long long)INT64_MAX) {
        return MAS_IO_ERROR_TOO_LARGE;
    }
    *data_len = (size_t)in->data.len;
    return MAS_IO_OK;
}

/** 读取完整文件为字节；失败时交付有效空字节，句柄一定被关闭。 */
void mas_io_read_file(MasIoPathArg* in, MasIoBytesResult* out) {
    size_t path_len = 0;
    unsigned char* data = NULL;
    size_t size = 0;
    int64_t close_error = 0;
    int64_t code;

    if (out == NULL) {
        return;
    }
    if (in == NULL) {
        mas_io_store_bytes_result(out, MAS_IO_ERROR_INVALID_ARGUMENT, NULL, 0);
        return;
    }

    code = mas_io_validate_path(in->path.ptr, in->path.len, &path_len);
    if (code != MAS_IO_OK) {
        mas_io_store_bytes_result(out, code, NULL, 0);
        return;
    }

    code = mas_io_read_whole_file(in->path.ptr, path_len, &data, &size, &close_error);
    if (code != MAS_IO_OK) {
        mas_io_store_bytes_result(out, code, NULL, 0);
        return;
    }

    mas_io_store_bytes_result(out, MAS_IO_OK, data, size);
    free(data);
}

/** 读取完整文件为文本，先严格校验 UTF-8 再交付；空文件成功返回空串。 */
void mas_io_read_text(MasIoPathArg* in, MasIoTextResult* out) {
    size_t path_len = 0;
    unsigned char* data = NULL;
    size_t size = 0;
    int64_t close_error = 0;
    int64_t code;

    if (out == NULL) {
        return;
    }
    if (in == NULL) {
        mas_io_store_text_result(out, MAS_IO_ERROR_INVALID_ARGUMENT, NULL, 0);
        return;
    }

    code = mas_io_validate_path(in->path.ptr, in->path.len, &path_len);
    if (code != MAS_IO_OK) {
        mas_io_store_text_result(out, code, NULL, 0);
        return;
    }

    code = mas_io_read_whole_file(in->path.ptr, path_len, &data, &size, &close_error);
    if (code != MAS_IO_OK) {
        mas_io_store_text_result(out, code, NULL, 0);
        return;
    }
    if (!mas_io_utf8_is_valid(data, size)) {
        free(data);
        mas_io_store_text_result(out, MAS_IO_ERROR_INVALID_UTF8, NULL, 0);
        return;
    }

    mas_io_store_text_result(out, MAS_IO_OK, data, size);
    free(data);
}

/** 创建或覆盖文件后写入整段数据；失败时保留已接受字节数并清理句柄。 */
void mas_io_write_file(MasIoPathDataArg* in, MasIoWriteResult* out) {
    size_t path_len = 0;
    size_t data_len = 0;
    int64_t written = 0;
    int64_t close_error = 0;
    int64_t code;

    if (out == NULL) {
        return;
    }
    code = mas_io_check_path_data(in, &path_len, &data_len);
    if (code != MAS_IO_OK) {
        mas_io_store_write_result(out, code, 0);
        return;
    }

    code = mas_io_write_whole_file(
        in->path.ptr,
        path_len,
        MAS_IO_MODE_WRITE,
        in->data.ptr,
        data_len,
        &written,
        &close_error
    );
    mas_io_store_write_result(out, code, written);
}

/** 创建或追加文件后写入整段数据；每次写入都落在文件末尾。 */
void mas_io_append_file(MasIoPathDataArg* in, MasIoWriteResult* out) {
    size_t path_len = 0;
    size_t data_len = 0;
    int64_t written = 0;
    int64_t close_error = 0;
    int64_t code;

    if (out == NULL) {
        return;
    }
    code = mas_io_check_path_data(in, &path_len, &data_len);
    if (code != MAS_IO_OK) {
        mas_io_store_write_result(out, code, 0);
        return;
    }

    code = mas_io_write_whole_file(
        in->path.ptr,
        path_len,
        MAS_IO_MODE_APPEND,
        in->data.ptr,
        data_len,
        &written,
        &close_error
    );
    mas_io_store_write_result(out, code, written);
}

/**
 * 查询元数据：跟随符号链接，仅普通文件返回大小。
 * 大小盒子按设计使用 `mas_record_alloc(sizeof(int64_t), NULL, 0)` 分配。
 */
void mas_io_metadata(MasIoPathArg* in, MasIoMetadataResult* out) {
    size_t path_len = 0;
    int64_t kind = MAS_IO_KIND_OTHER;
    int64_t size = 0;
    int has_size = 0;
    int64_t* box = NULL;
    int64_t code;

    if (out == NULL) {
        return;
    }
    if (in == NULL) {
        mas_io_store_metadata_result(out, MAS_IO_ERROR_INVALID_ARGUMENT, MAS_IO_KIND_OTHER, NULL);
        return;
    }

    code = mas_io_validate_path(in->path.ptr, in->path.len, &path_len);
    if (code != MAS_IO_OK) {
        mas_io_store_metadata_result(out, code, MAS_IO_KIND_OTHER, NULL);
        return;
    }

    code = mas_io_platform_metadata(in->path.ptr, path_len, &kind, &size, &has_size);
    if (code != MAS_IO_OK) {
        mas_io_store_metadata_result(out, code, MAS_IO_KIND_OTHER, NULL);
        return;
    }
    if (has_size != 0) {
        if (size < 0) {
            mas_io_store_metadata_result(out, MAS_IO_ERROR_TOO_LARGE, MAS_IO_KIND_OTHER, NULL);
            return;
        }
        box = mas_io_make_i64(size);
        if (box == NULL) {
            mas_io_store_metadata_result(out, MAS_IO_ERROR_OTHER, MAS_IO_KIND_OTHER, NULL);
            return;
        }
    }

    mas_io_store_metadata_result(out, MAS_IO_OK, kind, box);
}

/** 打开目录迭代器；失败时句柄为 0，不遗留宿主资源。 */
void mas_io_dir_open(MasIoPathArg* in, MasIoDirOpenResult* out) {
    size_t path_len = 0;
    MasIoDirIterator* iterator;
    void* dir = NULL;
    int64_t code;

    if (out == NULL) {
        return;
    }
    if (in == NULL) {
        mas_io_store_dir_open_result(out, MAS_IO_ERROR_INVALID_ARGUMENT, 0);
        return;
    }

    code = mas_io_validate_path(in->path.ptr, in->path.len, &path_len);
    if (code != MAS_IO_OK) {
        mas_io_store_dir_open_result(out, code, 0);
        return;
    }

    code = mas_io_platform_dir_open(in->path.ptr, path_len, &dir);
    if (code != MAS_IO_OK) {
        mas_io_store_dir_open_result(out, code, 0);
        return;
    }

    iterator = (MasIoDirIterator*)calloc(1, sizeof(*iterator));
    if (iterator == NULL) {
        (void)mas_io_platform_dir_close(dir);
        mas_io_store_dir_open_result(out, MAS_IO_ERROR_OTHER, 0);
        return;
    }
    iterator->dir = dir;
    code = mas_io_register_dir(iterator);
    if (code != MAS_IO_OK) {
        (void)mas_io_platform_dir_close(dir);
        free(iterator);
        mas_io_store_dir_open_result(out, code, 0);
        return;
    }

    mas_io_store_dir_open_result(out, MAS_IO_OK, iterator->token);
}

/** 取下一个直接子项名称；正常结束时 `done` 为 1 且名字为空串。 */
void mas_io_dir_next(MasIoHandleArg* in, MasIoDirEntryResult* out) {
    MasIoDirIterator* iterator;
    unsigned char* name = NULL;
    size_t name_len = 0;
    int done = 0;
    int64_t code;

    if (out == NULL) {
        return;
    }
    if (in == NULL) {
        mas_io_store_dir_entry_result(out, MAS_IO_ERROR_INVALID_ARGUMENT, 0, NULL, 0);
        return;
    }

    iterator = mas_io_find_dir(in->handle);
    if (iterator == NULL) {
        mas_io_store_dir_entry_result(out, MAS_IO_ERROR_CLOSED, 0, NULL, 0);
        return;
    }

    code = mas_io_platform_dir_next(iterator->dir, &done, &name, &name_len);
    if (code != MAS_IO_OK) {
        free(name);
        mas_io_store_dir_entry_result(out, code, 0, NULL, 0);
        return;
    }
    if (done != 0) {
        free(name);
        mas_io_store_dir_entry_result(out, MAS_IO_OK, 1, NULL, 0);
        return;
    }

    mas_io_store_dir_entry_result(out, MAS_IO_OK, 0, name, name_len);
    free(name);
}

/** 关闭目录迭代器；即使关闭失败，令牌也已失效。 */
void mas_io_dir_close(MasIoHandleArg* in, MasIoStatus* out) {
    MasIoDirIterator* iterator;
    int64_t code;

    if (out == NULL) {
        return;
    }
    if (in == NULL) {
        mas_io_store_status(out, MAS_IO_ERROR_INVALID_ARGUMENT);
        return;
    }

    iterator = mas_io_find_dir(in->handle);
    if (iterator == NULL) {
        mas_io_store_status(out, MAS_IO_ERROR_CLOSED);
        return;
    }

    mas_io_unregister_dir(iterator);
    code = mas_io_platform_dir_close(iterator->dir);
    free(iterator);
    mas_io_store_status(out, code);
}

/** 创建一级目录：父目录缺失报 `NotFound`，已存在报 `AlreadyExists`。 */
void mas_io_make_dir(MasIoPathArg* in, MasIoStatus* out) {
    size_t path_len = 0;
    int64_t code;

    if (out == NULL) {
        return;
    }
    if (in == NULL) {
        mas_io_store_status(out, MAS_IO_ERROR_INVALID_ARGUMENT);
        return;
    }

    code = mas_io_validate_path(in->path.ptr, in->path.len, &path_len);
    if (code != MAS_IO_OK) {
        mas_io_store_status(out, code);
        return;
    }

    mas_io_store_status(out, mas_io_platform_make_dir(in->path.ptr, path_len));
}

/** 同一文件系统内移动且禁止覆盖；目标已存在报 `AlreadyExists`。 */
void mas_io_rename(MasIoRenameArg* in, MasIoStatus* out) {
    size_t source_len = 0;
    size_t destination_len = 0;
    int64_t code;

    if (out == NULL) {
        return;
    }
    if (in == NULL) {
        mas_io_store_status(out, MAS_IO_ERROR_INVALID_ARGUMENT);
        return;
    }

    code = mas_io_validate_path(in->source.ptr, in->source.len, &source_len);
    if (code != MAS_IO_OK) {
        mas_io_store_status(out, code);
        return;
    }
    code = mas_io_validate_path(in->destination.ptr, in->destination.len, &destination_len);
    if (code != MAS_IO_OK) {
        mas_io_store_status(out, code);
        return;
    }

    mas_io_store_status(
        out,
        mas_io_platform_rename(in->source.ptr, source_len, in->destination.ptr, destination_len)
    );
}

/** 删除文件或指向文件的符号链接；目录不能用该入口删除。 */
void mas_io_remove_file(MasIoPathArg* in, MasIoStatus* out) {
    size_t path_len = 0;
    int64_t code;

    if (out == NULL) {
        return;
    }
    if (in == NULL) {
        mas_io_store_status(out, MAS_IO_ERROR_INVALID_ARGUMENT);
        return;
    }

    code = mas_io_validate_path(in->path.ptr, in->path.len, &path_len);
    if (code != MAS_IO_OK) {
        mas_io_store_status(out, code);
        return;
    }

    mas_io_store_status(out, mas_io_platform_remove_file(in->path.ptr, path_len));
}

/** 删除空目录；非空报 `DirectoryNotEmpty`，符号链接与目录重解析点被拒绝。 */
void mas_io_remove_dir(MasIoPathArg* in, MasIoStatus* out) {
    size_t path_len = 0;
    int64_t code;

    if (out == NULL) {
        return;
    }
    if (in == NULL) {
        mas_io_store_status(out, MAS_IO_ERROR_INVALID_ARGUMENT);
        return;
    }

    code = mas_io_validate_path(in->path.ptr, in->path.len, &path_len);
    if (code != MAS_IO_OK) {
        mas_io_store_status(out, code);
        return;
    }

    mas_io_store_status(out, mas_io_platform_remove_dir(in->path.ptr, path_len));
}
