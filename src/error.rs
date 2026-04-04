use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;
use std::path::PathBuf;

/// 统一描述 fama 首版命令执行过程中可能出现的错误。
#[derive(Debug)]
pub enum FamaError {
    InvalidProjectName(String),
    ProjectAlreadyExists(PathBuf),
    MissingFile(PathBuf),
    InvalidSettings { path: PathBuf, message: String },
    InvalidManifest { path: PathBuf, message: String },
    InvalidLockFile { path: PathBuf, message: String },
    InvalidGlobalSettings { path: PathBuf, message: String },
    DependencyConflict {
        name: String,
        existing: String,
        incoming: String,
    },
    DependencyInstall(String),
    CommandFailed { command: String, code: Option<i32> },
    Compiler(String),
    Io { path: PathBuf, source: io::Error },
}

impl FamaError {
    /// 构造带路径上下文的 IO 错误，便于用户快速定位失败位置。
    pub fn io(path: PathBuf, source: io::Error) -> Self {
        Self::Io { path, source }
    }
}

impl Display for FamaError {
    /// 将内部错误转换为适合命令行展示的中文消息。
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProjectName(message) => write!(f, "{message}"),
            Self::ProjectAlreadyExists(path) => {
                write!(f, "目标目录已存在：{}", path.display())
            }
            Self::MissingFile(path) => {
                write!(f, "缺少必需文件：{}", path.display())
            }
            Self::InvalidSettings { path, message } => {
                write!(f, "项目配置无效：{}，原因：{message}", path.display())
            }
            Self::InvalidManifest { path, message } => {
                write!(f, "依赖清单无效：{}，原因：{message}", path.display())
            }
            Self::InvalidLockFile { path, message } => {
                write!(f, "锁文件无效：{}，原因：{message}", path.display())
            }
            Self::InvalidGlobalSettings { path, message } => {
                write!(f, "全局配置无效：{}，原因：{message}", path.display())
            }
            Self::DependencyConflict {
                name,
                existing,
                incoming,
            } => {
                write!(
                    f,
                    "依赖 `{name}` 发生冲突：已有 `{existing}`，新值 `{incoming}`"
                )
            }
            Self::DependencyInstall(message) => write!(f, "{message}"),
            Self::CommandFailed { command, code } => match code {
                Some(code) => write!(f, "命令执行失败：{command}，退出码：{code}"),
                None => write!(f, "命令执行失败：{command}"),
            },
            Self::Compiler(message) => write!(f, "{message}"),
            Self::Io { path, source } => {
                write!(f, "访问路径失败：{}，原因：{source}", path.display())
            }
        }
    }
}

impl Error for FamaError {
    /// 暴露底层源错误，便于测试和上层诊断。
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
