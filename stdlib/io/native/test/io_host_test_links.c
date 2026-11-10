/* 链接与重解析点测试：文件链接删除不影响目标，目录链接与 junction 被 remove_dir 拒绝。
 * 这些用例依赖宿主是否允许创建链接，条件不满足时用 SKIP 明确标注而不是静默跳过。 */

#include "io_test_support.h"

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#include <stdio.h>
#include <process.h>
#include <string.h>


/** 尝试创建符号链接；宿主不支持或没有权限时返回 0，由调用方跳过。 */
static int create_symbolic_link(const char* link, const char* target, int directory) {
    wchar_t wide_link[MAX_PATH];
    wchar_t wide_target[MAX_PATH];
    DWORD flags = directory != 0 ? SYMBOLIC_LINK_FLAG_DIRECTORY : 0;

    if (!io_test_wide_from_utf8(link, wide_link, MAX_PATH)) {
        return 0;
    }
    if (!io_test_wide_from_utf8(target, wide_target, MAX_PATH)) {
        return 0;
    }
    if (CreateSymbolicLinkW(
            wide_link,
            wide_target,
            flags | SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE
        ) != 0) {
        return 1;
    }
    return CreateSymbolicLinkW(wide_link, wide_target, flags) != 0;
}

/** 用 `mklink /J` 创建目录 junction：不需要管理员权限，同样产生目录重解析点。 */
static int create_junction(const char* link, const char* target) {
    wchar_t wide_link[MAX_PATH];
    wchar_t wide_target[MAX_PATH];
    wchar_t command[MAX_PATH * 4];

    if (!io_test_wide_from_utf8(link, wide_link, MAX_PATH)) {
        return 0;
    }
    if (!io_test_wide_from_utf8(target, wide_target, MAX_PATH)) {
        return 0;
    }
    _snwprintf_s(
        command,
        sizeof(command) / sizeof(command[0]),
        _TRUNCATE,
        L"mklink /J \"%s\" \"%s\" >nul",
        wide_link,
        wide_target
    );
    return _wsystem(command) == 0;
}

/** 覆盖链接：文件链接可删除且不影响目标，目录链接与 junction 被 remove_dir 拒绝。 */
static void test_symbolic_links(void) {
    const char* target = io_test_path("link-target.txt");
    const char* file_link = io_test_path("link-file.txt");
    const char* directory = io_test_path("linkdir");
    const char* directory_link = io_test_path("linkdir-link");
    wchar_t wide_cleanup[MAX_PATH];

    CHECK(io_test_write_file(target, "t", 1).ok, "准备链接目标文件");
    if (create_symbolic_link(file_link, target, 0)) {
        CHECK(io_test_remove_file(file_link).ok, "删除文件符号链接成功");
        CHECK(io_test_metadata(target).ok, "链接目标文件仍然存在");
    } else {
        SKIP("宿主不允许创建文件符号链接，跳过文件链接删除用例");
    }

    CHECK(io_test_make_dir(directory).ok, "创建目录链接目标");
    if (!create_symbolic_link(directory_link, directory, 1)
        && !create_junction(directory_link, directory)) {
        SKIP("宿主不允许创建目录符号链接或 junction，跳过目录重解析点用例");
        return;
    }
    {
        MasIoStatus refused = io_test_remove_dir(directory_link);

        CHECK(
            !refused.ok && refused.code == MAS_IO_ERROR_NOT_SUPPORTED,
            "remove_dir 拒绝目录重解析点"
        );
    }
    if (io_test_wide_from_utf8(directory_link, wide_cleanup, MAX_PATH)) {
        CHECK(RemoveDirectoryW(wide_cleanup) != 0, "用宿主原语清理目录链接");
    }
}

void io_test_run_links(void) {
    test_symbolic_links();
}
