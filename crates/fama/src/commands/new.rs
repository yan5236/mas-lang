use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::NewArgs;
use crate::error::FamaError;
use crate::template::ProjectTemplate;

/// 执行 `fama new` 命令，创建项目目录与默认模板文件。
pub fn run(args: NewArgs) -> Result<(), FamaError> {
    let project_name = validate_project_name(&args.project_name)?;
    let project_root = PathBuf::from(project_name);

    if project_root.exists() {
        return Err(FamaError::ProjectAlreadyExists(project_root));
    }

    create_project(&project_root, project_name)
}

/// 校验项目名是否合法，避免创建出难以处理的路径结构。
fn validate_project_name(project_name: &str) -> Result<&str, FamaError> {
    let trimmed = project_name.trim();

    if trimmed.is_empty() {
        return Err(FamaError::InvalidProjectName("项目名不能为空".to_string()));
    }

    if trimmed == "." || trimmed == ".." {
        return Err(FamaError::InvalidProjectName(
            "项目名不能为当前目录或上级目录".to_string(),
        ));
    }

    if trimmed.contains(['/', '\\']) {
        return Err(FamaError::InvalidProjectName(
            "项目名不能包含路径分隔符".to_string(),
        ));
    }

    if trimmed.contains(['<', '>', ':', '"', '|', '?', '*']) {
        return Err(FamaError::InvalidProjectName(
            "项目名包含系统不允许的字符".to_string(),
        ));
    }

    Ok(trimmed)
}

/// 按固定顺序创建项目目录与文件，便于定位任何一步的失败原因。
fn create_project(project_root: &Path, project_name: &str) -> Result<(), FamaError> {
    fs::create_dir(project_root)
        .map_err(|source| FamaError::io(project_root.to_path_buf(), source))?;

    let src_dir = project_root.join("src");
    let mod_module_dir = project_root.join("mod_module");

    fs::create_dir(&src_dir).map_err(|source| FamaError::io(src_dir.clone(), source))?;
    fs::create_dir(&mod_module_dir)
        .map_err(|source| FamaError::io(mod_module_dir.clone(), source))?;

    write_file(
        &project_root.join("mod-lock.yml"),
        &ProjectTemplate::mod_lock_yml(),
    )?;
    write_file(
        &project_root.join("settings.toml"),
        &ProjectTemplate::settings_toml(project_name),
    )?;
    write_file(&src_dir.join("main.ms"), &ProjectTemplate::main_ms())?;

    Ok(())
}

/// 以 UTF-8 无 BOM 文本形式写入文件内容。
fn write_file(path: &Path, content: &str) -> Result<(), FamaError> {
    fs::write(path, content).map_err(|source| FamaError::io(path.to_path_buf(), source))
}

#[cfg(test)]
mod tests {
    use super::validate_project_name;

    /// 校验合法项目名能够原样通过验证。
    #[test]
    fn should_accept_valid_project_name() {
        let validated = validate_project_name("hello_world").unwrap();
        assert_eq!(validated, "hello_world");
    }

    /// 校验空白项目名会被明确拒绝。
    #[test]
    fn should_reject_blank_project_name() {
        let error = validate_project_name("   ").unwrap_err();
        assert!(error.to_string().contains("项目名不能为空"));
    }

    /// 校验带路径分隔符的项目名会被拒绝。
    #[test]
    fn should_reject_project_name_with_separator() {
        let error = validate_project_name("hello/world").unwrap_err();
        assert!(error.to_string().contains("路径分隔符"));
    }
}
