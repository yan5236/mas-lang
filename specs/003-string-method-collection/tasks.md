# 实施任务清单：字符串集合

> 本清单对应的主编译器范围实现与验证已完成，以下任务状态用于追踪 `maslang` 的实际交付结果。

## 阶段 1：规格对齐与语法入口

- [x] 1.1 在 `specs/003-string-method-collection/` 三件套完成评审定稿，并确认该功能对应路线图任务 `3.8` [关联: design.md 1, 2.2]
- [x] 1.2 扩展 `crates/maslang/src/ast.rs` 的 `MethodName`，新增 `trim`、`starts_with`、`ends_with`、`replace` 并复用 `contains` 作为字符串方法名 [依赖: 1.1] [验证 Req 1]
- [x] 1.3 调整 `crates/maslang/src/parser.rs` 的方法名解析与报错路径，确保字符串方法能够进入统一的 `Expr::MethodCall` 表达式树 [依赖: 1.2] [验证 Req 1]
- [x] 1.4 为解析层补充字符串方法正向样例与未知方法负向样例，避免新方法破坏现有 `list/map` 方法解析 [依赖: 1.3] [验证 Req 1, Req 4]

## 阶段 2：语义分析与诊断

- [x] 2.1 在 `crates/maslang/src/semantic` 中新增 `ValueType::String` 的方法分派逻辑，并把 `trim/contains/starts_with/ends_with/replace` 映射到明确的返回类型 [依赖: 1.3] [验证 Req 2]
- [x] 2.2 为 `contains/starts_with/ends_with` 实现“1 个 `string` 参数”的统一校验，为 `replace` 实现“2 个 `string` 参数”的统一校验 [依赖: 2.1] [验证 Req 2]
- [x] 2.3 调整错误消息，覆盖字符串目标类型错误、未支持方法、参数数量错误、参数类型错误四类场景 [依赖: 2.1] [验证 Req 1, Req 2]
- [x] 2.4 在 `crates/maslang/src/semantic/tests.rs` 中补充正反两类测试，确保字符串方法与现有容器方法共存 [依赖: 2.3] [验证 Req 2, Req 4]

## 阶段 3：LLVM 后端与运行时实现

- [x] 3.1 在 `crates/maslang/src/llvm_backend/runtime.rs` 与相关模块中声明 `string_trim`、`string_contains`、`string_starts_with`、`string_ends_with`、`string_replace` 运行时符号 [依赖: 2.1] [验证 Req 3]
- [x] 3.2 在 `crates/maslang/src/llvm_backend/expressions.rs` 中封装字符串方法调用编译逻辑，统一处理字符串参数拆包、布尔返回值和字符串结果重建 [依赖: 3.1] [验证 Req 3]
- [x] 3.3 在 `crates/maslang/src/runtime_bootstrap.rs` 中实现五个字符串运行时函数，并复用现有 UTF-8 辅助工具避免字节级截断 [依赖: 3.1] [验证 Req 3]
- [x] 3.4 为 `replace(from, to)` 的空 `from` 场景补充运行时保护逻辑，确保其返回原字符串而不是执行隐式插入 [依赖: 3.3] [验证 Req 2, Req 3]

## 阶段 4：验收覆盖与路线图回写

- [x] 4.1 在 `crates/maslang/src/llvm_backend/tests.rs` 中增加 IR 级断言，验证新增字符串方法会发出正确的运行时符号调用 [依赖: 3.2] [验证 Req 3, Req 4]
- [x] 4.2 在 `crates/maslang/src/testdata/` 中新增至少一个包含中文文本的字符串方法集成样例 [依赖: 3.3] [验证 Req 3, Req 4]
- [x] 4.3 运行解析层、语义层与后端相关测试，确认字符串方法没有回归现有 `list/map`、模板字符串和转换链路 [依赖: 4.1, 4.2] [验证 Req 1, Req 2, Req 3, Req 4]
- [x] 4.4 在功能完成后回写 `specs/002-mascript-language-roadmap/tasks.md` 的 `3.8 实现字符串实例方法集合` 状态，并补充必要的实现备注 [依赖: 4.3] [验证 Req 4]
