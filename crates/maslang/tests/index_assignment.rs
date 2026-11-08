use maslang::optimization::OptLevel;
use maslang::settings::{CompilerSettings, UiLanguage};
use maslang::source::SourceFile;
use maslang::temp_directory::TemporaryDirectory;
use maslang::{CompileOptions, analyze_source, compile_to_executable};
use std::process::{Command, Output};

/// 编译并执行指定优化等级的 MS 源码，返回真实进程输出；临时文件由目录守卫清理。
fn execute(text: &str, level: OptLevel) -> Output {
    let root = TemporaryDirectory::create("mascript-index-assignment").unwrap();
    let source = SourceFile::new(
        root.path().join("main.ms").display().to_string(),
        text.to_string(),
    );
    let settings = CompilerSettings::new(UiLanguage::ZhCn);
    let binary = compile_to_executable(
        &source,
        &settings,
        &CompileOptions::new(Some(root.path().join("main.exe"))).with_opt_level(level),
    )
    .unwrap_or_else(|e| panic!("{}", e.render(settings.language, Some(&source))));
    Command::new(binary).output().unwrap()
}

/// 验证四类元素、动态下标、列表别名/传参、数组值复制/传参和常量绑定的元素更新；长度必须保持不变。
#[test]
fn updates_preserve_container_semantics() {
    let source = r#"
var xs = [1, 2, 3]
var alias = xs
func update(values:list<int>):
    values[1] = 42
var a = array<int,3>(1,2,3)
var copy = a
func local(values:array<int,3>) -> int:
    values[0] = 99
    return values[0]
func main():
    update(xs)
    xs[0] = xs[1] + 1
    print(alias[0])
    print(alias[1])
    print(len(xs))
    a[1] = 7
    print(local(a))
    print(a[0])
    print(a[1])
    print(copy[1])
    print(len(a))
    var zero:array<int,2>
    zero[1] = 8
    print(zero[1])
    const fixed = array<int,1>(0)
    fixed[0] = 4
    print(fixed[0])
    var fs = [1.0]
    fs[0] = 2.5
    print(fs[0] == 2.5)
    var bs = [false]
    bs[0] = true
    print(bs[0])
    var ss = ["old"]
    ss[0] = "new" + string(1)
    print(ss[0])
    var fa = array<float,1>(1.0)
    fa[0] = 2.5
    print(fa[0] == 2.5)
    var ba = array<bool,1>(false)
    ba[0] = true
    print(ba[0])
    var sa = array<string,1>("old")
    sa[0] = "new" + string(2)
    print(sa[0])
    var map = {1:2}
    map[1] = 3
    print(map[1])
"#;
    for level in [OptLevel::O0, OptLevel::O2, OptLevel::O3] {
        let output = execute(source, level);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "43\n42\n3\n99\n1\n7\n2\n3\n8\n4\ntrue\ntrue\nnew1\ntrue\ntrue\nnew2\n3\n"
        );
    }
}

/// 下标和右值各执行一次；右值扩容仍写入同一列表，字符串经强制 GC 后保留，覆盖的旧字符串可回收。
#[test]
fn evaluation_order_and_gc_are_safe() {
    let source = r#"
extern func mas_gc_collect()
extern func mas_gc_live_objects() -> int
var xs = [0]
var calls = 0
func index() -> int:
    calls += 1
    print(1)
    return 0
func value() -> int:
    print(2)
    xs.append(8)
    return 7
func strings():
    var ss = ["old" + string(0)]
    var a = array<string,1>("old" + string(1))
    for i in 0..1000:
        ss[0] = "list" + string(i)
        a[0] = "array" + string(i)
    mas_gc_collect()
    print(ss[0])
    print(a[0])
func main():
    xs[index()] = value()
    print(calls)
    print(xs[0])
    print(xs[1])
    strings()
    mas_gc_collect()
    print(mas_gc_live_objects())
"#;
    for level in [OptLevel::O0, OptLevel::O3] {
        let output = execute(source, level);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "1\n2\n1\n7\n8\nlist999\narray999\n1\n"
        );
    }
}

/// 负数、长度位置、极大下标与空列表写入均必须失败；右值缩短列表后也不可写入已移除的槽位。
#[test]
fn invalid_indices_fail_before_writing() {
    for declaration in ["var x = [1]", "var x = array<int,1>(1)"] {
        for index in ["-1", "1", "9223372036854775807"] {
            let output = execute(&format!("{declaration}\nfunc main():\n    x[{index}] = 2\n"), OptLevel::O2);
            assert!(!output.status.success(), "{output:?}");
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("越界"),
                "{output:?}"
            );
        }
    }
    for source in [
        "var x:list<int> = []\nfunc main():\n    x[0] = 1\n",
        "var x = [1]\nfunc shrink() -> int:\n    return x.pop()\nfunc main():\n    x[0] = shrink()\n",
    ] {
        let output = execute(source, OptLevel::O2);
        assert!(!output.status.success(), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("越界"));
    }
}

/// 在语义阶段拒绝非整数序列下标、错误元素类型及非容器目标，避免错误 LLVM 类型进入后端。
#[test]
fn assignment_types_are_checked() {
    let settings = CompilerSettings::new(UiLanguage::ZhCn);
    for (text, message) in [
        ("var x = [1]\nx[true] = 2\n", "下标必须是整数"),
        ("var x = array<int,1>(1)\nx[0.0] = 2\n", "下标必须是整数"),
        ("var x = [1]\nx[0] = true\n", "元素类型"),
        ("var x = array<int,1>(1)\nx[0] = \"bad\"\n", "元素类型"),
        ("var x = \"str\"\nx[0] = 1\n", "只支持列表、数组或字典"),
    ] {
        let source = SourceFile::new("invalid.ms", text.to_string());
        let error = analyze_source(&source, &settings)
            .unwrap_err()
            .render(settings.language, Some(&source));
        assert!(error.contains(message), "{error}");
    }
}
