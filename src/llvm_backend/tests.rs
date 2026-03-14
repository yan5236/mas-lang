use super::{compile_program, compile_program_to_ir};
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::settings::{CompilerSettings, UiLanguage};
use crate::source::SourceFile;

/// 先做前端分析，再把程序交给 LLVM 后端生成测试产物。
fn parse_program(text: &str) -> (SourceFile, crate::ast::Program, CompilerSettings) {
    let settings = CompilerSettings::new(UiLanguage::ZhCn);
    let source = SourceFile::new("<test>", text.to_string());
    let tokens = Lexer::new(&source, &settings).tokenize().unwrap();
    let program = Parser::new(tokens, &settings).parse_program().unwrap();
    (source, program, settings)
}

/// 校验 LLVM 后端能够生成非空目标文件。
#[test]
fn should_emit_object_bytes() {
    let (source, program, settings) = parse_program("print(1)\n");
    let object = compile_program(&source, &settings, &program).unwrap();
    assert!(!object.object_bytes.is_empty());
}

/// 校验 `print` 调用会绑定到正确的运行时符号。
#[test]
fn should_emit_print_runtime_symbol() {
    let (source, program, _) = parse_program("print(1)\n");
    let ir = compile_program_to_ir(&source, &program).unwrap();
    assert!(ir.contains("print_int"));
}

/// 校验字符串拼接路径会引用 `string_concat` 运行时符号。
#[test]
fn should_emit_string_concat_runtime_symbol() {
    let (source, program, _) = parse_program("print(\"a\" + \"b\")\n");
    let ir = compile_program_to_ir(&source, &program).unwrap();
    assert!(ir.contains("string_concat"));
}

/// 校验字符串相等比较路径会引用 `string_equals` 运行时符号。
#[test]
fn should_emit_string_equals_runtime_symbol() {
    let (source, program, _) = parse_program("print(\"a\" == \"b\")\n");
    let ir = compile_program_to_ir(&source, &program).unwrap();
    assert!(ir.contains("string_equals"));
}

/// 校验顶层变量与同名局部变量共存时，后端仍会同时生成全局与局部存储。
#[test]
fn should_keep_top_level_and_local_storage_separate() {
    let (source, program, _) = parse_program(
        "var value:int = 1\nfunc main():\n    var value:int = 2\n    print(value)\nprint(value)\n",
    );
    let ir = compile_program_to_ir(&source, &program).unwrap();
    assert!(ir.contains("__ms_global_value_0"));
    assert!(ir.contains("alloca i64"));
}

/// 校验 `if / elif / else` 仍会生成条件分支与汇合块。
#[test]
fn should_emit_if_blocks() {
    let (source, program, _) = parse_program(
        "if true:\n    print(1)\nelif false:\n    print(2)\nelse:\n    print(3)\n",
    );
    let ir = compile_program_to_ir(&source, &program).unwrap();
    assert!(ir.contains("if.then"));
    assert!(ir.contains("if.after"));
}

/// 校验 `while` 与 `continue` 仍会生成循环头与退出块。
#[test]
fn should_emit_while_blocks() {
    let (source, program, _) = parse_program(
        "var value:int = 0\nwhile value < 3:\n    value += 1\n    if value == 2:\n        continue\n    print(value)\n",
    );
    let ir = compile_program_to_ir(&source, &program).unwrap();
    assert!(ir.contains("while.header"));
    assert!(ir.contains("while.exit"));
}

/// 校验 `for range` 仍会生成迭代头、继续块与退出块。
#[test]
fn should_emit_for_range_blocks() {
    let (source, program, _) = parse_program("for i in 0..3:\n    print(i)\n");
    let ir = compile_program_to_ir(&source, &program).unwrap();
    assert!(ir.contains("for.header"));
    assert!(ir.contains("for.continue"));
    assert!(ir.contains("for.exit"));
}

/// 校验整数提升到浮点数、浮点比较、整数除法与取模路径仍保持原先指令选择。
#[test]
fn should_emit_numeric_and_compare_instructions() {
    let (source, program, _) = parse_program(
        "var left:int = 1\nvar right:float = 2.5\nvar dividend:int = 7\nvar divisor:int = 2\nprint(left + right)\nprint(left < right)\nprint(dividend %/% divisor)\nprint(dividend % divisor)\n",
    );
    let ir = compile_program_to_ir(&source, &program).unwrap();
    assert!(ir.contains("fadd double"));
    assert!(ir.contains("fcmp olt double"));
    assert!(ir.contains("sdiv i64"));
    assert!(ir.contains("srem i64"));
}

/// 校验布尔与字符串打印仍会分派到各自的运行时符号。
#[test]
fn should_dispatch_print_runtime_by_value_type() {
    let (source, program, _) = parse_program("print(true)\nprint(\"text\")\n");
    let ir = compile_program_to_ir(&source, &program).unwrap();
    assert!(ir.contains("print_bool"));
    assert!(ir.contains("print_string"));
}
