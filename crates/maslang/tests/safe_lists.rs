use maslang::optimization::OptLevel;
use maslang::settings::CompilerSettings;
use maslang::source::SourceFile;
use maslang::temp_directory::TemporaryDirectory;
use maslang::{CompileOptions, compile_to_executable};
use std::process::{Command, Output};

/// 编译并运行真实 EXE；临时工程由 TemporaryDirectory 自动清理，编译失败时输出完整诊断。
fn execute(text: &str, level: OptLevel) -> Output {
    let root = TemporaryDirectory::create("safe-lists").unwrap();
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

/// 在 O0 与默认（O3）两个优化级别下运行同一程序，断言退出码为零且标准输出完全一致。
fn assert_output(text: &str, expected: &str) {
    for level in [OptLevel::O0, OptLevel::O3] {
        let output = execute(text, level);
        assert!(output.status.success(), "{level:?}: {output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            expected,
            "{level:?}"
        );
    }
}

/// 列表下标读写越界、负下标都返回失败结果，容器内容与后续代码不受影响。
#[test]
fn list_try_get_and_try_set_are_recoverable() {
    let text = r#"
func main():
    var xs = [10, 20, 30]
    var first = xs.try_get(0)
    print(first.ok)
    print(first.value)
    var out = xs.try_get(3)
    print(out.ok)
    print(out.value)
    var negative = xs.try_get(-1)
    print(negative.ok)
    print(xs.try_set(1, 99))
    print(xs[1])
    print(xs.try_set(3, 1))
    print(xs.try_set(-1, 1))
    print(len(xs))
    print(xs[0])
    print(xs[1])
    print(xs[2])
    print("done")
"#;
    assert_output(
        text,
        "true\n10\nfalse\n0\nfalse\ntrue\n99\nfalse\nfalse\n3\n10\n99\n30\ndone\n",
    );
}

/// `try_set` 失败后容器保持原值；空列表的失败写入不影响之后的追加。
#[test]
fn failed_try_set_keeps_container_unchanged() {
    let text = r#"
func main():
    var xs: list<int> = []
    print(xs.try_set(0, 7))
    print(xs.try_set(-1, 7))
    print(len(xs))
    xs.append(1)
    print(xs[0])
    print(xs.try_set(1, 7))
    print(len(xs))
    print(xs.try_set(0, 5))
    print(xs[0])
    print("done")
"#;
    assert_output(text, "false\nfalse\n0\n1\nfalse\n1\ntrue\n5\ndone\n");
}

/// `try_set` 在右值求值之后按当前长度判断下标，与下标赋值的求值顺序一致。
#[test]
fn try_set_checks_length_after_evaluating_value() {
    let text = r#"
func grow(xs: list<int>) -> int:
    xs.append(99)
    return 5
func main():
    var xs = [1]
    print(xs.try_set(1, grow(xs)))
    print(len(xs))
    print(xs[1])
    var ys = [1]
    print(ys.try_set(5, grow(ys)))
    print(len(ys))
    print("done")
"#;
    assert_output(text, "true\n2\n5\nfalse\n2\ndone\n");
}

/// 空列表弹出、越界弹出都返回 `ok=false` 且不删除元素；失败后容器仍可正常增长。
#[test]
fn list_try_pop_recovers_from_empty_and_out_of_range() {
    let text = r#"
func main():
    var xs: list<int> = []
    var empty = xs.try_pop()
    print(empty.ok)
    print(len(xs))
    xs.append(1)
    xs.append(2)
    var last = xs.try_pop()
    print(last.ok)
    print(last.value)
    var tail = xs.try_pop(5)
    print(tail.ok)
    print(len(xs))
    var head = xs.try_pop(0)
    print(head.ok)
    print(head.value)
    print(len(xs))
    var again = xs.try_pop(0)
    print(again.ok)
    print(len(xs))
    xs.append(7)
    print(len(xs))
    print(xs[0])
    print("done")
"#;
    assert_output(
        text,
        "false\n0\ntrue\n2\nfalse\n1\ntrue\n1\n0\nfalse\n0\n1\n7\ndone\n",
    );
}

/// 非法切片（倒序、越界、负下标）返回 `ok=false` 且不改动原列表；合法切片复制元素槽位。
#[test]
fn list_try_slice_rejects_invalid_ranges() {
    let text = r#"
func main():
    var xs = [1, 2, 3, 4]
    var mid = xs.try_slice(1, 3)
    print(mid.ok)
    print(len(mid.value))
    print(mid.value[0])
    print(mid.value[1])
    var whole = xs.try_slice(0, 4)
    print(whole.ok)
    print(len(whole.value))
    var blank = xs.try_slice(2, 2)
    print(blank.ok)
    print(len(blank.value))
    print(xs.try_slice(3, 1).ok)
    print(xs.try_slice(0, 5).ok)
    print(xs.try_slice(-1, 2).ok)
    print(xs.try_slice(0, -1).ok)
    print(len(xs))
    print(xs[0])
    print(xs[3])
    xs.append(5)
    print(len(xs))
    print("done")
"#;
    assert_output(
        text,
        "true\n2\n2\n3\ntrue\n4\ntrue\n0\nfalse\nfalse\nfalse\nfalse\n4\n1\n4\n5\ndone\n",
    );
}

/// `try_index`/`try_remove` 未命中返回失败结果且不删除元素，命中时删除第一个匹配项。
#[test]
fn list_try_index_and_try_remove_report_misses() {
    let text = r#"
func main():
    var xs = [5, 6, 7, 6]
    var hit = xs.try_index(6)
    print(hit.ok)
    print(hit.value)
    var miss = xs.try_index(9)
    print(miss.ok)
    print(miss.value)
    print(xs.try_remove(9))
    print(len(xs))
    print(xs.try_remove(6))
    print(len(xs))
    print(xs[1])
    print(xs[2])
    print(xs.try_remove(6))
    print(len(xs))
    print(xs.try_index(6).ok)
    print("done")
"#;
    assert_output(
        text,
        "true\n1\nfalse\n0\nfalse\n4\ntrue\n3\n7\n6\ntrue\n2\nfalse\ndone\n",
    );
}

/// 可空元素的 `ok` 区分“未命中”和“命中但值为 null”，可空结构体元素同样如此。
#[test]
fn nullable_elements_distinguish_miss_from_null() {
    let text = r#"
struct Person:
    name: string
func main():
    var xs: list<int?> = [1, null]
    var present = xs.try_get(0)
    print(present.ok)
    print(present.value.value)
    var null_hit = xs.try_get(1)
    print(null_hit.ok)
    print(null_hit.value == null)
    var miss = xs.try_get(2)
    print(miss.ok)
    var hit_index = xs.try_index(null)
    print(hit_index.ok)
    print(hit_index.value)
    print(xs.try_remove(null))
    print(len(xs))
    print(xs[0].value)
    var people: list<Person?> = [null, Person("a")]
    var person = people.try_get(1)
    print(person.ok)
    print(person.value.value.name)
    var null_person = people.try_get(0)
    print(null_person.ok)
    print(null_person.value == null)
    print("done")
"#;
    assert_output(
        text,
        "true\n1\ntrue\ntrue\nfalse\ntrue\n1\ntrue\n1\n1\ntrue\na\ntrue\ntrue\ndone\n",
    );
}

/// 标量元素（float/bool/string）的安全读取、写入、查找、删除与弹出一致工作。
#[test]
fn scalar_element_kinds_round_trip_through_safe_access() {
    let text = r#"
func main():
    var floats = [1.5, 2.5]
    var f = floats.try_get(1)
    print(f.ok)
    print(f.value)
    print(floats.try_get(2).ok)
    var flags = [true, false]
    var b = flags.try_get(1)
    print(b.ok)
    print(b.value)
    var texts = ["a", "b"]
    var s = texts.try_get(1)
    print(s.ok)
    print(s.value)
    print(texts.try_get(2).ok)
    print(texts.try_set(0, "z"))
    print(texts[0])
    print(texts.try_remove("z"))
    print(len(texts))
    print(texts.try_remove("missing"))
    var idx = texts.try_index("b")
    print(idx.ok)
    print(idx.value)
    var popped = texts.try_pop()
    print(popped.value)
    print("done")
"#;
    assert_output(
        text,
        "true\n2.5\nfalse\ntrue\nfalse\ntrue\nb\nfalse\ntrue\nz\ntrue\n1\nfalse\ntrue\n0\nb\ndone\n",
    );
}

/// `try_result<string>` 与 `try_slice` 得到的字符串列表在强制 GC 后仍可安全读取；
/// 弹出的字符串已从列表移除，只能靠结果槽的根保活。
#[test]
fn string_payloads_survive_gc() {
    let text = r#"
extern func mas_gc_collect()
func main():
    var texts = ["a" + string(1), "b" + string(2), "c" + string(3)]
    var part = texts.try_slice(0, 2)
    mas_gc_collect()
    print(part.ok)
    print(part.value[0])
    print(part.value[1])
    var one = texts.try_get(1)
    mas_gc_collect()
    print(one.ok)
    print(one.value)
    var miss = texts.try_get(3)
    print(miss.ok)
    var popped = texts.try_pop()
    mas_gc_collect()
    print(popped.value)
    print(len(texts))
    print("done")
"#;
    assert_output(text, "true\na1\nb2\ntrue\nb2\nfalse\nc3\n2\ndone\n");
}

/// 结构体元素、嵌套列表与嵌套字典的安全读取/切片/写入；强制 GC 后结果载荷仍可访问。
#[test]
fn struct_and_nested_container_elements_survive_gc() {
    let text = r#"
extern func mas_gc_collect()
struct Person:
    name: string
    age: int
struct Group:
    members: list<Person>
func make(n: int) -> Person:
    mas_gc_collect()
    return Person("user" + string(n), n)
func run():
    var group = Group([make(1), make(2)])
    mas_gc_collect()
    var hit = group.members.try_get(1)
    print(hit.ok)
    print(hit.value.name)
    mas_gc_collect()
    print(hit.value.name)
    var miss = group.members.try_get(9)
    print(miss.ok)
    var slice = group.members.try_slice(0, 1)
    mas_gc_collect()
    print(slice.ok)
    print(slice.value[0].name)
    print(group.members.try_set(0, make(3)))
    mas_gc_collect()
    print(group.members[0].name)
    var found = group.members.try_index(hit.value)
    print(found.ok)
    print(found.value)
    var popped = group.members.try_pop()
    mas_gc_collect()
    print(popped.ok)
    print(popped.value.name)
    print(popped.value.age)
    var rows: list<list<int>> = [[1, 2], [3]]
    var row = rows.try_get(0)
    mas_gc_collect()
    print(row.ok)
    print(len(row.value))
    print(row.value[1])
    var maps: list<map<string,int>> = [{"a": 7}]
    var entry = maps.try_get(0)
    mas_gc_collect()
    print(entry.ok)
    print(entry.value["a"])
    print(maps.try_get(1).ok)
    print(maps.try_set(0, {"b": 2}))
    print(maps[0]["b"])
    print(rows.try_set(0, [4, 5]))
    print(len(rows[0]))
    print(rows[0][1])
func main():
    run()
    mas_gc_collect()
"#;
    assert_output(
        text,
        "true\nuser2\nuser2\nfalse\ntrue\nuser1\ntrue\nuser3\ntrue\n1\ntrue\nuser2\n2\ntrue\n2\n2\ntrue\n7\nfalse\ntrue\n2\ntrue\n2\n5\n",
    );
}

/// 固定数组越界、负下标的读写返回失败结果且写失败保持原元素。
#[test]
fn array_try_get_and_try_set_are_recoverable() {
    let text = r#"
struct Holder:
    items: array<int,2>
func main():
    var arr = array<int,3>(10, 20, 30)
    var first = arr.try_get(0)
    print(first.ok)
    print(first.value)
    var out = arr.try_get(3)
    print(out.ok)
    print(out.value)
    print(arr.try_get(-1).ok)
    print(arr.try_set(1, 99))
    print(arr[1])
    print(arr.try_set(3, 1))
    print(arr.try_set(-1, 1))
    print(arr[1])
    print(len(arr))
    var names = array<string,2>("a", "b")
    var name = names.try_get(1)
    print(name.ok)
    print(name.value)
    print(names.try_get(2).ok)
    print(names.try_set(0, "z"))
    print(names[0])
    print(names.try_set(2, "z"))
    print(names[0])
    var holder = Holder(array<int,2>(1, 2))
    print(holder.items.try_set(0, 5))
    print(holder.items[0])
    print(holder.items.try_get(2).ok)
    print("done")
"#;
    assert_output(
        text,
        "true\n10\nfalse\n0\nfalse\ntrue\n99\nfalse\nfalse\n99\n3\ntrue\nb\nfalse\ntrue\nz\nfalse\nz\ntrue\n5\nfalse\ndone\n",
    );
}

/// 数组元素本身是数组时 `try_get` 返回嵌套数组值，`try_set` 直接写回元素槽位；
/// `array<list<int>,N>` 的托管元素在强制 GC 后仍可访问。
#[test]
fn nested_array_elements_and_managed_payloads() {
    let text = r#"
extern func mas_gc_collect()
func make_list(n: int) -> list<int>:
    mas_gc_collect()
    return [n]
func main():
    var grid = array<array<int, 2>, 2>(array<int, 2>(1, 2), array<int, 2>(3, 4))
    var row = grid.try_get(0)
    print(row.ok)
    print(row.value[1])
    var miss = grid.try_get(2)
    print(miss.ok)
    print(grid.try_set(1, array<int, 2>(7, 8)))
    print(grid[1][0])
    print(grid.try_set(2, array<int, 2>(7, 8)))
    print(grid[1][0])
    var rows = array<list<int>, 2>(make_list(1), make_list(2))
    var cell = rows.try_get(1)
    mas_gc_collect()
    print(cell.ok)
    print(cell.value[0])
    print(rows.try_set(0, make_list(9)))
    mas_gc_collect()
    print(rows[0][0])
    print(rows.try_get(2).ok)
"#;
    assert_output(text, "true\n2\nfalse\ntrue\n7\nfalse\n7\ntrue\n2\ntrue\n9\nfalse\n");
}

/// 没有可写地址的数组值（数组元素、函数返回的临时数组）在编译期被拒绝，
/// 而不是写进副本后仍报告 `true`。
#[test]
fn array_try_set_requires_writable_storage() {
    for body in [
        "var grid = array<array<int,2>,2>(array<int,2>(1,2), array<int,2>(3,4))\nfunc main():\n    print(grid[0].try_set(1, 9))\n",
        "func make() -> array<int,2>:\n    return array<int,2>(1,2)\nfunc main():\n    print(make().try_set(0, 9))\n",
    ] {
        let root = TemporaryDirectory::create("safe-lists-storage").unwrap();
        let source = SourceFile::new(
            root.path().join("main.ms").display().to_string(),
            body.into(),
        );
        let settings = CompilerSettings::default_settings();
        let error = compile_to_executable(
            &source,
            &settings,
            &CompileOptions::new(Some(root.path().join("main.exe"))),
        )
        .expect_err("数组写入位置缺失必须在编译期报告");
        let rendered = error.render(settings.language, Some(&source));
        assert!(rendered.contains("可写入的存储位置"), "{rendered}");
    }
}
