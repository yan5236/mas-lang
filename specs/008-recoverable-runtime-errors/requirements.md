# 功能需求：可恢复运行时错误

> 本规格承接 [HANDOFF](../../target/deepseek-handoff/HANDOFF.md) 提出的七项修复：让真实进程中止、标准库宿主失败、容器访问、整数运算与安全转换的失败在 Mas 代码里可判断、可继续，同时不破坏既有严格接口。

## 需求 1：浮点列表排序保持全序

**用户故事**：作为 Mas lang 开发者，我想要对含 NaN 的浮点列表排序，以便排序结果稳定可解释、程序不会因比较器不自洽而崩溃。

#### 验收标准 (EARS格式)

1. WHEN 浮点列表包含 NaN AND 调用 `sort()` THEN 系统 SHALL 把全部 NaN 排在所有数值之后，并按位模式稳定排列 NaN。
2. WHEN 浮点列表不含 NaN THEN 系统 SHALL 保持数值升序，并把 `-0.0` 与 `0.0` 视为相等。
3. WHEN 排序完成 THEN 系统 SHALL 继续执行后续语句，不得因比较器返回不可传递结果而 panic。

## 需求 2：标准输出失败可观察

**用户故事**：作为 Mas lang 开发者，我想要在标准输出不可写时得到可判断的结果，以便程序不会因宿主 I/O 故障直接 panic。

#### 验收标准 (EARS格式)

1. WHEN 标准输出不可写（例如管道读端已关闭） AND 调用 `try_print(x)` THEN 系统 SHALL 返回 `false`，并允许后续语句继续执行。
2. WHEN 标准输出不可写 AND 调用 `print(x)` THEN 系统 SHALL 输出中文运行时错误并以状态码 1 退出，不得出现 Rust panic 文本。
3. WHEN 标准输出可写 THEN 系统 SHALL 保持 `print`/`try_print` 的既有输出格式与调用方式。

## 需求 3：time 标准库宿主失败可处理

**用户故事**：作为 Mas lang 开发者，我想要在宿主时钟、等待或格式化失败时得到失败状态，以便业务代码可以降级而不是进程中止。

#### 验收标准 (EARS格式)

1. WHEN 调用 `try_sleep(ms)` AND `ms < 0` THEN 系统 SHALL 返回 `ok=false` 且不终止进程；零等待立即成功；正数等待至少阻塞指定毫秒数。
2. WHEN 宿主时钟读取失败、UTC 拆解失败或时间格式化失败 THEN 系统 SHALL 通过 `try_unix_seconds`/`try_unix_millis`/`try_monotonic_millis`/`try_now_utc`/`try_now_email_date`/`try_now_http_date` 返回 `ok=false`。
3. WHEN Windows 宿主时钟早于 Unix 纪元 THEN 系统 SHALL 以 `ok=false, code=2` 报告失败，不得用负时间戳充当失败哨兵。
4. WHEN POSIX `nanosleep` 被 `EINTR` 中断 THEN 系统 SHALL 继续等待剩余时间。
5. WHEN 调用既有严格 API（`sleep`、`unix_seconds` 等） THEN 系统 SHALL 保持原有函数名、返回类型与“中文错误 + 状态码 1”的失败语义；诊断文本按错误码归并，不承诺与 0.1.0 逐字一致。

## 需求 4：列表与固定数组可恢复访问

**用户故事**：作为 Mas lang 开发者，我想要在越界或未命中时得到失败结果，以便不中断流程地处理非法访问。

#### 验收标准 (EARS格式)

1. WHEN 列表下标越界、负下标或切片范围非法 AND 调用对应 `try_*` 方法 THEN 系统 SHALL 返回 `ok=false`，且列表内容、长度与元素引用保持不变。
2. WHEN 空列表 `try_pop()` 或越界 `try_pop(i)` THEN 系统 SHALL 返回 `ok=false` 且不删除元素。
3. WHEN `try_index`/`try_remove` 未命中 THEN 系统 SHALL 返回 `ok=false` / `false`，不报告运行时错误。
4. WHEN 固定数组 `try_get`/`try_set` 下标越界 THEN 系统 SHALL 返回失败状态且不访问元素地址。
5. WHEN 元素类型为结构体、嵌套容器或可空值 THEN 系统 SHALL 保留共享引用语义，并保证 `try_result<T>` 载荷在后续 GC 中仍然存活。
6. WHEN 集合允许 `null` 元素 THEN 系统 SHALL 用 `ok` 区分“未命中”与“命中但元素为 null”。
7. WHEN 安全入口失败 AND 载荷为字符串 THEN `.value` SHALL 是有效空串，可以安全做长度与比较。
8. WHEN 安全入口失败 AND 载荷为结构体、列表、字典或含托管元素的数组 THEN 读取 `.value` SHALL 报告带源码位置的语言错误并以状态码 1 退出，不得把空引用交给运行时。
9. WHEN `try_pop(i)` 的下标实参改变列表长度 THEN 系统 SHALL 在实参求值后按当前长度判断：缩容后拒绝旧下标，扩容后允许弹出新增元素。

## 需求 5：字典缺键可恢复

**用户故事**：作为 Mas lang 开发者，我想要区分“缺键”和“键存在但值为 null”，以便缓存与配置读取可以安全降级。

#### 验收标准 (EARS格式)

1. WHEN 调用 `map.try_get(k)` AND 键不存在 THEN 系统 SHALL 返回 `ok=false`。
2. WHEN 键存在且值为 `null` THEN 系统 SHALL 返回 `ok=true` 且 `.value == null`。
3. WHEN 调用 `map.try_remove(k)` AND 键不存在 THEN 系统 SHALL 返回 `false` 且字典内容不变。
4. WHEN 键类型为整数、字符串或结构体引用 THEN 系统 SHALL 沿用现有 `contains` 的键比较语义（含 `map<int,int>` 优化路径）。

## 需求 6：受检整数运算

**用户故事**：作为 Mas lang 开发者，我想要在溢出或非法除法时得到失败结果，以便边界计算可以走安全分支。

#### 验收标准 (EARS格式)

1. WHEN 整数加、减、乘溢出 AND 调用对应 `try_*` THEN 系统 SHALL 返回 `ok=false`，不得回绕后冒充成功值。
2. WHEN `try_div`/`try_mod` 除数为零，或操作数为 `MIN` 与 `-1` THEN 系统 SHALL 返回 `ok=false`，且危险指令只在合法分支执行。
3. WHEN `try_neg(MIN)` THEN 系统 SHALL 返回 `ok=false`。
4. WHEN 使用严格运算符 `+ - * %/% %` 与一元 `-` THEN 系统 SHALL 保持既有“带源码位置报错并退出”的语义。
5. WHEN 在 O0 与默认优化级别编译 THEN 系统 SHALL 产生一致的 `ok`/`value` 结果。

## 需求 7：安全转换拒绝非法 UTF-8

**用户故事**：作为通过原生模块接收字节的开发者，我想要安全转换在非法 UTF-8 时返回失败，以便不终止进程。

#### 验收标准 (EARS格式)

1. WHEN `try_int`/`try_float`/`try_bool` 收到非法 UTF-8 字节 THEN 系统 SHALL 返回 `ok=false`，且后续代码可达。
2. WHEN 严格转换 `int()`/`float()` 收到非法 UTF-8 THEN 系统 SHALL 保持“字符串不是合法的 UTF-8 文本”并退出。
3. WHEN 输入为普通无效文本或浮点转整数越界 THEN 系统 SHALL 保持既有成功值、失败值与边界语义。

## 技术约束 (非功能需求)

1. 结果表示：有值的安全入口沿用 `try_result<T>`（`.ok` / `.value`），只表示成功/失败的操作返回 `bool`。
2. 兼容性：既有严格接口的函数名、返回类型、成功路径与“中文错误 + 状态码 1”的失败语义保持不变；time 严格接口的诊断文本按错误码归并，该变化单独记录。
3. 原生 ABI：time 标准库升级到 `abi = 2`，预编译产物与清单必须同步刷新，禁止旧对象与新调用方式混用。
4. 平台支持：Windows（x86_64-pc-windows-msvc）为本机验证目标；Linux/macOS 产物由 GitHub Actions 原生 runner 生成后回填。
5. 测试：每个安全入口同时断言失败结果、后续代码可达与失败后的状态；收尾执行 `cargo test --workspace --locked`。
