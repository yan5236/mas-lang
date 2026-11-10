# 实施任务清单：可恢复运行时错误

## 阶段 1：契约与脚手架

- [x] 1.1 确定统一结果表示（复用 `try_result<T>`，只表达成败的操作返回 `bool`）[关联: design.md §2.3]
- [x] 1.2 扩展 `MethodName`（`try_get`/`try_set`/`try_pop`/`try_index`/`try_remove`/`try_slice`）与解析 [关联: design.md §2.3]
- [x] 1.3 语义分析：列表/数组/字典安全方法类型、`try_print` 与受检整数内建类型 [关联: design.md §2.3]
- [x] 1.4 代码生成分派：`safe_lists.rs`/`safe_arrays.rs`/`safe_result.rs`/`safe_maps.rs`/`safe_numeric.rs`/`safe_output.rs` 模块入口 [关联: design.md §2.2]
- [x] 1.5 GC 根布局：`try_result<T>` 载荷按字段 1 登记，保证引用型结果跨语句存活 [关联: design.md §2.3]

## 阶段 2：七项修复

- [x] 2.1 浮点列表排序全序（NaN 末尾、位模式稳定、`-0.0 == 0.0`）[验证 Req 1]
- [x] 2.2 标准输出 I/O 状态化：`print_*` 返回状态、`mas_output_error`、`try_print` [验证 Req 2]
- [x] 2.3 time 标准库 ABI 2：C 结果结构 + 错误码、`try_*` 包装、严格包装保持旧语义 [验证 Req 3]
- [x] 2.4 列表与固定数组安全访问/修改路径 [验证 Req 4]
- [x] 2.5 字典 `try_get`/`try_remove`（区分缺键与 `null` 值）[验证 Req 5]
- [x] 2.6 受检整数运算 `try_add`/`try_sub`/`try_mul`/`try_div`/`try_mod`/`try_neg` [验证 Req 6]
- [x] 2.7 `try_int`/`try_float`/`try_bool` 拒绝非法 UTF-8 且不退出 [验证 Req 7]
- [x] 2.8 内建名保留名单补齐（`parser.rs`、`semantic/analysis.rs`、`semantic/records.rs`）

## 阶段 3：测试与文档

- [x] 3.1 `crates/maslang/tests/float_sort.rs`（含交接文档给出的真实输入）[验证 Req 1]
- [x] 3.2 `crates/maslang/tests/output_errors.rs`（正常输出、关闭管道、严格 `print` 无 panic）[验证 Req 2]
- [x] 3.3 `crates/maslang/tests/safe_lists.rs`（越界/负下标/空列表/非法切片/可空/结构体/GC）[验证 Req 4]
- [x] 3.4 `crates/maslang/tests/safe_maps.rs`（缺键 vs null、整型键、结构体键、GC）[验证 Req 5]
- [x] 3.5 `crates/maslang/tests/checked_arithmetic.rs`（O0/O2/O3、危险指令位置断言）[验证 Req 6]
- [x] 3.6 `crates/maslang/tests/safe_conversion_bytes.rs`（原生模块构造非法 UTF-8）[验证 Req 7]
- [x] 3.7 `crates/fama/tests/time_package.rs`（`fama install` + `run` 端到端，`try_sleep(-1)` 继续执行）[验证 Req 3]
- [x] 3.8 `crates/maslang/src/lib.rs` time 专项测试更新为 0.2.0 / ABI 2，并新增 `try_sleep` 可恢复用例 [验证 Req 3]
- [x] 3.9 网站中文与英文文档：列表/数组/字典/内置函数/安全转换/time 四篇 [关联: design.md §2.3]
- [x] 3.10 语言服务补全列表补充新内建函数

## 阶段 4：产物、收尾与验证

- [x] 4.1 Windows 预编译产物按 ABI 2 重建并回填 `stdlib/time/native/artifacts/x86_64-pc-windows-msvc/`
- [x] 4.2 `.github/workflows/time-artifacts.yml` 四目标矩阵工作流（只上传 artifact，不写 git）
- [x] 4.3 通过 Actions 生成并回填 Linux/macOS 四份产物与清单（run 42562795801，四个矩阵任务 success）
- [x] 4.4 `cargo test --workspace --locked` 全绿
- [x] 4.5 `IMPLEMENTATION.md` 交付说明（基线、差异、契约、验证、遗留）

## 阶段 5：Codex 回审修正

- [x] 5.1 `try_pop(i)` 先求值下标实参再读取当前长度，补缩容/扩容双向回归与 GC 保活 [验证 Req 4.9]
- [x] 5.2 失败载荷契约：字符串复用有效空串；结构体/列表/字典/含托管元素数组在 `.value` 访问处检查 `.ok` 并给出受控语言诊断 [验证 Req 4.7/4.8]
- [x] 5.3 标准输出与标准错误同时断开时，`abort_runtime`/`abort_runtime_at` 尽力写诊断后仍以状态码 1 退出，补双管道回归 [验证 Req 2.2]
- [x] 5.4 `array.try_set` 的可写位置在语义分析阶段拒绝，并让语义与代码生成共用同一诊断文本
- [x] 5.5 按职责拆分 `safe_containers.rs` 为 `safe_lists.rs`/`safe_arrays.rs`/`safe_result.rs`，不增加转发层
- [x] 5.6 修正 time 诊断文本“逐字一致”的表述，在回交、规格、time README 与网站记录兼容性变化
- [x] 5.7 回交文档记录用户对 time ABI 2 与保留内建名的确认，以及嵌套数组写入未覆盖项
