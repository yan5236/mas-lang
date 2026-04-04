use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::FamaError;

/// 保存 `settings.toml` 顶层结构，便于一次性反序列化整个项目配置。
#[derive(Debug, Deserialize)]
struct SettingsFile {
    project: ProjectSection,
    build: BuildSection,
}

/// 保存项目基础元数据，目前只需要项目名。
#[derive(Debug, Deserialize)]
struct ProjectSection {
    name: String,
}

/// 保存构建相关配置，目前只使用入口文件和输出目录。
#[derive(Debug, Deserialize)]
struct BuildSection {
    entry: String,
    output: String,
}

/// 汇总项目根目录、源码入口和默认产物路径，供命令层直接复用。
#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub root: PathBuf,
    pub name: String,
    pub entry_path: PathBuf,
    pub output_dir: PathBuf,
}

impl ProjectContext {
    /// 从项目根目录读取 `settings.toml`，并解析出 build/run 需要的关键路径。
    pub fn load(project_dir: &Path) -> Result<Self, FamaError> {
        let root = resolve_project_root(project_dir)?;
        let settings_path = root.join("settings.toml");
        let settings_text = read_required_utf8_file(&settings_path)?;
        let settings = parse_settings(&settings_path, &settings_text)?;
        let entry_path = root.join(&settings.build.entry);
        let output_dir = root.join(&settings.build.output);

        ensure_required_file(&entry_path)?;

        Ok(Self {
            root,
            name: settings.project.name,
            entry_path,
            output_dir,
        })
    }

    /// 根据项目名和输出目录推导默认可执行文件路径。
    pub fn default_binary_path(&self) -> PathBuf {
        self.output_dir.join(format!("{}.exe", self.name))
    }

    /// 把命令行传入的输出路径规范化为可写的目标文件路径。
    pub fn resolve_output_path(&self, output: Option<&Path>) -> PathBuf {
        match output {
            Some(path) if path.is_absolute() => path.to_path_buf(),
            Some(path) => self.root.join(path),
            None => self.default_binary_path(),
        }
    }
}

/// 把用户给出的项目目录转成绝对路径，便于后续统一处理相对路径。
pub fn resolve_project_root(project_dir: &Path) -> Result<PathBuf, FamaError> {
    if !project_dir.exists() {
        return Err(FamaError::MissingFile(project_dir.to_path_buf()));
    }

    fs::canonicalize(project_dir).map_err(|source| FamaError::io(project_dir.to_path_buf(), source))
}

/// 读取必需的 UTF-8 文本文件，缺失时返回带路径上下文的错误。
fn read_required_utf8_file(path: &Path) -> Result<String, FamaError> {
    ensure_required_file(path)?;
    fs::read_to_string(path).map_err(|source| FamaError::io(path.to_path_buf(), source))
}

/// 检查给定路径上的必需文件是否存在，避免把路径问题拖到更后面的阶段。
fn ensure_required_file(path: &Path) -> Result<(), FamaError> {
    if path.is_file() {
        Ok(())
    } else {
        Err(FamaError::MissingFile(path.to_path_buf()))
    }
}

/// 使用 `toml` 反序列化项目配置，并把解析错误转换成用户可读的中文信息。
fn parse_settings(path: &Path, text: &str) -> Result<SettingsFile, FamaError> {
    toml::from_str(text).map_err(|source| FamaError::InvalidSettings {
        path: path.to_path_buf(),
        message: source.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::ProjectContext;

    /// 为项目配置测试创建唯一临时目录，避免不同用例互相覆盖。
    fn create_temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间异常")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("fama-project-{prefix}-{unique}"));
        fs::create_dir_all(&path).expect("创建临时目录失败");
        path
    }

    /// 在临时目录中写入一份最小可用项目配置，便于测试配置加载逻辑。
    fn write_project_files(root: &Path) {
        fs::create_dir_all(root.join("src")).expect("创建 src 目录失败");
        fs::write(
            root.join("settings.toml"),
            "[project]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ms\"\noutput = \"build\"\nopt-level = 2\n",
        )
        .expect("写入 settings.toml 失败");
        fs::write(
            root.join("src").join("main.ms"),
            "print(\"Hello world.\")\n",
        )
        .expect("写入 main.ms 失败");
    }

    /// 校验可以从标准项目结构中正确解析入口文件和默认输出目录。
    #[test]
    fn should_load_project_context_from_settings_file() {
        let root = create_temp_dir("load");
        write_project_files(&root);

        let context = ProjectContext::load(&root).unwrap();

        assert_eq!(context.name, "demo");
        assert!(context.entry_path.ends_with("src\\main.ms"));
        assert!(context.output_dir.ends_with("build"));

        fs::remove_dir_all(root).expect("删除临时目录失败");
    }

    /// 校验缺少 `settings.toml` 时会返回明确的缺文件错误。
    #[test]
    fn should_fail_when_settings_file_is_missing() {
        let root = create_temp_dir("missing-settings");
        let error = ProjectContext::load(&root).unwrap_err();

        assert!(error.to_string().contains("缺少必需文件"));

        fs::remove_dir_all(root).expect("删除临时目录失败");
    }
}
