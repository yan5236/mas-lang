use maslang::temp_directory::TemporaryDirectory;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

/// 运行当前编译器 CLI，强制使用本用例临时根，便于直接核对中间目录是否残留。
fn compiler(args: &[&std::ffi::OsStr], temp: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_maslang"))
        .args(args)
        .env("TMP", temp)
        .env("TEMP", temp)
        .output()
        .unwrap()
}

/// 确认隔离临时根已清空；失败时列出残留路径，避免只断言命令退出成功。
fn assert_empty(root: &Path) {
    let entries: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert!(entries.is_empty(), "leftover temporary files: {entries:?}");
}

/// 并发持有目录时名字不得重复；释放守卫后每个目录及文件必须删除。
#[test]
fn concurrent_directories_are_exclusive_and_removed() {
    let workers: Vec<_> = (0..64)
        .map(|_| {
            std::thread::spawn(|| {
                let directory = TemporaryDirectory::create("mascript-exclusive-test").unwrap();
                fs::write(directory.path().join("owned.txt"), "owned").unwrap();
                directory
            })
        })
        .collect();
    let directories: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    let paths: Vec<_> = directories.iter().map(|d| d.path().to_path_buf()).collect();
    assert_eq!(
        paths.iter().collect::<std::collections::HashSet<_>>().len(),
        64
    );
    drop(directories);
    assert!(paths.iter().all(|p| !p.exists()));
}

/// 并行构建六个 EXE，校验成功、失败均清理临时文件，且运行失败保留已经打印的日志。
#[test]
fn compiler_cleans_success_failure_and_parallel_builds() {
    let root = TemporaryDirectory::create("mascript-cleanup-test").unwrap();
    let temp = root.path().join("temp");
    fs::create_dir(&temp).unwrap();
    let source = root.path().join("main.ms");
    fs::write(&source, "func main():\n    print(42)\n").unwrap();
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..6)
            .map(|i| {
                let source = &source;
                let temp = &temp;
                let output_path = root.path().join(format!("output-{i}.exe"));
                scope.spawn(move || {
                    let output = compiler(
                        &[
                            "build".as_ref(),
                            source.as_os_str(),
                            "-o".as_ref(),
                            output_path.as_os_str(),
                        ],
                        temp,
                    );
                    assert!(output.status.success(), "{output:?}");
                    let run = Command::new(output_path).output().unwrap();
                    assert!(run.status.success(), "{run:?}");
                    assert_eq!(run.stdout, b"42\n");
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
    });
    assert_empty(&temp);
    let run = compiler(&["run".as_ref(), source.as_os_str()], &temp);
    assert!(run.status.success(), "{run:?}");
    assert_empty(&temp);
    fs::write(&source, "func main():\n    print(42)\n    print(10 %/% 0)\n").unwrap();
    let run = compiler(&["run".as_ref(), source.as_os_str()], &temp);
    assert!(!run.status.success(), "{run:?}");
    assert_eq!(run.stdout, b"42\n");
    assert!(String::from_utf8_lossy(&run.stderr).contains("整数除零"));
    assert_empty(&temp);
    fs::write(
        &source,
        "extern func missing_cleanup_test_symbol()\nfunc main():\n    missing_cleanup_test_symbol()\n",
    )
    .unwrap();
    let output_path = root.path().join("missing.exe");
    let failure = compiler(
        &[
            "build".as_ref(),
            source.as_os_str(),
            "-o".as_ref(),
            output_path.as_os_str(),
        ],
        &temp,
    );
    assert!(!failure.status.success(), "{failure:?}");
    assert!(String::from_utf8_lossy(&failure.stderr).contains("missing_cleanup_test_symbol"));
    assert_empty(&temp);
}
