/* Windows 适配层基础部分：错误映射、路径编码和常规文件读写。
 * 文件系统入口（元数据/目录/改名/删除）在 `io_windows_fs.c`。 */

#include "io_internal.h"

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#include <stdlib.h>
#include <string.h>
#include <wchar.h>

/**
 * 把宿主 Win32 错误映射为公开错误类别。整个 Windows 适配层只有这一处映射，
 * 调用方必须在其他系统调用之前先把 `GetLastError()` 取出来传进来。
 * 未列出的错误统一归为 `Other`。
 */
int64_t mas_io_win_map(unsigned long error) {
    switch (error) {
    case ERROR_FILE_NOT_FOUND:
    case ERROR_PATH_NOT_FOUND:
    case ERROR_INVALID_DRIVE:
        return MAS_IO_ERROR_NOT_FOUND;
    case ERROR_ACCESS_DENIED:
    case ERROR_SHARING_VIOLATION:
    case ERROR_LOCK_VIOLATION:
    case ERROR_PRIVILEGE_NOT_HELD:
    case ERROR_WRITE_PROTECT:
        return MAS_IO_ERROR_PERMISSION_DENIED;
    case ERROR_FILE_EXISTS:
    case ERROR_ALREADY_EXISTS:
        return MAS_IO_ERROR_ALREADY_EXISTS;
    case ERROR_INVALID_NAME:
    case ERROR_BAD_PATHNAME:
    case ERROR_INVALID_PARAMETER:
    case ERROR_FILENAME_EXCED_RANGE:
    case ERROR_DIRECTORY:
    case ERROR_NEGATIVE_SEEK:
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    case ERROR_DIR_NOT_EMPTY:
        return MAS_IO_ERROR_DIRECTORY_NOT_EMPTY;
    case ERROR_NOT_SAME_DEVICE:
    case ERROR_NOT_SUPPORTED:
    case ERROR_INVALID_FUNCTION:
    case ERROR_CALL_NOT_IMPLEMENTED:
        return MAS_IO_ERROR_NOT_SUPPORTED;
    default:
        return MAS_IO_ERROR_OTHER;
    }
}

/**
 * 把已验证的 UTF-8 路径转换为以零结尾的宽字符路径。
 * 转换失败说明输入不是合法 UTF-8（公共层已先行校验），按设计归为 `InvalidArgument`。
 */
int64_t mas_io_win_wide(const unsigned char* path, size_t path_len, wchar_t** out) {
    int needed;
    wchar_t* buffer;

    if (out == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    *out = NULL;
    if (path == NULL || path_len == 0 || path_len > (size_t)INT_MAX) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }

    needed = MultiByteToWideChar(
        CP_UTF8,
        MB_ERR_INVALID_CHARS,
        (const char*)path,
        (int)path_len,
        NULL,
        0
    );
    if (needed <= 0) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }

    /* 额外一个宽字符存放终止零，宽字符 API 不接受带长度的路径。 */
    buffer = (wchar_t*)malloc(((size_t)needed + 1) * sizeof(wchar_t));
    if (buffer == NULL) {
        return MAS_IO_ERROR_OTHER;
    }
    if (MultiByteToWideChar(
            CP_UTF8,
            MB_ERR_INVALID_CHARS,
            (const char*)path,
            (int)path_len,
            buffer,
            needed
        ) != needed) {
        free(buffer);
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    buffer[needed] = L'\0';
    *out = buffer;
    return MAS_IO_OK;
}

/**
 * 把宽字符文件名转换为 UTF-8；非法 UTF-16 或转换失败返回 `InvalidUtf8`。
 * 结果以 `malloc` 分配，长度不含终止零。
 */
int64_t mas_io_win_utf8(const wchar_t* text, unsigned char** out, size_t* out_len) {
    int needed;
    unsigned char* buffer;

    if (text == NULL || out == NULL || out_len == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    *out = NULL;
    *out_len = 0;

    needed = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, text, -1, NULL, 0, NULL, NULL);
    if (needed <= 0) {
        return MAS_IO_ERROR_INVALID_UTF8;
    }
    buffer = (unsigned char*)malloc((size_t)needed);
    if (buffer == NULL) {
        return MAS_IO_ERROR_OTHER;
    }
    if (WideCharToMultiByte(
            CP_UTF8,
            WC_ERR_INVALID_CHARS,
            text,
            -1,
            (char*)buffer,
            needed,
            NULL,
            NULL
        ) != needed) {
        free(buffer);
        return MAS_IO_ERROR_INVALID_UTF8;
    }

    *out = buffer;
    *out_len = (size_t)needed - 1;
    return MAS_IO_OK;
}

/**
 * 打开常规文件。目录和目录重解析点不是普通文件，先按属性拒绝，
 * 避免把 `ERROR_ACCESS_DENIED` 误报成权限问题。
 */
int64_t mas_io_platform_open(const unsigned char* path, size_t path_len, int64_t mode, void** out) {
    wchar_t* wide = NULL;
    HANDLE handle;
    DWORD access;
    DWORD disposition;
    DWORD attributes;
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

    attributes = GetFileAttributesW(wide);
    if (attributes != INVALID_FILE_ATTRIBUTES && (attributes & FILE_ATTRIBUTE_DIRECTORY) != 0) {
        free(wide);
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }

    switch (mode) {
    case MAS_IO_MODE_READ:
        access = GENERIC_READ;
        disposition = OPEN_EXISTING;
        break;
    case MAS_IO_MODE_WRITE:
        access = GENERIC_WRITE;
        disposition = CREATE_ALWAYS;
        break;
    case MAS_IO_MODE_APPEND:
        /* 只申请 FILE_APPEND_DATA：宿主保证每次写入原子地落在文件末尾，
         * 多个进程并发追加不会定位到同一位置互相覆盖。 */
        access = FILE_APPEND_DATA;
        disposition = OPEN_ALWAYS;
        break;
    case MAS_IO_MODE_READ_WRITE:
        access = GENERIC_READ | GENERIC_WRITE;
        disposition = OPEN_EXISTING;
        break;
    default:
        free(wide);
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }

    handle = CreateFileW(
        wide,
        access,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        NULL,
        disposition,
        FILE_ATTRIBUTE_NORMAL,
        NULL
    );
    free(wide);
    if (handle == INVALID_HANDLE_VALUE) {
        error = GetLastError();
        return mas_io_win_map(error);
    }

    *out = (void*)handle;
    return MAS_IO_OK;
}

/** 读取一块数据；宿主返回 0 字节即为末尾，`ReadFile` 的 `ERROR_HANDLE_EOF` 也按末尾处理。 */
int64_t mas_io_platform_read(void* file, unsigned char* buffer, size_t capacity, size_t* read_out) {
    DWORD request;
    DWORD got = 0;

    if (file == NULL || buffer == NULL || read_out == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    *read_out = 0;
    if (capacity == 0) {
        return MAS_IO_OK;
    }

    request = capacity > (size_t)MAXDWORD ? MAXDWORD : (DWORD)capacity;
    if (!ReadFile((HANDLE)file, buffer, request, &got, NULL)) {
        DWORD error = GetLastError();

        if (error == ERROR_HANDLE_EOF) {
            return MAS_IO_OK;
        }
        return mas_io_win_map(error);
    }

    *read_out = (size_t)got;
    return MAS_IO_OK;
}

/** 写入一块数据；`*written_out` 是宿主本次接受的字节数，可能小于请求长度。 */
int64_t mas_io_platform_write(void* file, const unsigned char* data, size_t len, size_t* written_out) {
    DWORD request;
    DWORD wrote = 0;

    if (file == NULL || (data == NULL && len > 0) || written_out == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    *written_out = 0;
    if (len == 0) {
        return MAS_IO_OK;
    }

    request = len > (size_t)MAXDWORD ? MAXDWORD : (DWORD)len;
    if (!WriteFile((HANDLE)file, data, request, &wrote, NULL)) {
        DWORD error = GetLastError();

        return mas_io_win_map(error);
    }

    *written_out = (size_t)wrote;
    return MAS_IO_OK;
}

/**
 * 刷新输出。适配层直接写宿主句柄，没有用户态写缓冲可刷；
 * 设计的 `flush` 也不承诺断电持久化，因此这里只报告成功。
 */
int64_t mas_io_platform_flush(void* file) {
    if (file == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    return MAS_IO_OK;
}

/** 按起点、当前位置或末尾定位，返回从文件开头计数的绝对位置。 */
int64_t mas_io_platform_seek(void* file, int64_t offset, int64_t origin, int64_t* position_out) {
    LARGE_INTEGER distance;
    LARGE_INTEGER position;
    DWORD method;

    if (file == NULL || position_out == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }

    switch (origin) {
    case MAS_IO_SEEK_START:
        method = FILE_BEGIN;
        break;
    case MAS_IO_SEEK_CURRENT:
        method = FILE_CURRENT;
        break;
    case MAS_IO_SEEK_END:
        method = FILE_END;
        break;
    default:
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }

    distance.QuadPart = offset;
    if (!SetFilePointerEx((HANDLE)file, distance, &position, method)) {
        DWORD error = GetLastError();

        return mas_io_win_map(error);
    }

    *position_out = (int64_t)position.QuadPart;
    return MAS_IO_OK;
}

/** 查询文件当前字节大小；`GetFileSizeEx` 不改变文件指针。 */
int64_t mas_io_platform_size(void* file, int64_t* size_out) {
    LARGE_INTEGER size;

    if (file == NULL || size_out == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    *size_out = 0;
    if (!GetFileSizeEx((HANDLE)file, &size)) {
        DWORD error = GetLastError();

        return mas_io_win_map(error);
    }
    if (size.QuadPart < 0) {
        return MAS_IO_ERROR_TOO_LARGE;
    }
    *size_out = (int64_t)size.QuadPart;
    return MAS_IO_OK;
}

/** 关闭平台句柄并释放表项；句柄在调用后不可再用。 */
int64_t mas_io_platform_close(void* file) {
    if (file == NULL) {
        return MAS_IO_ERROR_INVALID_ARGUMENT;
    }
    if (!CloseHandle((HANDLE)file)) {
        DWORD error = GetLastError();

        return mas_io_win_map(error);
    }
    return MAS_IO_OK;
}
