use maslang::optimization::OptLevel;
use maslang::settings::{CompilerSettings, UiLanguage};
use maslang::source::SourceFile;
use maslang::temp_directory::TemporaryDirectory;
use maslang::{CompileOptions, analyze_source, compile_to_executable};
use std::process::{Command, Output};

/// 编译真实可执行文件并运行；编译错误显示中文诊断，测试目录在进程退出后清理。
fn execute(text: &str, level: OptLevel) -> Output {
    let root = TemporaryDirectory::create("mascript-records").unwrap();
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

/// 覆盖递归结构、前向类型、字段修改、引用别名、可空返回/参数/标量与强制 GC 下存活。
#[test]
fn records_nullable_and_gc_execute() {
    let text = r#"
extern func mas_gc_collect()
extern func mas_gc_live_objects() -> int
struct Node:
    item: int
    left: Node?
    right: Node?
struct User:
    name: string
    age: int?
    ok: bool
    value: string
    values: list<int>
    fixed: array<string,2>
func build(depth: int) -> Node:
    if depth == 0:
        return Node(1, null, null)
    return Node(1, build(depth - 1), build(depth - 1))
func count(node: Node?) -> int:
    if node == null:
        return 0
    var n = node.value
    return n.item + count(n.left) + count(n.right)
func optional(found: bool) -> int?:
    if found:
        return 42
    return null
func read(n: int?) -> int:
    if n != null:
        return n.value
    return -1
func run():
    var tree = build(9)
    mas_gc_collect()
    print(count(tree))
    var alias = tree
    alias.item += 3
    print(tree.item)
    tree.left.value.item = 8
    print(tree.left.value.item)
    var u = User("hello" + string(9), null, true, "v", [], array<string,2>("a", "b"))
    u.age = optional(true)
    u.values.append(7)
    u.fixed[0] = "updated" + string(1)
    mas_gc_collect()
    print(u.name)
    print(u.age.value)
    print(u.ok)
    print(u.value)
    print(u.values[0])
    print(u.fixed[0])
    u.age = null
    print(u.age == null)
    var xs: list<int>? = []
    xs = []
    xs.value.append(6)
    var mapping: map<string,int>? = {}
    var arr: array<string,2>? = array<string,2>("a" + string(1), "b")
    mas_gc_collect()
    arr.value[0] = "a" + string(2)
    print(xs.value[0])
    print(len(mapping.value))
    print(arr.value[0])
    var zero: int? = 0
    var flag: bool? = false
    var fraction: float? = 2.5
    var empty: string? = ""
    mas_gc_collect()
    print(zero != null)
    print(zero.value)
    print(flag.value)
    print(fraction.value)
    print(len(empty.value))
    print(read(5))
    print(read(null))
    print(optional(false) == null)
    tree.left = tree
    tree.right = null
run()
mas_gc_collect()
print(mas_gc_live_objects())
"#;
    for level in [OptLevel::O0, OptLevel::O3] {
        let output = execute(text, level);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "1023\n4\n8\nhello9\n42\ntrue\nv\n7\nupdated1\ntrue\n6\n0\na2\ntrue\n0\nfalse\n2.5\n0\n5\n-1\ntrue\n0\n"
        );
    }
}

/// 空值取值必须报告语言错误而不是访问违规；逻辑短路必须避免执行空值读取。
#[test]
fn null_access_reports_source_and_short_circuit_is_safe() {
    for level in [OptLevel::O0, OptLevel::O3] {
        let output = execute(
            "var n: int? = null\nprint(n != null and n.value > 0)\nprint(n.value)\n",
            level,
        );
        assert!(!output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "false\n");
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            error.contains("null") && error.contains("main.ms"),
            "{error}"
        );
    }
}

/// 非空类型、字段名、字段数量、可空运算和类型推断错误必须在执行前被拒绝。
#[test]
fn invalid_records_are_rejected() {
    let cases = [
        "var xs = [null]",
        "struct int:\n    x: int",
        "var n: int = null",
        "var n = null",
        "var n: int? = null\nprint(n + 1)",
        "struct A:\n    x: int\nvar a = A(null)",
        "struct A:\n    x: int\nvar a = A()",
        "struct A:\n    x: int\nvar a = A(1)\nprint(a.missing)",
        "struct A:\n    x: int\nvar a: A? = null\nprint(a.x)",
        "struct A:\n    x: int\n    x: int",
        "struct A:\n    x: Unknown",
        "struct A:\n    x: int\nvar a = A(1)\na.x = false",
        "struct A:\n    x: int\nfunc f():\n    struct B:\n        x: int",
        "func f() -> int?:\n    return null\nvar n: int = f()",
    ];
    for text in cases {
        let source = SourceFile::new("<record-negative>", format!("{text}\n"));
        assert!(
            analyze_source(&source, &CompilerSettings::default_settings()).is_err(),
            "accepted {text}"
        );
    }
}

/// 公开结构体、别名导入及重导出的字段/参数/返回类型应指向同一模块内部类型。
#[test]
fn imported_records_keep_type_identity() {
    let root = TemporaryDirectory::create("mascript-record-import").unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("mod.yml"),
        "name: demo\nversion: 0.1.0\ndependencies: {}\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/model.ms"),
        "pub struct Person:\n    age: int?\npub func make() -> Person:\n    return Person(18)\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/api.ms"),
        "export import local.model { Person, make }\n",
    )
    .unwrap();
    let text = "import local.api { Person as User, make }\nfunc read(p: User) -> int?:\n    return p.age\nvar p: User = make()\np.age = 20\nprint(read(p).value)\nvar people: list<User> = [p, make()]\npeople.append(make())\nprint(people[1].age.value)\nprint(len(people))\n";
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
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "20\n18\n3\n");
}

/// 字段接收者只执行一次；右侧强制回收及重绑全局后仍写回原对象，混合数值复合赋值遵循已有规则。
#[test]
fn field_evaluation_preserves_receiver_and_numeric_rules() {
    let text = r#"
extern func mas_gc_collect()
struct Box:
    numbers: array<int,2>
    amount: float
var box = Box(array<int,2>(1,2), 1.5)
var old = box
var calls = 0
func target() -> Box:
    calls += 1
    return box
func rhs() -> int:
    box = Box(array<int,2>(8,9), 0.0)
    mas_gc_collect()
    return 7
target().numbers[0] = rhs()
old.amount += 2
print(calls)
print(old.numbers[0])
print(box.numbers[0])
print(old.amount)
"#;
    for level in [OptLevel::O0, OptLevel::O3] {
        let output = execute(text, level);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "1\n7\n8\n3.5\n");
    }
}
