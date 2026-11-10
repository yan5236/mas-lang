/* POSIX 文件系统适配层：元数据、目录枚举、建目录、禁止覆盖改名与删除。
 * 路径编码与错误映射在 `io_posix.c`，本文件复用它们。 */

#define _GNU_SOURCE 1
#define _FILE_OFFSET_BITS 64

#include "io_internal.h"

#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

#if defined(__linux__)
/* glibc 2.28 起才在 <stdio.h> 声明 renameat2；自行声明以便旧头文件也能编译。 */
extern int renameat2(
    int olddirfd,
    const char* oldpath,
    int newdirfd,
    const char* newpath,
    unsigned int flags
);
#ifndef RENAME_NOREPLACE
#define RENAME_NOREPLACE (1u << 0)
#endif
#ifndef AT_FDCWD
#define AT_FDCWD (-100)
#endif
#endif

#if defined(__APPLE__)
/* macOS 10.12 起提供 renamex_np；<sys/stdio.h> 只在 __DARWIN_C_FULL 下声明它，
 * 这里自行声明，避免依赖特性宏组合。RENAME_EXCL 表示目标已存在时失败。 */
extern int renamex_np(const char* from, const char* to, unsigned int flags);
#ifndef RENAME_EXCL
#define RENAME_EXCL 0x00000004u
#endif
#endif

/** 跟随符号链接查询元数据：`stat` 取目标信息，非普通文件不返回大小。 */
int64_t mas_io_platform_metadata(
    const unsigned char* path,
    size_t path_len,
    int64_t* kind,
    int64_t* size,
    int* has_size
) {
    char* text = NULL;
    struct stat info;
    int error;
    int64_t code;

    if (kind == NULL || size == NULL || has_size == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    *kind = MAS_IO_KIND_OTHER;
    *size = 0;
    *has_size = 0;

    code = mas_io_posix_path(path, path_len, &text);
    if (code != MAS_IO_OK) {
        return code;
    }

    if (stat(text, &info) != 0) {
        error = errno;
        free(text);
        return mas_io_posix_map(error);
    }
    free(text);

    if (S_ISDIR(info.st_mode)) {
        *kind = MAS_IO_KIND_DIRECTORY;
        return MAS_IO_OK;
    }
    if (!S_ISREG(info.st_mode)) {
        return MAS_IO_OK;
    }

    if ((off_t)(int64_t)info.st_size != info.st_size) {
        return MAS_IO_ERROR_TOO_LARGE;
    }
    *kind = MAS_IO_KIND_REGULAR;
    *size = (int64_t)info.st_size;
    *has_size = 1;
    return MAS_IO_OK;
}

/** 打开目录迭代器；`DIR*` 直接作为平台句柄保存，必然非 NULL。 */
int64_t mas_io_platform_dir_open(const unsigned char* path, size_t path_len, void** out) {
    char* text = NULL;
    DIR* stream;
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

    stream = opendir(text);
    if (stream == NULL) {
        error = errno;
        free(text);
        return mas_io_posix_map(error);
    }
    free(text);

    *out = (void*)stream;
    return MAS_IO_OK;
}

/**
 * 取下一个直接子项名称，跳过 `.` 与 `..`。
 * `readdir` 返回 NULL 时先看 `errno` 区分“读完”和“读失败”；名字必须是合法 UTF-8。
 */
int64_t mas_io_platform_dir_next(void* dir, int* done, unsigned char** name, size_t* name_len) {
    DIR* stream = (DIR*)dir;

    if (dir == NULL || done == NULL || name == NULL || name_len == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    *done = 0;
    *name = NULL;
    *name_len = 0;

    for (;;) {
        struct dirent* entry;
        const char* text;
        unsigned char* copy;
        size_t length;

        errno = 0;
        entry = readdir(stream);
        if (entry == NULL) {
            int error = errno;

            if (error != 0) {
                return mas_io_posix_map(error);
            }
            *done = 1;
            return MAS_IO_OK;
        }

        text = entry->d_name;
        length = strlen(text);
        if (length == 1 && text[0] == '.') {
            continue;
        }
        if (length == 2 && text[0] == '.' && text[1] == '.') {
            continue;
        }
        if (!mas_io_utf8_is_valid((const unsigned char*)text, length)) {
            return MAS_IO_ERROR_INVALID_UTF8;
        }

        copy = (unsigned char*)malloc(length == 0 ? 1 : length);
        if (copy == NULL) {
            return MAS_IO_ERROR_OTHER;
        }
        memcpy(copy, text, length);
        *name = copy;
        *name_len = length;
        return MAS_IO_OK;
    }
}

/** 关闭目录迭代器；`closedir` 失败只回报状态，句柄已不可再用。 */
int64_t mas_io_platform_dir_close(void* dir) {
    if (dir == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    if (closedir((DIR*)dir) != 0) {
        int error = errno;

        return mas_io_posix_map(error);
    }
    return MAS_IO_OK;
}

/** 创建一级目录的薄包装；权限位 0777 实际生效值由进程 umask 收紧。 */
static int mas_io_posix_mkdir(const char* path) {
    return mkdir(path, (mode_t)0777);
}

/** 创建一级目录：父目录缺失 `ENOENT` 映射 `NotFound`，已存在 `EEXIST` 映射 `AlreadyExists`。 */
int64_t mas_io_platform_make_dir(const unsigned char* path, size_t path_len) {
    char* text = NULL;
    int error;
    int64_t code;

    code = mas_io_posix_path(path, path_len, &text);
    if (code != MAS_IO_OK) {
        return code;
    }
    if (mas_io_posix_mkdir(text) != 0) {
        error = errno;
        free(text);
        return mas_io_posix_map(error);
    }
    free(text);
    return MAS_IO_OK;
}

/**
 * 禁止覆盖的移动：Linux 优先 `renameat2(..., RENAME_NOREPLACE)`；
 * macOS 使用 `renamex_np(..., RENAME_EXCL)`，都是单次原子移动且支持目录。
 * 其他 POSIX 退回 `link()` + `unlink()` 的原子组合，但不支持目录，
 * 两者都不可用（例如目录或没有硬链接的文件系统）返回 `NotSupported`。
 * 绝不使用“先检查目标不存在再普通 `rename`”这种有竞态的做法。
 */
int64_t mas_io_platform_rename(
    const unsigned char* source,
    size_t source_len,
    const unsigned char* destination,
    size_t destination_len
) {
    char* from = NULL;
    char* to = NULL;
    int error;
    int64_t code;

    code = mas_io_posix_path(source, source_len, &from);
    if (code != MAS_IO_OK) {
        return code;
    }
    code = mas_io_posix_path(destination, destination_len, &to);
    if (code != MAS_IO_OK) {
        free(from);
        return code;
    }

#if defined(__APPLE__)
    if (renamex_np(from, to, RENAME_EXCL) == 0) {
        free(from);
        free(to);
        return MAS_IO_OK;
    }
    error = errno;
    free(from);
    free(to);
    /* 宿主或文件系统不支持独占重命名时按设计返回 NotSupported，不回退到非原子做法。 */
    return mas_io_posix_map(error);
#endif

#if defined(__linux__)
    if (renameat2(AT_FDCWD, from, AT_FDCWD, to, RENAME_NOREPLACE) == 0) {
        free(from);
        free(to);
        return MAS_IO_OK;
    }
    error = errno;
    if (error != ENOSYS && error != EINVAL) {
        free(from);
        free(to);
        return mas_io_posix_map(error);
    }
#endif

    if (link(from, to) != 0) {
        error = errno;
        free(from);
        free(to);
        /* 硬链接不支持目录或该文件系统时，禁止覆盖的重命名没有可用原语。 */
        if (error == EPERM || error == ENOSYS) {
            return MAS_IO_ERROR_NOT_SUPPORTED;
        }
#ifdef EOPNOTSUPP
        if (error == EOPNOTSUPP) {
            return MAS_IO_ERROR_NOT_SUPPORTED;
        }
#endif
        return mas_io_posix_map(error);
    }

    if (unlink(from) != 0) {
        error = errno;
        /* 目标链接已经建立，删除源失败时尽力回滚，再按首次错误返回。 */
        (void)unlink(to);
        free(from);
        free(to);
        return mas_io_posix_map(error);
    }

    free(from);
    free(to);
    return MAS_IO_OK;
}

/** 删除文件或指向文件的符号链接；`unlink` 不跟随符号链接。 */
int64_t mas_io_platform_remove_file(const unsigned char* path, size_t path_len) {
    char* text = NULL;
    int error;
    int64_t code;

    code = mas_io_posix_path(path, path_len, &text);
    if (code != MAS_IO_OK) {
        return code;
    }
    if (unlink(text) != 0) {
        error = errno;
        free(text);
        return mas_io_posix_map(error);
    }
    free(text);
    return MAS_IO_OK;
}

/** 判断宿主对象是否为符号链接。 */
static int mas_io_posix_is_symlink(const struct stat* info) {
    return S_ISLNK(info->st_mode) != 0;
}

/**
 * 删除空目录。先用 `lstat` 拒绝符号链接（不跟随链接删除目标），
 * 再要求宿主对象确实是目录，最后 `rmdir` 保证只删除空目录。
 */
int64_t mas_io_platform_remove_dir(const unsigned char* path, size_t path_len) {
    char* text = NULL;
    struct stat info;
    int error;
    int64_t code;

    code = mas_io_posix_path(path, path_len, &text);
    if (code != MAS_IO_OK) {
        return code;
    }

    if (lstat(text, &info) != 0) {
        error = errno;
        free(text);
        return mas_io_posix_map(error);
    }
    if (mas_io_posix_is_symlink(&info)) {
        free(text);
        return MAS_IO_ERROR_NOT_SUPPORTED;
    }
    if (!S_ISDIR(info.st_mode)) {
        free(text);
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }

    if (rmdir(text) != 0) {
        error = errno;
        free(text);
        return mas_io_posix_map(error);
    }
    free(text);
    return MAS_IO_OK;
}
