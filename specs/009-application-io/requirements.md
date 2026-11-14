# 应用级同步 IO 需求

面向小型服务端和桌面程序，交付两者都能复用的字节、文件和标准流基础。实现入口见 [交接任务](tasks.md)，公开契约见 [技术设计](design.md)。

## Req 1 字节数据

**用户故事**　应用开发者需要无损读取和处理图片、协议内容及任意文件。

1. WHEN 数据含 NUL 或非法 UTF-8 THEN `bytes` SHALL 原样保留，长度、索引和切片均以字节计算。
2. WHEN 应用解码非法 UTF-8 THEN 解码 SHALL 返回可判断的失败。
3. WHEN 字节对象跨函数、模块、结构体字段、可空值或原生调用存活 THEN 工具链 SHALL 保持类型、数据和所有权正确。

## Req 2 文件和标准流

**用户故事**　应用开发者需要整文件入口、分块复制和持续写日志。

1. WHEN 分块读取 THEN IO SHALL 区分数据、EOF 和失败，并允许短读。
2. WHEN 写入发生故障 THEN IO SHALL 报告错误和已接受字节数。WHEN 文件经任一别名关闭 THEN 后续操作 SHALL 返回关闭错误，其他文件 SHALL 保持可用。
3. WHEN 读取 UTF-8 行 THEN IO SHALL 区分空行、EOF、非法编码和其他读取失败。
4. WHEN 使用整文件入口 THEN IO SHALL 按设计创建、覆盖或追加文件，并传播关闭失败。
5. WHEN 标准流被重定向 THEN IO SHALL 使用原进程的标准流，并 SHALL 与既有输出 API 保持输出顺序。

## Req 3 文件系统

**用户故事**　应用开发者需要浏览目录和管理应用生成的文件。

1. WHEN 枚举目录、查询元数据、建目录、重命名或删除 THEN IO SHALL 按设计返回结果并区分常见错误。
2. WHEN 路径包含 Unicode THEN IO SHALL 按宿主规则访问。WHEN 路径含 NUL 或非法 UTF-8 THEN IO SHALL 拒绝。
3. WHEN 删除目录或重命名 THEN IO SHALL 执行非递归和禁止覆盖的约定。

## Req 4 字符串方法

**用户故事**　应用开发者需要切分、搜索和整理读入的文本。

1. WHEN 文本包含中文、emoji 或组合字符 THEN 查找和截取 SHALL 沿用 `len(string)` 的 Unicode 标量计数。
2. WHEN 传入空分隔符、负索引、越界区间或非法 UTF-8 THEN 新方法 SHALL 执行设计指定的边界语义。
3. WHEN 使用现有字符串方法 THEN 工具链 SHALL 保持原有行为。

## Req 5 基础枚举

**用户故事**　库作者需要让文件模式和错误类别在编译期可检查。

1. WHEN 两个枚举具有同名成员或相同内部序号 THEN 语义分析 SHALL 按声明身份区分它们。
2. WHEN 公开枚举用于导入或公开签名 THEN 编译器与语言服务 SHALL 正确解析类型和成员。
3. WHEN IO 从宿主接到错误码 THEN 包装层 SHALL 转为公开枚举，未知错误归为 `Other`。

## Req 6 工具链与分发

**用户故事**　使用者在普通工程或单个文件中直接使用 IO，编辑器需要给出一致的诊断和补全。

1. WHEN 当前包未声明 `[dependencies].io` THEN 裸 `import io { ... }` SHALL 指向工具链随附的 IO，构建 SHALL 使用随附包内当前目标的预编译对象；该导入 SHALL 优先于项目内同名源码。
2. WHEN 当前包声明了 `[dependencies].io` THEN 系统 SHALL 仍按声明版本使用已安装的包，旧工程无需改动；WHEN 同一可执行程序的模块图实际加载了随附 IO 与显式 IO 两套原生实现 THEN 链接前 SHALL 给出冲突诊断与迁移办法。
3. WHEN 入口没有项目清单或只是一个 `.ms` 文件 THEN 随附 IO 导入 SHALL 仍可解析并链接；其他导入 SHALL 保持原有行为，不扩大本地模块或普通包的搜索范围。
4. WHEN 用户编辑公开示例或单文件 THEN LSP SHALL 解析随附 IO 的公开导出，不产生导入错误，并提供类型、成员与符号补全。
5. WHEN 发布标记某目标为已验证 THEN 对应目标 SHALL 实际构建并运行 IO 样例或等价集成测试；矩阵外目标导入 IO 并链接时 SHALL 报出目标三元组，不得静默借用其他目标对象。

> 2026-11-11 修订：随附分发取代“必须先声明依赖并 `fama install`”的旧消费约定。历史实施与 CI 记录保留在原文件与 `tasks.md` 的历史小节中。

## 技术约束

- 同步 IO。发布矩阵沿用 `time` 的 Windows x64、Linux x64/ARM64、macOS x64/ARM64。
- 现有包、ABI、容器、字符串和输出行为须通过回归。实现遵守仓库 `AGENTS.md`。
- 自定义泛型、带载荷枚举、模式匹配、异步、网络、GUI、可写字节缓冲区和原子保存另立规范。
