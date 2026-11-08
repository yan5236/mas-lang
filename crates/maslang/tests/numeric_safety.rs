use maslang::settings::{CompilerSettings, UiLanguage};
use maslang::source::SourceFile;
use maslang::{CompileOptions, analyze_source, compile_to_executable};
use std::process::{Command, Output};

/// 校验混合排序在正负舍入边界、整数上界、分数、无穷和 NaN 上保留数学顺序。
#[test]
fn mixed_order_comparison_preserves_integer_precision() {
    let output = execute(
        "func main():\n    print(9007199254740993 > 9007199254740992.0)\n    print(9007199254740992.0 < 9007199254740993)\n    print(-9007199254740993 >= -9007199254740992.0)\n    print(9223372036854775807 < 9223372036854775808.0)\n    print(-9223372036854775808 >= -9223372036854775808.0)\n    print(0 <= -0.0)\n    print(2 < 2.5)\n    print(1 < (1.0 / 0.0))\n    print(1 >= (0.0 / 0.0))\n    print((0.0 / 0.0) >= 1)\n",
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        output.stdout,
        b"true\ntrue\nfalse\ntrue\ntrue\ntrue\ntrue\ntrue\nfalse\nfalse\n"
    );
}

/// 使用当前编译器生成并运行 EXE；清理本用例目录后返回输出，编译错误保留完整诊断。
fn execute(text: &str) -> Output {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("mascript-numeric-{unique}"));
    std::fs::create_dir_all(&root).unwrap();
    let source = SourceFile::new(root.join("main.ms").display().to_string(), text.to_string());
    let settings = CompilerSettings::new(UiLanguage::ZhCn);
    let binary = compile_to_executable(
        &source,
        &settings,
        &CompileOptions::new(Some(root.join("main.exe"))),
    )
    .unwrap_or_else(|e| panic!("{}", e.render(settings.language, Some(&source))));
    let output = Command::new(binary).output().unwrap();
    std::fs::remove_dir_all(root).unwrap();
    output
}

/// 断言危险运算以统一的语言错误退出并包含源码位置，不以宿主异常或任意值冒充结果。
fn fails_at_source(text: &str, message: &str) {
    let output = execute(text);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr.contains(message), "{stderr}");
    assert!(stderr.contains("main.ms:"), "{stderr}");
    assert!(output.stdout.is_empty(), "{output:?}");
}

/// 整数除法与取余在除数为零时均须语言诊断，包含函数参数和复合赋值入口。
#[test]
fn integer_zero_divisors_are_diagnosed() {
    for op in ["%/%", "%"] {
        fails_at_source(
            &format!("func div(n:int) -> int:\n    return 10 {op} n\nfunc main():\n    print(div(0))\n"),
            "整数除零",
        );
    }
    fails_at_source("var n = 3\nfunc main():\n    n %= 0\n    print(n)\n", "整数除零");
}

/// 固定数组拒绝上界、负数和极大动态下标，所有元素类型都应在读取前检查。
#[test]
fn array_bounds_are_checked() {
    for index in ["3", "5", "-1", "9223372036854775807"] {
        fails_at_source(
            &format!(
                "func read(i:int) -> int:\n    var arr = array<int,3>(10,20,30)\n    return arr[i]\nfunc main():\n    print(read({index}))\n"
            ),
            "数组下标越界",
        );
    }
    fails_at_source(
        "var arr = array<string,1>(\"hello\")\nfunc main():\n    print(arr[1])\n",
        "数组下标越界",
    );
}

/// 加减乘、取负及 MIN 除以负一均不得回绕，普通表达式与复合赋值共享检查。
#[test]
fn integer_overflow_is_diagnosed() {
    for expr in [
        "9223372036854775807 + 1",
        "int(\"-9223372036854775808\") - 1",
        "9223372036854775807 * 2",
        "-int(\"-9223372036854775808\")",
        "int(\"-9223372036854775808\") %/% -1",
        "int(\"-9223372036854775808\") % -1",
        "--9223372036854775808",
    ] {
        fails_at_source(&format!("func main():\n    print({expr})\n"), "整数溢出");
    }
    fails_at_source(
        "var n = 9223372036854775807\nfunc main():\n    n += 1\n    print(n)\n",
        "整数溢出",
    );
    for op in ["+", "*", "-"] {
        let left = if op == "-" {
            "-9223372036854775808"
        } else {
            "9223372036854775807"
        };
        fails_at_source(
            &format!(
                "func calc(a:int, b:int) -> int:\n    return a {op} b\nfunc main():\n    print(calc({left}, 2))\n"
            ),
            "整数溢出",
        );
    }
}

/// 非有限或越界浮点转换不得返回成功；强制转换报告位置，安全转换失败值保持零。
#[test]
fn float_integer_conversions_check_range() {
    let output = execute(
        "func check(x:float):\n    var r = try_int(x)\n    print(r.ok)\n    print(r.value)\nfunc main():\n    check(float(\"1e20\"))\n    check(float(\"-1e20\"))\n    check(float(\"NaN\"))\n    check(float(\"inf\"))\n    check(float(\"-inf\"))\n    check(9223372036854775808.0)\n    check(-9223372036854775808.0)\n    check(9223372036854774784.0)\n    check(-3.9)\n",
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "{}true\n-9223372036854775808\ntrue\n9223372036854774784\ntrue\n-3\n",
            "false\n0\n".repeat(6)
        )
    );
    for value in ["1e20", "NaN", "inf", "-inf"] {
        fails_at_source(
            &format!("func main():\n    print(int(float(\"{value}\")))\n"),
            "浮点数无法转换为整数",
        );
    }
}

/// NaN 与任意浮点值不等，普通值、无穷和正负零维持既有 IEEE 比较结果。
#[test]
fn nan_inequality_is_consistent() {
    let output = execute(
        "var n = 0.0 / 0.0\nfunc main():\n    print(n != 1.0)\n    print(n != n)\n    print(n == n)\n    print(float(\"inf\") == float(\"inf\"))\n    print(0.0 == -0.0)\n    print(1.5 != 1.5)\n",
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "true\ntrue\nfalse\ntrue\ntrue\nfalse\n"
    );
}

/// 最小整数字面量无精度损失地编译；越界正负字面量及减法右侧越界值仍被拒绝。
#[test]
fn minimum_integer_literal_is_exact() {
    let output = execute("func main():\n    print(-9223372036854775808)\n    print(9223372036854775807)\n");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "-9223372036854775808\n9223372036854775807\n"
    );
    for expression in [
        "9223372036854775808",
        "-9223372036854775809",
        "0 - 9223372036854775808",
        "18446744073709551616",
    ] {
        let source = SourceFile::new("literal.ms", format!("func main():\n    print({expression})\n"));
        assert!(
            analyze_source(&source, &CompilerSettings::new(UiLanguage::ZhCn)).is_err(),
            "{expression}"
        );
    }
}

/// 合法边界、负数截断和数组首尾读取继续产生准确结果。
#[test]
fn valid_arithmetic_and_array_edges_work() {
    let output = execute(
        "var arr = array<int,3>(10,20,30)\nfunc main():\n    print(arr[0])\n    print(arr[2])\n    print(9223372036854775806 + 1)\n    print(int(\"-9223372036854775808\") + 1)\n    print(-7 %/% 3)\n    print(-7 % 3)\n    print(7 * -3)\n    print(int(-3.9))\n",
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "10\n30\n9223372036854775807\n-9223372036854775807\n-2\n-1\n-21\n-3\n"
    );
}

/// 十进制输入先舍入为 f64；恰好 -2^63 仍应成功，下一可表示的更小浮点值必须失败。
#[test]
fn float_rounding_preserves_exact_minimum_contract() {
    let output = execute(
        "var minimum = -9223372036854775808.0\nfunc main():\n    print(-9223372036854776000.0 == minimum)\n    print(-9223372036854775809.0 == minimum)\n    print(int(-9223372036854776000.0))\n    var rounded = try_int(-9223372036854775809.0)\n    print(rounded.ok)\n    print(rounded.value)\n    var below = try_int(-9223372036854777856.0)\n    print(below.ok)\n    print(9223372036854775807.0 == 9223372036854775808.0)\n    var above = try_int(9223372036854775807.0)\n    print(above.ok)\n",
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "true\ntrue\n-9223372036854775808\ntrue\n-9223372036854775808\nfalse\ntrue\nfalse\n"
    );
    fails_at_source(
        "func main():\n    print(int(-9223372036854777856.0))\n",
        "浮点数无法转换为整数",
    );
}
