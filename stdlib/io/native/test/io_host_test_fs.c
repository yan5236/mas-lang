/* 文件系统测试：整文件入口、元数据、目录枚举、建目录、禁止覆盖重命名与删除规则。
 * 目录与文件都建在临时目录下，不依赖进程当前目录或用户机器上的固定路径。 */

#include "io_test_support.h"

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#include <stdio.h>
#include <process.h>
#include <string.h>

#ifndef SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE
#define SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE 0x2
#endif

/** 构造内嵌 NUL 或指定长度的路径参数，用于覆盖非法输入。 */
static MasIoPathArg make_path_arg(const char* data, size_t length) {
    MasIoPathArg arg;

    memset(&arg, 0, sizeof(arg));
    arg.path.ptr = (const unsigned char*)data;
    arg.path.len = (unsigned long long)length;
    return arg;
}

/** 遍历目录，统计直接子项数量并判断是否包含指定名字；迭代器一定关闭。 */
static int dir_scan(const char* path, const char* expected, int* count_out, int64_t* close_code) {
    MasIoDirOpenResult open = io_test_dir_open(path);
    int found = 0;
    int count = 0;

    *count_out = 0;
    *close_code = MAS_IO_OK;
    if (!open.ok) {
        return -1;
    }

    for (;;) {
        MasIoDirEntryResult entry = io_test_dir_next(open.handle);

        if (!entry.ok || entry.done != 0) {
            if (!entry.ok) {
                found = -1;
            }
            break;
        }
        count += 1;
        if (expected != NULL && io_test_string_equal(entry.name, expected, strlen(expected))) {
            found = 1;
        }
    }

    *count_out = count;
    *close_code = io_test_dir_close(open.handle).code;
    return found;
}

/** 覆盖整文件入口的成功路径：字节、文本、元数据一致，空文件也成功。 */
static void test_whole_file_roundtrip(void) {
    const char* path = io_test_path("whole.txt");
    MasIoWriteResult wrote = io_test_write_file(path, "hello io", 8);
    MasIoBytesResult bytes;
    MasIoTextResult text;
    MasIoMetadataResult meta;

    CHECK(wrote.ok && wrote.written == 8, "整文件写入 8 字节");
    bytes = io_test_read_file(path);
    text = io_test_read_text(path);
    meta = io_test_metadata(path);
    CHECK(bytes.ok && io_test_bytes_equal(bytes.value, "hello io", 8), "整文件字节读回一致");
    CHECK(text.ok && io_test_string_equal(text.value, "hello io", 8), "整文件文本读回一致");
    CHECK(meta.ok && meta.kind == MAS_IO_KIND_REGULAR, "普通文件类型为 Regular");
    CHECK(meta.size != NULL && *meta.size == 8, "普通文件大小为 8");

    CHECK(!io_test_read_file(io_test_path("absent.txt")).ok, "读取不存在文件失败");
    CHECK(
        io_test_read_file(io_test_path("absent.txt")).code == MAS_IO_ERROR_NOT_FOUND,
        "不存在文件返回 NotFound"
    );
}

/** 覆盖覆盖与截断：write_file 总是创建或覆盖，空数据得到空文件。 */
static void test_write_file_truncates(void) {
    const char* path = io_test_path("truncate.txt");
    MasIoBytesResult after_short;
    MasIoBytesResult after_empty;

    CHECK(io_test_write_file(path, "abcdef", 6).ok, "先写入 6 字节");
    CHECK(io_test_write_file(path, "xy", 2).ok, "覆盖为 2 字节");
    after_short = io_test_read_file(path);
    CHECK(
        after_short.ok && io_test_bytes_equal(after_short.value, "xy", 2),
        "覆盖后文件只剩 2 字节"
    );

    CHECK(io_test_write_file(path, NULL, 0).ok, "空数据写入成功");
    after_empty = io_test_read_file(path);
    CHECK(after_empty.ok && after_empty.value.len == 0, "空数据把文件截断为空");
}

/** 覆盖追加入口：创建或追加，多次追加按顺序落在末尾。 */
static void test_append_file_grows(void) {
    const char* path = io_test_path("appended.txt");
    MasIoBytesResult content;

    CHECK(io_test_append_file(path, "one", 3).ok, "向不存在的文件追加会创建它");
    CHECK(io_test_append_file(path, "two", 3).ok, "再次追加");
    content = io_test_read_file(path);
    CHECK(
        content.ok && io_test_bytes_equal(content.value, "onetwo", 6),
        "两次追加按顺序拼接"
    );
}

/** 覆盖文本读取的严格校验：非法 UTF-8 报 InvalidUtf8，字节入口仍原样返回。 */
static void test_read_text_validation(void) {
    const char* path = io_test_path("invalid.txt");
    const unsigned char raw[] = { 0xFF, 0xFE, 0x41 };
    MasIoTextResult text;
    MasIoBytesResult bytes;

    CHECK(io_test_write_file(path, raw, sizeof(raw)).ok, "写入非法 UTF-8 字节");
    text = io_test_read_text(path);
    bytes = io_test_read_file(path);
    CHECK(!text.ok && text.code == MAS_IO_ERROR_INVALID_UTF8 && text.value.len == 0, "文本读取拒绝非法 UTF-8");
    CHECK(bytes.ok && io_test_bytes_equal(bytes.value, raw, sizeof(raw)), "字节读取原样保留");
}

/** 覆盖元数据类型：目录没有大小，缺失路径返回 NotFound。 */
static void test_metadata_kinds(void) {
    const char* directory = io_test_path("metadir");
    MasIoMetadataResult directory_meta;
    MasIoMetadataResult missing_meta;

    CHECK(io_test_make_dir(directory).ok, "创建目录用于元数据测试");
    directory_meta = io_test_metadata(directory);
    CHECK(
        directory_meta.ok && directory_meta.kind == MAS_IO_KIND_DIRECTORY && directory_meta.size == NULL,
        "目录类型为 Directory 且没有大小"
    );

    missing_meta = io_test_metadata(io_test_path("meta-absent"));
    CHECK(
        !missing_meta.ok && missing_meta.code == MAS_IO_ERROR_NOT_FOUND
            && missing_meta.kind == MAS_IO_KIND_OTHER && missing_meta.size == NULL,
        "缺失路径的元数据为 Other 且没有大小"
    );
}

/** 覆盖目录枚举：排除点与双点，返回直接子项名字，正常结束并关闭迭代器。 */
static void test_list_dir(void) {
    const char* directory = io_test_path("listdir");
    const char* child_dir = io_test_path("listdir\\child");
    int count = 0;
    int64_t close_code = 0;
    int found_a;
    int found_child;
    int found_unicode;
    MasIoDirOpenResult open;

    CHECK(io_test_make_dir(directory).ok, "创建枚举目录");
    CHECK(io_test_write_file(io_test_path("listdir\\a.txt"), "a", 1).ok, "写入 a.txt");
    CHECK(io_test_write_file(io_test_path("listdir\\中文.txt"), "u", 1).ok, "写入中文文件名");
    CHECK(io_test_make_dir(child_dir).ok, "创建子目录");

    found_a = dir_scan(directory, "a.txt", &count, &close_code);
    CHECK(found_a == 1, "枚举包含 a.txt");
    CHECK(count == 3, "只返回 3 个直接子项，不含点与双点");
    CHECK(close_code == MAS_IO_OK, "迭代器正常关闭");

    found_unicode = dir_scan(directory, "中文.txt", &count, &close_code);
    found_child = dir_scan(directory, "child", &count, &close_code);
    CHECK(found_unicode == 1, "中文文件名以合法 UTF-8 返回");
    CHECK(found_child == 1, "子目录作为直接子项返回");

    open = io_test_dir_open(directory);
    CHECK(open.ok && open.handle != 0, "目录迭代器句柄非 0");
    if (open.ok) {
        MasIoDirEntryResult last;
        MasIoStatus closed;

        for (;;) {
            MasIoDirEntryResult entry = io_test_dir_next(open.handle);

            if (!entry.ok) {
                CHECK(0, "枚举过程中出现错误");
                break;
            }
            if (entry.done != 0) {
                break;
            }
        }
        last = io_test_dir_next(open.handle);
        CHECK(last.ok && last.done == 1 && last.name.len == 0, "结束后继续调用仍然报告 done");
        closed = io_test_dir_close(open.handle);
        CHECK(closed.ok, "关闭目录迭代器");
        CHECK(io_test_dir_next(open.handle).code == MAS_IO_ERROR_CLOSED, "关闭后取下一项返回 Closed");
        CHECK(io_test_dir_close(open.handle).code == MAS_IO_ERROR_CLOSED, "重复关闭目录迭代器返回 Closed");
    }

    CHECK(io_test_dir_open(io_test_path("list-absent")).code == MAS_IO_ERROR_NOT_FOUND, "打开不存在目录返回 NotFound");
}

/** 覆盖建目录：成功、已存在与父目录缺失。 */
static void test_make_dir_rules(void) {
    const char* directory = io_test_path("newdir");
    MasIoStatus created = io_test_make_dir(directory);
    MasIoStatus again;
    MasIoStatus nested;

    CHECK(created.ok, "创建一级目录成功");
    again = io_test_make_dir(directory);
    CHECK(!again.ok && again.code == MAS_IO_ERROR_ALREADY_EXISTS, "重复创建返回 AlreadyExists");
    nested = io_test_make_dir(io_test_path("newdir\\missing\\leaf"));
    CHECK(!nested.ok && nested.code == MAS_IO_ERROR_NOT_FOUND, "父目录缺失返回 NotFound");
}

/** 覆盖重命名：同一文件系统内移动、目标存在时禁止覆盖、源缺失报错。 */
static void test_rename_rules(void) {
    const char* source = io_test_path("rename-src.txt");
    const char* destination = io_test_path("rename-dst.txt");
    const char* other = io_test_path("rename-other.txt");
    MasIoStatus moved;
    MasIoStatus conflict;
    MasIoStatus missing;
    MasIoBytesResult content;

    CHECK(io_test_write_file(source, "s", 1).ok, "准备源文件");
    moved = io_test_rename(source, destination);
    CHECK(moved.ok, "重命名成功");
    CHECK(io_test_read_file(source).code == MAS_IO_ERROR_NOT_FOUND, "源路径不再存在");
    content = io_test_read_file(destination);
    CHECK(content.ok && io_test_bytes_equal(content.value, "s", 1), "目标内容保持不变");

    CHECK(io_test_write_file(other, "o", 1).ok, "准备第二个文件");
    conflict = io_test_rename(other, destination);
    CHECK(!conflict.ok && conflict.code == MAS_IO_ERROR_ALREADY_EXISTS, "目标存在时禁止覆盖");
    content = io_test_read_file(destination);
    CHECK(content.ok && io_test_bytes_equal(content.value, "s", 1), "禁止覆盖后目标内容未被改写");

    missing = io_test_rename(io_test_path("rename-absent.txt"), io_test_path("rename-absent2.txt"));
    CHECK(!missing.ok && missing.code == MAS_IO_ERROR_NOT_FOUND, "源缺失返回 NotFound");

    {
        const char* source_dir = io_test_path("rename-dir");
        const char* destination_dir = io_test_path("rename-dir-moved");
        MasIoStatus moved_dir;

        CHECK(io_test_make_dir(source_dir).ok, "准备待重命名目录");
        moved_dir = io_test_rename(source_dir, destination_dir);
        CHECK(moved_dir.ok, "同一文件系统内可以重命名目录");
        CHECK(
            io_test_metadata(destination_dir).kind == MAS_IO_KIND_DIRECTORY,
            "重命名后目标仍是目录"
        );
    }
}

/** 覆盖删除文件：删除后打开返回 NotFound，重复删除返回 NotFound。 */
static void test_remove_file_rules(void) {
    const char* path = io_test_path("removed.txt");
    MasIoStatus removed;
    MasIoStatus again;

    CHECK(io_test_write_file(path, "x", 1).ok, "准备待删除文件");
    removed = io_test_remove_file(path);
    CHECK(removed.ok, "删除文件成功");
    CHECK(io_test_open(path, MAS_IO_MODE_READ).code == MAS_IO_ERROR_NOT_FOUND, "删除后打开返回 NotFound");
    again = io_test_remove_file(path);
    CHECK(!again.ok && again.code == MAS_IO_ERROR_NOT_FOUND, "重复删除返回 NotFound");
}

/** 覆盖删除目录：非空拒绝，清空后成功，普通文件不能用目录入口删除。 */
static void test_remove_dir_rules(void) {
    const char* directory = io_test_path("nonempty");
    const char* child = io_test_path("nonempty\\child.txt");
    MasIoStatus blocked;
    MasIoStatus removed;
    MasIoStatus on_file;
    const char* file_path = io_test_path("not-a-dir.txt");

    CHECK(io_test_make_dir(directory).ok, "创建待删除目录");
    CHECK(io_test_write_file(child, "c", 1).ok, "放入一个子文件");
    blocked = io_test_remove_dir(directory);
    CHECK(!blocked.ok && blocked.code == MAS_IO_ERROR_DIRECTORY_NOT_EMPTY, "非空目录返回 DirectoryNotEmpty");
    CHECK(io_test_remove_file(child).ok, "删除子文件");
    removed = io_test_remove_dir(directory);
    CHECK(removed.ok, "空目录删除成功");
    CHECK(io_test_metadata(directory).code == MAS_IO_ERROR_NOT_FOUND, "删除后目录不再存在");

    CHECK(io_test_write_file(file_path, "f", 1).ok, "准备一个普通文件");
    on_file = io_test_remove_dir(file_path);
    CHECK(!on_file.ok && on_file.code == MAS_IO_ERROR_INVALID_ARGUMENT, "对普通文件调用 remove_dir 返回 InvalidArgument");
    CHECK(io_test_remove_file(file_path).ok, "普通文件用 remove_file 删除");

    {
        const char* directory_path = io_test_path("remove-file-dir");
        MasIoStatus on_directory;

        CHECK(io_test_make_dir(directory_path).ok, "创建一个目录");
        on_directory = io_test_remove_file(directory_path);
        CHECK(!on_directory.ok, "remove_file 不能删除目录");
        CHECK(io_test_remove_dir(directory_path).ok, "目录改用 remove_dir 删除");
    }
}

/** 覆盖把目录当文件打开：两个平台都返回 InvalidArgument。 */
static void test_open_directory_as_file(void) {
    const char* directory = io_test_path("dir-as-file");

    CHECK(io_test_make_dir(directory).ok, "创建目录");
    CHECK(
        io_test_open(directory, MAS_IO_MODE_READ).code == MAS_IO_ERROR_INVALID_ARGUMENT,
        "以 Read 模式打开目录返回 InvalidArgument"
    );
    CHECK(
        io_test_open(directory, MAS_IO_MODE_WRITE).code == MAS_IO_ERROR_INVALID_ARGUMENT,
        "以 Write 模式打开目录返回 InvalidArgument"
    );
    CHECK(
        io_test_read_file(directory).code == MAS_IO_ERROR_INVALID_ARGUMENT,
        "read_file 打开目录返回 InvalidArgument"
    );
    CHECK(
        io_test_write_file(directory, "x", 1).code == MAS_IO_ERROR_INVALID_ARGUMENT,
        "write_file 打开目录返回 InvalidArgument"
    );
}

/** 覆盖 Unicode 路径：中文文件名在整文件入口、元数据与目录枚举中都正确。 */
static void test_unicode_paths(void) {
    const char* directory = io_test_path("unicodedir");
    const char* path = io_test_path("unicodedir\\中文 文件.txt");
    MasIoBytesResult content;
    MasIoMetadataResult meta;
    int count = 0;
    int64_t close_code = 0;

    CHECK(io_test_make_dir(directory).ok, "创建 Unicode 目录");
    CHECK(io_test_write_file(path, "内容", 6).ok, "写入中文文件名");
    content = io_test_read_file(path);
    CHECK(content.ok && io_test_bytes_equal(content.value, "内容", 6), "中文路径内容一致");
    meta = io_test_metadata(path);
    CHECK(meta.ok && meta.kind == MAS_IO_KIND_REGULAR && *meta.size == 6, "中文路径元数据正确");

    {
        int found = dir_scan(directory, "中文 文件.txt", &count, &close_code);

        CHECK(found == 1 && count == 1, "目录枚举返回中文文件名");
    }
}

/** 覆盖非法路径：内嵌 NUL 与空路径都被拒绝，且不触达宿主。 */
static void test_invalid_paths(void) {
    const char raw[] = { 'a', 'b', '\0', 'c' };
    MasIoPathArg with_nul = make_path_arg(raw, 4);
    MasIoPathArg empty = make_path_arg("", 0);
    MasIoStatus status;
    MasIoOpenResult open;
    MasIoDirOpenResult dir;
    MasIoOpenArg open_arg;

    memset(&status, 0, sizeof(status));
    mas_io_make_dir(&with_nul, &status);
    CHECK(!status.ok && status.code == MAS_IO_ERROR_INVALID_ARGUMENT, "内嵌 NUL 的路径返回 InvalidArgument");

    memset(&open, 0, sizeof(open));
    memset(&open_arg, 0, sizeof(open_arg));
    open_arg.path = with_nul.path;
    open_arg.mode = MAS_IO_MODE_READ;
    mas_io_open(&open_arg, &open);
    CHECK(!open.ok && open.code == MAS_IO_ERROR_INVALID_ARGUMENT, "内嵌 NUL 的打开返回 InvalidArgument");

    memset(&dir, 0, sizeof(dir));
    mas_io_dir_open(&with_nul, &dir);
    CHECK(!dir.ok && dir.code == MAS_IO_ERROR_INVALID_ARGUMENT, "内嵌 NUL 的目录打开返回 InvalidArgument");
    CHECK(dir.handle == 0, "失败时目录句柄为 0");

    memset(&status, 0, sizeof(status));
    mas_io_remove_file(&empty, &status);
    CHECK(!status.ok && status.code == MAS_IO_ERROR_INVALID_ARGUMENT, "空路径返回 InvalidArgument");

    {
        MasIoMetadataResult empty_meta;

        memset(&empty_meta, 0, sizeof(empty_meta));
        mas_io_metadata(&empty, &empty_meta);
        CHECK(
            !empty_meta.ok && empty_meta.code == MAS_IO_ERROR_INVALID_ARGUMENT,
            "空路径元数据返回 InvalidArgument"
        );
    }
}

/** 覆盖 NULL 指针防护：入口只返回失败，不崩溃也不终止进程。 */
static void test_null_arguments(void) {
    MasIoStatus status;
    MasIoBytesResult bytes;
    MasIoMetadataResult meta;

    memset(&status, 0, sizeof(status));
    mas_io_remove_file(NULL, &status);
    CHECK(!status.ok && status.code == MAS_IO_ERROR_INVALID_ARGUMENT, "NULL 路径参数返回 InvalidArgument");

    memset(&bytes, 0, sizeof(bytes));
    mas_io_read_file(NULL, &bytes);
    CHECK(!bytes.ok && bytes.value.len == 0, "NULL 读取参数返回失败与空字节");

    memset(&meta, 0, sizeof(meta));
    mas_io_metadata(NULL, &meta);
    CHECK(!meta.ok && meta.kind == MAS_IO_KIND_OTHER, "NULL 元数据参数返回 Other");

    mas_io_remove_file(NULL, NULL);
    CHECK(1, "NULL 结果指针被安全忽略");
}

void io_test_run_fs(void) {
    test_whole_file_roundtrip();
    test_write_file_truncates();
    test_append_file_grows();
    test_read_text_validation();
    test_metadata_kinds();
    test_list_dir();
    test_make_dir_rules();
    test_rename_rules();
    test_remove_file_rules();
    test_remove_dir_rules();
    test_open_directory_as_file();
    test_unicode_paths();
    test_invalid_paths();
    test_null_arguments();
}