/* 测试用的公开 API 薄包装：统一初始化输入盒和结果结构，让用例只关心行为。
 * 运行时桩、路径辅助与临时目录在 `io_host_test.c`。 */

#include "io_test_support.h"

#include <string.h>

/* ------------------------------------------------------------------------- */
/* API 薄包装                                                                 */
/* ------------------------------------------------------------------------- */

MasIoOpenResult io_test_open(const char* path, int64_t mode) {
    MasIoOpenArg arg;
    MasIoOpenResult result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.path = io_test_string(path);
    arg.mode = mode;
    mas_io_open(&arg, &result);
    return result;
}

MasIoOpenResult io_test_open_std(int64_t which) {
    MasIoStdArg arg;
    MasIoOpenResult result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.which = which;
    mas_io_std(&arg, &result);
    return result;
}

MasIoReadResult io_test_read(MasIoFile* file, int64_t count) {
    MasIoReadArg arg;
    MasIoReadResult result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.token = file == NULL ? -1 : file->token;
    arg.count = count;
    mas_io_read(&arg, &result);
    return result;
}

MasIoLineResult io_test_read_line(MasIoFile* file) {
    MasIoTokenArg arg;
    MasIoLineResult result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.token = file == NULL ? -1 : file->token;
    mas_io_read_line(&arg, &result);
    return result;
}

MasIoWriteResult io_test_write(MasIoFile* file, const void* data, size_t length) {
    MasIoWriteArg arg;
    MasIoWriteResult result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.token = file == NULL ? -1 : file->token;
    arg.data.ptr = (const unsigned char*)data;
    arg.data.len = (unsigned long long)length;
    mas_io_write(&arg, &result);
    return result;
}

MasIoStatus io_test_flush(MasIoFile* file) {
    MasIoTokenArg arg;
    MasIoStatus result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.token = file == NULL ? -1 : file->token;
    mas_io_flush(&arg, &result);
    return result;
}

MasIoStatus io_test_close(MasIoFile* file) {
    MasIoTokenArg arg;
    MasIoStatus result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.token = file == NULL ? -1 : file->token;
    mas_io_close(&arg, &result);
    return result;
}

MasIoSeekResult io_test_seek(MasIoFile* file, int64_t offset, int64_t origin) {
    MasIoSeekArg arg;
    MasIoSeekResult result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.token = file == NULL ? -1 : file->token;
    arg.offset = offset;
    arg.origin = origin;
    mas_io_seek(&arg, &result);
    return result;
}

MasIoBytesResult io_test_read_file(const char* path) {
    MasIoPathArg arg;
    MasIoBytesResult result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.path = io_test_string(path);
    mas_io_read_file(&arg, &result);
    return result;
}

MasIoTextResult io_test_read_text(const char* path) {
    MasIoPathArg arg;
    MasIoTextResult result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.path = io_test_string(path);
    mas_io_read_text(&arg, &result);
    return result;
}

MasIoWriteResult io_test_write_file(const char* path, const void* data, size_t length) {
    MasIoPathDataArg arg;
    MasIoWriteResult result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.path = io_test_string(path);
    arg.data.ptr = (const unsigned char*)data;
    arg.data.len = (unsigned long long)length;
    mas_io_write_file(&arg, &result);
    return result;
}

MasIoWriteResult io_test_append_file(const char* path, const void* data, size_t length) {
    MasIoPathDataArg arg;
    MasIoWriteResult result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.path = io_test_string(path);
    arg.data.ptr = (const unsigned char*)data;
    arg.data.len = (unsigned long long)length;
    mas_io_append_file(&arg, &result);
    return result;
}

MasIoMetadataResult io_test_metadata(const char* path) {
    MasIoPathArg arg;
    MasIoMetadataResult result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.path = io_test_string(path);
    mas_io_metadata(&arg, &result);
    return result;
}

MasIoDirOpenResult io_test_dir_open(const char* path) {
    MasIoPathArg arg;
    MasIoDirOpenResult result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.path = io_test_string(path);
    mas_io_dir_open(&arg, &result);
    return result;
}

MasIoDirEntryResult io_test_dir_next(int64_t handle) {
    MasIoHandleArg arg;
    MasIoDirEntryResult result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.handle = handle;
    mas_io_dir_next(&arg, &result);
    return result;
}

MasIoStatus io_test_dir_close(int64_t handle) {
    MasIoHandleArg arg;
    MasIoStatus result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.handle = handle;
    mas_io_dir_close(&arg, &result);
    return result;
}

MasIoStatus io_test_make_dir(const char* path) {
    MasIoPathArg arg;
    MasIoStatus result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.path = io_test_string(path);
    mas_io_make_dir(&arg, &result);
    return result;
}

MasIoStatus io_test_rename(const char* source, const char* destination) {
    MasIoRenameArg arg;
    MasIoStatus result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.source = io_test_string(source);
    arg.destination = io_test_string(destination);
    mas_io_rename(&arg, &result);
    return result;
}

MasIoStatus io_test_remove_file(const char* path) {
    MasIoPathArg arg;
    MasIoStatus result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.path = io_test_string(path);
    mas_io_remove_file(&arg, &result);
    return result;
}

MasIoStatus io_test_remove_dir(const char* path) {
    MasIoPathArg arg;
    MasIoStatus result;

    memset(&arg, 0, sizeof(arg));
    memset(&result, 0, sizeof(result));
    arg.path = io_test_string(path);
    mas_io_remove_dir(&arg, &result);
    return result;
}
