use maslang::temp_directory::TemporaryDirectory;
use std::{fs, process::Command};

/// 从真实 CLI 构建三个档位，验证行为一致及 O2/O3 的栈槽/GC 裁剪；允许私有函数被完整内联，O0 保留函数与 GC 调用。
#[test]
fn optimization_levels_preserve_results_and_run_ir_passes() {
    let root = TemporaryDirectory::create("mas-optimization").unwrap();
    let source = root.path().join("main.ms");
    fs::write(&source, "func sum(n:int) -> int:\n    var total = 0\n    var i = 0\n    while i < n:\n        total += i\n        i += 1\n    return total\nfunc fib(n:int) -> int:\n    if n < 2:\n        return n\n    return fib(n-1) + fib(n-2)\nfunc main():\n    var map:map<string,int> = {}\n    for i in 0..1000:\n        map[string(i % 32)] = i\n    print(sum(100))\n    print(fib(12))\n    print(map[\"0\"])\n    print(len(map))\n").unwrap();
    for level in ["0", "2", "3"] {
        let dir = root.path().join(level);
        let exe = dir.join("main.exe");
        let build = Command::new(env!("CARGO_BIN_EXE_maslang"))
            .arg("build")
            .arg(&source)
            .arg(format!("-O{level}"))
            .arg("--emit-dir")
            .arg(&dir)
            .arg("-o")
            .arg(&exe)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let output = Command::new(exe).output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
            "4950\n144\n992\n32\n"
        );
        let raw = fs::read_to_string(dir.join("main.raw.ll")).unwrap();
        let optimized = fs::read_to_string(dir.join("main.opt.ll")).unwrap();
        assert!(dir.join("main.s").is_file());
        let sum = optimized.split("\ndefine ").find(|body| {
            body.lines()
                .next()
                .is_some_and(|line| line.contains("_sum("))
        });
        if let Some(sum) = sum {
            let sum = sum.split("\n}").next().unwrap();
            assert_eq!(sum.contains("@mas_gc_"), level == "0");
        } else {
            assert_ne!(level, "0", "O0 must retain sum");
            assert!(
                !optimized.contains("_sum("),
                "inlined function must have no calls"
            );
        }
        if level != "0" {
            assert!(optimized.matches("alloca").count() < raw.matches("alloca").count());
            assert!(
                optimized.matches("call void @mas_gc_enter").count()
                    < raw.matches("call void @mas_gc_enter").count()
            );
        }
    }
}

/// 在 O2/O3 下验证优化不会删除语言定义的溢出检查，非法档位必须在构建前拒绝。
#[test]
fn optimized_cli_keeps_errors_and_rejects_invalid_level() {
    let root = TemporaryDirectory::create("mas-optimization-error").unwrap();
    let source = root.path().join("error.ms");
    fs::write(&source, "var n = 9223372036854775807\nfunc main():\n    print(n + 1)\n").unwrap();
    for level in ["-O2", "-O3"] {
        let output = Command::new(env!("CARGO_BIN_EXE_maslang"))
            .arg("run")
            .arg(&source)
            .arg(level)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("溢出"));
    }
    let output = Command::new(env!("CARGO_BIN_EXE_maslang"))
        .arg("build")
        .arg(source)
        .arg("-O9")
        .output()
        .unwrap();
    assert!(!output.status.success());
}
