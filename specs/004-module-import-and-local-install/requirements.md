# 功能需求：模块导入与本地依赖安装

> 本功能用于承接 `specs/002-mascript-language-roadmap/tasks.md` 中未完成的 `4.5`、`4.6`、`4.7`、`4.8`、`4.9`、`5.1`、`5.2`、`5.3`、`5.4` 条目，交付范围聚焦 `fama` 与主力编译器 `maslang` 的本地依赖链路。

## 需求 1：支持本地路径依赖声明与安装

**用户故事**：作为 Mas lang 项目开发者，我想要通过 `fama add` 与 `fama install` 管理本地模块依赖，以便在不手工复制源码的情况下复用本地库工程。

#### 验收标准 (EARS格式)

1. WHEN 开发者执行 `fama add <本地路径>` THEN 系统 SHALL 读取目标项目的 `mod.yml`，并把依赖名、版本号与相对路径写入当前项目的 `mod.yml`。
2. WHEN 开发者执行 `fama install` THEN 系统 SHALL 读取当前项目 `mod.yml` 中的本地路径依赖，并把依赖源码安装到当前项目的 `mod_module/` 平铺缓存中。
3. IF 本地路径不存在、目标目录缺少 `mod.yml`、依赖项目缺少 `src/` 或依赖元数据不完整 THEN 系统 SHALL 给出明确的中文错误，而不是生成部分安装结果。
4. WHEN 系统生成或更新 `mod-lock.yml` THEN 系统 SHALL 记录每个已安装依赖的名称、版本、来源类型、来源路径和缓存目录。

## 需求 2：以平铺缓存方式管理依赖，避免嵌套安装

**用户故事**：作为包管理器维护者，我想要让依赖统一存放在当前项目根目录的 `mod_module/` 中，以便避免 Node 风格的递归嵌套依赖目录。

#### 验收标准 (EARS格式)

1. WHEN 系统安装依赖 THEN 系统 SHALL 把缓存目录组织为 `mod_module/<包名>@<版本>/` 的平铺结构，而不是在依赖内部继续生成自己的 `mod_module/`。
2. WHEN 一个依赖还依赖其他本地依赖 THEN 系统 SHALL 继续把传递依赖安装到当前项目根目录的同一个 `mod_module/` 下，而不是嵌套拷贝到父依赖目录内。
3. IF 安装过程中发现同名依赖解析到多个不同版本或多个不同来源 THEN 系统 SHALL 终止安装并给出冲突诊断，避免生成不确定的解析结果。
4. WHEN 系统重复执行 `fama install` AND 锁文件与缓存已匹配 THEN 系统 SHALL 复用已有缓存目录，而不是重复写入同内容依赖。

## 需求 3：支持本地模块与已安装依赖的导入解析

**用户故事**：作为 Mas lang 开发者，我想要在源码中使用 `import` 拆分本地模块和依赖模块，以便把程序组织成可维护的多文件工程。

#### 验收标准 (EARS格式)

1. WHEN 开发者在源码中使用 `import foo.bar { baz }` THEN 系统 SHALL 解析对应模块并把导入符号接入当前编译单元。
2. WHEN 开发者在源码中使用 `import foo.bar { baz as qux }` 或 `import foo.bar.*` THEN 系统 SHALL 支持别名导入与全部导入。
3. WHEN 开发者导入本地模块 THEN 系统 SHALL 把项目 `src/` 作为模块根目录，并支持 `a.ms -> a`、`dir/index.ms -> dir`、`dir/file.ms -> dir.file` 的路径映射。
4. WHEN 顶层模块名命中 `mod.yml` 依赖 THEN 系统 SHALL 从当前项目 `mod_module/` 的已安装缓存中解析该依赖模块；IF 需要强制解析本地同名模块 THEN 系统 SHALL 支持 `local.` 前缀。
5. IF 模块文件不存在、导入符号不存在、导入目标未公开或导入图出现循环 THEN 系统 SHALL 在编译阶段给出明确的中文诊断。

## 需求 4：支持公开导出与目录聚合入口

**用户故事**：作为库作者，我想要控制模块的公开符号，并能通过 `index.ms` 聚合导出，以便为调用方提供稳定的模块入口。

#### 验收标准 (EARS格式)

1. WHEN 开发者使用 `pub func`、`pub const` 或 `pub var` 声明顶层符号 THEN 系统 SHALL 仅允许这些符号被其他模块导入。
2. IF 顶层声明未使用 `pub` THEN 系统 SHALL 把该声明视为模块私有，禁止其他模块直接导入。
3. WHEN 开发者在 `index.ms` 中使用 `export import foo.bar { baz }` THEN 系统 SHALL 允许当前模块把导入符号重新公开给外部调用方。
4. WHEN 调用方导入目录模块 THEN 系统 SHALL 优先使用该目录下的 `index.ms` 作为聚合入口。

## 需求 5：让 fama 默认走主力编译器并支持全局工具链设置

**用户故事**：作为 Mas lang 项目使用者，我想要让 `fama build/run` 直接走主力编译器，并通过全局设置声明编译器路径，以便项目命令行为稳定一致。

#### 验收标准 (EARS格式)

1. WHEN 开发者执行 `fama build` 或 `fama run` THEN 系统 SHALL 使用 `maslang` 的编译链路，而不是遗留 `compiler`。
2. WHEN `fama` 需要确定编译器可执行文件或相关工具链路径 THEN 系统 SHALL 从 `~/.fama/setting.toml` 读取全局设置。
3. IF `~/.fama/setting.toml` 不存在 THEN 系统 SHALL 回退到默认设置并继续执行，而不是把缺省配置视为错误。
4. IF `~/.fama/setting.toml` 内容无法解析 THEN 系统 SHALL 给出明确的中文诊断，并说明读取的是 `~/.fama/setting.toml`。

## 技术约束 (非功能需求)

1. 主线范围：本功能只支持本地路径依赖，`git` 仓库依赖保留为后续能力，不纳入本次交付。
2. 编码约束：新增或修改的规格文档、源码、配置文件和测试样例必须使用 UTF-8 无 BOM。
3. 工程约束：本功能必须以最小化改动为原则，优先新增模块，不得大范围重写既有语义分析或 LLVM 后端。
4. 兼容约束：不得破坏现有单文件编译、`fama new` 项目骨架、字符串方法、集合能力和已有中文诊断。
5. 依赖约束：当前阶段不支持同名多版本依赖共存；遇到版本或来源冲突时必须失败退出。
