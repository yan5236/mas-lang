use std::env;
use std::fs;
use std::path::PathBuf;
use std::io::{self, Write};
use std::process::{Command, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::diagnostic::{CompilerError, MessageId, render_message};
use crate::settings::{CompilerSettings, UiLanguage, load_settings_with_diagnostic};
use crate::source::SourceFile;
use crate::{CompileOptions, compile_to_executable};

enum CliCommand {
    Help,
    Build {
        file: PathBuf,
        output: Option<PathBuf>,
    },
    Run {
        file: PathBuf,
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
        .unwrap_or_else(|| "compiler-llvm".to_string());
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
        CliCommand::Build { file, output } => build_file(&settings, file, output),
        CliCommand::Run { file } => run_file(&settings, file),
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
            other => {
                return Err(CompilerError::cli(
                    MessageId::CliUnexpectedArgument,
                    vec![other.to_string()],
                ));
            }
        }
    }

    Ok(CliCommand::Build { file, output })
}

/// 解析 `run` 子命令参数。
fn parse_run_args(args: &[String]) -> Result<CliCommand, CompilerError> {
    if args.is_empty() {
        return Err(CompilerError::cli(MessageId::CliMissingInputFile, vec![]));
    }
    if args.len() > 1 {
        return Err(CompilerError::cli(
            MessageId::CliUnexpectedArgument,
            vec![args[1].clone()],
        ));
    }
    Ok(CliCommand::Run {
        file: PathBuf::from(&args[0]),
    })
}

/// 读取源码并执行编译命令，把结果输出到终端。
fn build_file(settings: &CompilerSettings, path: PathBuf, output: Option<PathBuf>) -> ExitCode {
    let source = match read_source(&path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{}", error.render(settings.language, None));
            return ExitCode::FAILURE;
        }
    };

    match compile_to_executable(&source, settings, &CompileOptions::new(output)) {
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

/// 读取源码、编译临时可执行文件并立即运行。
fn run_file(settings: &CompilerSettings, path: PathBuf) -> ExitCode {
    let source = match read_source(&path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{}", error.render(settings.language, None));
            return ExitCode::FAILURE;
        }
    };

    let output_path = temp_run_binary_path();
    let binary =
        match compile_to_executable(&source, settings, &CompileOptions::new(Some(output_path))) {
            Ok(binary) => binary,
            Err(error) => {
                eprintln!("{}", error.render(settings.language, Some(&source)));
                return ExitCode::FAILURE;
            }
        };

    match Command::new(binary).output() {
        Ok(output) if output.status.success() => {
            if write_child_output(&output.stdout, &output.stderr).is_err() {
                eprintln!(
                    "{}",
                    CompilerError::cli(
                        MessageId::CliRunBinaryFailed,
                        vec!["写入子进程输出失败".to_string()]
                    )
                    .render(settings.language, None)
                );
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Ok(output) if !output.stderr.is_empty() => {
            let _ = io::stderr().write_all(&output.stderr);
            ExitCode::FAILURE
        }
        Ok(output) => {
            eprintln!(
                "{}",
                CompilerError::cli(
                    MessageId::CliRunBinaryExited,
                    vec![output.status.to_string()]
                )
                    .render(settings.language, None)
            );
            ExitCode::FAILURE
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

/// 把子进程的标准输出和标准错误原样转发到当前终端，保留运行时诊断文本。
fn write_child_output(stdout: &[u8], stderr: &[u8]) -> io::Result<()> {
    io::stdout().write_all(stdout)?;
    io::stderr().write_all(stderr)?;
    Ok(())
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

/// 为 `run` 子命令生成一个唯一的临时可执行文件路径。
fn temp_run_binary_path() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    env::temp_dir().join(format!("mascript-llvm-run-{unique}.exe"))
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

    format!(
        "{title}\n{usage}\n\n{commands}\n\n{options_title}:\n{option_line}",
        title = render_message(language, MessageId::CliHelpTitle, &[]),
        usage = usage,
        commands = commands,
        options_title = options_title,
        option_line = option_line
    )
}

#[cfg(test)]
mod tests {
    use super::{parse_args, render_help};
    use crate::settings::UiLanguage;

    /// 校验英文帮助文本会按当前语言输出英文内容。
    #[test]
    fn should_render_english_help() {
        let help = render_help("compiler-llvm", UiLanguage::EnUs);
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
