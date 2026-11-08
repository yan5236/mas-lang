# 功能需求：time 标准库

> 本功能用于承接 [tasks.md](../002-mascript-language-roadmap/tasks.md#L50) 中未完成的 `5.5 实现 std.io、std.time、std.fs、std.net、std.math 标准库入口`，当前交付范围聚焦 `stdlib/time` 标准库包与 `examples/projects/huoqutime` 使用示例，不在本轮直接扩展其他标准库模块。

## 需求 1：提供当前系统时间读取能力

**用户故事**：作为 Mas lang 开发者，我想要从 `time` 标准库直接获取当前系统时间，以便在日志、接口签名和业务流程中使用统一的当前时间来源。

#### 验收标准 (EARS格式)

1. WHEN 开发者导入 `time` 模块并调用 `now_unix()` THEN 系统 SHALL 返回当前 UTC 时间对应的 Unix 秒级时间戳，类型为 `int`。
2. WHEN 开发者导入 `time` 模块并调用 `now_unix_millis()` THEN 系统 SHALL 返回当前 UTC 时间对应的 Unix 毫秒级时间戳，类型为 `int`。
3. WHEN 开发者连续调用 `now_unix()` 或 `now_unix_millis()` THEN 系统 SHALL 每次都从宿主系统重新读取当前时间，而不是返回编译期常量或缓存值。
4. IF 宿主运行时无法读取系统时间 THEN 系统 SHALL 给出明确的中文运行时错误，而不是返回伪造值或空字符串。

## 需求 2：提供标准时间字符串格式输出

**用户故事**：作为 Mas lang 开发者，我想要直接获得通用的时间格式字符串，以便和 HTTP 协议、日志系统以及外部服务对接。

#### 验收标准 (EARS格式)

1. WHEN 开发者调用 `now_iso8601()` THEN 系统 SHALL 返回符合 ISO 8601 / RFC 3339 的 UTC 时间字符串，类型为 `string`。
2. WHEN 开发者调用 `now_rfc2822()` THEN 系统 SHALL 返回符合 RFC 2822 的时间字符串，类型为 `string`。
3. WHEN 开发者调用 `now_rfc7231()` THEN 系统 SHALL 返回符合 RFC 7231 HTTP-date 规范的 GMT 时间字符串，类型为 `string`。
4. IF 返回结果包含时区信息 THEN 系统 SHALL 对 `now_iso8601()` 使用 UTC 偏移表示，对 `now_rfc7231()` 使用 `GMT` 文本表示，避免依赖宿主本地时区。

## 需求 3：提供计时器与阻塞等待能力

**用户故事**：作为 Mas lang 开发者，我想要用 `time` 标准库测量代码耗时并执行简单等待，以便实现轮询、重试和基础性能观测。

#### 验收标准 (EARS格式)

1. WHEN 开发者调用 `monotonic_millis()` THEN 系统 SHALL 返回基于单调时钟的毫秒计数值，类型为 `int`，用于计算耗时差值。
2. WHEN 开发者在同一进程内先后两次调用 `monotonic_millis()` THEN 系统 SHALL 保证后一次结果不小于前一次结果，前提是宿主时钟正常工作。
3. WHEN 开发者调用 `sleep(ms)` AND `ms` 为大于等于 `0` 的整数 THEN 系统 SHALL 阻塞当前线程至少指定毫秒数。
4. IF 开发者向 `sleep(ms)` 传入负数 THEN 系统 SHALL 在运行前拒绝该调用，并给出明确的中文诊断或运行时错误说明。

## 需求 4：提供稳定的模块公开入口

**用户故事**：作为标准库维护者，我想要把 `time` 模块整理成稳定公开入口，以便应用工程可以通过统一导入方式使用时间能力。

#### 验收标准 (EARS格式)

1. WHEN 团队实现 `stdlib/time` THEN 系统 SHALL 在 `src/index.ms` 或等价聚合入口中公开 `now_unix`、`now_unix_millis`、`now_iso8601`、`now_rfc2822`、`now_rfc7231`、`monotonic_millis`、`sleep`。
2. WHEN 应用工程在 `mod.yml` 中声明对 `time` 的本地路径依赖 THEN 系统 SHALL 允许调用方通过 `import time { ... }` 导入公开函数。
3. IF 标准库内部存在私有辅助模块 THEN 系统 SHALL 仅暴露标记为 `pub` 或通过 `export import` 重导出的符号，不泄漏内部实现细节。
4. WHERE 当前阶段 `std.time` 总入口尚未完成 WHEN 团队交付本功能 THEN 系统 SHALL 保持 `time` 包导入方式稳定，并为后续映射到 `std.time` 预留一致的公开 API 名称。

## 需求 5：建立可追溯的示例与验证覆盖

**用户故事**：作为维护者，我想要同时拥有标准库示例和验证路径，以便后续可以快速确认 time 标准库对外行为没有回归。

#### 验收标准 (EARS格式)

1. WHEN 团队完成 `time` 标准库最小能力 THEN 系统 SHALL 在 `examples/projects/huoqutime` 中提供一个完整示例，展示时间戳获取、三种格式化输出和计时器耗时测量。
2. WHEN 团队验证 `huoqutime` 示例 THEN 系统 SHALL 覆盖 `import time { ... }` 的依赖导入路径，而不是仅验证本地模块调用。
3. WHEN 团队为本功能补充验证 THEN 系统 SHALL 覆盖至少一条正向样例，证明 `ISO 8601`、`RFC 2822`、`RFC 7231` 输出均非空且格式前缀符合预期。
4. WHEN 本功能完成验收 THEN 系统 SHALL 能把结果回写到 [tasks.md](../002-mascript-language-roadmap/tasks.md#L50) 的 `5.5` 标准库入口任务。

## 需求 6：通过标准库原生绑定承载宿主时间能力

**用户故事**：作为标准库维护者，我希望 `time` 模块通过通用原生绑定能力调用独立的宿主实现，而不是继续把 `time` API 名称硬编码进编译器，这样后续扩展时间能力时不需要反复修改编译器业务逻辑。

#### 验收标准 (EARS格式)

1. WHEN 团队为 `time` 标准库新增宿主函数 THEN 系统 SHALL 通过标准库原生绑定声明映射到宿主符号，而不是新增 `__time_*` 形式的 time 专用内建函数。
2. WHEN 编译器为标准库提供宿主调用能力 THEN 系统 SHALL 只提供与模块名无关的通用原生符号声明、调用和链接机制，不在语义分析或代码生成阶段枚举 `now_unix`、`sleep` 等 time API 名称。
3. IF `time` 包需要处理平台差异 THEN 系统 SHALL 把差异封装在 `stdlib/time` 自带的 C 宿主层或其构建配置中，而不是散落在编译器业务代码中。
4. WHEN `time` 包迁移完成 THEN 系统 SHALL 允许后续新增时间 API 时仅修改 `stdlib/time` 的 MS 包装层、原生绑定层和 C 宿主层，而不需要新增 time 专项编译器分支。

## 技术约束 (非功能需求)

1. 主线范围：本功能优先修改 `stdlib/time`、`examples/projects/huoqutime`，以及为“通用原生绑定/链接能力”所必需的 `maslang`、`fama` 主线模块，不扩展遗留 `compiler`，也不再接受新的 time 专项编译器内建。
2. 编码约束：新增或更新的规格文档、源码、示例与测试文件必须使用 UTF-8 无 BOM。
3. 时区约束：所有“当前时间”字符串接口默认基于 UTC 生成；`RFC 7231` 输出固定使用 `GMT`。
4. 范围约束：本功能不包含时区转换、自定义格式模板、日期解析、日历运算、周期调度、异步定时器与时区数据库支持。
5. 兼容约束：API 名称与导入方式应兼容后续 `std.time` 总入口映射，避免未来再次重命名公开函数。
6. 构建约束：当前主线通过 `rustc` 直接编译临时 `bootstrap.rs` 参与最终链接；若本功能引入 C 宿主层，则必须补齐可复现的原生源码编译、产物选择与链接方案，不能依赖人工手动预编译。
7. 跨平台约束：时间宿主层必须以统一 ABI 对外暴露能力，并通过平台适配保证 Windows 与 POSIX 类平台在 UTC、格式化、单调时钟和睡眠语义上保持一致。
