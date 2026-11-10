/* 独立 C 测试驱动的公共部分：断言、临时目录、运行时桩与 API 包装。
 * 用 `native/test/run-tests.ps1` 调用本机 cl.exe 编译，不依赖 Mas 工具链。 */

#include "io_test_support.h"

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

/** 断言与跳过计数，`main` 用它决定退出码。 */
static int g_test_checks = 0;
static int g_test_failures = 0;

/** 测试临时目录的宽字符绝对路径。 */
static wchar_t g_test_root[MAX_PATH];

/** 路径辅助按轮转复用缓冲区，支持同一条语句里同时构造两个路径。 */
#define IO_TEST_PATH_SLOTS 6
static char g_test_path_slots[IO_TEST_PATH_SLOTS][MAX_PATH * 4];
static size_t g_test_path_slot = 0;

/** std 流桩的状态，测试通过 `io_test_stdin_*` 与全局开关控制。 */
static unsigned char g_stdin_data[4096];
static size_t g_stdin_length = 0;
static size_t g_stdin_offset = 0;
static size_t g_stdin_fail_offset = (size_t)-1;

unsigned long long g_stdout_bytes = 0;
unsigned long long g_stderr_bytes = 0;
unsigned long long g_stdout_flushes = 0;
unsigned long long g_stderr_flushes = 0;
int g_std_write_fail_code = 0;
long long g_std_write_limit = -1;

void io_test_check(int condition, const char* file, int line, const char* message) {
    g_test_checks += 1;
    if (condition == 0) {
        g_test_failures += 1;
        printf("FAIL %s:%d %s\n", file, line, message);
    }
}

void io_test_skip(const char* file, int line, const char* message) {
    printf("SKIP %s:%d %s\n", file, line, message);
}

/** 运行时字符串复制桩：用 `malloc` 模拟托管分配，内容逐字节可校验。 */
MasString mas_runtime_string_clone(const unsigned char* ptr, unsigned long long len) {
    MasString result;
    unsigned char* copy;

    result.ptr = (const unsigned char*)"";
    result.len = 0;
    if (len == 0 || ptr == NULL) {
        return result;
    }
    copy = (unsigned char*)malloc((size_t)len);
    if (copy == NULL) {
        return result;
    }
    memcpy(copy, ptr, (size_t)len);
    result.ptr = copy;
    result.len = len;
    return result;
}

/** 运行时字节复制桩，语义与字符串桩一致。 */
MasBytes mas_runtime_bytes_clone(const unsigned char* ptr, unsigned long long len) {
    MasBytes result;
    unsigned char* copy;

    result.ptr = (const unsigned char*)"";
    result.len = 0;
    if (len == 0 || ptr == NULL) {
        return result;
    }
    copy = (unsigned char*)malloc((size_t)len);
    if (copy == NULL) {
        return result;
    }
    memcpy(copy, ptr, (size_t)len);
    result.ptr = copy;
    result.len = len;
    return result;
}

/** 托管记录分配桩：零初始化即可，测试不追踪 GC 根。 */
void* mas_record_alloc(unsigned long long size, const unsigned long long* offsets, unsigned long long count) {
    (void)offsets;
    (void)count;
    if (size == 0) {
        return NULL;
    }
    return calloc(1, (size_t)size);
}

/** 原生层不应触发运行时错误；一旦触发即判失败并退出，便于定位。 */
void mas_runtime_abort(const char* message) {
    printf("ABORT %s\n", message == NULL ? "(null)" : message);
    g_test_failures += 1;
    exit(70);
}

long long mas_runtime_std_read(unsigned char* ptr, unsigned long long capacity) {
    size_t available;
    size_t take;

    if (ptr == NULL || capacity == 0) {
        return 0;
    }
    if (g_stdin_offset >= g_stdin_fail_offset) {
        return -(long long)MAS_IO_ERROR_OTHER;
    }
    available = g_stdin_length - g_stdin_offset;
    if (g_stdin_fail_offset != (size_t)-1 && g_stdin_fail_offset - g_stdin_offset < available) {
        available = g_stdin_fail_offset - g_stdin_offset;
    }
    take = available < (size_t)capacity ? available : (size_t)capacity;
    if (take == 0) {
        return 0;
    }
    memcpy(ptr, g_stdin_data + g_stdin_offset, take);
    g_stdin_offset += take;
    return (long long)take;
}

long long mas_runtime_std_write(unsigned long long which, const unsigned char* ptr, unsigned long long len) {
    unsigned long long accepted = len;

    (void)ptr;
    if (g_std_write_fail_code != 0) {
        return -(long long)g_std_write_fail_code;
    }
    if (which != MAS_IO_STDOUT && which != MAS_IO_STDERR) {
        return -(long long)MAS_IO_ERROR_NOT_SUPPORTED;
    }
    if (g_std_write_limit >= 0) {
        unsigned long long used = which == MAS_IO_STDOUT ? g_stdout_bytes : g_stderr_bytes;
        long long remaining = g_std_write_limit - (long long)used;
        if (remaining <= 0) {
            return -(long long)MAS_IO_ERROR_OTHER;
        }
        if ((unsigned long long)remaining < accepted) {
            accepted = (unsigned long long)remaining;
        }
    }
    if (accepted == 0) {
        return -(long long)MAS_IO_ERROR_OTHER;
    }
    if (which == MAS_IO_STDOUT) {
        g_stdout_bytes += accepted;
    } else {
        g_stderr_bytes += accepted;
    }
    return (long long)accepted;
}

long long mas_runtime_std_flush(unsigned long long which) {
    if (which == MAS_IO_STDOUT) {
        g_stdout_flushes += 1;
        return 0;
    }
    if (which == MAS_IO_STDERR) {
        g_stderr_flushes += 1;
        return 0;
    }
    return -(long long)MAS_IO_ERROR_NOT_SUPPORTED;
}

void io_test_stdin_set(const char* data, size_t length) {
    if (length > sizeof(g_stdin_data)) {
        length = sizeof(g_stdin_data);
    }
    memset(g_stdin_data, 0, sizeof(g_stdin_data));
    if (data != NULL && length > 0) {
        memcpy(g_stdin_data, data, length);
    }
    g_stdin_length = length;
    g_stdin_offset = 0;
    g_stdin_fail_offset = (size_t)-1;
}

void io_test_stdin_fail_after(size_t offset) {
    g_stdin_fail_offset = offset;
}

void io_test_std_reset(void) {
    io_test_stdin_set(NULL, 0);
    g_stdout_bytes = 0;
    g_stderr_bytes = 0;
    g_stdout_flushes = 0;
    g_stderr_flushes = 0;
    g_std_write_fail_code = 0;
    g_std_write_limit = -1;
}

/** UTF-8 转宽字符，返回 1 表示成功。 */
int io_test_wide_from_utf8(const char* text, wchar_t* out, size_t capacity) {
    int written;

    if (text == NULL || out == NULL || capacity == 0) {
        return 0;
    }
    written = MultiByteToWideChar(CP_UTF8, 0, text, -1, out, (int)capacity);
    return written > 0;
}

/** 宽字符转 UTF-8，返回 1 表示成功。 */
static int io_test_utf8_from_wide(const wchar_t* text, char* out, size_t capacity) {
    int written;

    if (text == NULL || out == NULL || capacity == 0) {
        return 0;
    }
    written = WideCharToMultiByte(CP_UTF8, 0, text, -1, out, (int)capacity, NULL, NULL);
    return written > 0;
}

/** 在缓冲区末尾追加一段宽字符文本，容量不足返回 0。 */
static int io_test_append(wchar_t* buffer, size_t capacity, const wchar_t* text) {
    size_t used = wcslen(buffer);
    size_t length = wcslen(text);

    if (used + length + 1 > capacity) {
        return 0;
    }
    memcpy(buffer + used, text, (length + 1) * sizeof(wchar_t));
    return 1;
}

/** 把临时目录和名字拼成宽字符路径。 */
static int io_test_join_wide(const wchar_t* name, wchar_t* out, size_t capacity) {
    size_t root_length = wcslen(g_test_root);
    size_t name_length = wcslen(name);

    if (root_length + name_length + 2 > capacity) {
        return 0;
    }
    memcpy(out, g_test_root, root_length * sizeof(wchar_t));
    out[root_length] = L'\\';
    memcpy(out + root_length + 1, name, (name_length + 1) * sizeof(wchar_t));
    return 1;
}

int io_test_wide_path(const char* name, wchar_t* out, size_t capacity) {
    wchar_t wide_name[MAX_PATH];

    if (!io_test_wide_from_utf8(name, wide_name, MAX_PATH)) {
        return 0;
    }
    return io_test_join_wide(wide_name, out, capacity);
}

const char* io_test_path(const char* name) {
    wchar_t wide[MAX_PATH];
    char* slot;

    slot = g_test_path_slots[g_test_path_slot];
    g_test_path_slot = (g_test_path_slot + 1) % IO_TEST_PATH_SLOTS;
    if (!io_test_wide_path(name, wide, MAX_PATH) || !io_test_utf8_from_wide(wide, slot, MAX_PATH * 4)) {
        slot[0] = '\0';
    }
    return slot;
}

const char* io_test_root_path(void) {
    static char root_buffer[MAX_PATH * 4];

    if (!io_test_utf8_from_wide(g_test_root, root_buffer, sizeof(root_buffer))) {
        root_buffer[0] = '\0';
    }
    return root_buffer;
}

MasString io_test_string(const char* text) {
    MasString value;

    value.ptr = (const unsigned char*)text;
    value.len = text == NULL ? 0 : (unsigned long long)strlen(text);
    return value;
}

int io_test_bytes_equal(MasBytes value, const void* expected, size_t length) {
    if (value.len != (unsigned long long)length) {
        return 0;
    }
    if (length == 0) {
        return 1;
    }
    if (value.ptr == NULL || expected == NULL) {
        return 0;
    }
    return memcmp(value.ptr, expected, length) == 0;
}

int io_test_string_equal(MasString value, const char* expected, size_t length) {
    if (value.len != (unsigned long long)length) {
        return 0;
    }
    if (length == 0) {
        return 1;
    }
    if (value.ptr == NULL || expected == NULL) {
        return 0;
    }
    return memcmp(value.ptr, expected, length) == 0;
}

/** 用整文件入口准备一份文本内容，返回是否成功。 */
int io_test_prepare_text(const char* path, const char* text) {
    MasIoWriteResult wrote = io_test_write_file(path, text, strlen(text));

    return wrote.ok && wrote.written == (int64_t)strlen(text);
}

/** 读取并校验整文件内容，便于在多个用例里复用。 */
int io_test_file_equals(const char* path, const void* expected, size_t length) {
    MasIoBytesResult content = io_test_read_file(path);

    return content.ok && io_test_bytes_equal(content.value, expected, length);
}

/* ------------------------------------------------------------------------- */
/* 临时目录与入口                                                             */
/* ------------------------------------------------------------------------- */

/** 递归删除测试目录；重解析点只删除链接本身，不进入目标。 */
static void io_test_remove_tree(const wchar_t* path) {
    wchar_t pattern[MAX_PATH];
    WIN32_FIND_DATAW data;
    HANDLE handle;

    pattern[0] = L'\0';
    if (!io_test_append(pattern, MAX_PATH, path) || !io_test_append(pattern, MAX_PATH, L"\\*")) {
        return;
    }

    handle = FindFirstFileW(pattern, &data);
    if (handle != INVALID_HANDLE_VALUE) {
        do {
            wchar_t child[MAX_PATH];

            if (wcscmp(data.cFileName, L".") == 0 || wcscmp(data.cFileName, L"..") == 0) {
                continue;
            }
            child[0] = L'\0';
            if (!io_test_append(child, MAX_PATH, path)
                || !io_test_append(child, MAX_PATH, L"\\")
                || !io_test_append(child, MAX_PATH, data.cFileName)) {
                continue;
            }
            if ((data.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0) {
                if ((data.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0) {
                    (void)RemoveDirectoryW(child);
                } else {
                    io_test_remove_tree(child);
                }
            } else {
                (void)DeleteFileW(child);
            }
        } while (FindNextFileW(handle, &data) != 0);
        (void)FindClose(handle);
    }
    (void)RemoveDirectoryW(path);
}

/** 创建本次运行的临时目录；失败返回 0。 */
static int io_test_begin(void) {
    wchar_t temp[MAX_PATH];
    wchar_t name[96];
    DWORD length;

    length = GetTempPathW(MAX_PATH, temp);
    if (length == 0 || length >= MAX_PATH) {
        return 0;
    }
    g_test_root[0] = L'\0';
    if (!io_test_append(g_test_root, MAX_PATH, temp)) {
        return 0;
    }
    _snwprintf_s(
        name,
        sizeof(name) / sizeof(name[0]),
        _TRUNCATE,
        L"mas-io-native-test-%lu-%llu",
        (unsigned long)GetCurrentProcessId(),
        (unsigned long long)GetTickCount64()
    );
    if (!io_test_append(g_test_root, MAX_PATH, name)) {
        return 0;
    }
    if (!CreateDirectoryW(g_test_root, NULL)) {
        return GetLastError() == ERROR_ALREADY_EXISTS;
    }
    return 1;
}

int main(void) {
    if (!io_test_begin()) {
        printf("FAIL 无法创建测试临时目录\n");
        return 2;
    }
    printf("临时目录: %s\n", io_test_root_path());

    io_test_run_streams();
    io_test_run_std();
    io_test_run_text();
    io_test_run_fs();
    io_test_run_links();

    io_test_remove_tree(g_test_root);
    printf("断言 %d 条，失败 %d 条\n", g_test_checks, g_test_failures);
    return g_test_failures == 0 ? 0 : 1;
}
