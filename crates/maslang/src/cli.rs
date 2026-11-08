use crate::temp_directory::TemporaryDirectory;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use crate::diagnostic::{CompilerError, MessageId, render_message};
use crate::settings::{CompilerSettings, UiLanguage, load_settings_with_diagnostic};
use crate::source::SourceFile;
use crate::{CompileOptions, compile_to_executable};

enum CliCommand {
    Help,
    Build {
        file: PathBuf,
        output: Option<PathBuf>,
        options: CompileOptions,
    },
    Run {
        file: PathBuf,
        options: CompileOptions,
    },
}

/// 解析命令行参数、执行对应编译流程，并把结果映射为进程退出码。
pub fn run() -> ExitCode {
    let (settings, settings_diagnostic) = load_settings_with_diagnostic();
    if let Some(diagnostic) = settings_diagnostic {
        eprintln!("{}", diagnostic.render(settings.language, None));
    }

    let program_name = env::args()
        .next()
        .unwrap_or_else(|| "maslang".to_string());
    let args: Vec<String> = env::args().skip(1).collect();

    let command = match parse_args(&args) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("{}", error.render(settings.language, None));
            eprintln!();
            println!("{}", render_help(&program_name, settings.language));
            return ExitCode::FAILURE;
        }
    };

    match command {
        CliCommand::Help => {
            println!("{}", render_help(&program_name, settings.language));
            ExitCode::SUCCESS
        }
        CliCommand::Build {
            file,
            output,
            options,
        } => build_file(&settings, file, output, options),
        CliCommand::Run { file, options } => run_file(&settings, file, options),
    }
}

/// 解析原始命令行参数，并转换为内部命令结构。
fn parse_args(args: &[String]) -> Result<CliCommand, CompilerError> {
    if args.is_empty() {
        return Ok(CliCommand::Help);
    }

    match args[0].as_str() {
        "-h" | "--help" | "help" => Ok(CliCommand::Help),
        "build" => parse_build_args(&args[1..]),
        "run" => parse_run_args(&args[1..]),
        other => Err(CompilerError::cli(
            MessageId::CliUnknownCommand,
            vec![other.to_string()],
        )),
    }
}

/// 解析 `build` 子命令参数。
fn parse_build_args(args: &[String]) -> Result<CliCommand, CompilerError> {
    if args.is_empty() {
        return Err(CompilerError::cli(MessageId::CliMissingInputFile, vec![]));
    }

    let file = PathBuf::from(&args[0]);
    let mut output = None;
    let mut options = CompileOptions::new(None);
    let mut index = 1usize;

    while index < args.len() {
        match args[index].as_str() {
            "-o" | "--output" => {
                let path = args
                    .get(index + 1)
                    .ok_or_else(|| CompilerError::cli(MessageId::CliMissingOutputPath, vec![]))?;
                output = Some(PathBuf::from(path));
                index += 2;
            }
            "--allow-native-source-build" => {
                options.allow_native_source_build = true;
                index += 1;
            }
            "-O0" | "-O1" | "-O2" | "-O3" => {
                options.opt_level =
                    crate::optimization::OptLevel::parse(&args[index][2..]).unwrap();
                index += 1;
            }
            "--emit-dir" => {
                let dir = args.get(index + 1).ok_or_else(|| {
                    CompilerError::cli(
                        MessageId::CliUnexpectedArgument,
                        vec!["--emit-dir requires a path".into()],
                    )
                })?;
                options.emit_dir = Some(PathBuf::from(dir));
                index += 2;
            }
            other => {
                return Err(CompilerError::cli(
                    MessageId::CliUnexpectedArgument,
                    vec![other.to_string()],
                ));
            }
        }
    }

    Ok(CliCommand::Build {
        file,
        output,
        options,
    })
}

/// 解析 `run` 子命令参数。
fn parse_run_args(args: &[String]) -> Result<CliCommand, CompilerError> {
    if args.is_empty() {
        return Err(CompilerError::cli(MessageId::CliMissingInputFile, vec![]));
    }
    let file = PathBuf::from(&args[0]);
    let mut options = CompileOptions::new(None);
    let mut index = 1usize;
    while index < args.len() {
        match args[index].as_str() {
            "--allow-native-source-build" => {
                options.allow_native_source_build = true;
                index += 1;
            }
            "-O0" | "-O1" | "-O2" | "-O3" => {
                options.opt_level =
                    crate::optimization::OptLevel::parse(&args[index][2..]).unwrap();
                index += 1;
            }
            "--emit-dir" => {
                let dir = args.get(index + 1).ok_or_else(|| {
                    CompilerError::cli(
                        MessageId::CliUnexpectedArgument,
                        vec!["--emit-dir requires a path".into()],
                    )
                })?;
                options.emit_dir = Some(PathBuf::from(dir));
                index += 2;
            }
            other => {
                return Err(CompilerError::cli(
                    MessageId::CliUnexpectedArgument,
                    vec![other.to_string()],
                ));
            }
        }
    }
    Ok(CliCommand::Run { file, options })
}

/// 读取源码并执行编译命令，把结果输出到终端。
fn build_file(
    settings: &CompilerSettings,
    path: PathBuf,
    output: Option<PathBuf>,
    mut options: CompileOptions,
) -> ExitCode {
    let source = match read_source(&path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{}", error.render(settings.language, None));
            return ExitCode::FAILURE;
        }
    };

    options.output_path = output;
    match compile_to_executable(&source, settings, &options) {
        Ok(binary) => {
            println!("{}", binary.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", error.render(settings.language, Some(&source)));
            ExitCode::FAILURE
        }
    }
}

/// 在独占临时目录编译并运行源码，继承终端输入输出；退出后清理可执行文件及链接附属文件。
fn run_file(settings: &CompilerSettings, path: PathBuf, mut options: CompileOptions) -> ExitCode {
    let source = match read_source(&path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{}", error.render(settings.language, None));
            return ExitCode::FAILURE;
        }
    };

    let workspace = match TemporaryDirectory::create("mascript-llvm-run") {
        Ok(workspace) => workspace,
        Err(error) => {
            eprintln!(
                "{}",
                CompilerError::cli(MessageId::CliRunBinaryFailed, vec![error.to_string()])
                    .render(settings.language, None)
            );
            return ExitCode::FAILURE;
        }
    };
    let output_path = workspace
        .path()
        .join(format!("program{}", std::env::consts::EXE_SUFFIX));
    options.output_path = Some(output_path);
    let binary = match compile_to_executable(&source, settings, &options) {
        Ok(binary) => binary,
        Err(error) => {
            eprintln!("{}", error.render(settings.language, Some(&source)));
            return ExitCode::FAILURE;
        }
    };

    match Command::new(binary).status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) if status.code() == Some(1) => ExitCode::FAILURE,
        Ok(status) => {
            eprintln!(
                "{}",
                CompilerError::cli(
                    MessageId::CliRunBinaryExited,
                    vec![status.to_string()]
                )
                .render(settings.language, None)
            );
            status
                .code()
                .and_then(|code| u8::try_from(code).ok())
                .map_or(ExitCode::FAILURE, ExitCode::from)
        }
        Err(error) => {
            eprintln!(
                "{}",
                CompilerError::cli(MessageId::CliRunBinaryFailed, vec![error.to_string()])
                    .render(settings.language, None)
            );
            ExitCode::FAILURE
        }
    }
}

/// 从磁盘读取 UTF-8 源码，并包装成统一的源码对象。
fn read_source(path: &PathBuf) -> Result<SourceFile, CompilerError> {
    let text = fs::read_to_string(path).map_err(|error| {
        CompilerError::cli(
            MessageId::CliReadSourceFailed,
            vec![path.display().to_string(), error.to_string()],
        )
    })?;
    Ok(SourceFile::new(path.display().to_string(), text))
}

/// 根据当前语言生成完整的命令行帮助文本。
fn render_help(program_name: &str, language: UiLanguage) -> String {
    let usage = match language {
        UiLanguage::ZhCn => format!("用法: {program_name} <build|run> <file> [选项]"),
        UiLanguage::EnUs => format!("Usage: {program_name} <build|run> <file> [options]"),
    };
    let commands = render_message(language, MessageId::CliHelpCommands, &[]);
    let options_title = render_message(language, MessageId::CliHelpOptions, &[]);
    let option_line = match language {
        UiLanguage::ZhCn => "  -o, --output <path>      指定输出文件路径",
        UiLanguage::EnUs => "  -o, --output <path>      Set the output executable path",
    };
    let native_option_line = match language {
        UiLanguage::ZhCn => "  --allow-native-source-build  允许维护者以源码方式刷新原生产物",
        UiLanguage::EnUs => {
            "  --allow-native-source-build  Allow maintainers to refresh native artifacts from C sources"
        }
    };

    format!(
        "{title}\n{usage}\n\n{commands}\n\n{options_title}:\n{option_line}\n{native_option_line}\n  -O0|-O1|-O2|-O3          Optimization level (default: O2)\n  --emit-dir <path>         Write raw IR, optimized IR and assembly",
        title = render_message(language, MessageId::CliHelpTitle, &[]),
        usage = usage,
        commands = commands,
        options_title = options_title,
        option_line = option_line,
        native_option_line = native_option_line
    )
}

#[cfg(test)]
mod tests {
    use super::{parse_args, render_help};
    use crate::settings::UiLanguage;

    /// 校验英文帮助文本会按当前语言输出英文内容。
    #[test]
    fn should_render_english_help() {
        let help = render_help("maslang", UiLanguage::EnUs);
        assert!(help.contains("Usage"));
        assert!(help.contains("Build a Windows executable"));
    }

    /// 校验 `build` 子命令能够正确解析输出路径参数。
    #[test]
    fn should_parse_build_command() {
        let args = vec![
            "build".to_string(),
            "main.ms".to_string(),
            "-o".to_string(),
            "out.exe".to_string(),
        ];
        let command = parse_args(&args).unwrap();
        assert!(matches!(command, super::CliCommand::Build { .. }));
    }
}
