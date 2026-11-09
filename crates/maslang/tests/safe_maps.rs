use maslang::optimization::OptLevel;
use maslang::settings::{CompilerSettings, UiLanguage};
use maslang::source::SourceFile;
use maslang::temp_directory::TemporaryDirectory;
use maslang::{CompileOptions, compile_to_executable};
use std::process::Command;

/// 用默认优化级（O2）编译并运行真实 EXE，断言正常退出后返回标准输出。
fn run(text: &str) -> String {
    run_at(text, OptLevel::default())
}

/// 指定优化档位编译并运行真实 EXE；编译失败展开完整诊断，非零退出附带原始输出。
fn run_at(text: &str, level: OptLevel) -> String {
    let root = TemporaryDirectory::create("mascript-safe-maps").unwrap();
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
    .unwrap_or_else(|error| panic!("{}", error.render(settings.language, Some(&source))));
    let output = Command::new(binary).output().unwrap();
    assert!(output.status.success(), "程序以非零状态退出：{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

/// 普通字符串键路径：缺键返回 `ok=false` 与占位值，命中返回真实值，失败后程序继续执行。
#[test]
fn string_keys_separate_missing_from_hit() {
    let stdout = run(r#"
func main():
    var scores: map<string,int> = {"a": 1, "b": 2}
    var missing = scores.try_get("missing")
    print(missing.ok)
    print(missing.value)
    var hit = scores.try_get("b")
    print(hit.ok)
    print(hit.value)
    print(len(scores))
    print("继续执行")
"#);
    assert_eq!(stdout, "false\n0\ntrue\n2\n2\n继续执行\n");
}

/// 可空值路径：缺键与“键存在但值为 null”必须区分开，命中实值仍能解出载荷。
#[test]
fn nullable_values_distinguish_missing_key_from_null_value() {
    let stdout = run(r#"
func main():
    var cached: map<string,int?> = {"today": null, "count": 7}
    var today = cached.try_get("today")
    print(today.ok)
    print(today.value == null)
    var missing = cached.try_get("tomorrow")
    print(missing.ok)
    print(missing.value == null)
    var count = cached.try_get("count")
    print(count.ok)
    print(count.value == null)
    if count.ok:
        print(count.value.value)
    else:
        print(-1)
    print(len(cached))
"#);
    assert_eq!(stdout, "true\ntrue\nfalse\ntrue\ntrue\nfalse\n7\n2\n");
}

/// 整型键优化路径（`map<int,int>` 特化读写）：负数、稀疏大键与键域两端边界都必须命中，
/// 缺失的密集/稀疏键返回失败，删除同样沿用特化入口。
#[test]
fn integer_keys_cover_negative_sparse_and_boundary_values() {
    let stdout = run(r#"
func main():
    var table: map<int,int> = {}
    table[0] = 100
    table[3] = 300
    table[-7] = -700
    table[9223372036854775807] = 1
    table[-9223372036854775808] = 2
    var negative = table.try_get(-7)
    print(negative.ok)
    print(negative.value)
    var huge = table.try_get(9223372036854775807)
    print(huge.ok)
    print(huge.value)
    var minimum = table.try_get(-9223372036854775808)
    print(minimum.ok)
    print(minimum.value)
    var absent = table.try_get(4)
    print(absent.ok)
    print(absent.value)
    print(table.try_remove(4))
    print(table.try_remove(-7))
    print(len(table))
    print(table.try_get(-7).ok)
    print(table.try_get(0).value)
"#);
    assert_eq!(
        stdout,
        "true\n-700\ntrue\n1\ntrue\n2\nfalse\n0\nfalse\ntrue\n4\nfalse\n100\n"
    );
}

/// 结构体键按对象身份匹配：同一对象命中，同名字段的新对象未命中，删除也按身份判定。
#[test]
fn struct_reference_keys_match_by_identity() {
    let stdout = run(r#"
struct Person:
    name: string

var alice = Person("Alice")
func main():
    var scores: map<Person,int> = {alice: 90}
    var hit = scores.try_get(alice)
    print(hit.ok)
    print(hit.value)
    var other = Person("Alice")
    var missing = scores.try_get(other)
    print(missing.ok)
    print(missing.value)
    print(scores.try_remove(other))
    print(len(scores))
    print(scores.try_remove(alice))
    print(len(scores))
    print(scores.contains(alice))
"#);
    assert_eq!(stdout, "true\n90\nfalse\n0\nfalse\n1\ntrue\n0\nfalse\n");
}

/// 删除缺失键返回 `false` 且字典不变；命中删除返回 `true`，重新插入的键追加到末尾。
#[test]
fn try_remove_reports_failure_and_preserves_the_map() {
    let stdout = run(r#"
func main():
    var m: map<string,int> = {"a": 1, "b": 2, "c": 3}
    print(m.try_remove("b"))
    print(len(m))
    print(m.contains("b"))
    print(m.try_remove("b"))
    print(len(m))
    print(m["a"])
    print(m["c"])
    m["b"] = 9
    var keys = m.keys()
    print(len(keys))
    print(keys[0])
    print(keys[1])
    print(keys[2])
"#);
    assert_eq!(stdout, "true\n2\nfalse\nfalse\n2\n1\n3\n3\na\nc\nb\n");
}

/// 反复失败不改变字典，也不中断后续语句；循环复用同一槽位仍然得到正确结果。
#[test]
fn repeated_failures_keep_the_map_untouched() {
    let stdout = run(r#"
func main():
    var m: map<string,int> = {"x": 1}
    var failures = 0
    for i in 0..50:
        if m.try_remove("missing") == false:
            failures += 1
        if m.try_get("missing").ok == false:
            failures += 1
    print(failures)
    print(len(m))
    print(m.contains("x"))
    print(m["x"])
    print("依然继续")
"#);
    assert_eq!(stdout, "100\n1\ntrue\n1\n依然继续\n");
}

/// `try_result<string>`/`try_result<结构体>` 的载荷必须以字段 1 的地址参与 GC 保活：
/// 命中后先删除键让字典不再引用载荷，再强制收集，仍然能读出原值。
#[test]
fn result_payloads_survive_collection_without_the_map() {
    let source = r#"
extern func mas_gc_collect()

struct Person:
    name: string

func main():
    var texts: map<string,string> = {}
    for i in 0..200:
        texts[string(i)] = "值" + string(i)
    var kept = texts.try_get("150")
    print(kept.ok)
    texts.try_remove("150")
    mas_gc_collect()
    print(kept.value)
    print(len(texts))

    var people: map<string,Person> = {}
    for i in 0..200:
        people[string(i)] = Person("人" + string(i))
    var person = people.try_get("150")
    print(person.ok)
    people.try_remove("150")
    mas_gc_collect()
    print(person.value.name)
    print(len(people))

    var nested: map<string,list<int>> = {}
    nested["a"] = [1, 2, 3]
    var kept_list = nested.try_get("a")
    print(kept_list.ok)
    nested.try_remove("a")
    mas_gc_collect()
    print(kept_list.value[2])
    print("保活完成")
"#;
    let expected = "true\n值150\n199\ntrue\n人150\n199\ntrue\n3\n保活完成\n";
    for level in [OptLevel::O0, OptLevel::default()] {
        assert_eq!(run_at(source, level), expected, "优化级 {}", level.as_str());
    }
}

/// 缺键占位值必须自身安全：字符串占位是有效空串，可继续做长度和比较而不解引用空指针。
#[test]
fn missing_string_value_is_a_valid_empty_string() {
    let stdout = run(r#"
func main():
    var texts: map<string,string> = {"a": "alpha"}
    var missing = texts.try_get("b")
    print(missing.ok)
    print(len(missing.value))
    print(missing.value == "")
    var hit = texts.try_get("a")
    print(hit.ok)
    print(hit.value)
"#);
    assert_eq!(stdout, "false\n0\ntrue\ntrue\nalpha\n");
}
