use mascript_compiler_llvm::settings::CompilerSettings;

/// 封装分析服务需要共享的编译器设置，后续阶段会在这里接入真实前端分析。
#[derive(Debug)]
pub struct AnalysisService {
    settings: CompilerSettings,
}

impl AnalysisService {
    /// 创建默认分析服务，统一复用主力编译器的设置加载逻辑。
    pub fn new() -> Self {
        Self {
            settings: mascript_compiler_llvm::load_settings(),
        }
    }

    /// 暴露当前编译器设置引用，便于后续诊断和分析阶段保持同一份配置。
    pub fn settings(&self) -> &CompilerSettings {
        &self.settings
    }
}

impl Default for AnalysisService {
    /// 创建默认分析服务，方便服务器状态结构按默认值组合。
    fn default() -> Self {
        Self::new()
    }
}
