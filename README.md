# time 标准库原生产物说明

## 支持矩阵

当前 `time` 包默认提供以下预编译原生产物：

| 目标三元组 | 产物目录 | 状态 |
| --- | --- | --- |
| `x86_64-pc-windows-msvc` | `native/artifacts/x86_64-pc-windows-msvc/` | 已支持 |

普通调用方构建 `import time { ... }` 时，编译器会优先读取 `settings.toml` 中声明的 `native.prebuilt.<target>` 条目，并直接链接该目录下的对象文件。默认消费路径不会自动回退到源码编译，因此调用方机器不需要安装 `gcc`、`clang` 或 `cl.exe`。

## 目录协议

预编译产物统一放在包内稳定路径：

```text
native/
└── artifacts/
    └── x86_64-pc-windows-msvc/
        ├── manifest.toml
        ├── time_common.obj
        └── time_windows.obj
```

其中 `manifest.toml` 至少记录以下信息：

- 包名与包版本
- 目标三元组
- ABI 版本
- 源码与头文件指纹
- 对象文件清单

`fama install` 会保留 `native/artifacts/` 目录，因此本地路径依赖安装到 `mod_module/time@0.1.0/` 后仍然可以直接被消费。

## 角色分流

普通调用方：

- 直接执行 `fama build` 或 `fama run`
- 命中支持矩阵时会链接包内预编译对象
- 缺少产物、ABI 不匹配或目标不受支持时会收到中文诊断

维护者：

- 在需要刷新原生产物时执行 `fama build --allow-native-source-build`
- 编译器会在校验失败或产物缺失时，使用 `native.sources` 的 C 源文件重新生成对象文件和 `manifest.toml`
- 刷新后的产物会直接回写到 `native/artifacts/<target>/`

## 发布检查

当以下任一内容变更时，必须先刷新预编译产物，再发布新版本：

- `native/common/*.c`
- `native/platform/**/*.c`
- `native/include/**/*.h`
- `settings.toml` 中的 `native.abi`

发布前至少检查：

1. `native/artifacts/<target>/manifest.toml` 中的 `package_version`、`abi`、`source_hash`、`header_hash` 已更新。
2. `settings.toml` 中的 `native.prebuilt.<target>` 路径与目录内对象文件一致。
3. 普通 `fama build` 在不加 `--allow-native-source-build` 的情况下可以直接消费 `time`。
