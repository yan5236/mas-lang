use maslang::optimization::OptLevel;
use maslang::settings::CompilerSettings;
use maslang::source::SourceFile;
use maslang::{CompileOptions, compile_to_executable};
use std::process::{Command, Output};

/// 使用当前编译器生成并运行 EXE；失败时渲染完整编译诊断，运行后由临时目录守卫清理。
/// 目录通过 `TemporaryDirectory` 原子创建（PID + 计数器），避免并发用例共用目录导致链接中间文件互相覆盖。
fn execute(text: &str, level: OptLevel) -> Output {
    let workspace = maslang::temp_directory::TemporaryDirectory::create("mascript-float-sort").unwrap();
    let root = workspace.path();
    let source = SourceFile::new(root.join("main.ms").display().to_string(), text.to_string());
    let settings = CompilerSettings::new(maslang::settings::UiLanguage::ZhCn);
    let binary = compile_to_executable(
        &source,
        &settings,
        &CompileOptions::new(Some(root.join("main.exe"))).with_opt_level(level),
    )
    .unwrap_or_else(|e| panic!("{}", e.render(settings.language, Some(&source))));
    let output = Command::new(binary).output().unwrap();
    output
}

/// 交接文档给出的真实输入：混合重复值、正零与多个 NaN，排序必须全序完成且后续代码可达。
#[test]
fn float_sort_places_nan_last_and_keeps_numbers_ascending() {
    let text = r#"func main():
    var xs = [4.0, 2.0, 4.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0, 1.0, 3.0, 2.0, float("NaN"), 3.0, 3.0, 2.0, 1.0, 2.0, float("NaN"), float("NaN"), 3.0, 4.0, 3.0, 0.0, float("NaN"), 0.0, 4.0, 0.0, 0.0, float("NaN"), 4.0, 1.0, 4.0, 3.0, 2.0, 0.0, 1.0, float("NaN"), float("NaN"), 2.0]
    xs.sort()
    for value in xs:
        print(value)
    print(42)
"#;
    let expected = format!(
        "{}{}{}{}{}{}42\n",
        "0\n".repeat(9),
        "1\n".repeat(5),
        "2\n".repeat(7),
        "3\n".repeat(6),
        "4\n".repeat(6),
        "NaN\n".repeat(7),
    );
    for level in [OptLevel::O0, OptLevel::O3] {
        let output = execute(text, level);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected, "{level:?}");
    }
}

/// 数值顺序保持升序：负无穷、负零/正零（相等且稳定）、正无穷在 NaN 之前。
#[test]
fn float_sort_orders_infinities_and_signed_zero_before_nan() {
    let text = r#"func main():
    var xs = [float("NaN"), 3.0, float("-inf"), 0.0, -0.0, float("inf")]
    xs.sort()
    for value in xs:
        print(value)
    var nan_only = [float("NaN"), float("NaN")]
    nan_only.sort()
    print(len(nan_only))
    var empty: list<float> = []
    empty.sort()
    print(len(empty))
"#;
    for level in [OptLevel::O0, OptLevel::O3] {
        let output = execute(text, level);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "-inf\n0\n-0\n3\ninf\nNaN\n2\n0\n",
            "{level:?}"
        );
    }
}

/// 排序后列表长度、元素类型与后续索引访问保持可用，确认比较器没有破坏容器状态。
#[test]
fn float_sort_keeps_container_usable_afterwards() {
    let text = r#"func main():
    var xs = [2.0, float("NaN"), 1.0]
    xs.sort()
    print(len(xs))
    print(xs[0])
    print(xs[1])
    xs.append(0.5)
    xs.sort()
    print(xs[0])
    print(xs.contains(float("NaN")))
"#;
    let output = execute(text, OptLevel::O3);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "3\n1\n2\n0.5\nfalse\n"
    );
}
