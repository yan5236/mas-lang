mod cli;
mod commands;
mod config;
mod error;
mod install;
mod manifest;
mod project;
mod template;

use clap::Parser;
use cli::{Cli, Commands};
use config::ensure_global_settings_initialized;
use error::FamaError;

/// 程序入口，负责统一处理命令执行结果与退出码。
fn main() {
    if let Err(error) = run() {
        eprintln!("错误: {error}");
        std::process::exit(1);
    }
}

/// 先完成全局配置初始化，再解析命令行参数并分发到对应的子命令处理逻辑。
fn run() -> Result<(), FamaError> {
    ensure_global_settings_initialized()?;
    let cli = Cli::parse();

    match cli.command {
        Commands::New(args) => commands::new::run(args),
        Commands::Add(args) => commands::add::run(args),
        Commands::Install(args) => commands::install::run(args),
        Commands::Build(args) => commands::build::run(args),
        Commands::Run(args) => commands::run::run(args),
    }
}
