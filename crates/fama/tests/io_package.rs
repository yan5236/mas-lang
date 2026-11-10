// io 0.1.0（原生 ABI 1）的端到端验收：走真实 `fama install` / `fama build` / `fama run`
// 链路，覆盖二进制复制、文本行、文件系统、错误类别与标准流重定向。
//
// 普通依赖消费默认只使用包内预编译产物；只有维护者刷新产物时才打开
// `--allow-native-source-build`。

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use maslang::temp_directory::TemporaryDirectory;

/// 由 Cargo 注入的 `fama` 可执行文件路径，避免在测试里再次调用 `cargo run` 造成锁等待。
const FAMA_BIN: &str = env!("CARGO_BIN_EXE_fama");

/// 返回仓库根目录，供测试定位标准库包与示例工程。
fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("fama 应位于 crates/fama 目录下")
        .to_path_buf()
}

/// 使用隔离的全局配置目录执行一次 `fama` 子命令。
///
/// `~/.fama/setting.toml` 可能配置了外部编译器路径，测试必须避免读到用户环境，
/// 否则结果会随机器变化；这里把 `FAMA_HOME` 指向空目录以使用内置编译器。
fn run_fama(project_dir: &Path, arguments: &[&str]) -> Output {
    let home = TemporaryDirectory::create("fama-io-home").expect("创建临时全局配置目录失败");
    Command::new(FAMA_BIN)
        .args(arguments)
        .current_dir(project_dir)
        .env("FAMA_HOME", home.path())
        .output()
        .expect("执行 fama 子命令失败")
}

/// 把子进程输出整理成便于断言的文本，断言失败时能直接看到前后文。
fn render_output(output: &Output) -> String {
    format!(
        "状态码: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// 统一换行后再断言，避免 Windows 的 `\r\n` 影响按行匹配。
fn normalized_stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

/// 断言输出包含全部标记，任一缺失都会打印完整输出便于定位。
fn assert_markers(text: &str, markers: &[&str]) {
    for marker in markers {
        assert!(text.contains(marker), "输出缺少标记 `{marker}`：\n{text}");
    }
}

/// 创建临时工程并写入依赖清单与入口源码；依赖指向仓库内的 `stdlib/io`。
fn create_project(name: &str, source: &str) -> TemporaryDirectory {
    let root = TemporaryDirectory::create(name).expect("创建临时工程失败");
    fs::create_dir_all(root.path().join("src")).expect("创建 src 目录失败");
    let dependency = repository_root().join("stdlib/io");
    fs::write(
        root.path().join("settings.toml"),
        format!(
            "[project]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ms\"\noutput = \"build\"\nopt-level = 2\n\n[dependencies]\n\"io\" = {{ path = \"{}\", version = \"0.1.0\" }}\n",
            dependency.display().to_string().replace('\\', "/")
        ),
    )
    .expect("写入工程配置失败");
    fs::write(root.path().join("src/main.ms"), source).expect("写入入口源码失败");
    root
}

/// 示例工程必须完整跑通：二进制复制逐字节一致、中文路径可访问、配置行与日志追加正确。
#[test]
fn io_example_round_trips_binary_config_and_log() {
    let project = repository_root().join("examples/projects/io-api");
    let _ = fs::remove_dir_all(project.join("mod_module"));

    let install = run_fama(&project, &["install", "."]);
    assert!(
        install.status.success(),
        "fama install 失败：{}",
        render_output(&install)
    );

    let build = run_fama(&project, &["build", ".", "--allow-native-source-build"]);
    assert!(
        build.status.success(),
        "fama build 失败：{}",
        render_output(&build)
    );

    let run = run_fama(&project, &["run", "."]);
    assert!(
        run.status.success(),
        "fama run 失败：{}",
        render_output(&run)
    );
    let stdout = normalized_stdout(&run);
    assert_markers(
        &stdout,
        &[
            "write source ok=true",
            "write source bytes=256",
            "copy bytes=256",
            "copy identical=true",
            "copy preserves NUL=true",
            "copy preserves 0xFF=true",
            "write config ok=true",
            "config lines=4",
            "config first=name=样例",
            "config empty=true",
            "config last no newline=last",
            "log start ok=true",
            "log append ok=true",
            "log lines=3",
            "log last=第二行",
            "metadata regular=true",
            "metadata size=256",
            "listing ok=true",
            "seek ok=true",
            "seek position=250",
            "tail bytes=6",
            "tail first=250",
            "close ok=true",
            "close again closed=true",
            "rename ok=true",
            "rename no overwrite=true",
            "remove ok=true",
        ],
    );
    assert!(
        stdout.trim_end().ends_with("done"),
        "示例必须以 done 结束：\n{stdout}"
    );
}

/// 错误类别、关闭语义、模式校验与整文件文本校验都按设计返回公开枚举。
#[test]
fn io_error_kinds_and_closed_tokens_follow_the_contract() {
    let project = create_project(
        "io-behaviour",
        r#"import io {
    FileKind,
    FileMode,
    IOErrorKind,
    SeekFrom,
    close,
    flush,
    list_dir,
    make_dir,
    metadata,
    open,
    read,
    read_file,
    read_line,
    read_text,
    remove_dir,
    remove_file,
    rename,
    seek,
    write,
    write_file,
    write_text,
}

func main():
    var base = "work"
    var made = make_dir(base)
    print("make_dir ok=" + string(made.ok))
    var missing = open(base + "/nope.bin", FileMode.Read)
    print("missing not found=" + string(missing.code == IOErrorKind.NotFound))
    print("missing null value=" + string(missing.value == null))
    var created = open(base + "/bin.dat", FileMode.Write)
    print("create ok=" + string(created.ok))
    var handle = created.value.value
    var wrong = read(handle, 4)
    print("wrong mode=" + string(wrong.code == IOErrorKind.WrongMode))
    print("wrong mode eof=" + string(wrong.eof))
    var wrote = write(handle, bytes_from_list([10, 13, 65, 66]).value)
    print("write ok=" + string(wrote.ok))
    print("write written=" + string(wrote.written))
    var flushed = flush(handle)
    print("flush ok=" + string(flushed.ok))
    var closed = close(handle)
    print("close ok=" + string(closed.ok))
    var again = close(handle)
    print("close twice closed=" + string(again.code == IOErrorKind.Closed))
    var read_after_close = read(handle, 1)
    print("read after close=" + string(read_after_close.code == IOErrorKind.Closed))
    var reopened = open(base + "/bin.dat", FileMode.Read)
    var reader = reopened.value.value
    var zero = read(reader, 0)
    print("read zero ok=" + string(zero.ok))
    print("read zero eof=" + string(zero.eof))
    print("read zero len=" + string(len(zero.value)))
    var chunk = read(reader, 2)
    print("chunk len=" + string(len(chunk.value)))
    print("chunk first=" + string(chunk.value[0]))
    print("chunk eof=" + string(chunk.eof))
    var line = read_line(reader)
    print("line value=" + line.value)
    print("line eof=" + string(line.eof))
    var tail = read(reader, 4)
    print("tail eof=" + string(tail.eof))
    print("tail len=" + string(len(tail.value)))
    var seeked = seek(reader, 0, SeekFrom.Start)
    print("seek ok=" + string(seeked.ok))
    var after_seek = read(reader, 1)
    print("after seek first=" + string(after_seek.value[0]))
    close(reader)
    var bad_path = base + "/bad.txt"
    var bad = write_file(bad_path, bytes_from_list([255, 254, 10]).value)
    print("write invalid utf8 bytes ok=" + string(bad.ok))
    var bad_text = read_text(bad_path)
    print("read_text invalid=" + string(bad_text.code == IOErrorKind.InvalidUtf8))
    print("read_text empty=" + string(len(bad_text.value) == 0))
    var bad_open = open(bad_path, FileMode.Read)
    var bad_line = read_line(bad_open.value.value)
    print("read_line invalid=" + string(bad_line.code == IOErrorKind.InvalidUtf8))
    close(bad_open.value.value)
    var empty_path = base + "/empty.txt"
    var empty_write = write_file(empty_path, bytes_from_list([0]).value.slice(0, 0).value)
    print("write empty ok=" + string(empty_write.ok))
    var empty_read = read_file(empty_path)
    print("read empty ok=" + string(empty_read.ok))
    print("read empty len=" + string(len(empty_read.value)))
    var sub = make_dir(base + "/sub")
    print("make sub ok=" + string(sub.ok))
    var duplicate = make_dir(base + "/sub")
    print("duplicate exists=" + string(duplicate.code == IOErrorKind.AlreadyExists))
    var moved = rename(base + "/bin.dat", base + "/moved.dat")
    print("rename ok=" + string(moved.ok))
    var blocked = rename(base + "/moved.dat", bad_path)
    print("rename overwrite=" + string(blocked.code == IOErrorKind.AlreadyExists))
    var not_empty = remove_dir(base)
    print("remove non empty dir=" + string(not_empty.code == IOErrorKind.DirectoryNotEmpty))
    var absent = remove_file(base + "/absent.bin")
    print("remove absent=" + string(absent.code == IOErrorKind.NotFound))
    var dir_info = metadata(base)
    print("dir kind=" + string(dir_info.kind == FileKind.Directory))
    print("dir size null=" + string(dir_info.size == null))
    var missing_listing = list_dir(base + "/absent")
    print("missing listing=" + string(missing_listing.code == IOErrorKind.NotFound))
    var missing_directory = remove_dir(base + "/bin.dat")
    print("remove file as dir ok=" + string(missing_directory.ok))
    var text_written = write_text(base + "/unicode.txt", "文本")
    print("write text ok=" + string(text_written.ok))
    var text_read = read_text(base + "/unicode.txt")
    print("read text ok=" + string(text_read.ok))
    print("read text value=" + text_read.value)
    print("done")
"#,
    );
    let run = run_fama(project.path(), &["run", "."]);
    assert!(
        run.status.success(),
        "fama run 失败：{}",
        render_output(&run)
    );
    let stdout = normalized_stdout(&run);
    assert_markers(
        &stdout,
        &[
            "make_dir ok=true",
            "missing not found=true",
            "missing null value=true",
            "create ok=true",
            "wrong mode=true",
            "wrong mode eof=false",
            "write ok=true",
            "write written=4",
            "flush ok=true",
            "close ok=true",
            "close twice closed=true",
            "read after close=true",
            "read zero ok=true",
            "read zero eof=false",
            "read zero len=0",
            "chunk len=2",
            "chunk first=10",
            "chunk eof=false",
            "line value=AB",
            "line eof=false",
            "tail eof=true",
            "tail len=0",
            "seek ok=true",
            "after seek first=10",
            "write invalid utf8 bytes ok=true",
            "read_text invalid=true",
            "read_text empty=true",
            "read_line invalid=true",
            "write empty ok=true",
            "read empty ok=true",
            "read empty len=0",
            "make sub ok=true",
            "duplicate exists=true",
            "rename ok=true",
            "rename overwrite=true",
            "remove non empty dir=true",
            "remove absent=true",
            "dir kind=true",
            "dir size null=true",
            "missing listing=true",
            "remove file as dir ok=false",
            "write text ok=true",
            "read text ok=true",
            "read text value=文本",
        ],
    );
    assert!(
        stdout.trim_end().ends_with("done"),
        "行为用例必须以 done 结束：\n{stdout}"
    );
}

/// 标准流按原进程重定向工作：stdout 与 `print` 共用同一顺序，stderr 隔离，stdin 读原始字节。
#[test]
fn io_standard_streams_keep_order_and_redirect() {
    let project = create_project(
        "io-streams",
        r#"import io {
    IOErrorKind,
    SeekFrom,
    close,
    flush,
    read,
    seek,
    stderr,
    stdin,
    stdout,
    write,
    write_text_to,
}

func main():
    print("line-before")
    var written = write_text_to(stdout(), "stdout-bytes")
    print("")
    var out_close = close(stdout())
    var out_seek = seek(stdout(), 0, SeekFrom.Start)
    var flushed = flush(stdout())
    var error_write = write_text_to(stderr(), "stderr-only")
    var input = read(stdin(), 6)
    print("stdin ok=" + string(input.ok))
    print("stdin len=" + string(len(input.value)))
    print("stdin first=" + string(input.value[0]))
    var in_close = close(stdin())
    print("stdout close supported=" + string(out_close.code == IOErrorKind.NotSupported))
    print("stdout seek supported=" + string(out_seek.code == IOErrorKind.NotSupported))
    print("stdout flush ok=" + string(flushed.ok))
    print("stderr write ok=" + string(error_write.ok))
    print("stdin close supported=" + string(in_close.code == IOErrorKind.NotSupported))
    print("line-after")
"#,
    );
    let build = run_fama(project.path(), &["build", "."]);
    assert!(
        build.status.success(),
        "fama build 失败：{}",
        render_output(&build)
    );
    let binary = project.path().join("build/io-streams.exe");
    let mut child = Command::new(&binary)
        .current_dir(project.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("启动标准流用例失败");
    child
        .stdin
        .as_mut()
        .expect("stdin piped")
        .write_all(b"abcdef\n")
        .expect("写入标准输入失败");
    let output = child.wait_with_output().expect("等待标准流用例失败");
    assert!(output.status.success(), "{output:?}");

    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    let stderr = String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n");
    assert_eq!(
        stdout,
        "line-before\nstdout-bytes\nstdin ok=true\nstdin len=6\nstdin first=97\n\
stdout close supported=true\nstdout seek supported=true\nstdout flush ok=true\n\
stderr write ok=true\nstdin close supported=true\nline-after\n",
        "标准输出顺序或内容不符：\n{stdout}"
    );
    assert_eq!(stderr, "stderr-only", "标准错误必须独立：\n{stderr}");
}

/// 文本写入在打开文件前严格校验 UTF-8：非法文本返回 InvalidUtf8 且原文件内容不被截断。
#[test]
fn io_text_write_rejects_invalid_utf8_before_truncating() {
    let project = create_project(
        "io-invalid-text",
        r#"import io {
    IOErrorKind,
    read_text,
    write_text,
}

extern func bad_text() -> string

func main():
    var path = "work/keep.txt"
    var seeded = write_text(path, "原始内容")
    print("seed ok=" + string(seeded.ok))
    var rejected = write_text(path, bad_text())
    print("rejected invalid utf8=" + string(rejected.code == IOErrorKind.InvalidUtf8))
    print("rejected written=" + string(rejected.written))
    var kept = read_text(path)
    print("kept ok=" + string(kept.ok))
    print("kept value=" + kept.value)
    print("done")
"#,
    );
    // 工程需要额外原生源来构造硬件无法直接产生的非法 UTF-8 字符串。
    let manifest = fs::read_to_string(project.path().join("settings.toml")).expect("读取清单失败");
    let dependency = repository_root().join("stdlib/io");
    fs::write(
        project.path().join("settings.toml"),
        format!(
            "[project]\nname = \"io-invalid-text\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ms\"\noutput = \"build\"\nopt-level = 2\n\n[native]\nabi = 1\ninclude = [\".\"]\n\n[native.sources]\ncommon = [\"host.c\"]\n\n[dependencies]\n\"io\" = {{ path = \"{}\", version = \"0.1.0\" }}\n",
            dependency.display().to_string().replace('\\', "/")
        ),
    )
    .expect("写入带原生的工程配置失败");
    assert!(manifest.contains("io"), "基础清单必须包含 io 依赖");
    fs::write(
        project.path().join("host.c"),
        r#"#include "mas_runtime.h"

/* 返回原生构造的非法 UTF-8 字符串，用于验证文本写入在截断前就被拒绝。 */
const uint8_t *bad_text(unsigned long long *out_len) {
    static const uint8_t bytes[] = {0xff, 0xfe};
    *out_len = sizeof(bytes);
    return bytes;
}
"#,
    )
    .expect("写入原生源失败");
    fs::write(
        project.path().join("mas_runtime.h"),
        include_str!("../../maslang/native/include/mas_runtime.h"),
    )
    .expect("写入运行时头文件失败");
    fs::create_dir_all(project.path().join("work")).expect("创建工作目录失败");

    // 工程自身带原生源，需要维护者源码构建；构建后直接运行可执行文件断言行为。
    let build = run_fama(
        project.path(),
        &["build", ".", "--allow-native-source-build"],
    );
    assert!(
        build.status.success(),
        "fama build 失败：{}",
        render_output(&build)
    );
    let output = Command::new(project.path().join("build/io-invalid-text.exe"))
        .current_dir(project.path())
        .output()
        .expect("运行非法文本用例失败");
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_markers(
        &stdout,
        &[
            "seed ok=true",
            "rejected invalid utf8=true",
            "rejected written=0",
            "kept ok=true",
            "kept value=原始内容",
        ],
    );
}

/// 普通依赖消费必须使用包内预编译产物：不打开源码回退也能安装、构建并运行。
#[test]
fn io_normal_dependency_uses_prebuilt_objects() {
    let project = create_project(
        "io-prebuilt",
        r#"import io {
    FileMode,
    close,
    open,
    read,
    write,
    write_file,
}

func main():
    var base = "work"
    var made = open(base + "/prebuilt.bin", FileMode.Write)
    print("open ok=" + string(made.ok))
    var handle = made.value.value
    var wrote = write(handle, bytes_from_list([1, 2, 3]).value)
    print("write ok=" + string(wrote.ok))
    close(handle)
    var read_back = open(base + "/prebuilt.bin", FileMode.Read)
    var chunk = read(read_back.value.value, 8)
    print("read len=" + string(len(chunk.value)))
    print("read last=" + string(chunk.value[2]))
    close(read_back.value.value)
    print("done")
"#,
    );
    fs::create_dir_all(project.path().join("work")).expect("创建工作目录失败");
    let install = run_fama(project.path(), &["install", "."]);
    assert!(
        install.status.success(),
        "fama install 失败：{}",
        render_output(&install)
    );
    let build = run_fama(project.path(), &["build", "."]);
    let stderr = String::from_utf8_lossy(&build.stderr);
    assert!(
        !stderr.contains("--allow-native-source-build"),
        "普通构建不应要求源码回退：{}",
        render_output(&build)
    );
    assert!(
        build.status.success(),
        "普通构建应使用预编译产物：{}",
        render_output(&build)
    );
    let run = run_fama(project.path(), &["run", "."]);
    assert!(
        run.status.success(),
        "fama run 失败：{}",
        render_output(&run)
    );
    let stdout = normalized_stdout(&run);
    assert_markers(
        &stdout,
        &[
            "open ok=true",
            "write ok=true",
            "read len=3",
            "read last=3",
            "done",
        ],
    );
}
