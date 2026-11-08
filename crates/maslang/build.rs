#[path = "src/runtime_bootstrap.rs"]
mod runtime_bootstrap;

use std::{env, fs, path::PathBuf, process::Command};

/// 以 Cargo 使用的 rustc 为 Windows 目标构建各优化档的运行时库，并记录工具链版本供链接端校验。
/// 编译或写入失败时终止 Cargo 构建，避免把不完整的运行时嵌入编译器。
fn main() {
    println!("cargo:rerun-if-changed=src/runtime");
    println!("cargo:rerun-if-changed=src/runtime_bootstrap.rs");
    if env::var("TARGET").as_deref() != Ok("x86_64-pc-windows-msvc") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let rustc = env::var_os("RUSTC").unwrap();
    let version = Command::new(&rustc).arg("-vV").output().unwrap();
    assert!(version.status.success(), "无法获取运行时构建工具链版本");
    fs::write(out.join("runtime-rustc.txt"), version.stdout).unwrap();
    let source = out.join("mas_runtime.rs");
    fs::write(&source, runtime_bootstrap::bootstrap_source()).unwrap();
    for level in 0..=3 {
        let status = Command::new(&rustc)
            .args([
                "--edition=2024",
                "--target=x86_64-pc-windows-msvc",
                "--crate-type=rlib",
                "--cfg=mas_runtime_library",
                "-C",
            ])
            .arg(format!("opt-level={level}"))
            .arg(&source)
            .arg("-o")
            .arg(out.join(format!("libmas_runtime_{level}.rlib")))
            .status()
            .unwrap();
        assert!(status.success(), "无法构建 O{level} 运行时库");
    }
}
