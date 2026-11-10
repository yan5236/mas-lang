# Mas lang 回归样例

| 目录 | 内容 |
| --- | --- |
| [syntax](syntax/) | Hello World、变量作用域、控制流、返回值与递归 |
| [collections](collections/) | 列表、字典及其边界行为 |
| [types-strings](types-strings/) | 类型转换、字符串插值 |
| [modules](modules/) | 本地模块、包导入及传递依赖 |
| [compiler-stress](compiler-stress/) | 编译器后端回归，由 `cargo test -p maslang --locked msceshi` 执行 |
| [config](config/) | 工具链配置负例 |

完整项目放在 [projects](../projects/) 下，其中 [time-api](../projects/time-api/) 演示标准库时间 API，[io-api](../projects/io-api/) 演示标准库 IO（二进制复制、文本行、文件系统），[language-server-demo](../projects/language-server-demo/) 用于验证语言服务器的补全、诊断和符号定位。

## 运行方式

从仓库根目录执行：

```powershell
cargo run -p fama --locked -- run examples/regression/syntax/hello-world
cargo run -p maslang --locked -- run examples/regression/syntax/float-literal/main.ms
cargo run -p fama --locked -- run examples/regression/modules/dependency-app
```

Fama 项目通过 `settings.toml` 声明入口和依赖。`dependency-app` 依赖同级 `dependency-math`，后者依赖 `dependency-core`。程序分别使用依赖包和同名本地模块，输出 `3` 和 `103`。

## 预期报错的用例

- `collections/map-key-semantics/missing-key-panic.ms` 读取不存在的键，应报运行时错误。
- `collections/map-key-semantics/unsupported-key-type.ms` 使用容器作为字典键，应报语义错误。
- `config/invalid-global-settings/setting.toml` 包含故意写坏的用户级配置。

`fama run` 自动安装依赖并用临时可执行文件运行。样例的本地构建和缓存可用 `pwsh -File examples/regression/clean.ps1` 清理。
