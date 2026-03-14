use std::process::ExitCode;

/// 启动命令行入口，并把最终退出码交给操作系统。
fn main() -> ExitCode {
    compiler_llvm::cli::run()
}
