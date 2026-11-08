use maslang::optimization::OptLevel;
use maslang::settings::CompilerSettings;
use maslang::source::SourceFile;
use maslang::temp_directory::TemporaryDirectory;
use maslang::{CompileOptions, compile_to_executable};
use std::process::{Command, Output};

const HOST: &str = r#"
#include "mas_runtime.h"
typedef struct Time { int64_t seconds; double fraction; uint8_t valid; MasString label; } Time;
extern void mas_gc_collect(void);
/* 接收借入对象并写回字段；不保存或释放借入指针，文本用运行时复制。 */
void time_fill(Time *out) {
    out->seconds = 1234; out->fraction = 0.5; out->valid = 1;
    out->label = mas_runtime_string_clone((const uint8_t *)"native", 6);
    mas_gc_collect();
}
/* 使用静态追踪表创建新的 Time，所有权交给运行时；字段标签不参与 GC 扫描。 */
Time *time_create(uint8_t found) {
    if (!found) return NULL;
    static const uint64_t offsets[] = { offsetof(Time, label) };
    Time *value = (Time *)mas_record_alloc(sizeof(Time), offsets, 1);
    time_fill(value);
    return value;
}
/* 返回借入对象的同一引用，允许 null，对象生命周期仍由调用方和 GC 管理。 */
Time *time_identity(Time *value) { return value; }
/* 可空标量用单字段托管对象；C 以 int64_t 指针访问，null 表示缺失。 */
int64_t *time_optional(uint8_t found) {
    if (!found) return NULL;
    int64_t *value = (int64_t *)mas_record_alloc(sizeof(int64_t), NULL, 0);
    *value = 42; return value;
}
/* 借入的可空标量在非空时更新，空指针保持不变。 */
void time_increment(int64_t *value) { if (value) ++*value; }
/* 错误示例：静态区对象没有托管所有权，必须被原生返回检查拒绝。 */
Time *time_bad(void) { static Time value; return &value; }
/* 错误示例：非空返回类型不能返回 null。 */
Time *time_null(void) { return NULL; }
"#;

/// 创建包含真实 C 源的临时包并走现有维护者构建链路；产物运行后自动清理工程。
fn execute(body: &str, level: OptLevel) -> Output {
    let root = TemporaryDirectory::create("native-records").unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(root.path().join("settings.toml"), "[project]\nname = \"native-records\"\nversion = \"0.1.0\"\n[native]\nabi = 1\ninclude = [\".\"]\n[native.sources]\ncommon = [\"host.c\"]\n").unwrap();
    std::fs::write(root.path().join("host.c"), HOST).unwrap();
    std::fs::write(
        root.path().join("mas_runtime.h"),
        include_str!("../native/include/mas_runtime.h"),
    )
    .unwrap();
    let text = format!(
        "struct Time:\n    seconds: int\n    fraction: float\n    valid: bool\n    label: string\n{body}"
    );
    let path = root.path().join("src/main.ms");
    std::fs::write(&path, &text).unwrap();
    let source = SourceFile::new(path.display().to_string(), text);
    let settings = CompilerSettings::default_settings();
    let options = CompileOptions::new(Some(root.path().join("main.exe")))
        .with_opt_level(level)
        .with_allow_native_source_build(true);
    let binary = compile_to_executable(&source, &settings, &options)
        .unwrap_or_else(|e| panic!("{}", e.render(settings.language, Some(&source))));
    Command::new(binary).output().unwrap()
}

/// 真实链接 C 对象验证结构体布局、字段回写、共享身份、可空参数/返回、托管分配与强制回收。
#[test]
fn native_time_pointer_abi_executes() {
    let text = r#"
extern func time_fill(out: Time)
extern func time_create(found: bool) -> Time?
extern func time_identity(value: Time?) -> Time?
extern func time_optional(found: bool) -> int?
extern func time_increment(value: int?)
extern func mas_gc_collect()
extern func mas_gc_live_objects() -> int
func run():
    var t = Time(0, 0.0, false, "")
    time_fill(t)
    print(t.seconds)
    print(t.fraction)
    print(t.valid)
    print(t.label)
    var result = time_create(true)
    var alias = time_identity(result)
    alias.value.seconds = 99
    var times: list<Time?> = [result, time_create(false)]
    var by_name: map<string,Time> = {"now": result.value}
    var optional = time_optional(true)
    time_increment(optional)
    time_increment(null)
    mas_gc_collect()
    print(times[0].value.seconds)
    print(times[1] == null)
    print(by_name["now"].label)
    print(optional.value)
    print(time_optional(false) == null)
    print(time_identity(null) == null)
func main():
    run()
    mas_gc_collect()
    print(mas_gc_live_objects())
"#;
    for level in [OptLevel::O0, OptLevel::O3] {
        let output = execute(text, level);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "1234\n0.5\ntrue\nnative\n99\ntrue\nnative\n43\ntrue\ntrue\n0\n"
        );
    }
}

/// 非空 null 与非托管地址在跨过 FFI 返回边界时失败，不等到用户字段访问才产生崩溃。
#[test]
fn invalid_native_return_ownership_is_rejected() {
    for (name, message) in [
        ("time_bad", "非托管对象"),
        ("time_null", "非空类型返回了 null"),
    ] {
        let output = execute(
            &format!("extern func {name}() -> Time\nvar t = {name}()\nfunc main():\n    print(t.seconds)\n"),
            OptLevel::O3,
        );
        assert!(!output.status.success());
        assert!(String::from_utf8(output.stderr).unwrap().contains(message));
    }
}
