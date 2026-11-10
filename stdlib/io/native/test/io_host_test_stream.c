/* 流式 IO 测试（基础）：打开模式、短读与 EOF、写入、定位与读缓冲。
 * 读行与整段边界在 `io_host_test_text.c`，生命周期与标准流在 `io_host_test_std.c`。 */

#include "io_test_support.h"

#include <stdio.h>
#include <string.h>

static void test_open_modes(void) {
    const char* path = io_test_path("modes.txt");
    MasIoOpenResult open_write;
    MasIoOpenResult open_read;
    MasIoOpenResult open_append;
    MasIoOpenResult open_update;

    open_write = io_test_open(path, MAS_IO_MODE_WRITE);
    CHECK(open_write.ok && open_write.value != NULL, "Write 模式创建并打开新文件");
    if (open_write.ok) {
        MasIoWriteResult wrote = io_test_write(open_write.value, "hello", 5);

        CHECK(wrote.ok && wrote.written == 5, "Write 句柄写入 5 字节");
        CHECK(io_test_close(open_write.value).ok, "关闭 Write 句柄");
    }

    open_read = io_test_open(path, MAS_IO_MODE_READ);
    CHECK(open_read.ok, "Read 模式打开已存在文件");
    if (open_read.ok) {
        MasIoReadResult read = io_test_read(open_read.value, 5);

        CHECK(read.ok && io_test_bytes_equal(read.value, "hello", 5), "Read 句柄读回已写内容");
        CHECK(io_test_close(open_read.value).ok, "关闭 Read 句柄");
    }

    open_append = io_test_open(path, MAS_IO_MODE_APPEND);
    CHECK(open_append.ok, "Append 模式打开已存在文件");
    if (open_append.ok) {
        MasIoWriteResult wrote = io_test_write(open_append.value, "!", 1);

        CHECK(wrote.ok && wrote.written == 1, "Append 句柄写入 1 字节");
        CHECK(io_test_close(open_append.value).ok, "关闭 Append 句柄");
    }

    open_update = io_test_open(path, MAS_IO_MODE_READ_WRITE);
    CHECK(open_update.ok, "ReadWrite 模式打开已存在文件");
    if (open_update.ok) {
        MasIoReadResult read = io_test_read(open_update.value, 3);

        CHECK(read.ok && io_test_bytes_equal(read.value, "hel", 3), "ReadWrite 打开不截断文件");
        CHECK(io_test_close(open_update.value).ok, "关闭 ReadWrite 句柄");
    }

    CHECK(io_test_file_equals(path, "hello!", 6), "四种模式操作的是同一份文件内容");
}

/** 覆盖打开失败：缺失文件、越界模式；Write/Append 会创建文件。 */
static void test_open_errors(void) {
    const char* missing = io_test_path("missing.txt");
    MasIoOpenResult read_missing;
    MasIoOpenResult update_missing;
    MasIoOpenResult bad_mode;
    MasIoOpenResult created;

    read_missing = io_test_open(missing, MAS_IO_MODE_READ);
    CHECK(
        !read_missing.ok && read_missing.code == MAS_IO_ERROR_NOT_FOUND && read_missing.value == NULL,
        "Read 打开不存在文件返回 NotFound"
    );

    update_missing = io_test_open(missing, MAS_IO_MODE_READ_WRITE);
    CHECK(
        !update_missing.ok && update_missing.code == MAS_IO_ERROR_NOT_FOUND,
        "ReadWrite 要求文件已存在"
    );

    bad_mode = io_test_open(missing, 9);
    CHECK(
        !bad_mode.ok && bad_mode.code == MAS_IO_ERROR_INVALID_ARGUMENT,
        "越界模式返回 InvalidArgument"
    );

    created = io_test_open(missing, MAS_IO_MODE_APPEND);
    CHECK(created.ok, "Append 模式创建不存在的文件");
    if (created.ok) {
        CHECK(io_test_close(created.value).ok, "关闭新建的 Append 句柄");
    }
    CHECK(io_test_remove_file(missing).ok, "清理测试文件");
    CHECK(!io_test_open(missing, MAS_IO_MODE_READ).ok, "删除后再次打开返回失败");
}

/** 覆盖 read(0)、负数长度与写句柄上的读取：都不应触达宿主数据。 */
static void test_read_zero_and_negative(void) {
    const char* path = io_test_path("zero.txt");
    MasIoOpenResult open_read;
    MasIoOpenResult open_write;

    CHECK(io_test_prepare_text(path, "abcdef"), "准备 6 字节文件");

    open_read = io_test_open(path, MAS_IO_MODE_READ);
    CHECK(open_read.ok, "打开读句柄");
    if (open_read.ok) {
        MasIoReadResult zero = io_test_read(open_read.value, 0);
        MasIoReadResult negative = io_test_read(open_read.value, -1);

        CHECK(zero.ok && zero.value.len == 0 && zero.eof == 0, "read(0) 成功且不是 EOF");
        CHECK(!negative.ok && negative.code == MAS_IO_ERROR_INVALID_ARGUMENT, "负数长度返回 InvalidArgument");
        CHECK(io_test_close(open_read.value).ok, "关闭读句柄");
    }

    open_write = io_test_open(path, MAS_IO_MODE_WRITE);
    CHECK(open_write.ok, "打开写句柄");
    if (open_write.ok) {
        MasIoReadResult read = io_test_read(open_write.value, 0);
        MasIoLineResult line = io_test_read_line(open_write.value);

        CHECK(!read.ok && read.code == MAS_IO_ERROR_WRONG_MODE, "写句柄读取返回 WrongMode");
        CHECK(!line.ok && line.code == MAS_IO_ERROR_WRONG_MODE, "写句柄读行返回 WrongMode");
        CHECK(io_test_close(open_write.value).ok, "关闭写句柄");
    }
}

/** 覆盖短读与 EOF：有数据时 eof 为假，只有空读取才报告 EOF。 */
static void test_short_read_and_eof(void) {
    const char* path = io_test_path("short.txt");
    MasIoOpenResult open_read;

    CHECK(io_test_prepare_text(path, "abcdef"), "准备 6 字节文件");
    open_read = io_test_open(path, MAS_IO_MODE_READ);
    CHECK(open_read.ok, "打开读句柄");
    if (!open_read.ok) {
        return;
    }

    {
        MasIoReadResult first = io_test_read(open_read.value, 4);
        MasIoReadResult second = io_test_read(open_read.value, 4);
        MasIoReadResult third = io_test_read(open_read.value, 1);
        MasIoReadResult fourth = io_test_read(open_read.value, 1);

        CHECK(
            first.ok && io_test_bytes_equal(first.value, "abcd", 4) && first.eof == 0,
            "第一次读取 4 字节"
        );
        CHECK(
            second.ok && io_test_bytes_equal(second.value, "ef", 2) && second.eof == 0,
            "短读返回剩余 2 字节且不是 EOF"
        );
        CHECK(third.ok && third.value.len == 0 && third.eof == 1, "数据耗尽后空读取报告 EOF");
        CHECK(fourth.ok && fourth.value.len == 0 && fourth.eof == 1, "EOF 状态可以重复读取");
    }
    CHECK(io_test_close(open_read.value).ok, "关闭读句柄");
}

/** 覆盖模式不符：读句柄不能写、写句柄不能读，读句柄没有可刷新的输出缓冲。 */
static void test_wrong_mode(void) {
    const char* path = io_test_path("mode.txt");
    MasIoOpenResult open_read;
    MasIoOpenResult open_write;

    CHECK(io_test_prepare_text(path, "data"), "准备 4 字节文件");

    open_write = io_test_open(path, MAS_IO_MODE_WRITE);
    CHECK(open_write.ok, "打开写句柄");
    if (open_write.ok) {
        MasIoReadResult read = io_test_read(open_write.value, 1);

        CHECK(!read.ok && read.code == MAS_IO_ERROR_WRONG_MODE, "写句柄读取返回 WrongMode");
        CHECK(io_test_close(open_write.value).ok, "关闭写句柄");
    }

    open_read = io_test_open(path, MAS_IO_MODE_READ);
    CHECK(open_read.ok, "打开读句柄");
    if (open_read.ok) {
        MasIoWriteResult write = io_test_write(open_read.value, "x", 1);
        MasIoStatus flush = io_test_flush(open_read.value);

        CHECK(!write.ok && write.code == MAS_IO_ERROR_WRONG_MODE, "读句柄写入返回 WrongMode");
        CHECK(!flush.ok && flush.code == MAS_IO_ERROR_NOT_SUPPORTED, "读句柄 flush 返回 NotSupported");
        CHECK(io_test_close(open_read.value).ok, "关闭读句柄");
    }
}

/** 覆盖循环写完整段：超过读缓冲块大小的数据也必须完整写入并逐字节读回。 */
static void test_write_full_segment(void) {
    const char* path = io_test_path("large.bin");
    unsigned char data[20000];
    MasIoOpenResult open_write;
    size_t index;

    for (index = 0; index < sizeof(data); index += 1) {
        data[index] = (unsigned char)(index % 251);
    }

    open_write = io_test_open(path, MAS_IO_MODE_WRITE);
    CHECK(open_write.ok, "打开写句柄");
    if (!open_write.ok) {
        return;
    }
    {
        MasIoWriteResult wrote = io_test_write(open_write.value, data, sizeof(data));

        CHECK(wrote.ok && wrote.written == (int64_t)sizeof(data), "一次写入口循环写完整段");
        CHECK(io_test_close(open_write.value).ok, "关闭写句柄");
    }
    CHECK(io_test_file_equals(path, data, sizeof(data)), "大文件内容逐字节一致");
}

/** 覆盖 seek 的三种起点、定位后的读取以及非法起点与负位置。 */
static void test_seek_origins(void) {
    const char* path = io_test_path("seek.txt");
    MasIoOpenResult open_read;

    CHECK(io_test_prepare_text(path, "0123456789"), "准备 10 字节文件");
    open_read = io_test_open(path, MAS_IO_MODE_READ);
    CHECK(open_read.ok, "打开读句柄");
    if (!open_read.ok) {
        return;
    }

    {
        MasIoSeekResult start = io_test_seek(open_read.value, 3, MAS_IO_SEEK_START);
        MasIoReadResult read_start;
        MasIoSeekResult current;
        MasIoSeekResult end;
        MasIoReadResult read_end;
        MasIoSeekResult negative = io_test_seek(open_read.value, -1, MAS_IO_SEEK_START);
        MasIoSeekResult bad_origin = io_test_seek(open_read.value, 0, 5);

        CHECK(start.ok && start.position == 3, "seek(Start, 3) 返回绝对位置 3");
        read_start = io_test_read(open_read.value, 2);
        CHECK(io_test_bytes_equal(read_start.value, "34", 2), "定位后读到 34");

        current = io_test_seek(open_read.value, -2, MAS_IO_SEEK_CURRENT);
        CHECK(current.ok && current.position == 3, "seek(Current, -2) 相对逻辑位置回退");
        CHECK(io_test_bytes_equal(io_test_read(open_read.value, 2).value, "34", 2), "回退后读到 34");

        end = io_test_seek(open_read.value, -1, MAS_IO_SEEK_END);
        CHECK(end.ok && end.position == 9, "seek(End, -1) 返回倒数第一个字节位置");
        read_end = io_test_read(open_read.value, 1);
        CHECK(io_test_bytes_equal(read_end.value, "9", 1), "末尾定位读到 9");

        CHECK(!negative.ok && negative.code == MAS_IO_ERROR_INVALID_ARGUMENT, "负位置返回 InvalidArgument");
        CHECK(!bad_origin.ok && bad_origin.code == MAS_IO_ERROR_INVALID_ARGUMENT, "非法起点返回 InvalidArgument");
    }
    CHECK(io_test_close(open_read.value).ok, "关闭读句柄");
}

/** 覆盖读缓冲与逻辑位置：定位必须基于已交付字节，而不是预读后的宿主游标。 */
static void test_buffered_seek(void) {
    const char* path = io_test_path("buffer.txt");
    MasIoOpenResult open_read;

    CHECK(io_test_prepare_text(path, "abcdef"), "准备 6 字节文件");
    open_read = io_test_open(path, MAS_IO_MODE_READ);
    CHECK(open_read.ok, "打开读句柄");
    if (!open_read.ok) {
        return;
    }

    {
        MasIoReadResult first = io_test_read(open_read.value, 1);
        MasIoSeekResult current = io_test_seek(open_read.value, 0, MAS_IO_SEEK_CURRENT);
        MasIoReadResult second;

        CHECK(io_test_bytes_equal(first.value, "a", 1), "预读缓冲后交付第 1 个字节");
        CHECK(current.ok && current.position == 1, "seek(Current, 0) 使用逻辑位置 1");
        second = io_test_read(open_read.value, 2);
        CHECK(io_test_bytes_equal(second.value, "bc", 2), "定位后继续读到 bc");
    }

    {
        MasIoSeekResult start = io_test_seek(open_read.value, 0, MAS_IO_SEEK_START);
        MasIoReadResult reread;

        CHECK(start.ok && start.position == 0, "回到文件开头");
        reread = io_test_read(open_read.value, 2);
        CHECK(io_test_bytes_equal(reread.value, "ab", 2), "定位丢弃旧缓冲后从头读到 ab");
    }
    CHECK(io_test_close(open_read.value).ok, "关闭读句柄");
}


void io_test_run_streams(void) {
    test_open_modes();
    test_open_errors();
    test_read_zero_and_negative();
    test_short_read_and_eof();
    test_wrong_mode();
    test_write_full_segment();
    test_seek_origins();
    test_buffered_seek();
}
