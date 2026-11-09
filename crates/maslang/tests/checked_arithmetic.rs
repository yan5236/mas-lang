use maslang::optimization::OptLevel;
use maslang::settings::{CompilerSettings, UiLanguage};
use maslang::source::SourceFile;
use maslang::temp_directory::TemporaryDirectory;
use maslang::{CompileOptions, analyze_source, compile_to_executable};
use std::fs;
use std::process::{Command, Output};

/// 受检整数内建必须在未优化、默认与最高档位下给出相同结果，避免常量折叠或指令提升掩盖危险分支。
fn outputs_at_all_levels(text: &str) -> Vec<(OptLevel, Output)> {
    [OptLevel::O0, OptLevel::O2, OptLevel::O3]
        .into_iter()
        .map(|level| (level, execute(text, level)))
        .collect()
}

/// 使用当前编译器生成并运行 EXE；编译失败时渲染完整诊断，运行后清理临时目录。
fn execute(text: &str, level: OptLevel) -> Output {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("mascript-checked-arith-{unique}"));
    std::fs::create_dir_all(&root).unwrap();
    let source = SourceFile::new(root.join("main.ms").display().to_string(), text.to_string());
    let settings = CompilerSettings::new(UiLanguage::ZhCn);
    let binary = compile_to_executable(
        &source,
        &settings,
        &CompileOptions::new(Some(root.join("main.exe"))).with_opt_level(level),
    )
    .unwrap_or_else(|e| panic!("{}", e.render(settings.language, Some(&source))));
    let output = Command::new(binary).output().unwrap();
    std::fs::remove_dir_all(root).unwrap();
    output
}

/// 断言 std 输出与状态码；任一档位不一致时打印档位和完整进程输出。
fn assert_output(text: &str, expected: &str) {
    for (level, output) in outputs_at_all_levels(text) {
        assert!(output.status.success(), "{level:?}: {output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            expected,
            "{level:?}"
        );
    }
}

/// 严格运算符保持原有语义：报告源码位置并以状态码 1 退出，不返回可判断结果。
fn assert_strict_failure(text: &str, message: &str) {
    for level in [OptLevel::O0, OptLevel::O3] {
        let output = execute(text, level);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{level:?}: {output:?}");
        assert!(stderr.contains(message), "{level:?}: {stderr}");
        assert!(stderr.contains("main.ms:"), "{level:?}: {stderr}");
        assert!(output.stdout.is_empty(), "{level:?}: {output:?}");
    }
}

/// 零除数与 `MIN %/% -1`、`MIN % -1` 都返回 `ok=false` 和占位 0，失败后程序继续执行。
#[test]
fn division_boundaries_report_failure_and_continue() {
    let text = r#"func check(a:int, b:int):
    var quotient = try_div(a, b)
    print(quotient.ok)
    print(quotient.value)
    var remainder = try_mod(a, b)
    print(remainder.ok)
    print(remainder.value)
func main():
    check(10, int("0"))
    check(int("-9223372036854775808"), -1)
    check(int("-9223372036854775808"), 1)
    check(-7, 3)
    print(42)
"#;
    let expected = format!(
        "{}true\n-9223372036854775808\ntrue\n0\ntrue\n-2\ntrue\n-1\n42\n",
        "false\n0\n".repeat(4),
    );
    assert_output(text, &expected);
}

/// 加减乘溢出与 `MIN` 取负都只在结果中报告失败，不中止进程，失败负载为 0。
#[test]
fn arithmetic_overflow_reports_failure_and_continues() {
    let text = r#"func add_report(a:int, b:int):
    var result = try_add(a, b)
    print(result.ok)
    print(result.value)
func sub_report(a:int, b:int):
    var result = try_sub(a, b)
    print(result.ok)
    print(result.value)
func mul_report(a:int, b:int):
    var result = try_mul(a, b)
    print(result.ok)
    print(result.value)
func neg_report(a:int):
    var result = try_neg(a)
    print(result.ok)
    print(result.value)
func main():
    add_report(int("9223372036854775807"), 1)
    sub_report(int("-9223372036854775808"), 1)
    mul_report(int("9223372036854775807"), 2)
    mul_report(int("-9223372036854775808"), -1)
    neg_report(int("-9223372036854775808"))
    print(7)
"#;
    let expected = format!("{}7\n", "false\n0\n".repeat(5));
    assert_output(text, &expected);
}

/// 合法边界继续给出准确结果：`MAX-1+1`、`MIN+1`、`MIN-(-1)`、负数截断除法和取余、`-MAX`。
#[test]
fn valid_boundaries_succeed() {
    let text = r#"func main():
    var maximum_minus_one = int("9223372036854775806")
    var minimum = int("-9223372036854775808")
    var sum = try_add(maximum_minus_one, 1)
    print(sum.ok)
    print(sum.value)
    var near_minimum = try_add(minimum, 1)
    print(near_minimum.ok)
    print(near_minimum.value)
    var shifted = try_sub(minimum, -1)
    print(shifted.ok)
    print(shifted.value)
    var quotient = try_div(-7, 3)
    print(quotient.ok)
    print(quotient.value)
    var remainder = try_mod(-7, 3)
    print(remainder.ok)
    print(remainder.value)
    var negated = try_neg(9223372036854775807)
    print(negated.ok)
    print(negated.value)
    var scaled = try_mul(maximum_minus_one, 1)
    print(scaled.ok)
    print(scaled.value)
"#;
    let expected = "true\n9223372036854775807\ntrue\n-9223372036854775807\ntrue\n-9223372036854775807\ntrue\n-2\ntrue\n-1\ntrue\n-9223372036854775807\ntrue\n9223372036854775806\n";
    assert_output(text, expected);
}

/// 循环内反复失败不终止程序，`.ok` 可驱动分支，表达式位置的结果同样可读取字段。
#[test]
fn failure_inside_loop_keeps_execution_alive() {
    let text = r#"func main():
    var minimum = int("-9223372036854775808")
    var caught = 0
    var index = 0
    while index < 3:
        var result = try_neg(minimum)
        if result.ok:
            print("unexpected")
        else:
            caught += 1
        index += 1
    print(caught)
    print(try_div(9, 3).value)
    print(try_div(9, 3).ok)
    print(99)
"#;
    assert_output(text, "3\n3\ntrue\n99\n");
}

/// 复合赋值入口仍走严格检查：溢出和除零继续报告源码位置并以状态码 1 退出。
#[test]
fn strict_compound_assignments_still_terminate() {
    for (text, message) in [
        (
            "var n = 9223372036854775807\nfunc main():\n    n += 1\n    print(n)\n",
            "整数溢出",
        ),
        (
            "var n = -9223372036854775808\nfunc main():\n    n -= 1\n    print(n)\n",
            "整数溢出",
        ),
        (
            "var n = 9223372036854775807\nfunc main():\n    n *= 2\n    print(n)\n",
            "整数溢出",
        ),
        (
            "var n = 3\nfunc main():\n    n %= 0\n    print(n)\n",
            "整数除零",
        ),
    ] {
        assert_strict_failure(text, message);
    }
}

/// 严格运算符本身仍是不可恢复错误：普通表达式与一元取负继续带源码位置退出。
#[test]
fn strict_operators_keep_existing_semantics() {
    for (text, message) in [
        (
            "func main():\n    print(9223372036854775807 + 1)\n",
            "整数溢出",
        ),
        (
            "func main():\n    print(int(\"-9223372036854775808\") - 1)\n",
            "整数溢出",
        ),
        (
            "func main():\n    print(9223372036854775807 * 2)\n",
            "整数溢出",
        ),
        (
            "func main():\n    print(-int(\"-9223372036854775808\"))\n",
            "整数溢出",
        ),
        (
            "func div(n:int) -> int:\n    return 10 %/% n\nfunc main():\n    print(div(int(\"0\")))\n",
            "整数除零",
        ),
        (
            "func rem(n:int) -> int:\n    return 10 % n\nfunc main():\n    print(rem(int(\"0\")))\n",
            "整数除零",
        ),
    ] {
        assert_strict_failure(text, message);
    }
}

/// 内建名称被保留：用户函数或结构体不能与 `try_*` 同名，避免定义被代码生成静默忽略。
#[test]
fn builtin_names_cannot_be_redefined() {
    for text in [
        "func try_add(a:int, b:int) -> int:\n    return 99\nfunc main():\n    print(try_add(1, 2))\n",
        "func try_print(value:int) -> int:\n    return 99\nfunc main():\n    print(try_print(1))\n",
        "func try_neg(value:int) -> int:\n    return 99\nfunc main():\n    print(try_neg(1))\n",
        "struct try_div:\n    value: int\nfunc main():\n    print(1)\n",
    ] {
        let source = SourceFile::new("main.ms", text.to_string());
        let settings = CompilerSettings::new(UiLanguage::ZhCn);
        let error = match analyze_source(&source, &settings) {
            Ok(_) => panic!("同名定义必须被拒绝：{text}"),
            Err(error) => error,
        };
        let rendered = error.render(settings.language, Some(&source));
        assert!(
            rendered.contains("内建名称") || rendered.contains("内建函数"),
            "{rendered}"
        );
    }
}

/// 危险指令只在合法分支执行：raw IR 中 `sdiv`/`srem` 必须位于条件分支后的 `checked.div.safe` 块。
#[test]
fn division_instructions_stay_on_the_legal_branch() {
    let root = TemporaryDirectory::create("mas-checked-ir").unwrap();
    let source = root.path().join("main.ms");
    fs::write(
        &source,
        "func probe(a:int, b:int):\n    var quotient = try_div(a, b)\n    print(quotient.ok)\n    var remainder = try_mod(a, b)\n    print(remainder.ok)\nfunc main():\n    probe(int(\"10\"), int(\"0\"))\n    probe(int(\"7\"), int(\"2\"))\n    print(1)\n",
    )
    .unwrap();
    for level in ["0", "2", "3"] {
        let dir = root.path().join(level);
        let build = Command::new(env!("CARGO_BIN_EXE_maslang"))
            .arg("build")
            .arg(&source)
            .arg(format!("-O{level}"))
            .arg("--emit-dir")
            .arg(&dir)
            .arg("-o")
            .arg(dir.join("main.exe"))
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let raw = fs::read_to_string(dir.join("main.raw.ll")).unwrap();
        let probe = raw
            .split("\ndefine ")
            .find(|body| {
                body.lines()
                    .next()
                    .is_some_and(|line| line.contains("@ms_user_probe("))
            })
            .expect("raw IR must contain the probe function");
        let lines: Vec<&str> = probe.lines().collect();
        for (opcode, builtin) in [(" = sdiv i64", "try_div"), (" = srem i64", "try_mod")] {
            let index = lines
                .iter()
                .position(|line| line.contains(opcode))
                .unwrap_or_else(|| panic!("`{builtin}` must emit `{opcode}`"));
            let branch = lines[..index]
                .iter()
                .rposition(|line| line.contains("br i1 %checked.div.usable"))
                .unwrap_or_else(|| panic!("`{builtin}` must follow a conditional branch"));
            let last_header = lines[branch..index]
                .iter()
                .copied()
                .filter(|line| !line.is_empty() && !line.starts_with(' ') && line.contains(':'))
                .next_back()
                .expect("block header before the division instruction");
            assert!(
                last_header.starts_with("checked.div.safe"),
                "`{builtin}` at -O{level} must run in the legal branch, found {last_header}"
            );
        }
    }
}
