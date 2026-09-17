use mascript_compiler_llvm::settings::{CompilerSettings, UiLanguage};
use mascript_compiler_llvm::source::SourceFile;
use mascript_compiler_llvm::{CompileOptions, compile_to_executable};
use std::process::Command;

/// 编译当前源码并执行原生产物；测试目录只包含本用例文件，结束后主动清理。
fn run(name: &str, text: &str) -> String {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("mascript-gc-{name}-{unique}"));
    std::fs::create_dir_all(&root).unwrap();
    let source = SourceFile::new(root.join("main.ms").display().to_string(), text.to_string());
    let result = compile_to_executable(
        &source,
        &CompilerSettings::new(UiLanguage::ZhCn),
        &CompileOptions::new(Some(root.join("main.exe"))),
    );
    let binary = result.unwrap();
    let output = Command::new(binary).output().unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert!(
        output.status.success(),
        "exit={:?}\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// 百万轮真实分配必须自动收集；退出函数后强制收集应无残留托管对象。
#[test]
fn million_iterations_have_bounded_live_objects() {
    let output = run(
        "million",
        r#"
extern func mas_gc_collect()
extern func mas_gc_live_objects() -> int
extern func mas_gc_collections() -> int
func churn(n:int) -> int:
    var total = 0
    for i in 0..n:
        var s = "item-" + string(i)
        var xs = [s]
        var m = {s: s}
        total += len(xs) + len(m)
    mas_gc_collect()
    print(mas_gc_live_objects() < 40)
    return total
print(churn(10000))
mas_gc_collect()
print(mas_gc_live_objects())
print(churn(1000000))
mas_gc_collect()
print(mas_gc_live_objects())
print(mas_gc_collections() > 10)
"#,
    );
    assert_eq!(output, "true\n20000\n0\ntrue\n2000000\n0\ntrue\n");
}

/// 强制在嵌套函数内收集，验证实参、半构造容器、返回值、数组和隐式 keys 根。
#[test]
fn roots_survive_nested_calls_and_iteration() {
    let output = run(
        "roots",
        r#"
extern func mas_gc_collect()
func value(n:int) -> string:
    mas_gc_collect()
    return "v" + string(n)
func pair(a:string, b:string) -> string:
    mas_gc_collect()
    return a + b
func make() -> list<string>:
    return [value(1), value(2)]
func fixed() -> array<string,2>:
    return array<string,2>(value(7), value(8))
func nested(n:int) -> string:
    var s = value(n)
    if n == 0:
        return s
    return s + nested(n - 1)
func main():
    print(pair(value(3), value(4)))
    var xs = make()
    var alias = xs
    var part = xs[0:1]
    xs = [value(5)]
    mas_gc_collect()
    print(alias[1])
    print(part[0])
    var m = {value(1): value(2), value(3): value(4)}
    for k, v in m:
        mas_gc_collect()
        print(k + v)
    var a = fixed()
    mas_gc_collect()
    print(a[0] + a[1])
    print(nested(2))
    for i in 0..100:
        var temporary = value(i)
        if i < 99:
            continue
        break
    print(alias[0])
"#,
    );
    assert_eq!(output, "v3v4\nv2\nv1\nv1v2\nv3v4\nv7v8\nv2v1v0\nv1\n");
}

/// 全局覆盖不能回收仍在表达式中使用的旧值，复合赋值也必须保护旧左值。
#[test]
fn protects_values_when_callee_overwrites_global() {
    let output = run(
        "globals",
        r#"
extern func mas_gc_collect()
var global = "old" + "!"
func replace() -> string:
    global = "new" + "!"
    mas_gc_collect()
    return "tail"
func pair(a:string, b:string) -> string:
    mas_gc_collect()
    return a + b
print(pair(global, replace()))
global = "again" + "!"
global += replace()
print(global)
"#,
    );
    assert_eq!(output, "old!tail\nagain!tail\n");
}
