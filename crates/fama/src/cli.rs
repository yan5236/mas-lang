use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// 定义 fama 顶层命令行接口。
#[derive(Parser, Debug)]
#[command(name = "fama", version, about = "Mas lang 包管理器", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// 定义 fama 当前支持的全部子命令。
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// 创建一个新的 Mas lang 项目。
    New(NewArgs),
    /// 向当前项目添加一个本地路径依赖。
    Add(AddArgs),
    /// 安装 `mod.yml` 中声明的本地路径依赖。
    Install(InstallArgs),
    /// 读取项目配置并编译当前 Mas lang 项目。
    Build(BuildArgs),
    /// 编译当前 Mas lang 项目后立即运行生成的可执行文件。
    Run(RunArgs),
}

/// 定义 `fama new` 子命令所需的位置参数。
#[derive(Args, Debug, Clone)]
pub struct NewArgs {
    pub project_name: String,
}

/// 定义 `fama add` 子命令支持的依赖路径和项目路径参数。
#[derive(Args, Debug, Clone)]
pub struct AddArgs {
    /// 要添加的本地依赖项目路径。
    pub dependency_path: PathBuf,
    /// 指向当前 Mas lang 项目根目录，默认使用当前目录。
    #[arg(long, default_value = ".")]
    pub project_dir: PathBuf,
}

/// 定义 `fama install` 子命令支持的项目路径参数。
#[derive(Args, Debug, Clone)]
pub struct InstallArgs {
    /// 指向当前 Mas lang 项目根目录，默认使用当前目录。
    #[arg(default_value = ".")]
    pub project_dir: PathBuf,
}

/// 定义 `fama build` 子命令支持的项目路径和输出覆盖参数。
#[derive(Args, Debug, Clone)]
pub struct BuildArgs {
    /// 指向 Mas lang 项目根目录，默认使用当前目录。
    #[arg(default_value = ".")]
    pub project_dir: PathBuf,
    /// 覆盖 `settings.toml` 中的默认输出文件位置。
    #[arg(short, long)]
    pub output: Option<PathBuf>,
    /// 允许维护者从 C 源文件刷新包内预编译原生产物。
    #[arg(long)]
    pub allow_native_source_build: bool,
}

/// 定义 `fama run` 子命令支持的项目路径参数。
#[derive(Args, Debug, Clone)]
pub struct RunArgs {
    /// 指向 Mas lang 项目根目录，默认使用当前目录。
    #[arg(default_value = ".")]
    pub project_dir: PathBuf,
    /// 允许维护者从 C 源文件刷新包内预编译原生产物。
    #[arg(long)]
    pub allow_native_source_build: bool,
}
