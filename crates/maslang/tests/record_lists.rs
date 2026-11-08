use maslang::optimization::OptLevel;
use maslang::settings::CompilerSettings;
use maslang::source::SourceFile;
use maslang::temp_directory::TemporaryDirectory;
use maslang::{CompileOptions, analyze_source, compile_to_executable};
use std::process::Command;

/// 在 O0/O3 编译并执行真实程序，覆盖引用列表全链路及强制 GC，断言业务输出。
#[test]
fn record_lists_execute_and_survive_gc() {
    let text = r#"
extern func mas_gc_collect()
extern func mas_gc_live_objects() -> int
struct Person:
    name: string
    age: int?
struct Group:
    people: list<Person>
struct Node:
    children: list<Node>
var calls = 0
func person(n: int) -> Person:
    calls += 1
    mas_gc_collect()
    return Person("user" + string(n), n)
func build() -> list<Person>:
    return [person(1), person(2)]
func total(people: list<Person>) -> int:
    var sum = 0
    for p in people:
        mas_gc_collect()
        sum += p.age.value
    return sum
func cycle():
    var n = Node([])
    n.children.append(n)
    mas_gc_collect()
    print(len(n.children[0].children))
func run():
    var g = Group(build())
    mas_gc_collect()
    print(calls)
    print(g.people[0].name)
    g.people.append(person(3))
    var alias = g.people[0]
    g.people[0].age = 10
    print(alias.age.value)
    print(g.people.contains(alias))
    print(g.people.contains(Person("user1", 10)))
    print(g.people.index(alias))
    print(total(g.people))
    var sliced = g.people[0:2]
    g.people[0] = person(4)
    g.people.reverse()
    print(g.people[0].name)
    g.people.remove(g.people[1])
    var last = g.people.pop()
    var first = g.people.pop(0)
    mas_gc_collect()
    print(last.name)
    print(first.name)
    print(len(g.people))
    print(sliced[0].age.value)
    print(sliced[1].name)
    var empty: list<Person> = []
    empty.append(last)
    var optional: list<Person>? = empty
    print(optional.value[0].name)
    var inferred = [alias]
    print(inferred[0].age.value)
    cycle()
func main():
    run()
    mas_gc_collect()
    print(mas_gc_live_objects())
"#;
    for level in [OptLevel::O0, OptLevel::O3] {
        let root = TemporaryDirectory::create("record-lists").unwrap();
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
        .unwrap_or_else(|e| panic!("{}", e.render(settings.language, Some(&source))));
        let output = Command::new(binary).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "2\nuser1\n10\ntrue\nfalse\n0\n15\nuser3\nuser4\nuser3\n0\n10\nuser2\nuser4\n10\n1\n0\n"
        );
    }
}

/// 类型不同的结构体、错误元素写入及没有定义的排序必须在语义阶段被拒绝。
#[test]
fn record_list_errors_are_diagnosed() {
    let prefix = "struct A:\n    x: int\nstruct B:\n    x: int\n";
    for (body, expected) in [
        ("var xs = [A(1), B(2)]", "元素类型必须保持一致"),
        ("var xs: list<A> = []\nxs.append(B(1))", "参数类型不匹配"),
        ("var xs = [A(1)]\nxs[0] = B(2)", "类型"),
        ("var xs = [A(1)]\nxs.sort()", "没有默认排序规则"),
        ("var xs: list<Missing> = []", "未知类型"),
    ] {
        let source = SourceFile::new("<record-lists>", format!("{prefix}{body}\n"));
        let settings = CompilerSettings::default_settings();
        let error = analyze_source(&source, &settings).unwrap_err();
        assert!(
            error
                .render(
                    maslang::settings::UiLanguage::ZhCn,
                    Some(&source)
                )
                .contains(expected),
            "{error:?}"
        );
    }
}
