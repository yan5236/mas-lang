mod cli;
mod commands;
mod error;
mod project;
mod template;

use clap::Parser;
use cli::{Cli, Commands};
use error::FamaError;

/// 程序入口，负责统一处理命令执行结果与退出码。
fn main() {
    if let Err(error) = run() {
        eprintln!("错误: {error}");
        std::process::exit(1);
    }
}

/// 解析命令行参数并分发到对应的子命令处理逻辑。
fn run() -> Result<(), FamaError> {
    let cli = Cli::parse();

    match cli.command {
        Commands::New(args) => commands::new::run(args),
        Commands::Build(args) => commands::build::run(args),
        Commands::Run(args) => commands::run::run(args),
    }
}
