use clap::{Args, Parser, Subcommand};

/// 定义 fama 顶层命令行接口。
#[derive(Parser, Debug)]
#[command(name = "fama", version, about = "Mascript 包管理器", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// 定义 fama 当前支持的全部子命令。
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// 创建一个新的 Mascript 项目。
    New(NewArgs),
}

/// 定义 `fama new` 子命令所需的位置参数。
#[derive(Args, Debug, Clone)]
pub struct NewArgs {
    pub project_name: String,
}
