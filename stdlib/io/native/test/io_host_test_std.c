/* 流式 IO 测试（生命周期与标准流）：关闭与别名、同文件双开、标准流借用与 stdin 读取。 */

#include "io_test_support.h"

#include <stdio.h>
#include <string.h>

static void test_close_semantics(void) {
    const char* path = io_test_path("close.txt");
    MasIoOpenResult open_read;
    MasIoFile alias;

    CHECK(io_test_prepare_text(path, "xyz"), "准备 3 字节文件");
    open_read = io_test_open(path, MAS_IO_MODE_READ);
    CHECK(open_read.ok, "打开读句柄");
    if (!open_read.ok) {
        return;
    }

    alias = *open_read.value;
    CHECK(io_test_close(open_read.value).ok, "首次关闭成功");
    {
        MasIoStatus again = io_test_close(open_read.value);

        CHECK(!again.ok && again.code == MAS_IO_ERROR_CLOSED, "重复关闭返回 Closed");
    }
    CHECK(io_test_read(&alias, 1).code == MAS_IO_ERROR_CLOSED, "别名令牌在关闭后返回 Closed");
    CHECK(io_test_read_line(&alias).code == MAS_IO_ERROR_CLOSED, "关闭后读行返回 Closed");
    CHECK(io_test_write(&alias, "z", 1).code == MAS_IO_ERROR_CLOSED, "关闭后写入返回 Closed");
    CHECK(io_test_flush(&alias).code == MAS_IO_ERROR_CLOSED, "关闭后 flush 返回 Closed");
    CHECK(io_test_seek(&alias, 0, MAS_IO_SEEK_START).code == MAS_IO_ERROR_CLOSED, "关闭后 seek 返回 Closed");
}

/** 覆盖同一文件两次打开互不影响：关闭一个后另一个继续可用。 */
static void test_two_handles_independent(void) {
    const char* path = io_test_path("twice.txt");
    MasIoOpenResult first_open;
    MasIoOpenResult second_open;

    CHECK(io_test_prepare_text(path, "abcdef"), "准备 6 字节文件");
    first_open = io_test_open(path, MAS_IO_MODE_READ);
    second_open = io_test_open(path, MAS_IO_MODE_READ);
    CHECK(first_open.ok && second_open.ok, "两次打开同一路径得到两个表项");
    CHECK(
        first_open.ok && second_open.ok && first_open.value->token != second_open.value->token,
        "两次打开得到不同令牌"
    );
    if (!first_open.ok || !second_open.ok) {
        return;
    }

    CHECK(io_test_bytes_equal(io_test_read(first_open.value, 2).value, "ab", 2), "第一个句柄读到 ab");
    CHECK(io_test_bytes_equal(io_test_read(second_open.value, 2).value, "ab", 2), "第二个句柄独立从头读");
    CHECK(io_test_close(first_open.value).ok, "关闭第一个句柄");
    CHECK(io_test_bytes_equal(io_test_read(second_open.value, 2).value, "cd", 2), "关闭一个后另一个继续读到 cd");
    CHECK(io_test_close(second_open.value).ok, "关闭第二个句柄");
}

/** 覆盖标准流借用句柄：只读 stdin、只写 stdout/stderr、close 与 seek 被拒绝。 */
static void test_standard_streams(void) {
    MasIoOpenResult stdin_open;
    MasIoOpenResult stdout_open;
    MasIoOpenResult stdout_again;
    MasIoOpenResult stderr_open;
    MasIoOpenResult bad;

    io_test_std_reset();
    stdin_open = io_test_open_std(MAS_IO_STDIN);
    stdout_open = io_test_open_std(MAS_IO_STDOUT);
    stdout_again = io_test_open_std(MAS_IO_STDOUT);
    stderr_open = io_test_open_std(MAS_IO_STDERR);
    bad = io_test_open_std(5);

    CHECK(stdin_open.ok && stdin_open.value->token == 1, "stdin 借用令牌 1");
    CHECK(stdout_open.ok && stdout_open.value->token == 2, "stdout 借用令牌 2");
    CHECK(stderr_open.ok && stderr_open.value->token == 3, "stderr 借用令牌 3");
    CHECK(
        stdout_open.ok && stdout_again.ok && stdout_open.value->token == stdout_again.value->token,
        "重复借用 stdout 得到同一令牌"
    );
    CHECK(!bad.ok && bad.code == MAS_IO_ERROR_INVALID_ARGUMENT, "越界标准流编号返回 InvalidArgument");

    if (stdin_open.ok && stdout_open.ok && stderr_open.ok) {
        CHECK(io_test_read(stdout_open.value, 1).code == MAS_IO_ERROR_WRONG_MODE, "stdout 不能读");
        CHECK(io_test_write(stdin_open.value, "x", 1).code == MAS_IO_ERROR_WRONG_MODE, "stdin 不能写");

        {
            MasIoWriteResult wrote = io_test_write(stdout_open.value, "abc", 3);
            MasIoWriteResult to_stderr = io_test_write(stderr_open.value, "xy", 2);

            CHECK(wrote.ok && wrote.written == 3 && g_stdout_bytes == 3, "stdout 写入 3 字节");
            CHECK(to_stderr.ok && to_stderr.written == 2 && g_stderr_bytes == 2, "stderr 写入 2 字节");
        }

        CHECK(io_test_flush(stdout_open.value).ok && g_stdout_flushes == 1, "stdout flush 走运行时桥");
        CHECK(
            io_test_flush(stdin_open.value).code == MAS_IO_ERROR_NOT_SUPPORTED,
            "stdin 没有输出缓冲，flush 返回 NotSupported"
        );
        CHECK(io_test_close(stdout_open.value).code == MAS_IO_ERROR_NOT_SUPPORTED, "标准流 close 返回 NotSupported");
        CHECK(
            io_test_seek(stdout_open.value, 0, MAS_IO_SEEK_START).code == MAS_IO_ERROR_NOT_SUPPORTED,
            "标准流 seek 返回 NotSupported"
        );
    }
}

/** 覆盖标准流失败传播：桥返回的具体错误码原样收敛到结果结构。 */
static void test_standard_stream_failures(void) {
    MasIoOpenResult stdout_open;

    io_test_std_reset();
    stdout_open = io_test_open_std(MAS_IO_STDOUT);
    CHECK(stdout_open.ok, "借用 stdout");
    if (!stdout_open.ok) {
        return;
    }

    g_std_write_fail_code = MAS_IO_ERROR_NOT_SUPPORTED;
    {
        MasIoWriteResult failed = io_test_write(stdout_open.value, "abc", 3);

        CHECK(
            !failed.ok && failed.code == MAS_IO_ERROR_NOT_SUPPORTED && failed.written == 0,
            "桥返回的错误码原样传播"
        );
    }

    io_test_std_reset();
    g_std_write_limit = 4;
    {
        MasIoWriteResult partial = io_test_write(stdout_open.value, "hello world", 11);

        CHECK(
            !partial.ok && partial.written == 4 && partial.code == MAS_IO_ERROR_OTHER,
            "部分写入失败时返回宿主已接受的 4 字节"
        );
        CHECK(g_stdout_bytes == 4, "部分写入只交付已接受的字节");
    }
    io_test_std_reset();
}

/**
 * 覆盖 stdin 上的读取语义：部分成功后的待报错误、错误清除、读行错误与 EOF。
 * 标准流是进程级借用句柄，缓冲状态跨调用保留，因此这些场景按真实顺序串起来；
 * 读完 EOF 之后不能再假装成一条新流，这也是设计里“流状态跟随句柄”的直接体现。
 */
static void test_stdin_reads(void) {
    MasIoOpenResult stdin_open;

    io_test_std_reset();
    stdin_open = io_test_open_std(MAS_IO_STDIN);
    CHECK(stdin_open.ok, "借用 stdin");
    if (!stdin_open.ok) {
        return;
    }

    /* 第一段：3 字节数据后宿主报错，先交付数据，错误留到下一次读取。 */
    io_test_stdin_set("abc", 3);
    io_test_stdin_fail_after(3);
    {
        MasIoReadResult first = io_test_read(stdin_open.value, 10);
        MasIoReadResult second = io_test_read(stdin_open.value, 1);
        MasIoReadResult third;
        MasIoReadResult fourth;

        CHECK(
            first.ok && io_test_bytes_equal(first.value, "abc", 3) && first.eof == 0,
            "宿主部分成功后先交付 3 字节"
        );
        CHECK(!second.ok && second.code == MAS_IO_ERROR_OTHER, "错误留到下一次正数读取");
        third = io_test_read(stdin_open.value, 1);
        CHECK(!third.ok && third.code == MAS_IO_ERROR_OTHER, "错误清除后再次读取仍报宿主错误");
        fourth = io_test_read(stdin_open.value, 0);
        CHECK(fourth.ok && fourth.value.len == 0, "read(0) 不受待报错误影响");
    }

    /* 第二段：读行同样先返回待报的错误。 */
    io_test_stdin_set("ab", 2);
    io_test_stdin_fail_after(2);
    {
        MasIoReadResult first = io_test_read(stdin_open.value, 10);
        MasIoLineResult line = io_test_read_line(stdin_open.value);

        CHECK(first.ok && io_test_bytes_equal(first.value, "ab", 2), "先交付 2 字节并留下待报错误");
        CHECK(!line.ok && line.code == MAS_IO_ERROR_OTHER, "读行先返回待报错误");
    }

    /* 第三段：正常数据按行读取，最后报告 EOF。 */
    io_test_stdin_set("line1\nline2", 11);
    {
        MasIoLineResult first = io_test_read_line(stdin_open.value);
        MasIoLineResult second = io_test_read_line(stdin_open.value);
        MasIoLineResult done = io_test_read_line(stdin_open.value);

        CHECK(first.ok && io_test_string_equal(first.value, "line1", 5), "stdin 读第一行");
        CHECK(second.ok && io_test_string_equal(second.value, "line2", 5), "stdin 读第二行");
        CHECK(done.ok && done.eof == 1, "stdin 读完报告 EOF");
    }
    io_test_std_reset();
}

/** 覆盖 seek 清除 EOF：读到末尾后回到开头仍能继续读。 */
static void test_seek_clears_eof(void) {
    const char* path = io_test_path("cleareof.txt");
    MasIoOpenResult open_read;

    CHECK(io_test_prepare_text(path, "abc"), "准备 3 字节文件");
    open_read = io_test_open(path, MAS_IO_MODE_READ);
    CHECK(open_read.ok, "打开读句柄");
    if (!open_read.ok) {
        return;
    }

    {
        MasIoReadResult read1 = io_test_read(open_read.value, 3);
        MasIoReadResult eof = io_test_read(open_read.value, 1);
        MasIoSeekResult start = io_test_seek(open_read.value, 0, MAS_IO_SEEK_START);
        MasIoReadResult again = io_test_read(open_read.value, 1);

        CHECK(read1.ok && read1.value.len == 3 && read1.eof == 0, "读到 3 字节");
        CHECK(eof.ok && eof.eof == 1, "空读取报告 EOF");
        CHECK(start.ok && start.position == 0, "seek 回到开头");
        CHECK(again.ok && io_test_bytes_equal(again.value, "a", 1) && again.eof == 0, "seek 清除 EOF 后继续读");
    }
    CHECK(io_test_close(open_read.value).ok, "关闭读句柄");
}

void io_test_run_std(void) {
    test_close_semantics();
    test_two_handles_independent();
    test_standard_streams();
    test_standard_stream_failures();
    test_stdin_reads();
    test_seek_clears_eof();
}
