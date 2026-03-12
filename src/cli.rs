use std::path::PathBuf;

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
    /// 读取项目配置并编译当前 Mascript 项目。
    Build(BuildArgs),
    /// 编译当前 Mascript 项目后立即运行生成的可执行文件。
    Run(RunArgs),
}

/// 定义 `fama new` 子命令所需的位置参数。
#[derive(Args, Debug, Clone)]
pub struct NewArgs {
    pub project_name: String,
}

/// 定义 `fama build` 子命令支持的项目路径和输出覆盖参数。
#[derive(Args, Debug, Clone)]
pub struct BuildArgs {
    /// 指向 Mascript 项目根目录，默认使用当前目录。
    #[arg(default_value = ".")]
    pub project_dir: PathBuf,
    /// 覆盖 `settings.toml` 中的默认输出文件位置。
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

/// 定义 `fama run` 子命令支持的项目路径参数。
#[derive(Args, Debug, Clone)]
pub struct RunArgs {
    /// 指向 Mascript 项目根目录，默认使用当前目录。
    #[arg(default_value = ".")]
    pub project_dir: PathBuf,
}
