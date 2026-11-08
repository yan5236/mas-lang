# 功能需求：字符串集合

> 本功能用于承接 `specs/002-mascript-language-roadmap/tasks.md` 中未完成的 `3.8 实现字符串实例方法集合`，交付范围仅覆盖主力编译器 `maslang`。

## 需求 1：定义可用的字符串实例方法集合

**用户故事**：作为 Mas lang 开发者，我想要直接对 `string` 值调用一组基础实例方法，以便在不引入额外样板代码的情况下完成常见文本判断与清洗。

#### 验收标准 (EARS格式)

1. WHEN 开发者对 `string` 值调用 `trim()`、`contains(string)`、`starts_with(string)`、`ends_with(string)` 或 `replace(string, string)` THEN 系统 SHALL 在 `maslang` 中把该语法解析为合法的字符串实例方法调用。
2. IF 开发者对 `string` 值调用当前集合之外的方法 THEN 系统 SHALL 给出明确的未支持方法诊断，而不是静默接受或降级到其他语义。
3. IF 开发者把上述字符串实例方法用于 `int`、`float`、`bool`、`list`、`map` 或 `array` THEN 系统 SHALL 给出目标类型不匹配的编译诊断。
4. WHEN 开发者继续对 `list` 与 `map` 使用现有实例方法 THEN 系统 SHALL 保持原有方法集合与调用语义不变。

## 需求 2：定义字符串实例方法的语义与返回类型

**用户故事**：作为 Mas lang 开发者，我想要字符串方法拥有稳定、可预测的参数规则和返回值，以便我能把它们直接组合进条件判断、变量赋值和模板表达式。

#### 验收标准 (EARS格式)

1. WHEN 开发者调用 `text.trim()` THEN 系统 SHALL 返回去除字符串首尾空白后的 `string` 结果，并保留中间文本内容不变。
2. WHEN 开发者调用 `text.contains(needle)`、`text.starts_with(prefix)` 或 `text.ends_with(suffix)` THEN 系统 SHALL 要求参数数量为 1 且参数类型为 `string`，并返回 `bool` 结果。
3. WHEN 开发者调用 `text.replace(from, to)` THEN 系统 SHALL 要求参数数量为 2 且两个参数类型都为 `string`，并返回按从左到右规则替换全部非重叠匹配项后的 `string` 结果。
4. IF 字符串方法的参数数量或参数类型不符合要求 THEN 系统 SHALL 在语义分析阶段拒绝该调用，并指出具体的方法名与不匹配原因。
5. IF `replace` 的 `from` 参数在运行时为空字符串 THEN 系统 SHALL 直接返回原字符串，避免产生不透明的隐式插入行为。

## 需求 3：打通主编译链路中的字符串方法代码生成与运行时

**用户故事**：作为编译器维护者，我想要字符串实例方法沿用现有字符串 ABI 和 LLVM 后端链路，以便在不引入第二套字符串表示的前提下稳定交付该功能。

#### 验收标准 (EARS格式)

1. WHEN 字符串实例方法通过语义检查 THEN 系统 SHALL 在 `maslang` 的 LLVM 后端生成对应的运行时函数调用，而不是依赖遗留 `compiler`。
2. WHEN 字符串实例方法返回 `string` THEN 系统 SHALL 继续使用现有 `{ ptr, len }` 字符串 ABI 重建返回值，并保持 UTF-8 文本内容不被截断或损坏。
3. WHEN 字符串实例方法返回 `bool` THEN 系统 SHALL 复用现有布尔值表示与条件分支链路，不额外引入新的运行时包装结构。
4. WHERE 源码和运行时文本包含中文或其他 UTF-8 内容 WHEN 开发者执行上述字符串方法 THEN 系统 SHALL 保持文本比较、裁剪和替换结果与 UTF-8 输入一致。

## 需求 4：建立可追溯的验证覆盖

**用户故事**：作为维护者，我想要为字符串实例方法建立正反两类测试与路线图回写点，以便后续实现可以明确证明功能已经完成。

#### 验收标准 (EARS格式)

1. WHEN 团队实现字符串实例方法集合 THEN 系统 SHALL 补充解析层、语义层、LLVM 后端层与运行时层的自动化测试。
2. WHEN 团队验证该功能 THEN 系统 SHALL 覆盖至少一个包含中文文本的正向样例，以及未支持方法、参数数量错误、参数类型错误三类负向样例。
3. WHEN 本功能完成验收 THEN 系统 SHALL 能把实现结果回写到 `specs/002-mascript-language-roadmap/tasks.md` 的 `3.8` 条目。

## 技术约束 (非功能需求)

1. 主线范围：本功能只修改 `maslang` 相关模块，遗留 `compiler` 不纳入交付范围。
2. 编码约束：新增或更新的规格文档、测试样例与源码文件必须使用 UTF-8 无 BOM。
3. 兼容约束：不得破坏现有 `list`、`map`、模板字符串、`string()` 转换与字符串拼接链路。
4. 范围约束：本功能不包含字符串索引、字符串切片、`split`、大小写转换、正则能力与多行注释。
