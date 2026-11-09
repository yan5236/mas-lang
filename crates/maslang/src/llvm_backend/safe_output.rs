use inkwell::values::{BasicMetadataValueEnum, FunctionValue, IntValue};

use super::*;
use crate::ast::Expr;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 编译 `print`：保持无返回值；写入失败时进入统一语言错误分支，不以 Rust panic 终止进程。
    pub(super) fn compile_print_call(
        &mut self,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let (function, args) = self.build_print_arguments(arguments, span)?;
        let status = self.build_print_status(function, args, "call.print")?;
        let valid = self.map_builder(self.builder.build_int_compare(
            inkwell::IntPredicate::NE,
            status,
            self.context.i8_type().const_zero(),
            "print.ok",
        ))?;
        self.emit_output_check(valid)?;
        Ok(CodegenValue::Unit)
    }

    /// 编译 `try_print`：把运行时写入状态转换成调用方可判断的布尔值。
    pub(super) fn compile_try_print_call(
        &mut self,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let (function, args) = self.build_print_arguments(arguments, span)?;
        let status = self.build_print_status(function, args, "call.try_print")?;
        let ok = self.map_builder(self.builder.build_int_compare(
            inkwell::IntPredicate::NE,
            status,
            self.context.i8_type().const_zero(),
            "try_print.ok",
        ))?;
        Ok(TypedValue {
            value_type: ValueType::Bool,
            value: ok.into(),
        })
    }

    /// 按实参类型选择打印运行时入口，并把字符串拆成 `ptr + len` 参数。
    fn build_print_arguments(
        &mut self,
        arguments: &[Expr],
        span: Span,
    ) -> Result<(FunctionValue<'ctx>, Vec<BasicMetadataValueEnum<'ctx>>), CompilerError> {
        let compiled = self.compile_expr(&arguments[0])?;
        let argument = self.expect_value(compiled)?;
        match argument.value_type.clone() {
            ValueType::Int => Ok((self.runtime.print_int, vec![argument.value.into()])),
            ValueType::Float => Ok((self.runtime.print_float, vec![argument.value.into()])),
            ValueType::Bool => {
                let bool_value = self.map_builder(self.builder.build_int_z_extend(
                    argument.value.into_int_value(),
                    self.context.i8_type(),
                    "print.bool.cast",
                ))?;
                Ok((self.runtime.print_bool, vec![bool_value.into()]))
            }
            ValueType::String => {
                let string_value = argument.value.into_struct_value();
                let ptr = self.extract_string_part(string_value, 0, "print.ptr")?;
                let len = self.extract_string_part(string_value, 1, "print.len")?;
                Ok((self.runtime.print_string, vec![ptr.into(), len.into()]))
            }
            ValueType::Struct(_)
            | ValueType::Nullable(_)
            | ValueType::Null
            | ValueType::Unit
            | ValueType::SafeConvertResult(_)
            | ValueType::List(_)
            | ValueType::Map(_, _)
            | ValueType::Array(_, _) => Err(CompilerError::build_message(
                "print 不能打印当前值类型",
                span,
            )),
        }
    }

    /// 调用打印入口并返回运行时状态：`1` 表示写入成功，`0` 表示标准输出写入失败。
    fn build_print_status(
        &mut self,
        function: FunctionValue<'ctx>,
        args: Vec<BasicMetadataValueEnum<'ctx>>,
        call_name: &str,
    ) -> Result<IntValue<'ctx>, CompilerError> {
        let call = self.map_builder(self.builder.build_call(function, &args, call_name))?;
        Ok(call.try_as_basic_value().unwrap_basic().into_int_value())
    }

    /// 条件成立时继续执行；失败分支调用统一输出错误函数后终止，避免吞掉 I/O 失败。
    fn emit_output_check(&mut self, valid: IntValue<'ctx>) -> Result<(), CompilerError> {
        let function = self.current_function.expect("function context");
        let ready = self.context.append_basic_block(function, "output.ready");
        let failed = self.context.append_basic_block(function, "output.failed");
        self.map_builder(self.builder.build_conditional_branch(valid, ready, failed))?;
        self.builder.position_at_end(failed);
        let error = self.module.get_function("mas_output_error").unwrap_or_else(|| {
            self.module.add_function(
                "mas_output_error",
                self.context.void_type().fn_type(&[], false),
                None,
            )
        });
        self.map_builder(self.builder.build_call(error, &[], ""))?;
        self.map_builder(self.builder.build_unreachable())?;
        self.builder.position_at_end(ready);
        Ok(())
    }
}
