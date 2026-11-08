use maslang::temp_directory::TemporaryDirectory;
use std::{fs, process::Command};

/// 通过真实 CLI 比较 O0/O2/O3 的输出并检查构建器选择；覆盖 GC、不可变别名、跳转及保守回退。
#[test]
fn append_loops_preserve_strings_and_gc_roots() {
    let root = TemporaryDirectory::create("mas-string-builder").unwrap();
    let source = root.path().join("main.ms");
    fs::write(
        &source,
        r#"
extern func mas_gc_collect()
extern func mas_gc_live_objects() -> int
func part(i:int) -> string:
    mas_gc_collect()
    return string(i)
func eligible() -> string:
    var s = "前" + "缀"
    var alias = s
    var values = [s]
    var i = 0
    while i < 6:
        i += 1
        if i == 2:
            continue
        if i == 5:
            break
        s += part(i)
    print(alias)
    print(values[0])
    var other = ""
    for j in 0..3:
        s += "中"
        other += part(j)
    print(other)
    while false:
        s += "不可达"
    return s
func observed() -> string:
    var s = ""
    for i in 0..3:
        s += "x"
        print(s)
    return s
func self_read() -> string:
    var s = "a"
    for i in 0..3:
        s += s
    return s
func early_return() -> string:
    var s = ""
    for i in 0..3:
        s += "r"
        if i == 1:
            return s
    return s
func repeated() -> string:
    var s = ""
    for r in 0..3:
        s = ""
        for i in 0..4:
            s += "z"
    return s
var global_text = "g"
func global_append():
    for i in 0..2:
        global_text += "!"
func main():
    print(eligible())
    print(observed())
    print(self_read())
    print(early_return())
    print(repeated())
    global_append()
    print(global_text)
    global_text = ""
    mas_gc_collect()
    print(mas_gc_live_objects())
"#,
    )
    .unwrap();
    for level in ["0", "2", "3"] {
        let dir = root.path().join(level);
        let exe = dir.join("main.exe");
        let build = Command::new(env!("CARGO_BIN_EXE_maslang"))
            .arg("build")
            .arg(&source)
            .arg(format!("-O{level}"))
            .arg("--emit-dir")
            .arg(&dir)
            .arg("-o")
            .arg(&exe)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let output = Command::new(exe).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
            "前缀\n前缀\n012\n前缀134中中中\nx\nxx\nxxx\nxxx\naaaaaaaa\nrr\nzzzz\ng!!\n0\n"
        );
        let raw = fs::read_to_string(dir.join("main.raw.ll")).unwrap();
        for (name, optimized) in [
            ("eligible", true),
            ("repeated", true),
            ("observed", false),
            ("self_read", false),
            ("early_return", false),
            ("global_append", false),
        ] {
            let function = raw
                .split("\ndefine ")
                .find(|body| {
                    body.lines()
                        .next()
                        .is_some_and(|line| line.contains(&format!("_{name}(")))
                })
                .unwrap();
            let body = function.split("\n}").next().unwrap();
            assert_eq!(
                body.contains("@mas_string_builder_append"),
                optimized && level != "0",
                "{name} O{level}"
            );
        }
    }
}
