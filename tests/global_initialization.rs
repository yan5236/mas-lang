use mascript_compiler_llvm::settings::{CompilerSettings, UiLanguage};
use mascript_compiler_llvm::source::SourceFile;
use mascript_compiler_llvm::{CompileOptions, compile_to_executable};
use std::process::{Command, Output};

/// 将测试源码编译并运行真实 EXE，返回退出状态及输出，随后删除本用例目录。
fn execute(text: &str) -> Output {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("mascript-global-{unique}"));
    std::fs::create_dir_all(&root).unwrap();
    let source = SourceFile::new(root.join("main.ms").display().to_string(), text.to_string());
    let binary = compile_to_executable(
        &source,
        &CompilerSettings::new(UiLanguage::ZhCn),
        &CompileOptions::new(Some(root.join("main.exe"))),
    )
    .unwrap();
    let output = Command::new(binary).output().unwrap();
    std::fs::remove_dir_all(root).unwrap();
    output
}

/// 间接读取未初始化全局列表、字典或标量应有源码诊断，不能崩溃或读到伪造零值。
#[test]
fn rejects_reads_before_global_declaration() {
    for (initializer, expression, result_type) in [
        ("[1, 2]", "len(xs)", "int"),
        ("{1: 2}", "len(xs)", "int"),
        ("42", "xs", "int"),
        ("\"hello\"", "xs", "string"),
    ] {
        let text = format!(
            "func get() -> {result_type}:\n    return {expression}\nfunc indirect() -> {result_type}:\n    return get()\nprint(indirect())\nvar xs = {initializer}\n"
        );
        let output = execute(&text);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("全局变量尚未初始化"), "{stderr}");
        assert!(stderr.contains("main.ms:2:"), "{stderr}");
        assert!(output.stdout.is_empty());
    }
}

/// 普通赋值和复合赋值均不可绕过声明；初始化表达式内间接自读也必须被拒绝。
#[test]
fn rejects_writes_and_recursive_initialization() {
    for assignment in ["xs = 3", "xs += 1"] {
        let output = execute(&format!(
            "func set():\n    {assignment}\nset()\nvar xs = 1\n"
        ));
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("全局变量尚未初始化"));
    }
    let output = execute("func get() -> int:\n    return xs\nvar xs = get()\n");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("全局变量尚未初始化"));
}

/// 已初始化全局允许正常读写，局部同名参数不会被未初始化全局的标记误伤。
#[test]
fn initialized_globals_and_shadowed_parameters_work() {
    let output = execute(
        "func local(xs:int) -> int:\n    return xs\nprint(local(7))\nvar xs = 2\nfunc update():\n    xs += 3\nupdate()\nprint(xs)\nvar values = [1, 2]\nfunc count() -> int:\n    return len(values)\nprint(count())\n",
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "7\n5\n2\n");
}
