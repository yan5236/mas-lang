/// 统一维护项目初始化时写入的全部模板内容。
pub struct ProjectTemplate;

impl ProjectTemplate {
    /// 生成 `mod.yml` 的默认内容。
    pub fn mod_yml(project_name: &str) -> String {
        format!("name: {project_name}\nversion: 0.1.0\n\ndependencies: {{}}\n")
    }

    /// 生成 `mod-lock.yml` 的默认占位内容。
    pub fn mod_lock_yml() -> String {
        "{}\n".to_string()
    }

    /// 生成 `settings.toml` 的默认编译配置。
    pub fn settings_toml(project_name: &str) -> String {
        format!(
            "[project]\nname = \"{project_name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ms\"\noutput = \"build\"\nopt-level = 2\n"
        )
    }

    /// 生成 `src/main.ms` 的默认 Hello World 示例。
    pub fn main_ms() -> String {
        "print(\"Hello world.\")\n".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::ProjectTemplate;

    /// 校验 `mod.yml` 模板包含项目名、版本号和空依赖。
    #[test]
    fn should_render_mod_yml_with_expected_fields() {
        let content = ProjectTemplate::mod_yml("hello");

        assert!(content.contains("name: hello"));
        assert!(content.contains("version: 0.1.0"));
        assert!(content.contains("dependencies: {}"));
    }

    /// 校验 `settings.toml` 模板中的项目名与入口配置正确。
    #[test]
    fn should_render_settings_toml_with_expected_values() {
        let content = ProjectTemplate::settings_toml("hello");

        assert!(content.contains("name = \"hello\""));
        assert!(content.contains("entry = \"src/main.ms\""));
        assert!(content.contains("output = \"build\""));
    }

    /// 校验 `main.ms` 模板默认输出 Hello World 示例。
    #[test]
    fn should_render_main_ms_template() {
        let content = ProjectTemplate::main_ms();
        assert_eq!(content, "print(\"Hello world.\")\n");
    }
}
