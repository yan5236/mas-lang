use std::path::PathBuf;

/// 生成程序的优化档位，与编译器自身的 Cargo profile 相互独立。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OptLevel {
    O0,
    O1,
    #[default]
    O2,
    O3,
}
impl OptLevel {
    /// 解析 CLI 或项目配置的数字档位；拒绝未知值，不静默降级。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "0" => Some(Self::O0),
            "1" => Some(Self::O1),
            "2" => Some(Self::O2),
            "3" => Some(Self::O3),
            _ => None,
        }
    }
    /// 返回 rustc 与外部编译器 CLI 共用的数字参数。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::O0 => "0",
            Self::O1 => "1",
            Self::O2 => "2",
            Self::O3 => "3",
        }
    }
    /// 返回 LLVM 标准模块优化管线，保持严格浮点语义。
    pub(crate) fn pipeline(self) -> &'static str {
        match self {
            Self::O0 => "default<O0>",
            Self::O1 => "default<O1>",
            Self::O2 => "default<O2>",
            Self::O3 => "default<O3>",
        }
    }
    /// 将同一档位映射到目标机器的代码生成优化等级。
    pub(crate) fn llvm(self) -> inkwell::OptimizationLevel {
        match self {
            Self::O0 => inkwell::OptimizationLevel::None,
            Self::O1 => inkwell::OptimizationLevel::Less,
            Self::O2 => inkwell::OptimizationLevel::Default,
            Self::O3 => inkwell::OptimizationLevel::Aggressive,
        }
    }
}

/// 保存构建阶段允许外部指定的输出选项。
pub struct CompileOptions {
    pub output_path: Option<PathBuf>,
    pub allow_native_source_build: bool,
    pub opt_level: OptLevel,
    pub emit_dir: Option<PathBuf>,
}

impl CompileOptions {
    /// 设置生成程序的档位，同时作用于 LLVM IR、机器码和 Rust 运行时。
    pub fn with_opt_level(mut self, level: OptLevel) -> Self {
        self.opt_level = level;
        self
    }

    /// 使用可选输出路径创建新的编译选项对象。
    pub fn new(output_path: Option<PathBuf>) -> Self {
        Self {
            output_path,
            allow_native_source_build: false,
            opt_level: OptLevel::default(),
            emit_dir: None,
        }
    }

    /// 返回一个启用或关闭“维护者源码构建”模式的新选项对象，避免调用侧手写字段赋值。
    pub fn with_allow_native_source_build(mut self, allow_native_source_build: bool) -> Self {
        self.allow_native_source_build = allow_native_source_build;
        self
    }
}
