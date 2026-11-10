/* 流式 IO 测试（文本与边界）：读行、追加、读写切换、空写入与超大 count。 */

#include "io_test_support.h"

#include <stdio.h>
#include <string.h>

static void test_read_line_forms(void) {
    const char* path = io_test_path("lines.txt");
    MasIoOpenResult open_read;

    CHECK(io_test_prepare_text(path, "a\r\nb\n\nlast"), "准备含 CRLF、空行和末行无换行的文件");
    open_read = io_test_open(path, MAS_IO_MODE_READ);
    CHECK(open_read.ok, "打开读句柄");
    if (!open_read.ok) {
        return;
    }

    {
        MasIoLineResult first = io_test_read_line(open_read.value);
        MasIoLineResult second = io_test_read_line(open_read.value);
        MasIoLineResult third = io_test_read_line(open_read.value);
        MasIoLineResult fourth = io_test_read_line(open_read.value);
        MasIoLineResult fifth = io_test_read_line(open_read.value);
        MasIoLineResult sixth = io_test_read_line(open_read.value);

        CHECK(first.ok && io_test_string_equal(first.value, "a", 1) && first.eof == 0, "CRLF 行剥除 CR 与 LF");
        CHECK(second.ok && io_test_string_equal(second.value, "b", 1) && second.eof == 0, "LF 行剥除 LF");
        CHECK(third.ok && third.value.len == 0 && third.eof == 0, "空行返回空串且不是 EOF");
        CHECK(fourth.ok && io_test_string_equal(fourth.value, "last", 4) && fourth.eof == 0, "末行无换行仍作为一行交付");
        CHECK(fifth.ok && fifth.value.len == 0 && fifth.eof == 1, "下一次调用才报告 EOF");
        CHECK(sixth.ok && sixth.value.len == 0 && sixth.eof == 1, "EOF 状态可以重复读取");
    }
    CHECK(io_test_close(open_read.value).ok, "关闭读句柄");
}

/** 覆盖读与读行共用同一逻辑位置与缓冲，读行不能吞掉后续字节。 */
static void test_line_shared_position(void) {
    const char* path = io_test_path("shared.txt");
    MasIoOpenResult open_read;

    CHECK(io_test_prepare_text(path, "hello world\nsecond"), "准备两行文件");
    open_read = io_test_open(path, MAS_IO_MODE_READ);
    CHECK(open_read.ok, "打开读句柄");
    if (!open_read.ok) {
        return;
    }

    {
        MasIoReadResult prefix = io_test_read(open_read.value, 5);
        MasIoLineResult rest = io_test_read_line(open_read.value);
        MasIoLineResult next = io_test_read_line(open_read.value);
        MasIoLineResult done = io_test_read_line(open_read.value);

        CHECK(io_test_bytes_equal(prefix.value, "hello", 5), "先读取 5 字节");
        CHECK(
            rest.ok && io_test_string_equal(rest.value, " world", 6),
            "读行从逻辑位置继续，返回剩余部分"
        );
        CHECK(next.ok && io_test_string_equal(next.value, "second", 6), "第二行完整返回");
        CHECK(done.ok && done.value.len == 0 && done.eof == 1, "读完两行后报告 EOF");
    }
    CHECK(io_test_close(open_read.value).ok, "关闭读句柄");
}

/** 覆盖非法 UTF-8：消费该行并报告 InvalidUtf8，下一行仍可读。 */
static void test_line_invalid_utf8(void) {
    const char* path = io_test_path("bad.txt");
    const unsigned char raw[] = { 0x61, 0xFF, 0x0A, 0x62, 0x0A };
    MasIoOpenResult open_read;

    CHECK(io_test_write_file(path, raw, sizeof(raw)).ok, "写入含非法 UTF-8 的行");
    open_read = io_test_open(path, MAS_IO_MODE_READ);
    CHECK(open_read.ok, "打开读句柄");
    if (!open_read.ok) {
        return;
    }

    {
        MasIoLineResult bad = io_test_read_line(open_read.value);
        MasIoLineResult next = io_test_read_line(open_read.value);

        CHECK(!bad.ok && bad.code == MAS_IO_ERROR_INVALID_UTF8 && bad.value.len == 0, "非法行返回 InvalidUtf8 与空串");
        CHECK(next.ok && io_test_string_equal(next.value, "b", 1), "非法行已被消费，下一行正常");
    }
    CHECK(io_test_close(open_read.value).ok, "关闭读句柄");
}

/** 覆盖内嵌 NUL：合法的 UTF-8 文本必须原样保留，不能被当成结尾。 */
static void test_line_embedded_nul(void) {
    const char* path = io_test_path("embedded-nul.txt");
    const char raw[] = { 'a', '\0', 'b', '\n' };
    MasIoOpenResult open_read;

    CHECK(io_test_write_file(path, raw, sizeof(raw)).ok, "写入含内嵌 NUL 的行");
    open_read = io_test_open(path, MAS_IO_MODE_READ);
    CHECK(open_read.ok, "打开读句柄");
    if (open_read.ok) {
        MasIoLineResult line = io_test_read_line(open_read.value);

        CHECK(
            line.ok && io_test_string_equal(line.value, raw, 3),
            "读行保留内嵌 NUL 的 3 个字节"
        );
        CHECK(io_test_close(open_read.value).ok, "关闭读句柄");
    }
}

/** 覆盖追加语义：每次写入都定位到末尾，两个追加句柄互不影响。 */
static void test_append_semantics(void) {
    const char* path = io_test_path("append.txt");
    MasIoOpenResult first_open;
    MasIoOpenResult second_open;

    CHECK(io_test_prepare_text(path, "base"), "准备初始内容");
    first_open = io_test_open(path, MAS_IO_MODE_APPEND);
    second_open = io_test_open(path, MAS_IO_MODE_APPEND);
    CHECK(first_open.ok && second_open.ok, "打开两个独立追加句柄");
    if (!first_open.ok || !second_open.ok) {
        return;
    }

    {
        MasIoWriteResult first = io_test_write(first_open.value, "1", 1);
        MasIoSeekResult seeked = io_test_seek(first_open.value, 0, MAS_IO_SEEK_START);
        MasIoWriteResult second = io_test_write(first_open.value, "2", 1);
        MasIoWriteResult third = io_test_write(second_open.value, "3", 1);
        MasIoWriteResult fourth = io_test_write(first_open.value, "4", 1);

        CHECK(first.ok && first.written == 1, "第一个追加句柄写入");
        CHECK(io_test_read(first_open.value, 1).code == MAS_IO_ERROR_WRONG_MODE, "追加句柄不能读");
        CHECK(seeked.ok && seeked.position == 0, "追加句柄允许定位");
        CHECK(second.ok, "定位之后追加写入仍然落到末尾");
        CHECK(third.ok && fourth.ok, "两个追加句柄依次写入");
    }

    CHECK(io_test_close(first_open.value).ok, "关闭第一个追加句柄");
    CHECK(io_test_close(second_open.value).ok, "关闭第二个追加句柄");
    CHECK(io_test_file_equals(path, "base1234", 8), "追加结果按写入顺序落在末尾");
}

/** 覆盖 ReadWrite 的读写切换：读后写必须落在逻辑位置而不是预读之后。 */
static void test_read_write_switch(void) {
    const char* path = io_test_path("switch.txt");
    MasIoOpenResult open_update;

    CHECK(io_test_prepare_text(path, "abcdef"), "准备 6 字节文件");
    open_update = io_test_open(path, MAS_IO_MODE_READ_WRITE);
    CHECK(open_update.ok, "ReadWrite 打开文件");
    if (!open_update.ok) {
        return;
    }

    {
        MasIoReadResult first = io_test_read(open_update.value, 2);
        MasIoSeekResult finish = io_test_seek(open_update.value, 0, MAS_IO_SEEK_END);
        MasIoWriteResult wrote = io_test_write(open_update.value, "X", 1);
        MasIoSeekResult rewind = io_test_seek(open_update.value, 0, MAS_IO_SEEK_START);
        MasIoReadResult all = io_test_read(open_update.value, 16);

        CHECK(io_test_bytes_equal(first.value, "ab", 2), "先读 2 字节");
        CHECK(finish.ok && finish.position == 6, "定位到末尾");
        CHECK(wrote.ok, "追加一个字节");
        CHECK(rewind.ok, "回到开头");
        CHECK(
            all.ok && io_test_bytes_equal(all.value, "abcdefX", 7),
            "读写切换后内容完整且新字节在末尾"
        );
    }

    {
        MasIoSeekResult start = io_test_seek(open_update.value, 1, MAS_IO_SEEK_START);
        MasIoWriteResult wrote = io_test_write(open_update.value, "Z", 1);

        CHECK(start.ok && start.position == 1, "定位到第 2 个字节");
        CHECK(wrote.ok, "在逻辑位置写入 1 字节");
    }
    CHECK(io_test_close(open_update.value).ok, "关闭 ReadWrite 句柄");
    CHECK(io_test_file_equals(path, "aZcdefX", 7), "ReadWrite 写入落在逻辑位置");

    /* 预读后直接写：必须先按逻辑位置同步宿主游标，否则会写到预读区之后。 */
    {
        const char* buffered = io_test_path("switch-buffered.txt");
        MasIoOpenResult open_buffered;

        CHECK(io_test_prepare_text(buffered, "abcdef"), "准备 6 字节文件用于缓冲后写入");
        open_buffered = io_test_open(buffered, MAS_IO_MODE_READ_WRITE);
        CHECK(open_buffered.ok, "ReadWrite 打开缓冲用例文件");
        if (open_buffered.ok) {
            MasIoReadResult prefix = io_test_read(open_buffered.value, 2);
            MasIoWriteResult wrote = io_test_write(open_buffered.value, "XY", 2);

            CHECK(io_test_bytes_equal(prefix.value, "ab", 2), "读 2 字节后仍留有未消费预读");
            CHECK(wrote.ok, "读转写前把宿主游标同步回逻辑位置");
            CHECK(io_test_close(open_buffered.value).ok, "关闭 ReadWrite 句柄");
            CHECK(io_test_file_equals(buffered, "abXYef", 6), "写入落在逻辑位置而不是文件末尾");
        }
    }
}

/** 覆盖读写边界：空写入、写句柄 flush、空文件读行与巨量请求的可恢复失败。 */
static void test_stream_edges(void) {
    const char* path = io_test_path("edges.txt");
    const char* empty = io_test_path("empty.txt");
    MasIoOpenResult open_write;
    MasIoOpenResult open_read;

    CHECK(io_test_write_file(empty, NULL, 0).ok, "创建空文件");

    open_write = io_test_open(path, MAS_IO_MODE_WRITE);
    CHECK(open_write.ok, "打开写句柄");
    if (open_write.ok) {
        MasIoWriteResult empty_write = io_test_write(open_write.value, NULL, 0);

        CHECK(empty_write.ok && empty_write.written == 0, "空写入成功且 written 为 0");
        CHECK(io_test_flush(open_write.value).ok, "写句柄 flush 成功");
        CHECK(io_test_close(open_write.value).ok, "关闭写句柄");
    }

    open_read = io_test_open(empty, MAS_IO_MODE_READ);
    CHECK(open_read.ok, "打开空文件");
    if (open_read.ok) {
        MasIoLineResult line = io_test_read_line(open_read.value);
        MasIoReadResult huge;
        MasIoReadResult eof;

        CHECK(line.ok && line.value.len == 0 && line.eof == 1, "空文件读行直接报告 EOF");
        huge = io_test_read(open_read.value, INT64_MAX);
        CHECK(
            huge.ok && huge.value.len == 0 && huge.eof == 1,
            "超大 count 在 EOF 上不预分配巨量内存，直接报告 EOF"
        );
        eof = io_test_read(open_read.value, 1);
        CHECK(eof.ok && eof.eof == 1, "超大 count 之后仍能正常读取");
        CHECK(io_test_close(open_read.value).ok, "关闭读句柄");
    }

    {
        MasIoOpenResult open_small = io_test_open(path, MAS_IO_MODE_WRITE);
        MasIoReadResult huge_read;

        if (open_small.ok) {
            CHECK(io_test_write(open_small.value, "abc", 3).ok, "写入 3 字节用于有界扩容用例");
            CHECK(io_test_close(open_small.value).ok, "关闭写句柄");
            open_read = io_test_open(path, MAS_IO_MODE_READ);
            if (open_read.ok) {
                huge_read = io_test_read(open_read.value, INT64_MAX);
                CHECK(
                    huge_read.ok && io_test_bytes_equal(huge_read.value, "abc", 3) && huge_read.eof == 0,
                    "超大 count 只按实际字节扩容并返回短读结果"
                );
                CHECK(io_test_close(open_read.value).ok, "关闭读句柄");
            }
        }
    }
}

void io_test_run_text(void) {
    test_read_line_forms();
    test_line_shared_position();
    test_line_invalid_utf8();
    test_line_embedded_nul();
    test_append_semantics();
    test_read_write_switch();
    test_stream_edges();
}
