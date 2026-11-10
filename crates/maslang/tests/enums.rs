use maslang::optimization::OptLevel;
use maslang::settings::{CompilerSettings, UiLanguage};
use maslang::source::SourceFile;
use maslang::temp_directory::TemporaryDirectory;
use maslang::{CompileOptions, analyze_source, compile_to_executable};
use std::process::{Command, Output};

/// 编译真实可执行文件并运行；编译错误显示中文诊断，测试目录在进程退出后清理。
fn execute(text: &str, level: OptLevel) -> Output {
    let root = TemporaryDirectory::create("mascript-enums").unwrap();
    let source = SourceFile::new(
        root.path().join("main.ms").display().to_string(),
        text.into(),
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

/// 只做前端分析并返回渲染后的诊断文本，用于断言带源码位置的拒绝消息。
fn analyze_error(text: &str) -> String {
    let source = SourceFile::new("<enum-negative>", format!("{text}\n"));
    let settings = CompilerSettings::new(UiLanguage::ZhCn);
    analyze_source(&source, &settings)
        .expect_err("程序应被拒绝")
        .render(settings.language, Some(&source))
}

/// 覆盖声明、成员比较、函数参数/返回、结构体字段、可空枚举、全局与局部变量。
#[test]
fn enums_declare_compare_and_lower_to_ordinals() {
    let text = r#"
extern func mas_gc_collect()
pub enum Color:
    Red
    Green
    Blue
struct Pixel:
    color: Color
    fallback: Color?
var global_color: Color = Color.Blue
func index_of(color: Color) -> int:
    if color == Color.Red:
        return 0
    if color == Color.Green:
        return 1
    return 2
func pick(want: Color) -> Color:
    return want
func main():
    var color = Color.Green
    print(color == Color.Green)
    print(color != Color.Red)
    print(index_of(color))
    print(index_of(pick(Color.Blue)))
    print(index_of(global_color))
    var pixel = Pixel(Color.Red, null)
    print(index_of(pixel.color))
    print(pixel.fallback == null)
    pixel.fallback = Color.Blue
    mas_gc_collect()
    print(index_of(pixel.fallback.value))
    pixel.color = Color.Green
    mas_gc_collect()
    print(index_of(pixel.color))
    var nullable: Color? = null
    print(nullable == null)
    nullable = Color.Red
    mas_gc_collect()
    print(index_of(nullable.value))
"#;
    for level in [OptLevel::O0, OptLevel::O3] {
        let output = execute(text, level);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "true\ntrue\n1\n2\n2\n0\ntrue\n2\n1\ntrue\n0\n"
        );
    }
}

/// 同一枚举的不同成员可比较，且只按声明顺序的相等性参与判断。
#[test]
fn enum_members_are_distinct_but_comparable() {
    let text = r#"
enum Mode:
    First
    Second
    Third
func main():
    print(Mode.First == Mode.First)
    print(Mode.First == Mode.Second)
    print(Mode.Second != Mode.Third)
    print(Mode.Third == Mode.Third)
    var mode = Mode.Second
    print(mode == Mode.Second)
    print(mode == Mode.First)
"#;
    let output = execute(text, OptLevel::O0);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "true\nfalse\ntrue\ntrue\ntrue\nfalse\n"
    );
}

/// 重复成员、空枚举、未知成员和未知枚举都必须在执行前被拒绝。
#[test]
fn invalid_enum_declarations_are_rejected() {
    let cases = [
        ("enum Mode:\n    Read\n    Read", "重复"),
        ("enum Mode:", "枚举至少需要一个成员"),
        (
            "pub enum FileMode:\n    Read\nfunc main():\n    var mode = FileMode.Bogus",
            "没有成员",
        ),
        (
            "pub enum FileMode:\n    Read\nfunc main():\n    var mode = UnknownEnum.Read",
            "不是已声明的枚举",
        ),
        ("enum int:\n    Read", "枚举名称"),
        (
            "func main():\n    enum Inner:\n        Read",
            "只能出现在顶层",
        ),
    ];
    for (text, expected) in cases {
        let error = analyze_error(text);
        assert!(
            error.contains(expected),
            "期望包含 `{expected}`，实际诊断：{error}"
        );
    }
}

/// 跨枚举比较、枚举与整数比较、隐式整数转换和打印枚举都必须报错。
#[test]
fn enum_type_rules_reject_implicit_conversions() {
    let cases = [
        (
            "enum A:\n    One\nenum B:\n    One\nfunc main():\n    print(A.One == B.One)",
            "不同枚举类型",
        ),
        (
            "enum A:\n    One\nfunc main():\n    print(A.One == 1)",
            "只能与同一枚举类型比较",
        ),
        (
            "enum A:\n    One\nvar value: A = 1",
            "类型与初始化表达式不匹配",
        ),
        (
            "enum A:\n    One\nvar number: int = A.One",
            "类型与初始化表达式不匹配",
        ),
        (
            "enum A:\n    One\nfunc main():\n    print(int(A.One))",
            "只支持基础类型转换",
        ),
        (
            "enum A:\n    One\nfunc main():\n    print(A.One)",
            "不接受枚举值",
        ),
        (
            "enum A:\n    One\nfunc consume(value: A) -> int:\n    return 0\nfunc main():\n    print(consume(1))",
            "参数类型不匹配",
        ),
        (
            "enum A:\n    One\nfunc make() -> A:\n    return 1\nfunc main():\n    print(0)",
            "返回值类型不匹配",
        ),
        (
            "enum A:\n    One\nfunc main():\n    var values: list<A> = [A.One]",
            "容器元素暂不支持枚举",
        ),
        (
            "enum A:\n    One\nfunc main():\n    var value = A()",
            "不能当作结构体构造",
        ),
    ];
    for (text, expected) in cases {
        let error = analyze_error(text);
        assert!(
            error.contains(expected),
            "期望包含 `{expected}`，实际诊断：{error}"
        );
    }
}

/// 公开枚举支持导入、别名和 `export import` 重导出，成员序号在模块间保持一致。
#[test]
fn imported_enums_share_nominal_identity() {
    let root = TemporaryDirectory::create("mascript-enum-import").unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("settings.toml"),
        "[project]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/model.ms"),
        "pub enum FileMode:\n    Read\n    Write\n    Append\n    ReadWrite\n\npub func mode_code(mode: FileMode) -> int:\n    if mode == FileMode.Read:\n        return 10\n    if mode == FileMode.Write:\n        return 20\n    if mode == FileMode.Append:\n        return 30\n    return 40\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/api.ms"),
        "export import local.model { FileMode as Mode, mode_code as code }\n",
    )
    .unwrap();
    let text = r#"import local.model { FileMode }
import local.api { Mode, code }
struct Config:
    mode: FileMode
    previous: Mode?
func main():
    var config = Config(FileMode.Append, null)
    print(code(config.mode))
    print(config.previous == null)
    config.previous = Mode.ReadWrite
    print(code(config.previous.value))
    var alias: Mode = FileMode.Write
    print(code(alias))
    print(FileMode.Read == Mode.Read)
"#;
    let path = root.path().join("src/main.ms");
    std::fs::write(&path, text).unwrap();
    let source = SourceFile::new(path.display().to_string(), text.into());
    let settings = CompilerSettings::default_settings();
    let binary = compile_to_executable(
        &source,
        &settings,
        &CompileOptions::new(Some(root.path().join("main.exe"))),
    )
    .unwrap_or_else(|e| panic!("{}", e.render(settings.language, Some(&source))));
    let output = Command::new(binary).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "30\ntrue\n40\n20\ntrue\n"
    );
}

/// 两个模块各自声明同名枚举、成员名和序号相同时，仍不能互相赋值或比较。
#[test]
fn cross_module_same_named_enums_are_not_interchangeable() {
    let root = TemporaryDirectory::create("mascript-enum-nominal").unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("settings.toml"),
        "[project]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/a.ms"),
        "pub enum Mode:\n    On\n    Off\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/b.ms"),
        "pub enum Mode:\n    On\n    Off\n",
    )
    .unwrap();
    let path = root.path().join("src/main.ms");
    let settings = CompilerSettings::new(UiLanguage::ZhCn);
    for text in [
        "import local.a { Mode as AMode }\nimport local.b { Mode as BMode }\nfunc main():\n    var value: AMode = BMode.On\n    print(0)\n",
        "import local.a { Mode as AMode }\nimport local.b { Mode as BMode }\nfunc main():\n    print(AMode.On == BMode.On)\n",
    ] {
        std::fs::write(&path, text).unwrap();
        let source = SourceFile::new(path.display().to_string(), text.into());
        let error = analyze_source(&source, &settings)
            .expect_err("跨模块同名枚举不应互通")
            .render(settings.language, Some(&source));
        assert!(
            error.contains("AMode") || error.contains("BMode") || error.contains("枚举"),
            "诊断应指向名义身份差异：{error}"
        );
        assert!(
            !error.contains("__ms_"),
            "诊断不应泄漏内部重写前缀：{error}"
        );
    }
}
