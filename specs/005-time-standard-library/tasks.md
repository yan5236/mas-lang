# 实施任务清单：time 标准库

## 阶段 1：规格重置与迁移边界确认

- [x] 1.1 重新基线化 `specs/005-time-standard-library/` 的 `requirements.md`、`design.md`、`tasks.md`，把方案从“time 专项编译器内建”改为“通用原生绑定 + C 宿主层 + MS 包装层” [关联: design.md 1, 2.1]
- [x] 1.2 确认 `005-time-standard-library` 直接承载“最小原生绑定能力”，并以 `time` 作为首个落地模块 [依赖: 1.1] [验证 Req 6]
- [x] 1.3 确认跨平台目标至少覆盖 Windows 与 POSIX 类平台，并冻结原生产物的组织方式、目录约定与选择策略 [依赖: 1.1] [验证 技术约束 6, 技术约束 7]

## 阶段 2：补齐通用原生绑定最小能力

- [x] 2.1 在 `maslang` 中设计并实现最小原生函数声明机制，例如 `extern func` 或等价语法，仅支持 `int`、`float`、`bool`、`string`、`unit` [依赖: 1.2] [验证 Req 6]
- [x] 2.2 在语义分析与类型系统中为通用原生函数补齐参数检查、返回类型推导和错误诊断，禁止再通过 time 专项函数名分支处理标准库能力 [依赖: 2.1] [验证 Req 6]
- [x] 2.3 在 LLVM 代码生成层实现通用原生调用 lowering，包括基础类型传参和 `string` 返回值的统一 ABI 重建 [依赖: 2.1] [验证 Req 2, Req 6]
- [x] 2.4 在 `runtime_bootstrap.rs` 中抽出与模块无关的原生运行时辅助符号，例如统一字符串构造/复制与原生错误上报入口，不再内嵌 time 业务逻辑 [依赖: 2.3] [验证 Req 3, Req 6]
- [x] 2.5 在 `fama` 与最终链接链路中支持收集包级 C 源、按目标平台编译原生产物并参与最终链接，保证流程可复现且不依赖手工预编译 [依赖: 1.3, 2.1] [验证 技术约束 6, Req 6]
- [x] 2.6 为通用原生绑定补充最小端到端验证，至少覆盖 `int` 返回、`string` 返回、`void` 调用与中文错误路径 [依赖: 2.2, 2.3, 2.4, 2.5] [验证 Req 6]

## 阶段 3：实现 `time` 包自带的 C 宿主层

- [x] 3.1 在 `stdlib/time` 下建立 `native/include/`、`native/common/`、`native/platform/windows/`、`native/platform/posix/` 或等价目录，固定 time 宿主层组织方式 [依赖: 1.3, 2.5] [验证 Req 6]
- [x] 3.2 在 time C 宿主层中实现 UTC 秒级与毫秒级时间戳读取，优先基于标准 C 的 `timespec_get(TIME_UTC)` 或等价能力 [依赖: 3.1] [验证 Req 1]
- [x] 3.3 在 time C 宿主层中实现 `now_iso8601()`、`now_rfc2822()`、`now_rfc7231()` 的 UTC 格式化输出，统一走 `gmtime_s/gmtime_r` 兼容层与固定格式模板 [依赖: 3.2] [验证 Req 2]
- [x] 3.4 在平台适配层中实现 `monotonic_millis()`，明确禁止把 `clock()` 当作单调墙钟接口 [依赖: 3.1] [验证 Req 3, 技术约束 7]
- [x] 3.5 在平台适配层中实现 `sleep(ms)`，保证“至少等待指定毫秒数”并统一负数输入的中文错误语义 [依赖: 3.1] [验证 Req 3]
- [ ] 3.6 为 time C 宿主层补充固定时间点与错误路径验证，至少覆盖 Unix 纪元、闰年日期、`GMT` 输出与宿主失败场景 [依赖: 3.3, 3.4, 3.5] [验证 Req 1, Req 2, Req 3, Req 5]

## 阶段 4：完成 `stdlib/time` 的 MS 包装层

- [x] 4.1 新增 `stdlib/time/src/native.ms` 或等价私有绑定层，声明 `mas_time_*` 原生符号并限制为模块内部使用 [依赖: 2.1, 3.1] [验证 Req 4, Req 6]
- [x] 4.2 重写 `system.ms`，使 `now_unix`、`now_unix_millis`、`now_iso8601`、`now_rfc2822`、`now_rfc7231` 全部通过私有绑定层转发，而不是调用 `__time_*` [依赖: 4.1, 3.2, 3.3] [验证 Req 1, Req 2, Req 4, Req 6]
- [x] 4.3 重写 `timer.ms`，使 `monotonic_millis` 与 `sleep` 全部通过私有绑定层转发，而不是调用 `__time_*` [依赖: 4.1, 3.4, 3.5] [验证 Req 3, Req 4, Req 6]
- [x] 4.4 更新 `index.ms`，仅通过 `export import` 暴露公开函数，继续隐藏原生绑定与平台实现细节 [依赖: 4.2, 4.3] [验证 Req 4]
- [x] 4.5 更新 `settings.toml`、`mod.yml` 或等价包配置，声明 time 包的原生源码与构建入口 [依赖: 2.5, 3.1] [验证 Req 4, Req 6]

## 阶段 5：移除旧的 time 专项编译器耦合

- [x] 5.1 从 `semantic/analysis.rs` 中移除 `__time_*` 保留内建名，防止用户继续通过旧入口绕过标准库边界 [依赖: 4.4] [验证 Req 6]
- [x] 5.2 从 `semantic/operations.rs` 中移除 time 专项返回类型推导与参数检查分支，仅保留通用原生绑定规则 [依赖: 5.1] [验证 Req 6]
- [x] 5.3 从 `llvm_backend/expressions.rs` 中移除 `compile_builtin_time_call` 及其相关分派逻辑，改由通用原生调用路径承接 [依赖: 5.2] [验证 Req 6]
- [x] 5.4 从 `runtime_bootstrap.rs` 与 `llvm_backend/runtime.rs` 中移除 time 专项运行时符号与测试断言，只保留通用运行时辅助能力 [依赖: 5.3] [验证 Req 6]
- [x] 5.5 为旧入口补充回归验证，确认直接调用 `__time_*` 时会以明确方式失败或提示迁移 [依赖: 5.1, 5.3] [验证 Req 6]

## 阶段 6：示例工程与端到端验收

- [x] 6.1 在 `examples/projects/huoqutime/mod.yml` 中继续以本地依赖方式声明 `time`，并确保示例项目能携带所需原生构建元数据 [依赖: 4.5] [验证 Req 4, Req 5]
- [x] 6.2 重写 `examples/projects/huoqutime/src/main.ms` 示例，展示时间戳读取、三种格式化输出、单调耗时测量与 `sleep` 联合使用 [依赖: 4.4, 6.1] [验证 Req 5]
- [x] 6.3 执行 `time` 包与 `huoqutime` 示例的端到端构建验证，覆盖依赖导入、原生编译、最终链接和程序输出 [依赖: 2.6, 3.6, 6.2] [验证 Req 1, Req 2, Req 3, Req 4, Req 5, Req 6]
- [x] 6.4 在验收完成后回写 [tasks.md](../002-mascript-language-roadmap/tasks.md#L50) 的 `5.5` 状态，并备注 time 模块已迁移为“标准库自带 C 宿主层 + 通用原生绑定”模式 [依赖: 6.3] [验证 Req 5]
