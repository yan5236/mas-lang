/* POSIX 适配层基础部分：错误映射、路径编码和常规文件读写。
 * 文件系统入口（元数据/目录/改名/删除）在 `io_posix_fs.c`。
 *
 * 本文件与 `io_posix_fs.c` 只出现在 POSIX 目标的 `native.sources.posix` 中，
 * 刻意不写 `#ifndef _WIN32` 条件编译：在任意主机上都能用 `-fsyntax-only` 做静态检查。 */
#define _GNU_SOURCE 1
#define _FILE_OFFSET_BITS 64

#include "io_internal.h"

#include <errno.h>
#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

/** 文件描述符以 `fd + 1` 编码成指针，避免把合法的 0 号描述符当成空句柄。 */
static void* mas_io_posix_encode(int descriptor) {
    return (void*)(intptr_t)(descriptor + 1);
}

/** 还原 `mas_io_posix_encode` 的结果。 */
static int mas_io_posix_decode(void* file) {
    return (int)(intptr_t)file - 1;
}

/**
 * 把宿主 errno 映射为公开错误类别。整个 POSIX 适配层只有这一处映射，
 * 调用方必须在任何后续系统调用之前先把 `errno` 取出来。
 */
int64_t mas_io_posix_map(int error) {
    switch (error) {
    case ENOENT:
    case ENOTDIR:
        return MAS_IO_ERROR_NOT_FOUND;
    case EACCES:
    case EPERM:
    case EROFS:
        return MAS_IO_ERROR_PERMISSION_DENIED;
    case EEXIST:
        return MAS_IO_ERROR_ALREADY_EXISTS;
    case EINVAL:
    case ENAMETOOLONG:
    case ELOOP:
    case EBADF:
    case EISDIR:
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    case ENOTEMPTY:
        return MAS_IO_ERROR_DIRECTORY_NOT_EMPTY;
    case EXDEV:
    case ENOSYS:
#ifdef EOPNOTSUPP
    case EOPNOTSUPP:
#endif
        return MAS_IO_ERROR_NOT_SUPPORTED;
    case EFBIG:
    case EOVERFLOW:
        return MAS_IO_ERROR_TOO_LARGE;
    default:
        return MAS_IO_ERROR_OTHER;
    }
}

/** 把已验证的 UTF-8 路径复制成以零结尾的字节串；路径本身不允许内嵌 NUL。 */
int64_t mas_io_posix_path(const unsigned char* path, size_t path_len, char** out) {
    char* buffer;

    if (out == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    *out = NULL;
    if (path == NULL || path_len == 0) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }

    buffer = (char*)malloc(path_len + 1);
    if (buffer == NULL) {
        return MAS_IO_ERROR_OTHER;
    }
    memcpy(buffer, path, path_len);
    buffer[path_len] = '\0';
    *out = buffer;
    return MAS_IO_OK;
}

/**
 * 打开常规文件。POSIX 允许 `open` 目录，这里用 `fstat` 显式拒绝，
 * 让两个平台对“把目录当文件打开”给出一致的 `InvalidArgument`。
 */
int64_t mas_io_platform_open(const unsigned char* path, size_t path_len, int64_t mode, void** out) {
    char* text = NULL;
    struct stat info;
    int flags;
    int descriptor;
    int error;
    int64_t code;

    if (out == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    *out = NULL;

    code = mas_io_posix_path(path, path_len, &text);
    if (code != MAS_IO_OK) {
        return code;
    }

    switch (mode) {
    case MAS_IO_MODE_READ:
        flags = O_RDONLY;
        break;
    case MAS_IO_MODE_WRITE:
        flags = O_WRONLY | O_CREAT | O_TRUNC;
        break;
    case MAS_IO_MODE_APPEND:
        /* O_APPEND 让每次 write 原子地落到文件末尾，多进程并发追加不会互相覆盖。 */
        flags = O_WRONLY | O_CREAT | O_APPEND;
        break;
    case MAS_IO_MODE_READ_WRITE:
        flags = O_RDWR;
        break;
    default:
        free(text);
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
#ifdef O_CLOEXEC
    flags |= O_CLOEXEC;
#endif

    descriptor = open(text, flags, (mode_t)0666);
    if (descriptor < 0) {
        error = errno;
        free(text);
        return mas_io_posix_map(error);
    }
    free(text);

    if (fstat(descriptor, &info) != 0) {
        error = errno;
        (void)close(descriptor);
        return mas_io_posix_map(error);
    }
    if (S_ISDIR(info.st_mode)) {
        (void)close(descriptor);
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }

    *out = mas_io_posix_encode(descriptor);
    return MAS_IO_OK;
}

/** 读取一块数据；`EINTR` 后重试同一请求，其余错误立即映射。 */
int64_t mas_io_platform_read(void* file, unsigned char* buffer, size_t capacity, size_t* read_out) {
    int descriptor;

    if (file == NULL || buffer == NULL || read_out == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    *read_out = 0;
    if (capacity == 0) {
        return MAS_IO_OK;
    }

    descriptor = mas_io_posix_decode(file);
    for (;;) {
        ssize_t got = read(descriptor, buffer, capacity);

        if (got >= 0) {
            *read_out = (size_t)got;
            return MAS_IO_OK;
        }
        if (errno != EINTR) {
            int error = errno;

            return mas_io_posix_map(error);
        }
    }
}

/** 写入一块数据；`*written_out` 是宿主本次接受的字节数。 */
int64_t mas_io_platform_write(void* file, const unsigned char* data, size_t len, size_t* written_out) {
    int descriptor;

    if (file == NULL || (data == NULL && len > 0) || written_out == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    *written_out = 0;
    if (len == 0) {
        return MAS_IO_OK;
    }

    descriptor = mas_io_posix_decode(file);
    for (;;) {
        ssize_t wrote = write(descriptor, data, len);

        if (wrote >= 0) {
            *written_out = (size_t)wrote;
            return MAS_IO_OK;
        }
        if (errno != EINTR) {
            int error = errno;

            return mas_io_posix_map(error);
        }
    }
}

/**
 * 刷新输出。适配层直接写宿主描述符，没有用户态写缓冲；
 * 设计的 `flush` 也不承诺断电持久化，因此只报告成功。
 */
int64_t mas_io_platform_flush(void* file) {
    if (file == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    return MAS_IO_OK;
}

/** 按起点、当前位置或末尾定位；越界偏移由 `lseek` 报 `EINVAL`，映射为 `InvalidArgument`。 */
int64_t mas_io_platform_seek(void* file, int64_t offset, int64_t origin, int64_t* position_out) {
    int descriptor;
    int method;
    off_t request;
    off_t result;

    if (file == NULL || position_out == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }

    switch (origin) {
    case MAS_IO_SEEK_START:
        method = SEEK_SET;
        break;
    case MAS_IO_SEEK_CURRENT:
        method = SEEK_CUR;
        break;
    case MAS_IO_SEEK_END:
        method = SEEK_END;
        break;
    default:
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }

    request = (off_t)offset;
    if ((int64_t)request != offset) {
        /* 目标平台的 off_t 装不下该偏移，按设计返回 TooLarge 而不是截断。 */
        return MAS_IO_ERROR_TOO_LARGE;
    }

    descriptor = mas_io_posix_decode(file);
    result = lseek(descriptor, request, method);
    if (result < 0) {
        int error = errno;

        return mas_io_posix_map(error);
    }

    *position_out = (int64_t)result;
    return MAS_IO_OK;
}

/** 查询文件当前字节大小；`fstat` 不改变文件偏移。 */
int64_t mas_io_platform_size(void* file, int64_t* size_out) {
    struct stat info;
    int descriptor;

    if (file == NULL || size_out == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    *size_out = 0;
    descriptor = mas_io_posix_decode(file);
    if (fstat(descriptor, &info) != 0) {
        int error = errno;

        return mas_io_posix_map(error);
    }
    if ((off_t)(int64_t)info.st_size != info.st_size) {
        return MAS_IO_ERROR_TOO_LARGE;
    }
    *size_out = (int64_t)info.st_size;
    return MAS_IO_OK;
}

/** 关闭平台描述符；`EINTR` 后不重试，因为描述符状态已经不确定。 */
int64_t mas_io_platform_close(void* file) {
    int descriptor;

    if (file == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }

    descriptor = mas_io_posix_decode(file);
    if (close(descriptor) != 0) {
        int error = errno;

        return mas_io_posix_map(error);
    }
    return MAS_IO_OK;
}
