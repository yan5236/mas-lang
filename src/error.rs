use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;
use std::path::PathBuf;

/// 统一描述 fama 首版命令执行过程中可能出现的错误。
#[derive(Debug)]
pub enum FamaError {
    InvalidProjectName(String),
    ProjectAlreadyExists(PathBuf),
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
            Self::Io { path, source } => {
                write!(f, "写入路径失败：{}，原因：{source}", path.display())
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
