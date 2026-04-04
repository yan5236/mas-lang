use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::FamaError;

/// 保存 `~/.fama/setting.toml` 顶层结构，便于统一解析全局设置。
#[derive(Debug, Deserialize, Default)]
struct FamaSettingsFile {
    compiler: Option<CompilerSection>,
}

/// 保存编译器相关的全局设置，目前只约定可选的二进制路径。
#[derive(Debug, Deserialize, Default)]
struct CompilerSection {
    binary: Option<String>,
}

/// 汇总 `fama` 当前需要消费的全局设置项。
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct FamaSettings {
    pub compiler_binary: Option<PathBuf>,
}

/// 从约定的全局路径读取 `fama` 设置，不存在时返回默认值。
pub fn load_settings() -> Result<FamaSettings, FamaError> {
    let path = settings_path()?;
    load_settings_from_path(&path)
}

/// 确保全局配置目录与 `setting.toml` 存在，供首次启动时自动完成初始化。
pub fn ensure_global_settings_initialized() -> Result<PathBuf, FamaError> {
    let path = settings_path()?;
    ensure_settings_file_exists(&path)?;
    Ok(path)
}

/// 返回当前进程约定的全局设置文件路径。
pub fn settings_path() -> Result<PathBuf, FamaError> {
    if let Some(custom_home) = env::var_os("FAMA_HOME") {
        return Ok(PathBuf::from(custom_home).join("setting.toml"));
    }

    let home = resolve_home_directory()?;
    Ok(home.join(".fama").join("setting.toml"))
}

/// 从指定路径读取并解析全局设置，供测试和运行时共同复用。
pub fn load_settings_from_path(path: &Path) -> Result<FamaSettings, FamaError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FamaSettings::default());
        }
        Err(error) => return Err(FamaError::io(path.to_path_buf(), error)),
    };

    let parsed = parse_settings_text(path, &text)?;

    Ok(FamaSettings {
        compiler_binary: parsed
            .compiler
            .and_then(|compiler| compiler.binary)
            .map(PathBuf::from),
    })
}

/// 解析全局配置文本，并在 Windows 路径的常见写法上做一次有限兼容。
fn parse_settings_text(path: &Path, text: &str) -> Result<FamaSettingsFile, FamaError> {
    match toml::from_str(text) {
        Ok(parsed) => Ok(parsed),
        Err(source) => {
            if let Some(repaired_text) = repair_windows_binary_path_toml(text) {
                return toml::from_str(&repaired_text).map_err(|repaired_source| {
                    FamaError::InvalidGlobalSettings {
                        path: path.to_path_buf(),
                        message: repaired_source.to_string(),
                    }
                });
            }

            Err(FamaError::InvalidGlobalSettings {
                path: path.to_path_buf(),
                message: format!(
                    "{}；Windows 路径请使用正斜杠 `/`、双反斜杠 `\\\\`，或单引号字面量字符串",
                    source
                ),
            })
        }
    }
}

/// 修正常见的 `[compiler] binary = "C:\path\to\tool.exe"` 写法，避免 TOML 把反斜杠当转义。
fn repair_windows_binary_path_toml(text: &str) -> Option<String> {
    let mut repaired_lines = Vec::new();
    let mut changed = false;

    for line in text.lines() {
        if let Some(repaired_line) = repair_binary_assignment_line(line) {
            repaired_lines.push(repaired_line);
            changed = true;
            continue;
        }

        repaired_lines.push(line.to_string());
    }

    changed.then(|| repaired_lines.join("\n"))
}

/// 仅修正 `binary = "..."` 这一行中的反斜杠，避免影响其他 TOML 字段。
fn repair_binary_assignment_line(line: &str) -> Option<String> {
    let (prefix, value_part) = line.split_once('=')?;
    let key = prefix.trim();
    if key != "binary" {
        return None;
    }

    let value_part = value_part.trim_start();
    if !value_part.starts_with('"') {
        return None;
    }

    let closing_quote_index = value_part[1..].find('"')?;
    let closing_quote_index = closing_quote_index + 1;
    let value = &value_part[1..closing_quote_index];
    if !value.contains('\\') {
        return None;
    }

    let suffix = &value_part[(closing_quote_index + 1)..];
    let repaired_value = value.replace('\\', "\\\\");

    Some(format!("{prefix}= \"{repaired_value}\"{suffix}"))
}

/// 按需创建全局配置父目录和默认配置文件，避免首次运行时缺少 `~/.fama`。
fn ensure_settings_file_exists(path: &Path) -> Result<(), FamaError> {
    if path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| FamaError::io(parent.to_path_buf(), source))?;
    }

    fs::write(path, default_settings_template())
        .map_err(|source| FamaError::io(path.to_path_buf(), source))?;
    Ok(())
}

/// 生成默认的全局配置模板，便于用户后续直接填写编译器路径。
fn default_settings_template() -> &'static str {
    "# fama 全局配置文件\n# [compiler]\n# binary = \"C:/tools/mascript-compiler-llvm.exe\"\n"
}

/// 解析当前用户的 home 目录，供默认全局配置路径拼接使用。
fn resolve_home_directory() -> Result<PathBuf, FamaError> {
    if let Some(path) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }

    if let Some(path) = env::var_os("USERPROFILE").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }

    let home_drive = env::var_os("HOMEDRIVE").filter(|value| !value.is_empty());
    let home_path = env::var_os("HOMEPATH").filter(|value| !value.is_empty());
    if let (Some(home_drive), Some(home_path)) = (home_drive, home_path) {
        let mut path = PathBuf::from(home_drive);
        path.push(home_path);
        return Ok(path);
    }

    Err(FamaError::DependencyInstall(
        "无法解析当前用户的 home 目录，不能定位 ~/.fama/setting.toml".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        FamaSettings, ensure_settings_file_exists, load_settings_from_path, parse_settings_text,
        repair_windows_binary_path_toml,
    };

    /// 生成唯一的临时文件路径，避免不同测试共享同一个全局配置文件。
    fn create_temp_file(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间异常")
            .as_nanos();
        std::env::temp_dir().join(format!("fama-global-settings-{name}-{unique}.toml"))
    }

    /// 生成唯一的临时目录路径，供初始化目录创建场景测试复用。
    fn create_temp_dir(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间异常")
            .as_nanos();
        std::env::temp_dir().join(format!("fama-global-home-{name}-{unique}"))
    }

    /// 校验缺失的全局配置文件会回退为默认设置，而不是报错中断。
    #[test]
    fn should_fallback_to_default_when_global_settings_file_is_missing() {
        let path = create_temp_file("missing");
        let settings = load_settings_from_path(&path).unwrap();
        assert_eq!(settings, FamaSettings::default());
    }

    /// 校验合法的编译器二进制路径能够被正确解析出来。
    #[test]
    fn should_load_compiler_binary_from_global_settings() {
        let path = create_temp_file("binary");
        fs::write(&path, "[compiler]\nbinary = \"C:/tools/mascript-compiler-llvm.exe\"\n")
            .expect("写入临时全局配置失败");

        let settings = load_settings_from_path(&path).unwrap();
        assert_eq!(
            settings.compiler_binary.as_deref(),
            Some(std::path::Path::new("C:/tools/mascript-compiler-llvm.exe"))
        );
    }

    /// 校验 Windows 风格单反斜杠路径会被兼容处理，而不是直接报 TOML 转义错误。
    #[test]
    fn should_accept_windows_binary_path_with_single_backslashes() {
        let parsed = parse_settings_text(
            Path::new("C:/temp/setting.toml"),
            "[compiler]\nbinary = \"E:\\xiangmu\\mascript\\mascript-compiler-llvm\\target\\release\\mascript-compiler-llvm.exe\"\n",
        )
        .unwrap();

        assert_eq!(
            parsed.compiler.and_then(|compiler| compiler.binary),
            Some(
                "E:\\xiangmu\\mascript\\mascript-compiler-llvm\\target\\release\\mascript-compiler-llvm.exe"
                    .to_string()
            )
        );
    }

    /// 校验修复逻辑会把 `binary` 中的单反斜杠转换成可被 TOML 接受的双反斜杠。
    #[test]
    fn should_repair_binary_line_with_single_backslashes() {
        let repaired = repair_windows_binary_path_toml(
            "[compiler]\nbinary = \"E:\\xiangmu\\mascript\\mascript-compiler-llvm\\target\\release\\mascript-compiler-llvm.exe\"\n",
        )
        .unwrap();

        assert!(repaired.contains("E:\\\\xiangmu\\\\mascript\\\\mascript-compiler-llvm"));
    }

    /// 校验首次初始化会自动创建缺失的父目录与默认全局配置文件。
    #[test]
    fn should_create_global_settings_file_and_parent_directory_when_missing() {
        let home_dir = create_temp_dir("dir");
        let path = home_dir.join("setting.toml");
        let _ = fs::remove_dir_all(&home_dir);

        ensure_settings_file_exists(&path).unwrap();

        assert!(path.is_file());
        let text = fs::read_to_string(&path).expect("读取初始化后的全局配置失败");
        assert!(text.contains("# fama 全局配置文件"));
        fs::remove_dir_all(&home_dir).expect("删除临时全局配置目录失败");
    }
}
