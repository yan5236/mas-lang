# io-api 示例

这个工程演示 `io` 标准库 `0.1.0` 的常用入口：分块复制二进制文件、逐行读配置、追加 UTF-8 日志，以及目录枚举、元数据、定位、显式关闭、禁止覆盖重命名和删除。

所有文件都写在工程自己的 `build/io-data/数据/` 下，不需要管理员权限，也不依赖固定机器路径；每次运行会先清理旧数据，输出稳定可重复。

## 运行

`io` 随工具链分发，本工程没有声明任何依赖，也不需要安装。先在仓库根目录构建工具链，然后进入本目录执行：

```sh
# 如果 PATH 里已经有 fama
fama build
fama run
```

在仓库内直接用源码里的 Fama 也可以：

```powershell
cargo run -q -p fama -- build .
cargo run -q -p fama -- run .
```

`fama run` 会在本工程根目录启动编译好的程序。io 的 Mas 源码和当前目标的预编译对象都内嵌在工具链里，普通构建直接消费，不需要额外参数。只有维护者刷新原生对象时才用 `--allow-native-source-build`：

```powershell
cargo run -q -p fama -- build . --allow-native-source-build
```

发布矩阵内的五个目标（Windows x64、Linux x64/ARM64、macOS x64/ARM64）都带预编译产物。旧工程如果显式声明了 `[dependencies].io`，可以保持原样继续用已安装的版本；想改用随附 io，删掉声明后正常构建即可。同一程序如果实际加载了两套 io 原生实现，编译器会在链接前报错并提示迁移。

## 预期输出

```text
write source ok=true
write source bytes=256
copy bytes=256
copy identical=true
copy preserves NUL=true
copy preserves 0xFF=true
write config ok=true
config lines=4
config first=name=样例
config empty=true
config last no newline=last
log start ok=true
log append ok=true
log lines=3
log last=第二行
metadata regular=true
metadata size=256
listing ok=true
listing count=4
seek ok=true
seek position=250
tail bytes=6
tail first=250
close ok=true
close again closed=true
rename ok=true
rename no overwrite=true
remove ok=true
done
```

## 输出含义

| 输出 | 覆盖点 |
| --- | --- |
| `write source ok=true` / `bytes=256` | 用 `write_file` 写入 0..255 全部字节值，得到 256 字节文件 |
| `copy bytes=256` | `copy_file` 按 32 字节分块循环读，`eof` 与失败分开判断 |
| `copy identical=true` | 复制前后用逐字节比较确认内容完全一致 |
| `copy preserves NUL=true` / `0xFF=true` | `bytes` 原样保留 NUL 与 `0xFF`，没有被当成文本破坏 |
| `config lines=4` / `empty=true` | `read_line` 区分空行、4 行配置和文件末尾；空行是成功结果 |
| `config last no newline=last` | 最后一段没有换行的文本仍作为一行返回 |
| `log lines=3` / `log last=第二行` | `append_text` 两次追加 UTF-8 日志，内容与顺序正确 |
| `metadata regular=true` / `size=256` | `metadata` 返回 `FileKind.Regular`，普通文件带字节大小 |
| `listing count=4` | `list_dir` 返回 4 个直接子项名称 |
| `seek position=250` / `tail bytes=6` / `first=250` | `seek` 到第 250 字节，再读 6 字节，验证定位与后续读取 |
| `close ok=true` / `close again closed=true` | 显式关闭；重复关闭返回 `IOErrorKind.Closed` |
| `rename ok=true` | 同一文件系统内重命名成功 |
| `rename no overwrite=true` | 目标已存在时返回 `IOErrorKind.AlreadyExists`，没有被覆盖 |
| `remove ok=true` | `remove_file` 删除文件 |
| `done` | 所有检查完成，程序正常退出 |

## 目录里生成了什么

运行结束后，`build/io-data/数据/` 里留下这三个文件：

| 文件 | 内容 |
| --- | --- |
| `source.bin` | 0..255 共 256 字节 |
| `config.txt` | `name=样例`、`mode=io`、空行、末尾无换行的 `last` |
| `app.log` | `日志开始`、`第一行`、`第二行`，共 3 行 |

中间文件 `copied.bin` 是分块复制的目标，检查完成后被重命名为 `moved.bin` 再删除，所以运行结束时不保留。

`build/` 是本地构建输出，不进入版本控制。

## 覆盖范围

这个示例覆盖：分块二进制复制、短读与 EOF、逐行文本读取（含空行与无换行末行）、UTF-8 追加、元数据、目录枚举、`seek(Start)`、显式关闭与令牌失效、禁止覆盖重命名、删除文件。

没有覆盖：标准流（`stdin`/`stdout`/`stderr`）、权限不足/磁盘错误等宿主失败、`remove_dir` 删除目录、`write_text` 的非法 UTF-8 拒绝、POSIX 平台行为。这些在 `stdlib/io/README.md` 和仓库测试里有说明。
