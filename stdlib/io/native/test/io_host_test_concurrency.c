/* 并发测试：同一文件由多个子进程同时追加。
 * 追加落点必须由宿主保证（Windows `FILE_APPEND_DATA` / POSIX `O_APPEND`）；
 * 若实现退化成「先定位到末尾再写」，多个进程会定位到同一位置并互相覆盖，
 * 所有调用仍然报告成功、最终大小却小于写入总量。 */

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "io_test_support.h"

/** 并发追加的进程数、每进程追加次数与每次追加字节数。 */
#define IO_TEST_APPEND_PROCESSES 8
#define IO_TEST_APPEND_ITERATIONS 30
#define IO_TEST_APPEND_CHUNK_BYTES (1024 * 1024)

/** 子进程工作模式：向 `path` 追加 `iterations` 次、每次 `chunk` 字节；全部成功返回 0。 */
int io_test_run_append_worker(const char* path, int iterations, size_t chunk) {
    unsigned char* data;
    MasIoOpenResult opened;
    size_t index;
    int succeeded = 1;

    if (path == NULL || iterations <= 0 || chunk == 0) {
        return 2;
    }
    data = (unsigned char*)malloc(chunk);
    if (data == NULL) {
        return 2;
    }
    for (index = 0; index < chunk; index += 1) {
        data[index] = (unsigned char)(index % 251);
    }

    opened = io_test_open(path, MAS_IO_MODE_APPEND);
    if (!opened.ok) {
        free(data);
        return 3;
    }
    for (index = 0; index < (size_t)iterations; index += 1) {
        MasIoWriteResult wrote = io_test_write(opened.value, data, chunk);

        if (!wrote.ok || wrote.written != (int64_t)chunk) {
            succeeded = 0;
            break;
        }
    }
    if (!io_test_close(opened.value).ok) {
        succeeded = 0;
    }
    free(data);
    return succeeded ? 0 : 4;
}

/** 用当前测试可执行文件派生一个追加子进程；成功时把进程信息写入 `child`。 */
static int spawn_worker(
    const wchar_t* executable,
    const wchar_t* path,
    int iterations,
    size_t chunk,
    PROCESS_INFORMATION* child
) {
    wchar_t command[MAX_PATH * 3];
    STARTUPINFOW startup;
    int length;

    length = _snwprintf_s(
        command,
        sizeof(command) / sizeof(command[0]),
        _TRUNCATE,
        L"\"%ls\" --append-worker \"%ls\" %d %llu",
        executable,
        path,
        iterations,
        (unsigned long long)chunk
    );
    if (length <= 0) {
        return 0;
    }

    ZeroMemory(&startup, sizeof(startup));
    startup.cb = sizeof(startup);
    ZeroMemory(child, sizeof(*child));
    return CreateProcessW(
        executable,
        command,
        NULL,
        NULL,
        FALSE,
        0,
        NULL,
        NULL,
        &startup,
        child
    ) != 0;
}

/**
 * 覆盖多进程并发追加：8 个进程各向同一文件追加 30 MiB，
 * 最终大小必须等于 240 MiB，且每个子进程都报告成功。
 */
static void test_concurrent_append(void) {
    const char* path = io_test_path("concurrent-append.log");
    wchar_t executable[MAX_PATH];
    wchar_t wide_path[MAX_PATH];
    PROCESS_INFORMATION children[IO_TEST_APPEND_PROCESSES];
    int64_t expected = (int64_t)IO_TEST_APPEND_PROCESSES * IO_TEST_APPEND_ITERATIONS
        * IO_TEST_APPEND_CHUNK_BYTES;
    int started = 0;
    int failed = 0;
    int index;
    MasIoMetadataResult meta;

    if (GetModuleFileNameW(NULL, executable, MAX_PATH) == 0) {
        CHECK(0, "取当前测试可执行文件路径");
        return;
    }
    if (!io_test_wide_path("concurrent-append.log", wide_path, MAX_PATH)) {
        CHECK(0, "构造并发追加测试路径");
        return;
    }
    (void)io_test_remove_file(path);

    for (index = 0; index < IO_TEST_APPEND_PROCESSES; index += 1) {
        if (!spawn_worker(
                executable,
                wide_path,
                IO_TEST_APPEND_ITERATIONS,
                IO_TEST_APPEND_CHUNK_BYTES,
                &children[index]
            )) {
            break;
        }
        started += 1;
    }
    CHECK(started == IO_TEST_APPEND_PROCESSES, "并发追加用例启动了全部子进程");

    for (index = 0; index < started; index += 1) {
        DWORD status = 1;

        (void)WaitForSingleObject(children[index].hProcess, INFINITE);
        if (!GetExitCodeProcess(children[index].hProcess, &status) || status != 0) {
            failed += 1;
        }
        CloseHandle(children[index].hThread);
        CloseHandle(children[index].hProcess);
    }
    CHECK(failed == 0, "每个并发追加子进程都报告成功");

    meta = io_test_metadata(path);
    CHECK(meta.ok && meta.size != NULL, "并发追加后能读取文件大小");
    if (meta.ok && meta.size != NULL) {
        printf("并发追加实测大小: %lld（预期 %lld）\n", (long long)*meta.size, (long long)expected);
        CHECK(*meta.size == expected, "并发追加的字节数等于所有进程写入量之和");
    }
    CHECK(io_test_remove_file(path).ok, "清理并发追加文件");
}

void io_test_run_concurrency(void) {
    test_concurrent_append();
}
