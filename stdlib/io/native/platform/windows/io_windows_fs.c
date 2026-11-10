/* Windows 文件系统适配层：元数据、目录枚举、建目录、禁止覆盖改名与删除。
 * 路径编码与错误映射在 `io_windows.c`，本文件复用它们。 */

#include "io_internal.h"

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#include <stdlib.h>
#include <string.h>
#include <wchar.h>

/**
 * 跟随符号链接查询元数据：`CreateFileW` 默认打开目标而不是链接本身，
 * 配合 `FILE_FLAG_BACKUP_SEMANTICS` 也能打开目录。
 */
int64_t mas_io_platform_metadata(
    const unsigned char* path,
    size_t path_len,
    int64_t* kind,
    int64_t* size,
    int* has_size
) {
    wchar_t* wide = NULL;
    HANDLE handle;
    BY_HANDLE_FILE_INFORMATION info;
    DWORD type;
    unsigned long long bytes;
    DWORD error;
    int64_t code;

    if (kind == NULL || size == NULL || has_size == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    *kind = MAS_IO_KIND_OTHER;
    *size = 0;
    *has_size = 0;

    code = mas_io_win_wide(path, path_len, &wide);
    if (code != MAS_IO_OK) {
        return code;
    }

    handle = CreateFileW(
        wide,
        0,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        NULL,
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS,
        NULL
    );
    free(wide);
    if (handle == INVALID_HANDLE_VALUE) {
        error = GetLastError();
        return mas_io_win_map(error);
    }

    /* 设备、管道等不是磁盘文件，统一归为 Other。 */
    type = GetFileType(handle);
    if (type != FILE_TYPE_DISK) {
        (void)CloseHandle(handle);
        return MAS_IO_OK;
    }

    if (!GetFileInformationByHandle(handle, &info)) {
        error = GetLastError();
        (void)CloseHandle(handle);
        return mas_io_win_map(error);
    }
    (void)CloseHandle(handle);

    if ((info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0) {
        *kind = MAS_IO_KIND_DIRECTORY;
        return MAS_IO_OK;
    }

    bytes = ((unsigned long long)info.nFileSizeHigh << 32) | (unsigned long long)info.nFileSizeLow;
    if (bytes > (unsigned long long)INT64_MAX) {
        return MAS_IO_ERROR_TOO_LARGE;
    }

    *kind = MAS_IO_KIND_REGULAR;
    *size = (int64_t)bytes;
    *has_size = 1;
    return MAS_IO_OK;
}

/** Windows 目录迭代状态：查找句柄加当前项数据。 */
typedef struct MasIoWinDir {
    HANDLE handle;
    WIN32_FIND_DATAW data;
    int first;
} MasIoWinDir;

/**
 * 打开目录迭代器。空目录由 `FindFirstFileW` 返回 `ERROR_FILE_NOT_FOUND`，
 * 这里把它当作“立即结束”而不是错误。
 */
int64_t mas_io_platform_dir_open(const unsigned char* path, size_t path_len, void** out) {
    wchar_t* wide = NULL;
    wchar_t* pattern;
    size_t wide_len;
    WIN32_FIND_DATAW data;
    HANDLE handle;
    MasIoWinDir* dir;
    DWORD error;
    int64_t code;

    if (out == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    *out = NULL;

    code = mas_io_win_wide(path, path_len, &wide);
    if (code != MAS_IO_OK) {
        return code;
    }

    wide_len = wcslen(wide);
    /* 末尾已是分隔符时只补通配符，否则补上分隔符和通配符。 */
    if (wide[wide_len - 1] == L'\\' || wide[wide_len - 1] == L'/') {
        pattern = (wchar_t*)malloc((wide_len + 2) * sizeof(wchar_t));
        if (pattern == NULL) {
            free(wide);
            return MAS_IO_ERROR_OTHER;
        }
        memcpy(pattern, wide, wide_len * sizeof(wchar_t));
        pattern[wide_len] = L'*';
        pattern[wide_len + 1] = L'\0';
    } else {
        pattern = (wchar_t*)malloc((wide_len + 3) * sizeof(wchar_t));
        if (pattern == NULL) {
            free(wide);
            return MAS_IO_ERROR_OTHER;
        }
        memcpy(pattern, wide, wide_len * sizeof(wchar_t));
        pattern[wide_len] = L'\\';
        pattern[wide_len + 1] = L'*';
        pattern[wide_len + 2] = L'\0';
    }
    free(wide);

    handle = FindFirstFileW(pattern, &data);
    error = handle == INVALID_HANDLE_VALUE ? GetLastError() : ERROR_SUCCESS;
    free(pattern);

    if (handle == INVALID_HANDLE_VALUE) {
        if (error != ERROR_FILE_NOT_FOUND && error != ERROR_NO_MORE_FILES) {
            return mas_io_win_map(error);
        }
        dir = (MasIoWinDir*)calloc(1, sizeof(*dir));
        if (dir == NULL) {
            return MAS_IO_ERROR_OTHER;
        }
        dir->handle = INVALID_HANDLE_VALUE;
        dir->first = 0;
        *out = (void*)dir;
        return MAS_IO_OK;
    }

    dir = (MasIoWinDir*)calloc(1, sizeof(*dir));
    if (dir == NULL) {
        (void)FindClose(handle);
        return MAS_IO_ERROR_OTHER;
    }
    dir->handle = handle;
    dir->data = data;
    dir->first = 1;
    *out = (void*)dir;
    return MAS_IO_OK;
}

/** 取下一个直接子项名称，跳过 `.` 与 `..`；名字必须是合法 UTF-8。 */
int64_t mas_io_platform_dir_next(void* dir_handle, int* done, unsigned char** name, size_t* name_len) {
    MasIoWinDir* dir = (MasIoWinDir*)dir_handle;

    if (dir == NULL || done == NULL || name == NULL || name_len == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    *done = 0;
    *name = NULL;
    *name_len = 0;

    for (;;) {
        if (dir->first != 0) {
            dir->first = 0;
        } else {
            if (dir->handle == INVALID_HANDLE_VALUE) {
                *done = 1;
                return MAS_IO_OK;
            }
            if (!FindNextFileW(dir->handle, &dir->data)) {
                DWORD error = GetLastError();

                if (error == ERROR_NO_MORE_FILES) {
                    *done = 1;
                    return MAS_IO_OK;
                }
                return mas_io_win_map(error);
            }
        }

        if (wcscmp(dir->data.cFileName, L".") == 0 || wcscmp(dir->data.cFileName, L"..") == 0) {
            continue;
        }
        return mas_io_win_utf8(dir->data.cFileName, name, name_len);
    }
}

/** 关闭目录迭代器并释放迭代状态。 */
int64_t mas_io_platform_dir_close(void* dir_handle) {
    MasIoWinDir* dir = (MasIoWinDir*)dir_handle;
    int64_t code = MAS_IO_OK;

    if (dir == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    if (dir->handle != INVALID_HANDLE_VALUE && !FindClose(dir->handle)) {
        DWORD error = GetLastError();

        code = mas_io_win_map(error);
    }
    free(dir);
    return code;
}

/** 创建一级目录：已存在由 `ERROR_ALREADY_EXISTS` 映射为 `AlreadyExists`。 */
int64_t mas_io_platform_make_dir(const unsigned char* path, size_t path_len) {
    wchar_t* wide = NULL;
    DWORD error;
    int64_t code;

    code = mas_io_win_wide(path, path_len, &wide);
    if (code != MAS_IO_OK) {
        return code;
    }
    if (!CreateDirectoryW(wide, NULL)) {
        error = GetLastError();
        free(wide);
        return mas_io_win_map(error);
    }
    free(wide);
    return MAS_IO_OK;
}

/**
 * 禁止覆盖的移动：`MoveFileExW` 不带 `MOVEFILE_REPLACE_EXISTING`，
 * 目标已存在由宿主原子拒绝；跨卷由 `ERROR_NOT_SAME_DEVICE` 映射为 `NotSupported`。
 */
int64_t mas_io_platform_rename(
    const unsigned char* source,
    size_t source_len,
    const unsigned char* destination,
    size_t destination_len
) {
    wchar_t* wide_source = NULL;
    wchar_t* wide_destination = NULL;
    DWORD error;
    int64_t code;

    code = mas_io_win_wide(source, source_len, &wide_source);
    if (code != MAS_IO_OK) {
        return code;
    }
    code = mas_io_win_wide(destination, destination_len, &wide_destination);
    if (code != MAS_IO_OK) {
        free(wide_source);
        return code;
    }

    if (!MoveFileExW(wide_source, wide_destination, 0)) {
        error = GetLastError();
        free(wide_source);
        free(wide_destination);
        return mas_io_win_map(error);
    }

    free(wide_source);
    free(wide_destination);
    return MAS_IO_OK;
}

/** 删除文件或指向文件的符号链接；`DeleteFileW` 不会沿着链接删除目标。 */
int64_t mas_io_platform_remove_file(const unsigned char* path, size_t path_len) {
    wchar_t* wide = NULL;
    DWORD error;
    int64_t code;

    code = mas_io_win_wide(path, path_len, &wide);
    if (code != MAS_IO_OK) {
        return code;
    }
    if (!DeleteFileW(wide)) {
        error = GetLastError();
        free(wide);
        return mas_io_win_map(error);
    }
    free(wide);
    return MAS_IO_OK;
}

/**
 * 删除空目录。先按属性拒绝符号链接与目录重解析点，避免跟随链接删除目标。
 * 检查与删除之间仍有极小的竞态窗口；`RemoveDirectoryW` 对重解析点本身会失败，
 * 因此不会删除链接指向的目录内容，失败按宿主错误映射。
 */
int64_t mas_io_platform_remove_dir(const unsigned char* path, size_t path_len) {
    wchar_t* wide = NULL;
    DWORD attributes;
    DWORD error;
    int64_t code;

    code = mas_io_win_wide(path, path_len, &wide);
    if (code != MAS_IO_OK) {
        return code;
    }

    attributes = GetFileAttributesW(wide);
    if (attributes == INVALID_FILE_ATTRIBUTES) {
        error = GetLastError();
        free(wide);
        return mas_io_win_map(error);
    }
    if ((attributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0) {
        free(wide);
        return MAS_IO_ERROR_NOT_SUPPORTED;
    }

    if (!RemoveDirectoryW(wide)) {
        error = GetLastError();
        free(wide);
        return mas_io_win_map(error);
    }
    free(wide);
    return MAS_IO_OK;
}
