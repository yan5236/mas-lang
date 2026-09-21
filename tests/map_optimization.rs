use mascript_compiler_llvm::optimization::OptLevel;
use mascript_compiler_llvm::settings::CompilerSettings;
use mascript_compiler_llvm::source::SourceFile;
use mascript_compiler_llvm::temp_directory::TemporaryDirectory;
use mascript_compiler_llvm::{CompileOptions, compile_to_executable};
use std::process::{Command, Output};

/// 经正式编译/链接链路生成并执行程序，返回退出状态和输出；临时产物随守卫自动清理。
fn execute(text: &str, level: OptLevel) -> Output {
    let root = TemporaryDirectory::create("mascript-map-optimization").unwrap();
    let source = SourceFile::new(
        root.path().join("main.ms").display().to_string(),
        text.into(),
    );
    let settings = CompilerSettings::default_settings();
    let binary = compile_to_executable(
        &source,
        &settings,
        &CompileOptions::new(Some(root.path().join("main.exe"))).with_opt_level(level),
    )
    .unwrap_or_else(|error| panic!("{}", error.render(settings.language, Some(&source))));
    Command::new(binary).output().unwrap()
}

/// 在 O0/O2/O3 实测完整词频、稠密/稀疏混合键、别名、压缩重插及条件分配/回边的 GC 安全。
#[test]
fn maps_and_gc_preserve_results_at_all_levels() {
    let source = r#"
extern func mas_gc_collect()
extern func mas_gc_live_objects() -> int
func count_words() -> int:
    var counts:map<string,int> = {}
    for i in 0..100000:
        var key = "词" + string(i % 37)
        if counts.contains(key):
            counts[key] = counts[key] + 1
        else:
            counts[key] = 1
        if i % 257 == 0:
            mas_gc_collect()
    var total = 0
    for i in 0..37:
        var key = "词" + string(i)
        total += counts[key]
    return total
func exercise():
    var table:map<int,string> = {}
    table[100] = "sparse"
    table[-1] = "negative"
    table[9223372036854775807] = "huge"
    for i in 0..256:
        if i != 100:
            table[i] = "value" + string(i)
    var alias = table
    alias[100] = "updated"
    for i in 0..240:
        table.remove(i)
    table[100] = "reinserted"
    mas_gc_collect()
    print(table[-1])
    print(table[9223372036854775807])
    print(table[255])
    print(table[100])
    print(len(table))
    var keys = table.keys()
    print(keys[0])
    print(keys[len(keys)-1])
    var values:map<int,string> = {}
    for i in 0..10000:
        if i % 3 == 0:
            values[0] = "branch" + string(i)
        else:
            var current = values[0]
            if i % 17 == 0:
                mas_gc_collect()
    print(values[0])
print(count_words())
exercise()
mas_gc_collect()
print(mas_gc_live_objects())
"#;
    for level in [OptLevel::O0, OptLevel::O2, OptLevel::O3] {
        let output = execute(source, level);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "100000\nnegative\nhuge\nvalue255\nreinserted\n19\n-1\n100\nbranch9999\n0\n"
        );
    }
}

/// 直接索引空洞与删除后的键仍须报缺失错误，优化不能把空槽或旧位置当作有效值。
#[test]
fn missing_integer_keys_keep_source_diagnostics() {
    for source in [
        "var m:map<int,int> = {}\nm[2] = 7\nprint(m[1])\n",
        "var m = {2:7}\nm.remove(2)\nprint(m[2])\n",
    ] {
        for level in [OptLevel::O0, OptLevel::O2, OptLevel::O3] {
            let output = execute(source, level);
            assert_eq!(output.status.code(), Some(1));
            let error = String::from_utf8(output.stderr).unwrap();
            assert!(error.contains("字典中不存在目标键"), "{error}");
            assert!(error.contains("main.ms:3:"), "{error}");
        }
    }
}

/// 栈键写入后必须独立存活；覆盖四类值、负数和整数极值、keys 逃逸、别名、压缩及强制 GC。
#[test]
fn borrowed_keys_are_owned_before_insertion() {
    let source = r#"
extern func mas_gc_collect()
extern func mas_gc_live_objects() -> int
func build() -> map<string,int>:
    var m:map<string,int> = {}
    for i in 0..300:
        var key = string(i - 150)
        m[key] = i
        mas_gc_collect()
    return m
func retained_keys() -> list<string>:
    var m:map<string,int> = {}
    for i in 0..80:
        var key = string(i)
        m[key] = i
    return m.keys()
func extremes(m:map<string,int>, n:int):
    var key = string(n)
    m[key] = 7
func other_values():
    var floats:map<string,float> = {}
    var bools:map<string,bool> = {}
    var texts:map<string,string> = {}
    for i in 0..10000:
        var key = string(i % 3)
        floats[key] = 1.5
        bools[key] = true
        texts[key] = "值" + string(i)
        if i % 29 == 0:
            mas_gc_collect()
    print(floats["0"])
    print(bools["1"])
    print(texts["2"])
func exercise():
    var m = build()
    var alias = m
    for i in 0..260:
        alias.remove(string(i - 150))
    for i in 0..260:
        var key = string(i - 150)
        alias[key] = i + 1000
    extremes(m, -9223372036854775807 - 1)
    extremes(m, 9223372036854775807)
    mas_gc_collect()
    print(m["-150"])
    print(m["149"])
    print(m["-9223372036854775808"])
    print(m["9223372036854775807"])
    print(len(m))
    var keys = m.keys()
    print(keys[0])
    print(keys[40])
    var detached = retained_keys()
    mas_gc_collect()
    print(detached[0])
    print(detached[79])
    other_values()
exercise()
mas_gc_collect()
print(mas_gc_live_objects())
"#;
    for level in [OptLevel::O0, OptLevel::O2, OptLevel::O3] {
        let output = execute(source, level);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "1000\n299\n7\n7\n302\n110\n-150\n0\n79\n1.5\ntrue\n值9998\n0\n"
        );
    }
}

/// 返回、列表/字典值、别名、重赋值与函数实参都可能延长键寿命；这些路径必须保留普通托管字符串。
#[test]
fn escaping_string_conversions_keep_their_lifetimes() {
    let source = r#"
extern func mas_gc_collect()
var saved = ""
var calls = 0
func next() -> int:
    calls += 1
    return calls
func returned(n:int) -> string:
    var key = string(n)
    return key
func capture(s:string):
    saved = s
func exercise():
    var values:list<string> = []
    var m:map<string,string> = {}
    for i in 0..2000:
        var key = string(i)
        values.append(key)
        m[key] = key
    for i in 0..10:
        var key = string(i)
        capture(key)
    var result = returned(-17)
    var key = string(30)
    var alias = key
    key = string(31)
    mas_gc_collect()
    print(result)
    print(values[0])
    print(values[1999])
    print(m["57"])
    print(saved)
    print(alias)
    print(key)
    var counted:map<string,int> = {}
    for i in 0..20:
        var key = string(next())
        counted[key] = 1
    print(calls)
    print(len(counted))
exercise()
"#;
    for level in [OptLevel::O0, OptLevel::O2, OptLevel::O3] {
        let output = execute(source, level);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "-17\n0\n1999\n57\n9\n30\n31\n20\n20\n"
        );
    }
}

/// 溢出 intrinsic 在优化开关两端都保留边界值与错误诊断；动态调用覆盖加减乘，不允许回绕。
#[test]
fn checked_arithmetic_preserves_boundaries_at_all_levels() {
    let good = r#"
func add(a:int,b:int) -> int:
    return a+b
func sub(a:int,b:int) -> int:
    return a-b
func mul(a:int,b:int) -> int:
    return a*b
var high = int("9223372036854775807")
var low = int("-9223372036854775808")
print(add(high - 1,1))
print(sub(low + 1,1))
print(sub(low,-1))
print(mul(low,1))
print(mul(int("-3037000499"),3037000499))
print(mul(low,0))
"#;
    for level in [OptLevel::O0, OptLevel::O2, OptLevel::O3] {
        let output = execute(good, level);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "9223372036854775807\n-9223372036854775808\n-9223372036854775807\n-9223372036854775808\n-9223372030926249001\n0\n"
        );
        for (op, a, b) in [
            ("+", "9223372036854775807", "1"),
            ("+", "-9223372036854775808", "-1"),
            ("-", "-9223372036854775808", "1"),
            ("-", "9223372036854775807", "-1"),
            ("*", "-9223372036854775808", "-1"),
            ("*", "3037000500", "3037000500"),
        ] {
            let source = format!(
                "func calc(a:int,b:int) -> int:\n    return a {op} b\nprint(calc(int(\"{a}\"),int(\"{b}\")))\n"
            );
            let output = execute(&source, level);
            assert_eq!(output.status.code(), Some(1), "{source}: {output:?}");
            let error = String::from_utf8(output.stderr).unwrap();
            assert!(
                error.contains("整数溢出") && error.contains("main.ms:2:"),
                "{error}"
            );
        }
    }
}
