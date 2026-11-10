use maslang::settings::CompilerSettings;
use maslang::source::SourceFile;
use maslang::temp_directory::TemporaryDirectory;
use maslang::{CompileOptions, compile_to_executable};
use std::process::{Command, Output};

/// 在临时目录里编译并运行真实 Mas 程序；`allow_native_source_build` 只在需要 C 源时打开。
fn execute(body: &str) -> Output {
    let root = TemporaryDirectory::create("mas-bytes").unwrap();
    let path = root.path().join("main.ms");
    std::fs::write(&path, body).unwrap();
    let source = SourceFile::new(path.display().to_string(), body.to_string());
    let settings = CompilerSettings::default_settings();
    let binary = compile_to_executable(
        &source,
        &settings,
        &CompileOptions::new(Some(root.path().join("main.exe"))),
    )
    .unwrap_or_else(|error| panic!("{}", error.render(settings.language, Some(&source))));
    Command::new(binary).output().unwrap()
}

/// 用真实 C 源写回字节字段，验证新增的 MasBytes ABI、托管复制入口与 GC 追踪。
const HOST: &str = r#"
#include "mas_runtime.h"

/* 与 Mas 侧 `struct Blob` 布局一致的记录：唯一字段是托管字节结构。 */
typedef struct Blob { MasBytes data; } Blob;

/* 把一段含 NUL 与 0xFF 的字节复制进调用方提供的结构体字段，随后强制回收。 */
void host_fill(Blob *out) {
    static const uint8_t payload[] = {0x00, 0x41, 0xff};
    extern void mas_gc_collect(void);
    out->data = mas_runtime_bytes_clone(payload, sizeof(payload));
    mas_gc_collect();
}

/* 读取输入记录的字节字段，把副本写回输出记录；全程使用结构体字段，避免顶层 bytes ABI。 */
void host_copy(Blob *input, Blob *out) {
    out->data = mas_runtime_bytes_clone(input->data.ptr, input->data.len);
}
"#;

/// 创建带真实 C 源的临时包并运行，用于验证 extern 出参里的字节字段。
fn execute_with_host(body: &str) -> Output {
    let root = TemporaryDirectory::create("mas-bytes-native").unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("settings.toml"),
        "[project]\nname = \"bytes-native\"\nversion = \"0.1.0\"\n[native]\nabi = 1\ninclude = [\".\"]\n[native.sources]\ncommon = [\"host.c\"]\n",
    )
    .unwrap();
    std::fs::write(root.path().join("host.c"), HOST).unwrap();
    std::fs::write(
        root.path().join("mas_runtime.h"),
        include_str!("../native/include/mas_runtime.h"),
    )
    .unwrap();
    let path = root.path().join("src/main.ms");
    std::fs::write(&path, body).unwrap();
    let source = SourceFile::new(path.display().to_string(), body.to_string());
    let settings = CompilerSettings::default_settings();
    let binary = compile_to_executable(
        &source,
        &settings,
        &CompileOptions::new(Some(root.path().join("main.exe")))
            .with_allow_native_source_build(true),
    )
    .unwrap_or_else(|error| panic!("{}", error.render(settings.language, Some(&source))));
    Command::new(binary).output().unwrap()
}

/// 构造、长度、索引、切片与逐字节比较按设计语义工作，非法输入返回 null 而不是崩溃。
#[test]
fn bytes_construction_index_slice_and_equality() {
    let output = execute(
        r#"func main():
    var values = [0, 1, 255]
    var created = bytes_from_list(values)
    print(created != null)
    var data = created.value
    print(len(data))
    print(data[0])
    print(data[2])
    var empty_list: list<int> = []
    var empty = bytes_from_list(empty_list)
    print(empty != null)
    print(len(empty.value))
    print(bytes_from_list([256]) == null)
    print(bytes_from_list([-1]) == null)
    var slice = data.slice(1, 3)
    print(slice != null)
    print(len(slice.value))
    print(slice.value[1])
    print(data.slice(3, 1) == null)
    print(data.slice(-1, 1) == null)
    print(data.slice(0, 4) == null)
    var same = bytes_from_list([0, 1, 255])
    print(same.value == data)
    print(same.value != data)
    print(bytes_from_list([1, 2]).value == bytes_from_list([1, 2]).value)
    print(bytes_from_list([1]).value == bytes_from_list([2]).value)
    print(empty.value == bytes_from_list(empty_list).value)
    print(len(bytes_from_list(values).value))
"#,
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "true\n3\n0\n255\ntrue\n0\ntrue\ntrue\ntrue\n2\n255\n\
true\ntrue\ntrue\ntrue\nfalse\ntrue\nfalse\ntrue\n3\n"
    );
}

/// UTF-8 编码与解码严格校验：非法编码返回 null，合法 NUL 保留且解码后内容一致。
#[test]
fn utf8_encode_and_decode_are_strict() {
    let output = execute(
        r#"func main():
    var encoded = encode_utf8("你好😀")
    print(encoded != null)
    print(len(encoded.value))
    var decoded = decode_utf8(encoded.value)
    print(decoded != null)
    print(decoded.value)
    print(len(decoded.value))
    var invalid = bytes_from_list([255, 254])
    print(decode_utf8(invalid.value) == null)
    var nul = bytes_from_list([0])
    print(decode_utf8(nul.value) != null)
    print(len(decode_utf8(nul.value).value))
    print(len(encode_utf8("").value))
    print(decode_utf8(encode_utf8("").value).value == "")
"#,
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "true\n10\ntrue\n你好😀\n3\ntrue\ntrue\n1\n0\ntrue\n"
    );
}

/// 字节对象跨 GC、结构体字段、列表元素和全局变量存活，内容不被回收破坏。
#[test]
fn bytes_survive_gc_across_storage_locations() {
    let output = execute(
        r#"struct Blob:
    data: bytes
extern func mas_gc_collect()
extern func mas_gc_live_objects() -> int
var global_data: bytes = bytes_from_list([7, 8]).value
func make() -> bytes:
    return bytes_from_list([1, 2, 3]).value
func describe(blob: Blob) -> int:
    return len(blob.data) + blob.data[0]
func main():
    var local = make()
    var holder = Blob(encode_utf8("abcd").value)
    var blobs: list<bytes> = [local, holder.data]
    mas_gc_collect()
    print(len(local))
    print(local[2])
    print(describe(holder))
    print(len(blobs))
    print(len(blobs[1]))
    print(blobs[0] == make())
    blobs.append(bytes_from_list([9, 9]).value)
    mas_gc_collect()
    print(len(blobs[2]))
    print(len(global_data))
    print(global_data[1])
    print(mas_gc_live_objects() >= 0)
    var nullable: bytes? = null
    print(nullable == null)
    nullable = local
    print(nullable != null)
    print(nullable.value[0])
"#,
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "3\n3\n101\n2\n4\ntrue\n2\n2\n8\ntrue\ntrue\ntrue\n1\n"
    );
}

/// extern 出参结构体的 bytes 字段由 C 通过托管复制入口填写，跨强制回收后仍可读取。
#[test]
fn native_bytes_field_round_trips_through_gc() {
    let output = execute_with_host(
        r#"struct Blob:
    data: bytes
extern func host_fill(out: Blob)
extern func host_copy(input: Blob, out: Blob)
extern func mas_gc_collect()
func main():
    var empty: list<int> = []
    var blob = Blob(bytes_from_list(empty).value)
    host_fill(blob)
    mas_gc_collect()
    print(len(blob.data))
    print(blob.data[0])
    print(blob.data[1])
    print(blob.data[2])
    var copied = Blob(bytes_from_list([5, 6]).value)
    host_copy(copied, blob)
    mas_gc_collect()
    print(len(blob.data))
    print(blob.data[0] + blob.data[1])
"#,
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "3\n0\n65\n255\n2\n11\n"
    );
}

/// `list<bytes>` 复用托管「指针 + 长度」槽位：增删查、可恢复读取与 GC 追踪都可用，
/// 但没有默认排序规则，按设计拒绝 `sort()`。
#[test]
fn bytes_lists_support_named_operations() {
    let output = execute(
        r#"extern func mas_gc_collect()
func main():
    var items: list<bytes> = [bytes_from_list([1]).value, bytes_from_list([2, 3]).value]
    print(items.index(bytes_from_list([2, 3]).value))
    print(items.contains(bytes_from_list([1]).value))
    print(len(items.pop()))
    print(len(items))
    items.append(bytes_from_list([9]).value)
    items.remove(bytes_from_list([9]).value)
    print(len(items))
    var maybe = items.try_get(0)
    print(maybe.ok)
    print(len(maybe.value))
    var missing = items.try_get(5)
    print(missing.ok)
    print(len(missing.value))
    mas_gc_collect()
    print(len(items[0]))
"#,
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "1\ntrue\n2\n1\n1\ntrue\n1\nfalse\n0\n1\n"
    );
}

/// 越界与负数下标进入带源码位置的运行时错误；不接受的用法在编译期报错。
#[test]
fn bytes_index_and_type_errors_are_reported() {
    for body in [
        "func main():\n    var data = bytes_from_list([1, 2]).value\n    print(data[2])\n",
        "func main():\n    var data = bytes_from_list([1, 2]).value\n    print(data[-1])\n",
    ] {
        let output = execute(body);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("字节下标越界"), "{stderr}");
        assert!(stderr.contains("main.ms"), "{stderr}");
    }

    let settings = CompilerSettings::default_settings();
    for (body, expected) in [
        (
            "func main():\n    var data = bytes_from_list([1]).value\n    print(data)\n",
            "print",
        ),
        (
            "func main():\n    var data = bytes_from_list([1]).value\n    print(int(data))\n",
            "int",
        ),
        (
            "func main():\n    var data = bytes_from_list([1]).value\n    print(data == \"a\")\n",
            "相等",
        ),
        (
            "func main():\n    var data = bytes_from_list([1]).value\n    print(decode_utf8(\"text\"))\n",
            "decode_utf8",
        ),
        (
            "func main():\n    print(encode_utf8(bytes_from_list([1]).value))\n",
            "encode_utf8",
        ),
        (
            "func main():\n    print(bytes_from_list([1, 2.5]))\n",
            "bytes_from_list",
        ),
        (
            "func main():\n    var data = bytes_from_list([1]).value\n    print(data.slice(0))\n",
            "slice",
        ),
        (
            "func main():\n    var data = bytes_from_list([1]).value\n    print(data.trim())\n",
            "trim",
        ),
        (
            "func main():\n    var m = {\"a\": bytes_from_list([1]).value}\n    print(len(m[\"a\"]))\n",
            "字典值",
        ),
        (
            "func main():\n    var items: list<bytes> = [bytes_from_list([1]).value]\n    items.sort()\n    print(len(items))\n",
            "sort",
        ),
        (
            "func main():\n    var data = bytes_from_list([1]).value\n    data[0] = 5\n    print(data[0])\n",
            "下标赋值",
        ),
        (
            "extern func host_len(data: bytes) -> int\nfunc main():\n    print(host_len(bytes_from_list([1]).value))\n",
            "原生函数",
        ),
        (
            "extern func host_make() -> bytes\nfunc main():\n    print(len(host_make()))\n",
            "原生函数",
        ),
    ] {
        let source = SourceFile::new("main.ms".to_string(), body.to_string());
        let rendered = maslang::analyze_source(&source, &settings)
            .unwrap_err()
            .render(settings.language, Some(&source));
        assert!(rendered.contains(expected), "{body} => {rendered}");
    }
}
