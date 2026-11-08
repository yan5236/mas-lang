use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::diagnostic::{CompilerError, MessageId};
use crate::source::Span;

/// 表示当前编译器可识别的界面语言。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiLanguage {
    ZhCn,
    EnUs,
}

impl UiLanguage {
    /// 根据配置文件中的语言代码解析成内部枚举值。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "zh-CN" => Some(Self::ZhCn),
            "en-US" => Some(Self::EnUs),
            _ => None,
        }
    }
}

/// 保存编译器运行期需要读取的全部设置项。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompilerSettings {
    pub language: UiLanguage,
}

impl CompilerSettings {
    /// 使用给定界面语言创建新的设置对象。
    pub fn new(language: UiLanguage) -> Self {
        Self { language }
    }

    /// 返回当前编译器使用的默认设置。
    pub fn default_settings() -> Self {
        Self::new(UiLanguage::ZhCn)
    }
}

#[derive(Debug, Deserialize)]
struct SettingsFile {
    ui: Option<UiSection>,
}

#[derive(Debug, Deserialize)]
struct UiSection {
    language: Option<String>,
}

/// 返回约定的用户级设置文件路径。
fn settings_path() -> PathBuf {
    if let Some(custom_home) = env::var_os("FAMA_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(custom_home).join("setting.toml");
    }

    resolve_home_directory().join(".fama").join("setting.toml")
}

/// 从用户目录读取编译器设置；当配置缺失或无效时回退到默认值。
pub fn load_settings() -> CompilerSettings {
    load_settings_with_diagnostic().0
}

/// 从默认路径读取设置，并在必要时附带一条本地化诊断信息。
pub fn load_settings_with_diagnostic() -> (CompilerSettings, Option<CompilerError>) {
    load_settings_from_path(&settings_path())
}

/// 从指定路径读取设置，供测试和命令行入口复用。
pub fn load_settings_from_path(path: &Path) -> (CompilerSettings, Option<CompilerError>) {
    let default = CompilerSettings::default_settings();
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (default, None),
        Err(error) => {
            return (
                default,
                Some(CompilerError::settings(
                    MessageId::SettingsReadFailed,
                    vec![path.display().to_string(), error.to_string()],
                    Span::new(0, 0),
                )),
            );
        }
    };

    match parse_settings_text(&text) {
        Ok(settings) => (settings, None),
        Err(error) => (
            default,
            Some(error.with_args_prepended(path.display().to_string())),
        ),
    }
}

/// 解析设置文件文本并转换为内部设置结构。
fn parse_settings_text(text: &str) -> Result<CompilerSettings, CompilerError> {
    let parsed: SettingsFile = toml::from_str(text).map_err(|error| {
        CompilerError::settings(
            MessageId::SettingsParseFailed,
            vec![String::new(), error.to_string()],
            Span::new(0, 0),
        )
    })?;

    let raw = parsed
        .ui
        .and_then(|section| section.language)
        .unwrap_or_else(|| "zh-CN".to_string());

    let language = UiLanguage::parse(&raw).ok_or_else(|| {
        CompilerError::settings(
            MessageId::SettingsInvalidLanguage,
            vec![String::new(), raw.clone()],
            Span::new(0, 0),
        )
    })?;

    Ok(CompilerSettings::new(language))
}

/// 解析当前用户的 home 目录，供默认全局设置路径拼接使用。
fn resolve_home_directory() -> PathBuf {
    if let Some(path) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }

    if let Some(path) = env::var_os("USERPROFILE").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }

    if let (Some(home_drive), Some(home_path)) = (
        env::var_os("HOMEDRIVE").filter(|value| !value.is_empty()),
        env::var_os("HOMEPATH").filter(|value| !value.is_empty()),
    ) {
        let mut path = PathBuf::from(home_drive);
        path.push(home_path);
        return path;
    }

    PathBuf::from(".")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{UiLanguage, load_settings_from_path};

    /// 生成一个唯一的临时设置文件路径，避免测试之间相互干扰。
    fn temp_path(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("mascript-settings-{name}-{unique}.toml"))
    }

    /// 校验合法的英文设置能够被正确解析出来。
    #[test]
    fn should_load_en_us_setting() {
        let path = temp_path("en");
        fs::write(&path, "[ui]\nlanguage = \"en-US\"\n").unwrap();
        let (settings, diagnostic) = load_settings_from_path(&path);
        assert_eq!(settings.language, UiLanguage::EnUs);
        assert!(diagnostic.is_none());
    }

    /// 校验非法语言值会回退为默认中文并返回诊断信息。
    #[test]
    fn should_fallback_when_language_is_invalid() {
        let path = temp_path("invalid");
        fs::write(&path, "[ui]\nlanguage = \"fr-FR\"\n").unwrap();
        let (settings, diagnostic) = load_settings_from_path(&path);
        assert_eq!(settings.language, UiLanguage::ZhCn);
        assert!(diagnostic.is_some());
    }
}

/// 判断依赖名称或版本能否安全拼入单层缓存路径；拒绝路径分隔符、盘符和特殊名称。
pub fn is_dependency_cache_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value.trim() == value
        && !value.ends_with('.')
        && !value.contains(|character: char| {
            character.is_control() || r#"/\:<>"|?*@"#.contains(character)
        })
}
