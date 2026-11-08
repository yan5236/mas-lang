use super::SemanticAnalyzer;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::settings::{CompilerSettings, UiLanguage};
use crate::source::SourceFile;

/// 执行一次完整的语义分析，用于校验成功或失败场景。
fn analyze(text: &str) -> Result<(), String> {
    let settings = CompilerSettings::new(UiLanguage::ZhCn);
    let source = SourceFile::new("<test>", text.to_string());
    let tokens = Lexer::new(&source, &settings).tokenize().unwrap();
    let program = Parser::new(tokens, &settings).parse_program().unwrap();
    SemanticAnalyzer::new(&settings)
        .analyze(&program)
        .map_err(|error| error.render(settings.language, Some(&source)))
}

/// 校验条件表达式必须是布尔值。
#[test]
fn should_reject_non_boolean_if_condition() {
    let error = analyze("func main():\n    if 1:\n        print(1)\n").unwrap_err();
    assert!(error.contains("条件表达式必须是布尔值"));
}

/// 校验 `break` 只能出现在循环体内部。
#[test]
fn should_reject_break_outside_loop() {
    let error = analyze("break\n").unwrap_err();
    assert!(error.contains("只能出现在循环体中"));
}

/// 校验函数返回值路径不完整时会被语义分析拦截。
#[test]
fn should_reject_missing_return_path() {
    let error = analyze("func add(a:int, b:int) -> int:\n    if true:\n        return a + b\n")
        .unwrap_err();
    assert!(error.contains("并非所有路径都会返回"));
}

/// 校验 `/` 在纯整数参与时也会被推导为浮点结果。
#[test]
fn should_reject_integer_variable_receiving_true_division_result() {
    let error = analyze("var result:int = 6 / 2\n").unwrap_err();
    assert!(error.contains("类型与初始化表达式不匹配"));
}

/// 校验 `%` 和 `%/%` 只接受整数运算数。
#[test]
fn should_reject_non_integer_modulo() {
    let error = analyze("func main():\n    print(5.0 % 2)\n").unwrap_err();
    assert!(error.contains("只支持整数"));
}

/// 校验常量一旦声明后就不能再次赋值。
#[test]
fn should_reject_const_reassignment() {
    let error = analyze("const answer = 42\nanswer = 1\n").unwrap_err();
    assert!(error.contains("常量 `answer` 不能重新赋值"));
}

/// 校验非同构列表字面量会在语义阶段被拒绝。
#[test]
fn should_reject_mixed_list_literal() {
    let error = analyze("var items = [1, true]\n").unwrap_err();
    assert!(error.contains("列表字面量中的元素类型必须保持一致"));
}

/// 校验空列表字面量必须带有显式类型上下文。
#[test]
fn should_reject_empty_list_without_annotation() {
    let error = analyze("var items = []\n").unwrap_err();
    assert!(error.contains("空列表字面量必须放在显式 `list<T>` 类型上下文中"));
}

/// 校验列表遍历会把循环变量绑定为元素类型。
#[test]
fn should_accept_for_each_on_list() {
    analyze("var items:list<int> = [1, 2]\nfunc main():\n    for item in items:\n        print(item)\n").unwrap();
}

/// 校验字典遍历会把循环变量分别绑定为键类型和值类型。
#[test]
fn should_accept_for_each_on_map() {
    analyze(
        "var user:map<string,int> = {\"age\": 18}\nfunc main():\n    for key, value in user:\n        print(key)\n        print(value)\n",
    )
    .unwrap();
}

/// 校验列表方法调用的参数类型需要和元素类型一致。
#[test]
fn should_reject_invalid_list_method_argument_type() {
    let error = analyze("var items:list<int> = [1]\nitems.append(true)\n").unwrap_err();
    assert!(error.contains("参数类型不匹配"));
}

/// 校验空字典字面量必须带有显式类型上下文。
#[test]
fn should_reject_empty_map_without_annotation() {
    let error = analyze("var user = {}\n").unwrap_err();
    assert!(error.contains("空字典字面量必须放在显式 `map<K,V>` 类型上下文中"));
}

/// 校验字典下标类型必须和声明的键类型一致。
#[test]
fn should_reject_invalid_map_index_type() {
    let error = analyze("var user:map<string,int> = {\"age\": 18}\nfunc main():\n    print(user[1])\n").unwrap_err();
    assert!(error.contains("字典下标的键类型不匹配"));
}

/// 校验字典方法调用的参数类型需要和键类型一致。
#[test]
fn should_reject_invalid_map_method_argument_type() {
    let error =
        analyze("var user:map<string,int> = {\"age\": 18}\nuser.contains(1)\n").unwrap_err();
    assert!(error.contains("参数类型不匹配"));
}

/// 校验字符串方法集合能够在语义阶段推导出稳定的返回类型。
#[test]
fn should_accept_string_methods() {
    analyze(
        "func main():\n    var text = \"  你好，世界  \"\n    var cleaned:string = text.trim()\n    var has_hao:bool = cleaned.contains(\"好\")\n    var starts:bool = cleaned.starts_with(\"你好\")\n    var ends:bool = cleaned.ends_with(\"世界\")\n    var replaced:string = cleaned.replace(\"世界\", \"Mas lang\")\n    print(cleaned)\n    print(has_hao)\n    print(starts)\n    print(ends)\n    print(replaced)\n",
    )
    .unwrap();
}

/// 校验字符串方法用于非字符串目标时会返回明确的目标类型诊断。
#[test]
fn should_reject_string_method_on_non_string_targets() {
    let cases = [
        ("var value:int = 1\nfunc main():\n    print(value.trim())\n", "int"),
        ("var value:float = 1.5\nfunc main():\n    print(value.trim())\n", "float"),
        ("var value:bool = true\nfunc main():\n    print(value.trim())\n", "bool"),
        (
            "var value:array<int,2>\nfunc main():\n    print(value.trim())\n",
            "array<int,2>",
        ),
    ];

    for (source, expected_type) in cases {
        let error = analyze(source).unwrap_err();
        assert!(
            error.contains("字符串方法 `trim` 只能用于 `string` 类型"),
            "{error}"
        );
        assert!(error.contains(expected_type), "{error}");
    }
}

/// 校验字符串专属方法不会意外污染现有列表与字典方法集合。
#[test]
fn should_reject_string_only_method_on_list_and_map() {
    let list_error = analyze("var items:list<int> = [1]\nitems.trim()\n").unwrap_err();
    assert!(list_error.contains("列表类型不支持方法 `trim`"));

    let map_error = analyze("var user:map<string,int> = {\"age\": 18}\nuser.trim()\n").unwrap_err();
    assert!(map_error.contains("字典类型不支持方法 `trim`"));
}

/// 校验字符串方法会分别拦截参数数量错误与参数类型错误。
#[test]
fn should_reject_invalid_string_method_arguments() {
    let missing_argument =
        analyze("var text = \"Mas lang\"\nfunc main():\n    print(text.contains())\n").unwrap_err();
    assert!(missing_argument.contains("字符串方法 `contains` 需要且只接受一个 string 参数"));

    let invalid_type =
        analyze("var text = \"Mas lang\"\nfunc main():\n    print(text.replace(\"M\", 1))\n").unwrap_err();
    assert!(invalid_type.contains("字符串方法 `replace` 的参数类型必须是 string"));
}

/// 校验数组默认声明和元素访问能够通过语义分析。
#[test]
fn should_accept_array_declaration_and_index() {
    analyze("var fixed:array<int,3>\nvar values = array<int,3>(1, 2, 3)\nfunc main():\n    print(values[1])\n")
        .unwrap();
}

/// 校验数组初始化的元素数量必须和固定长度完全一致。
#[test]
fn should_reject_array_initializer_length_mismatch() {
    let error = analyze("var values = array<int,3>(1, 2)\n").unwrap_err();
    assert!(error.contains("数组初始化需要 3 个元素"));
}

/// 校验四个基础类型转换内建函数都能在语义阶段推导出正确结果类型。
#[test]
fn should_accept_scalar_convert_builtins() {
    analyze(
        "var n:int = int(\"123\")\nvar f:float = float(true)\nvar s:string = string(42)\nvar b:bool = bool(\"hello\")\n",
    )
    .unwrap();
}

/// 校验容器类型不能直接传给当前阶段的基础类型转换内建函数。
#[test]
fn should_reject_non_scalar_convert_builtin_argument() {
    let error = analyze("var items:list<int> = [1]\nvar text = string(items)\n").unwrap_err();
    assert!(error.contains("只支持基础类型转换"));
}

/// 校验安全转换内建函数及其 `ok/value` 属性都能通过语义分析。
#[test]
fn should_accept_safe_convert_builtins() {
    analyze(
        "var parsed = try_int(\"123\")\nfunc main():\n    if parsed.ok:\n        print(parsed.value)\n    var float_parsed = try_float(\"3.14\")\n    if float_parsed.ok:\n        print(float_parsed.value)\n    var bool_parsed = try_bool(\"true\")\n    if bool_parsed.ok:\n        print(bool_parsed.value)\n",
    )
    .unwrap();
}

/// 校验 `extern func` 声明能够参与语义分析，并推导出稳定的参数与返回类型。
#[test]
fn should_accept_extern_function_declarations() {
    analyze(
        "extern func host_now() -> int\nextern func host_sleep(ms:int)\nextern func host_name() -> string\nvar unix:int = host_now()\nfunc main():\n    host_sleep(1)\n    print(host_name())\n    print(unix)\n",
    )
    .unwrap();
}

/// 校验 `extern func` 也会复用普通函数调用的参数数量与类型检查逻辑。
#[test]
fn should_reject_invalid_extern_function_arguments() {
    let count_error = analyze("extern func host_now() -> int\nfunc main():\n    print(host_now(1))\n").unwrap_err();
    assert!(count_error.contains("期望 0 个参数"));

    let type_error = analyze("extern func host_sleep(ms:int)\nhost_sleep(\"1\")\n").unwrap_err();
    assert!(type_error.contains("第 1 个参数类型不匹配"));
}

/// 校验旧的 `__time_*` 入口在迁移完成后会以未声明函数的方式失败，避免继续绕过标准库边界。
#[test]
fn should_reject_legacy_time_builtins_after_migration() {
    let error = analyze("func main():\n    print(__time_now_unix())\n").unwrap_err();
    assert!(error.contains("未声明函数 `__time_now_unix`"));
}

/// 校验只有安全转换结果才允许访问 `ok/value` 属性。
#[test]
fn should_reject_property_access_on_non_safe_convert_value() {
    let error = analyze("var value = 1\nfunc main():\n    print(value.ok)\n").unwrap_err();
    assert!(error.contains("当前表达式不支持属性访问"));
}

/// 校验模板字符串中的字符串与数值插值都能复用现有类型转换链路通过语义分析。
#[test]
fn should_accept_template_string_interpolation() {
    analyze("var name = \"Mas lang\"\nvar count = 3\nfunc main():\n    print(\"hello ${name}, count=${count}\")\n")
        .unwrap();
}
