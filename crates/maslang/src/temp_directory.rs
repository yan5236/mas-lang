use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

/// 持有本次操作独占的临时目录；仅清理自己通过原子创建取得的目录，供编译器与 fama 共用。
pub struct TemporaryDirectory {
    root: PathBuf,
}

impl TemporaryDirectory {
    /// 在系统临时目录创建以固定前缀、PID 和计数器命名的目录；碰撞时重试，不接管已有目录。
    pub fn create(prefix: &str) -> io::Result<Self> {
        if prefix.is_empty()
            || !prefix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid temporary directory prefix",
            ));
        }
        let parent = std::env::temp_dir();
        loop {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let root = parent.join(format!("{prefix}-{}-{sequence}", std::process::id()));
            match fs::create_dir(&root) {
                Ok(()) => return Ok(Self { root }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }

    /// 返回独占目录的路径；返回的借用不会延长目录生命周期，调用方应持有守卫直到子进程结束。
    pub fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for TemporaryDirectory {
    /// 正常返回、错误返回或栈展开时清理全部中间文件；清理失败仅报告，不在析构中再次 panic。
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            if error.kind() != io::ErrorKind::NotFound {
                eprintln!(
                    "Failed to clean temporary directory {}: {error}",
                    self.root.display()
                );
            }
        }
    }
}
