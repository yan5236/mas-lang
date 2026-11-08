use maslang::optimization::OptLevel;
use maslang::settings::CompilerSettings;
use maslang::source::SourceFile;
use maslang::temp_directory::TemporaryDirectory;
use maslang::{CompileOptions, compile_to_executable};
use std::process::Command;

/// 编译运行容器组合，覆盖可空基础值比较、结构体键身份、数组 GC 和嵌套空容器的类型提示。
#[test]
fn composite_containers_preserve_values_and_roots() {
    let text = r#"
extern func mas_gc_collect()
extern func mas_gc_live_objects() -> int
struct Person:
    name: string
struct Holder:
    items: array<Person?,2>
func make() -> Person:
    return Person("person" + string(1))
func run():
    var first: int? = 1
    var inferred = [first, 2, null]
    var inferred_map = {"a": first, "b": 3, "c": null}
    print(inferred[1].value + inferred_map["b"].value)
    var ints: list<int?> = [null, 2]
    ints.append(3)
    ints[0] = 1
    print(ints.contains(2))
    print(ints.index(3))
    ints.remove(2)
    ints.append(null)
    print(ints.pop() == null)
    print(ints.pop().value)
    var texts: list<string?> = [null, "a" + string(1)]
    print(texts.contains("a1"))
    print(texts.contains(null))
    var floats: list<float?> = [null, 1.5]
    var flags: list<bool?> = [null, false]
    print(floats.contains(1.5))
    print(flags.contains(false))
    var p = make()
    var people: list<Person?> = [null, p]
    people[0] = make()
    print(people.contains(p))
    var holder = Holder(array<Person?,2>(null, make()))
    holder.items[0] = p
    var nested = array<array<Person,1>,1>(array<Person,1>(make()))
    nested[0][0] = Person("nested")
    var by_name: map<string,Person?> = {"a": p, "b": null}
    by_name["c"] = make()
    by_name["b"] = p
    var by_person: map<Person,string> = {p: "hello" + string(2)}
    p.name = "changed"
    print(by_person.contains(p))
    print(by_person.contains(make()))
    print(by_person[p])
    var same: map<Person,Person> = {p: make()}
    var numbers: map<Person,int> = {p: 4}
    var fractions: map<Person,float> = {p: 2.5}
    var booleans: map<Person,bool> = {p: true}
    var by_int: map<int,Person> = {1: make()}
    var by_float: map<float,Person> = {1.5: make()}
    var by_bool: map<bool,Person> = {true: make()}
    var optional: map<string,int?> = {"a": null, "b": 7}
    optional["a"] = 8
    var matrix: list<list<int?>> = [[], [null, 9]]
    matrix.append([])
    matrix[0] = [10]
    var maps: map<string,list<Person>> = {"team": []}
    maps["team"].append(p)
    var snapshot = by_name.values()
    by_name.remove("c")
    mas_gc_collect()
    print(holder.items[1].value.name)
    print(nested[0][0].name)
    print(snapshot[2].value.name)
    print(same[p].name)
    print(numbers[p])
    print(fractions[p])
    print(booleans[p])
    print(by_int[1].name)
    print(by_float[1.5].name)
    print(by_bool[true].name)
    print(optional["a"].value)
    print(optional["b"].value)
    print(matrix[1][1].value)
    print(matrix[0][0].value)
    print(maps["team"][0].name)
    print(by_person.keys()[0].name)
    by_person.remove(p)
    print(len(by_person))
run()
mas_gc_collect()
print(mas_gc_live_objects())
"#;
    for level in [OptLevel::O0, OptLevel::O3] {
        let root = TemporaryDirectory::create("composite-containers").unwrap();
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
            "5\ntrue\n2\ntrue\n3\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\nfalse\nhello2\nperson1\nnested\nperson1\nperson1\n4\n2.5\ntrue\nperson1\nperson1\nperson1\n8\n7\n9\n10\nchanged\nchanged\n0\n0\n"
        );
    }
}
