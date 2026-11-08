use crate::settings::UiLanguage;
use crate::source::{SourceFile, Span};

/// 表示统一诊断系统中的错误来源阶段。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorStage {
    Cli,
    Settings,
    Lexical,
    Syntax,
    Semantic,
    Build,
}

/// 表示所有本地化消息模板的稳定标识。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageId {
    CustomMessage,
    CliHelpTitle,
    CliHelpUsage,
    CliHelpCommands,
    CliHelpBuild,
    CliHelpRun,
    CliHelpOptions,
    CliHelpOutput,
    CliUnknownCommand,
    CliMissingCommand,
    CliMissingInputFile,
    CliMissingOutputPath,
    CliUnexpectedArgument,
    CliReadSourceFailed,
    CliRunBinaryFailed,
    CliRunBinaryExited,
    SettingsReadFailed,
    SettingsParseFailed,
    SettingsInvalidLanguage,
    LexInvalidCharacter,
    LexTabIndent,
    LexTabInCode,
    LexIndentMultipleOfFour,
    LexIndentJumpTooLarge,
    LexIndentMismatch,
    LexUnterminatedString,
    LexInvalidEscape,
    LexInvalidInt,
    LexInvalidFloat,
    SyntaxExpectedExpression,
    SyntaxExpectedIdentifier,
    SyntaxExpectedEquals,
    SyntaxExpectedRightParen,
    SyntaxExpectedTypeName,
    SyntaxStatementsNeedNewline,
    SyntaxUnsupportedKeyword,
    SyntaxUnsupportedTopLevel,
    SemanticDuplicateVariable,
    SemanticUnknownVariable,
    SemanticTypeMismatch,
    SemanticInvalidUnaryOperand,
    SemanticInvalidBinaryOperand,
    SemanticPrintArity,
    SemanticUnknownFunction,
    SemanticOnlyNamedCall,
    BuildCreateOutputDirFailed,
    BuildWriteObjectFailed,
    BuildWriteBootstrapFailed,
    BuildRustcFailed,
    BuildRustcExitFailed,
    BuildVerifyModuleFailed,
    BuildCreateTargetFailed,
    BuildWriteIrFailed,
}

/// 表示编译流水线中的统一错误对象。
#[derive(Clone, Debug)]
pub struct CompilerError {
    pub stage: ErrorStage,
    pub message_id: MessageId,
    pub args: Vec<String>,
    pub span: Span,
    pub source: Option<SourceFile>,
}

impl CompilerError {
    /// 使用统一字段创建一个新的诊断对象。
    pub fn new(stage: ErrorStage, message_id: MessageId, args: Vec<String>, span: Span) -> Self {
        Self {
            source: None,
            stage,
            message_id,
            args,
            span,
        }
    }

    /// 为错误附加实际所属的原始文件，渲染时优先使用该文件而非入口文件。
    pub fn with_source(mut self, source: SourceFile) -> Self {
        self.source = Some(source);
        self
    }

    /// 构造一个设置读取相关的错误。
    pub fn settings(message_id: MessageId, args: Vec<String>, span: Span) -> Self {
        Self::new(ErrorStage::Settings, message_id, args, span)
    }

    /// 构造一个词法分析阶段错误。
    pub fn lexical(message_id: MessageId, args: Vec<String>, span: Span) -> Self {
        Self::new(ErrorStage::Lexical, message_id, args, span)
    }

    /// 构造一个语法分析阶段错误。
    pub fn syntax(message_id: MessageId, args: Vec<String>, span: Span) -> Self {
        Self::new(ErrorStage::Syntax, message_id, args, span)
    }

    /// 构造一个直接使用给定文本的语法分析阶段错误。
    pub fn syntax_message(message: impl Into<String>, span: Span) -> Self {
        Self::syntax(MessageId::CustomMessage, vec![message.into()], span)
    }

    /// 构造一个语义分析阶段错误。
    pub fn semantic(message_id: MessageId, args: Vec<String>, span: Span) -> Self {
        Self::new(ErrorStage::Semantic, message_id, args, span)
    }

    /// 构造一个直接使用给定文本的语义分析阶段错误。
    pub fn semantic_message(message: impl Into<String>, span: Span) -> Self {
        Self::semantic(MessageId::CustomMessage, vec![message.into()], span)
    }

    /// 构造一个命令行阶段错误。
    pub fn cli(message_id: MessageId, args: Vec<String>) -> Self {
        Self::new(ErrorStage::Cli, message_id, args, Span::new(0, 0))
    }

    /// 构造一个构建阶段错误。
    pub fn build(message_id: MessageId, args: Vec<String>, span: Span) -> Self {
        Self::new(ErrorStage::Build, message_id, args, span)
    }

    /// 构造一个直接使用给定文本的构建阶段错误。
    pub fn build_message(message: impl Into<String>, span: Span) -> Self {
        Self::build(MessageId::CustomMessage, vec![message.into()], span)
    }

    /// 在原有参数前面插入一个额外参数，便于补足路径等上下文信息。
    pub fn with_args_prepended(mut self, value: String) -> Self {
        self.args.insert(0, value);
        self
    }

    /// 根据是否存在源码上下文，把错误渲染为最终展示文本。
    pub fn render(&self, language: UiLanguage, source: Option<&SourceFile>) -> String {
        let message = render_message(language, self.message_id, &self.args);

        if let Some(source) = self.source.as_ref().or(source) {
            let (line, column) = source.line_col(self.span.start);
            let line_text = source.line_text(line);
            let caret_width = self.span.end.saturating_sub(self.span.start).max(1);
            let caret_padding = " ".repeat(column.saturating_sub(1));
            let carets = "^".repeat(caret_width);
            let line_number = line.to_string();

            return format!(
                "{stage}: {message}\n--> {path}:{line}:{column}\n{ln} | {text}\n{pad} | {caret_padding}{carets}",
                stage = render_stage(language, self.stage),
                message = message,
                path = source.path(),
                line = line,
                column = column,
                ln = line_number,
                text = line_text,
                pad = " ".repeat(line_number.len()),
                caret_padding = caret_padding,
                carets = carets
            );
        }

        format!("{}: {}", render_stage(language, self.stage), message)
    }
}

/// 根据当前语言返回阶段名称文本。
pub fn render_stage(language: UiLanguage, stage: ErrorStage) -> &'static str {
    match language {
        UiLanguage::ZhCn => match stage {
            ErrorStage::Cli => "命令行错误",
            ErrorStage::Settings => "设置错误",
            ErrorStage::Lexical => "词法错误",
            ErrorStage::Syntax => "语法错误",
            ErrorStage::Semantic => "语义错误",
            ErrorStage::Build => "构建错误",
        },
        UiLanguage::EnUs => match stage {
            ErrorStage::Cli => "CLI error",
            ErrorStage::Settings => "Settings error",
            ErrorStage::Lexical => "Lexical error",
            ErrorStage::Syntax => "Syntax error",
            ErrorStage::Semantic => "Semantic error",
            ErrorStage::Build => "Build error",
        },
    }
}

/// 根据语言和参数把消息标识渲染成最终文本。
pub fn render_message(language: UiLanguage, id: MessageId, args: &[String]) -> String {
    let arg = |index: usize| args.get(index).cloned().unwrap_or_default();

    match language {
        UiLanguage::ZhCn => match id {
            MessageId::CustomMessage => arg(0),
            MessageId::CliHelpTitle => "Maslang LLVM 编译器".to_string(),
            MessageId::CliHelpUsage => "用法: maslang <build|run> <file> [选项]".to_string(),
            MessageId::CliHelpCommands => "命令:\n  build <file> [-o <path>]  编译为 Windows 可执行文件\n  run <file>                编译到临时可执行文件后立即运行".to_string(),
            MessageId::CliHelpBuild => "把源码编译为 Windows 可执行文件".to_string(),
            MessageId::CliHelpRun => "把源码编译到临时文件后执行".to_string(),
            MessageId::CliHelpOptions => "选项".to_string(),
            MessageId::CliHelpOutput => "指定输出文件路径".to_string(),
            MessageId::CliUnknownCommand => format!("未知命令 `{}`", arg(0)),
            MessageId::CliMissingCommand => "缺少命令，请使用 build 或 run".to_string(),
            MessageId::CliMissingInputFile => "缺少输入源码文件路径".to_string(),
            MessageId::CliMissingOutputPath => "选项 -o/--output 后缺少输出路径".to_string(),
            MessageId::CliUnexpectedArgument => format!("发现多余参数 `{}`", arg(0)),
            MessageId::CliReadSourceFailed => format!("读取源码失败: {}: {}", arg(0), arg(1)),
            MessageId::CliRunBinaryFailed => format!("运行生成的可执行文件失败: {}", arg(0)),
            MessageId::CliRunBinaryExited => format!("程序以非零状态退出: {}", arg(0)),
            MessageId::SettingsReadFailed => format!("读取设置文件失败: {}: {}", arg(0), arg(1)),
            MessageId::SettingsParseFailed => format!("解析设置文件失败: {}: {}", arg(0), arg(1)),
            MessageId::SettingsInvalidLanguage => format!("设置文件中的 language 值无效: {}", arg(1)),
            MessageId::LexInvalidCharacter => format!("发现无法识别的字符 `{}`", arg(0)),
            MessageId::LexTabIndent => "缩进不允许使用制表符，请统一使用 4 个空格".to_string(),
            MessageId::LexTabInCode => "代码中不允许使用制表符，请改用空格".to_string(),
            MessageId::LexIndentMultipleOfFour => "缩进必须是 4 个空格的整数倍".to_string(),
            MessageId::LexIndentJumpTooLarge => "缩进每次只能增加 4 个空格".to_string(),
            MessageId::LexIndentMismatch => "缩进层级与前面的代码块不匹配".to_string(),
            MessageId::LexUnterminatedString => "字符串字面量缺少结束引号".to_string(),
            MessageId::LexInvalidEscape => format!("不支持的转义序列 `\\{}`", arg(0)),
            MessageId::LexInvalidInt => "整数字面量格式无效".to_string(),
            MessageId::LexInvalidFloat => "浮点数字面量格式无效".to_string(),
            MessageId::SyntaxExpectedExpression => "这里需要一个表达式".to_string(),
            MessageId::SyntaxExpectedIdentifier => "这里需要一个标识符".to_string(),
            MessageId::SyntaxExpectedEquals => "变量声明缺少 `=` 初始化符号".to_string(),
            MessageId::SyntaxExpectedRightParen => "缺少右括号 `)`".to_string(),
            MessageId::SyntaxExpectedTypeName => format!("未知的内建类型 `{}`", arg(0)),
            MessageId::SyntaxStatementsNeedNewline => "语句之间需要使用换行分隔".to_string(),
            MessageId::SyntaxUnsupportedKeyword => format!("当前阶段暂不支持 `{}` 语法", arg(0)),
            MessageId::SyntaxUnsupportedTopLevel => "当前阶段只支持变量声明和表达式语句".to_string(),
            MessageId::SemanticDuplicateVariable => format!("变量 `{}` 在当前作用域中重复声明", arg(0)),
            MessageId::SemanticUnknownVariable => format!("未声明变量 `{}`", arg(0)),
            MessageId::SemanticTypeMismatch => format!(
                "类型不匹配，期望 `{}`，实际得到 `{}`",
                arg(0),
                arg(1)
            ),
            MessageId::SemanticInvalidUnaryOperand => "当前一元表达式的操作数类型不支持该运算".to_string(),
            MessageId::SemanticInvalidBinaryOperand => "当前二元表达式的操作数类型不支持该运算".to_string(),
            MessageId::SemanticPrintArity => "内建函数 `print` 需要且只接受一个参数".to_string(),
            MessageId::SemanticUnknownFunction => format!("未声明函数 `{}`", arg(0)),
            MessageId::SemanticOnlyNamedCall => "当前阶段只支持调用具名函数".to_string(),
            MessageId::BuildCreateOutputDirFailed => format!("创建输出目录失败: {}: {}", arg(0), arg(1)),
            MessageId::BuildWriteObjectFailed => format!("写入目标文件失败: {}: {}", arg(0), arg(1)),
            MessageId::BuildWriteBootstrapFailed => format!("写入引导源码失败: {}: {}", arg(0), arg(1)),
            MessageId::BuildRustcFailed => format!("启动 rustc 失败: {}", arg(0)),
            MessageId::BuildRustcExitFailed => format!("rustc 链接失败，退出码: {}", arg(0)),
            MessageId::BuildVerifyModuleFailed => format!("LLVM 模块校验失败: {}", arg(0)),
            MessageId::BuildCreateTargetFailed => format!("创建 LLVM 目标机器失败: {}", arg(0)),
            MessageId::BuildWriteIrFailed => format!("生成目标文件失败: {}", arg(0)),
        },
        UiLanguage::EnUs => match id {
            MessageId::CustomMessage => arg(0),
            MessageId::CliHelpTitle => "Maslang LLVM Compiler".to_string(),
            MessageId::CliHelpUsage => "Usage: maslang <build|run> <file> [options]".to_string(),
            MessageId::CliHelpCommands => "Commands:\n  build <file> [-o <path>]  Build a Windows executable\n  run <file>                Build a temporary executable and run it".to_string(),
            MessageId::CliHelpBuild => "Build source code into a Windows executable".to_string(),
            MessageId::CliHelpRun => "Build source code to a temporary executable and run it".to_string(),
            MessageId::CliHelpOptions => "Options".to_string(),
            MessageId::CliHelpOutput => "Set the output executable path".to_string(),
            MessageId::CliUnknownCommand => format!("Unknown command `{}`", arg(0)),
            MessageId::CliMissingCommand => "Missing command, expected build or run".to_string(),
            MessageId::CliMissingInputFile => "Missing input source file path".to_string(),
            MessageId::CliMissingOutputPath => "Missing output path after -o/--output".to_string(),
            MessageId::CliUnexpectedArgument => format!("Unexpected argument `{}`", arg(0)),
            MessageId::CliReadSourceFailed => format!("Failed to read source: {}: {}", arg(0), arg(1)),
            MessageId::CliRunBinaryFailed => format!("Failed to run generated executable: {}", arg(0)),
            MessageId::CliRunBinaryExited => format!("Program exited with a non-zero status: {}", arg(0)),
            MessageId::SettingsReadFailed => format!("Failed to read settings file: {}: {}", arg(0), arg(1)),
            MessageId::SettingsParseFailed => format!("Failed to parse settings file: {}: {}", arg(0), arg(1)),
            MessageId::SettingsInvalidLanguage => format!("Invalid language value in settings file: {}", arg(1)),
            MessageId::LexInvalidCharacter => format!("Unrecognized character `{}`", arg(0)),
            MessageId::LexTabIndent => "Tabs are not allowed in indentation; use 4 spaces".to_string(),
            MessageId::LexTabInCode => "Tabs are not allowed in code; use spaces instead".to_string(),
            MessageId::LexIndentMultipleOfFour => "Indentation must be a multiple of 4 spaces".to_string(),
            MessageId::LexIndentJumpTooLarge => "Indentation can only increase by 4 spaces at a time".to_string(),
            MessageId::LexIndentMismatch => "Indentation level does not match a previous block".to_string(),
            MessageId::LexUnterminatedString => "String literal is missing the closing quote".to_string(),
            MessageId::LexInvalidEscape => format!("Unsupported escape sequence `\\{}`", arg(0)),
            MessageId::LexInvalidInt => "Invalid integer literal".to_string(),
            MessageId::LexInvalidFloat => "Invalid float literal".to_string(),
            MessageId::SyntaxExpectedExpression => "Expected an expression here".to_string(),
            MessageId::SyntaxExpectedIdentifier => "Expected an identifier here".to_string(),
            MessageId::SyntaxExpectedEquals => "Variable declaration is missing `=`".to_string(),
            MessageId::SyntaxExpectedRightParen => "Missing right parenthesis `)`".to_string(),
            MessageId::SyntaxExpectedTypeName => format!("Unknown builtin type `{}`", arg(0)),
            MessageId::SyntaxStatementsNeedNewline => "Statements must be separated by newlines".to_string(),
            MessageId::SyntaxUnsupportedKeyword => format!("`{}` syntax is not supported in this stage", arg(0)),
            MessageId::SyntaxUnsupportedTopLevel => "Only variable declarations and expression statements are supported in this stage".to_string(),
            MessageId::SemanticDuplicateVariable => format!("Variable `{}` is already declared in this scope", arg(0)),
            MessageId::SemanticUnknownVariable => format!("Undeclared variable `{}`", arg(0)),
            MessageId::SemanticTypeMismatch => format!(
                "Type mismatch: expected `{}`, got `{}`",
                arg(0),
                arg(1)
            ),
            MessageId::SemanticInvalidUnaryOperand => "This unary operator does not support the operand type".to_string(),
            MessageId::SemanticInvalidBinaryOperand => "This binary operator does not support the operand types".to_string(),
            MessageId::SemanticPrintArity => "Builtin function `print` requires exactly one argument".to_string(),
            MessageId::SemanticUnknownFunction => format!("Unknown function `{}`", arg(0)),
            MessageId::SemanticOnlyNamedCall => "Only named function calls are supported in this stage".to_string(),
            MessageId::BuildCreateOutputDirFailed => format!("Failed to create output directory: {}: {}", arg(0), arg(1)),
            MessageId::BuildWriteObjectFailed => format!("Failed to write object file: {}: {}", arg(0), arg(1)),
            MessageId::BuildWriteBootstrapFailed => format!("Failed to write bootstrap source: {}: {}", arg(0), arg(1)),
            MessageId::BuildRustcFailed => format!("Failed to start rustc: {}", arg(0)),
            MessageId::BuildRustcExitFailed => format!("rustc link step failed with exit status: {}", arg(0)),
            MessageId::BuildVerifyModuleFailed => format!("LLVM module verification failed: {}", arg(0)),
            MessageId::BuildCreateTargetFailed => format!("Failed to create LLVM target machine: {}", arg(0)),
            MessageId::BuildWriteIrFailed => format!("Failed to emit object file: {}", arg(0)),
        },
    }
}
