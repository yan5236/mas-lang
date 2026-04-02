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
    let (source, program, _) =
        parse_program("if true:\n    print(1)\nelif false:\n    print(2)\nelse:\n    print(3)\n");
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

/// 校验类型转换内建函数会引用对应的解析与格式化运行时符号。
#[test]
fn should_emit_convert_runtime_symbols() {
    let (source, program, _) = parse_program(
        "func main():\n    print(int(\"123\"))\n    print(float(\"3.14\"))\n    print(string(true))\n    print(bool(\"x\"))\n",
    );
    let ir = compile_program_to_ir(&source, &program).unwrap();
    assert!(ir.contains("string_to_int"));
    assert!(ir.contains("string_to_float"));
    assert!(ir.contains("bool_to_string"));
    assert!(ir.contains("call i64 @string_to_int("));
}

/// 校验安全类型转换会引用对应的安全解析运行时符号。
#[test]
fn should_emit_safe_convert_runtime_symbols() {
    let (source, program, _) = parse_program(
        "func main():\n    var parsed = try_int(\"123\")\n    print(parsed.ok)\n    print(parsed.value)\n    var flag = try_bool(\"true\")\n    print(flag.ok)\n    print(flag.value)\n",
    );
    let ir = compile_program_to_ir(&source, &program).unwrap();
    assert!(ir.contains("try_string_to_int"));
    assert!(ir.contains("try_string_to_bool"));
    assert!(ir.contains("call i8 @try_string_to_int("));
    assert!(ir.contains("call i8 @try_string_to_bool("));
}

/// 校验列表操作会引用新增的列表运行时符号。
#[test]
fn should_emit_list_runtime_symbols() {
    let (source, program, _) = parse_program(
        "func main():\n    var items:list<int> = [3, 1, 2]\n    items.append(4)\n    print(len(items))\n    print(items[1])\n    print(items[1:3][0])\n    print(items.contains(2))\n",
    );
    let ir = compile_program_to_ir(&source, &program).unwrap();
    assert!(ir.contains("list_create"));
    assert!(ir.contains("list_append_int"));
    assert!(ir.contains("list_len"));
    assert!(ir.contains("list_get_int"));
    assert!(ir.contains("list_slice"));
    assert!(ir.contains("list_contains_int"));
}

/// 校验字符串列表读取与弹出会改用指针返回加长度输出参数，避免 Windows ABI 结构体返回崩溃。
#[test]
fn should_emit_string_list_calls_with_out_len() {
    let (source, program, _) = parse_program(
        "func main():\n    var items:list<string> = [\"a\", \"b\"]\n    print(items[0])\n    print(items.pop())\n",
    );
    let ir = compile_program_to_ir(&source, &program).unwrap();
    assert!(ir.contains("call ptr @list_get_string("));
    assert!(ir.contains("call ptr @list_pop_string("));
}

/// 校验 `for item in list` 会生成独立的遍历控制流块。
#[test]
fn should_emit_for_each_blocks() {
    let (source, program, _) = parse_program(
        "func main():\n    var items:list<int> = [1, 2, 3]\n    for item in items:\n        print(item)\n",
    );
    let ir = compile_program_to_ir(&source, &program).unwrap();
    assert!(ir.contains("foreach.header"));
    assert!(ir.contains("foreach.continue"));
    assert!(ir.contains("foreach.exit"));
}

/// 校验固定长度数组会生成 LLVM 原生数组存储和元素地址计算指令。
#[test]
fn should_emit_array_storage_and_gep() {
    let (source, program, _) = parse_program(
        "func main():\n    var fixed:array<int,3>\n    var values = array<int,3>(1, 2, 3)\n    print(values[1])\n    print(fixed[0])\n",
    );
    let ir = compile_program_to_ir(&source, &program).unwrap();
    assert!(ir.contains("[3 x i64]"));
    assert!(ir.contains("getelementptr inbounds [3 x i64]"));
}

/// 校验字典操作会引用新增的字典运行时符号。
#[test]
fn should_emit_map_runtime_symbols() {
    let (source, program, _) = parse_program(
        "func main():\n    var user:map<string,int> = {\"age\": 18}\n    user[\"score\"] = 99\n    print(len(user))\n    print(user[\"age\"])\n    print(user.contains(\"score\"))\n    for key, value in user:\n        print(key)\n        print(value)\n",
    );
    let ir = compile_program_to_ir(&source, &program).unwrap();
    assert!(ir.contains("map_create"));
    assert!(ir.contains("map_set_string_int"));
    assert!(ir.contains("map_len"));
    assert!(ir.contains("map_get_string_int"));
    assert!(ir.contains("map_contains_string"));
    assert!(ir.contains("foreach.map.header"));
}

/// 校验返回字符串的字典读取会把 `out_len` 插入到键参数之后，避免生成错误的 LLVM 调用签名。
#[test]
fn should_emit_string_map_calls_with_out_len_before_error_site() {
    let (source, program, _) = parse_program(
        "func main():\n    var int_map:map<int,string> = {1: \"one\"}\n    var bool_map:map<bool,string> = {true: \"yes\", false: \"no\"}\n    print(int_map[1])\n    print(bool_map[true])\n    print(bool_map[false])\n",
    );
    let ir = compile_program_to_ir(&source, &program).unwrap();
    assert!(ir.contains("call ptr @map_get_int_string("));
    assert!(ir.contains("call ptr @map_get_bool_string("));
    assert!(!ir.contains("i64 61, ptr %map.string.out_len"));
}

/// 校验所有 `if / else` 分支都 `return` 时，后端不会再留下无终结器的悬空汇合块。
#[test]
fn should_compile_if_when_all_branches_return() {
    let (source, program, _) = parse_program(
        "func choose(value:int) -> int:\n    if value > 0:\n        return 1\n    else:\n        return 0\n",
    );
    let ir = compile_program_to_ir(&source, &program).unwrap();
    assert!(ir.contains("ms_user_choose"));
}
