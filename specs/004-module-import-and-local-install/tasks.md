# 实施任务清单：模块导入与本地依赖安装

## 阶段 1：规格对齐与数据模型

- [x] 1.1 在 `specs/004-module-import-and-local-install/` 三件套完成评审定稿，并确认本功能承接路线图 `4.5` 至 `5.4` 的最小本地依赖范围 [关联: design.md 1, 2.1]
- [x] 1.2 在 `fama` 中补充 `settings.toml`、`mod-lock.yml` 与 `~/.fama/setting.toml` 的结构体、读写工具和中文错误封装 [依赖: 1.1] [验证 Req 1, Req 5]
- [x] 1.3 为平铺缓存目录、锁文件记录与同名依赖冲突规则补充单元测试样例 [依赖: 1.2] [验证 Req 2]

## 阶段 2：fama 本地依赖命令与主编译器切换

- [x] 2.1 在 `crates/fama/src/cli.rs` 与命令模块中新增 `add`、`install` 子命令参数定义 [依赖: 1.2] [验证 Req 1]
- [x] 2.2 实现 `fama add <path>`，读取目标依赖的 `settings.toml` 并回写当前项目 `settings.toml` [依赖: 2.1] [验证 Req 1]
- [x] 2.3 实现 `fama install` 的本地路径递归安装、平铺缓存写入和 `mod-lock.yml` 回写逻辑 [依赖: 2.2] [验证 Req 1, Req 2]
- [x] 2.4 将 `fama build/run` 从遗留 `compiler` 切换到 `maslang` crate，并接入 `~/.fama/setting.toml` 的全局配置读取 [依赖: 1.2] [验证 Req 5]
- [x] 2.5 为 `fama new`、`add`、`install`、`build`、`run` 补充集成测试，覆盖成功路径和配置/依赖错误路径 [依赖: 2.3, 2.4] [验证 Req 1, Req 2, Req 5]

## 阶段 3：编译器模块语法与装载器

- [x] 3.1 在 `maslang` 的词法与语法层新增 `import`、`pub`、`export import`、别名导入和全部导入语法 [依赖: 1.1] [验证 Req 3, Req 4]
- [x] 3.2 新增模块装载器，支持 `src/` 本地模块路径映射、`index.ms` 聚合入口、依赖缓存解析与 `local.` 前缀 [依赖: 3.1, 2.3] [验证 Req 3, Req 4]
- [x] 3.3 在模块装载器中实现公开符号筛选、`export import` 重导出、循环依赖检测和导入冲突诊断 [依赖: 3.2] [验证 Req 3, Req 4]
- [x] 3.4 将编译入口改为“模块装载后再走既有语义分析与 LLVM 后端”，保持单文件场景兼容 [依赖: 3.2] [验证 Req 3, Req 5]

## 阶段 4：验收覆盖与路线图回写

- [x] 4.1 为编译器补充本地模块、目录模块、依赖模块、别名导入、全部导入、未公开符号和循环依赖的正反样例测试 [依赖: 3.4] [验证 Req 3, Req 4]
- [x] 4.2 校验所有新增或修改文件均为 UTF-8 无 BOM，并运行 `fama` 与 `maslang` 相关测试 [依赖: 2.5, 4.1] [验证 Req 1, Req 2, Req 3, Req 4, Req 5]
- [x] 4.3 在功能完成后回写 `specs/002-mascript-language-roadmap/tasks.md` 中 `4.5` 至 `5.4` 的实际状态，并补充必要的实现备注 [依赖: 4.2] [验证 Req 5]
