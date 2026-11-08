# msceshi — Mas lang 测试样例与回归用例集

> **定位**：验证编译器行为的样例与回归用例。**默认不作为产品源码修改入口**（见根目录 `AGENTS.md`）。
> 真源码只有约 23 KB。`build/`、`mod_module/`、`target/`、`*.exe`、`*.pdb` 全是**可再生的派生内容**，随时可以删。

整理前：23 个目录平铺、27.97 MB（其中 27.83 MB 是编译产物）。
整理后：6 个主题目录、23.1 KB。

---

## 目录速览

| 目录 | 主题 | 用什么跑 |
| --- | --- | --- |
| `compiler-stress/` | 编译器后端压力与回归 | `cargo test` |
| `syntax/` | 语法与基础语义 | `fama` |
| `collections/` | 列表 `list` 与字典 `map` | `fama` / 编译器 CLI |
| `types-strings/` | 类型转换与字符串 | `fama` / 编译器 CLI |
| `modules/` | 模块导入与依赖链 | `fama` |
| `misc/` | 其他 | 编译器 CLI |

---

## 怎么跑

### 1. fama 工程（目录里有 `mod.yml`）

```
cd <项目目录>
fama run
```

也可以从任意位置指定项目目录：`fama run <项目目录>`

几个实测出来的注意点：

- `fama run` 编译到临时文件后运行，**不在项目里留 `build/`**；只有 `fama build` 会生成 `build/`。
- **有依赖的工程必须先 `fama install`**——`fama run` 不会自动装依赖，`mod_module/` 不存在时会直接报 `读取依赖缓存目录失败`。受影响的是 `modules/spec004-app`（依赖 `spec004-lib`）。而 `clean.ps1` 会删掉 `mod_module/`，所以清理之后要重新 `fama install`。

### 2. 裸 `.ms` 文件（没有 `mod.yml`）

```
<maslang>/target/release/maslang.exe run <文件.ms>
<maslang>/target/release/maslang.exe build <文件.ms> -o <输出路径>
```

`run` 会编译到临时可执行文件后立即运行，**不会在 msceshi 里留下产物**。

### 3. 编译器内嵌回归

```
cd <maslang>
cargo test msceshi
```

---

## 用例清单

### `compiler-stress/` — 编译器后端压力与回归

⚠️ **不要改这 5 个文件的名字和内容。** 它们被 `maslang/src/linker.rs` 的 `msceshi_fixture_source()` 以字符串形式内嵌引用，文件名同时出现在测试名与注释里。改名会让代码和样例对不上。

| 文件 | 覆盖点 |
| --- | --- |
| `array-matrix.ms` | 固定长度数组的零值声明与索引；预期输出 `0 0 11 22 33 55` |
| `branch-return-matrix.ms` | 嵌套 `if/elif`、循环内提前 `return` 是否留下脏控制流 |
| `short-circuit-guard.ms` | `and`/`or` 短路不误求值右侧的越界访问 |
| `string-list-matrix.ms` | 字符串列表：空列表、`append`、索引、切片、`contains`/`index`/`remove`/`pop`/`sort`/`reverse` |
| `string-method-matrix.ms` | 字符串 `trim`/`contains`/`starts_with`/`ends_with`/`replace`，含中文 UTF-8 与空串替换保护 |

跑法：`cargo test msceshi`（当前 5 个用例全绿）。

### `syntax/` — 语法与基础语义

| 用例 | 类型 | 覆盖点 |
| --- | --- | --- |
| `hello-world/` | fama | 顶层语句版 Hello World（没有 `func main`） |
| `hello/` | fama | fama 脚手架产物。⚠️ `src/main.ms` 里写的是 `func mian()`（拼错），**运行输出为空** |
| `fama-basic/` | fama | 变量声明、块作用域遮蔽、字符串拼接 |
| `test-func/` | fama | 带返回值的函数 + `if` |
| `test-phase2/` | fama | 第二阶段全量：`if/elif/else`、`while`/`for`、`break`/`continue`、函数、`const`、`%` 与 `%/%`、复合赋值、比较、逻辑、短路求值 |
| `float-literal/` | 裸 `.ms` | 浮点字面量声明与打印（原 `ceshi3`） |
| `main-typo/` | 裸 `.ms` | 函数名拼成 `mian`、文件名也是 `mian.ms`（原 `ceshi4`） |

### `collections/` — 列表与字典

| 用例 | 类型 | 覆盖点 |
| --- | --- | --- |
| `list-phase3/` | 裸 `.ms` | `list<int>` 全操作：`len`/索引/`append`/切片/`contains`/`index`/`remove`/`pop`/`sort`/`reverse`/遍历 |
| `string-list-bug/` | 裸 `.ms` | 字符串列表的历史 bug 回归 |
| `dict-test/` | fama | `map` 字面量、类型声明、键值访问与改写、`remove`、`keys`/`values`、`for key, value` 遍历 |
| `dict-proof/` | 裸 `.ms` | 字典语义证明 4 例（原 `generic-proof`），见下表 |

`dict-proof/` 里有 3 个是**预期失败**的负例，跑出来报错才是正确行为：

| 文件 | 预期 |
| --- | --- |
| `scalar-key-order.ms` | ✅ 通过：`string`/`int`/`bool` 都可作键，遍历保持插入顺序 |
| `missing-key-panic.ms` | ❌ 预期失败：读缺失键不做默认值兜底，直接终止 |
| `nested-value-limit.ms` | ❌ 预期失败：`map<string,list<int>>` 这类嵌套容器在语义阶段被拒 |
| `unsupported-key-type.ms` | ❌ 预期失败：容器类型不能作为字典键类型 |

### `types-strings/` — 类型转换与字符串

| 用例 | 类型 | 覆盖点 |
| --- | --- | --- |
| `type-convert-test/` | ⚠️ 半成品 | `int`/`float`/`string`/`bool` 互相转换。**只有 `settings.toml`，没有 `mod.yml`**，fama 跑不起来；`main.ms` 可用 CLI 直接跑 |
| `safe-try-convert-test/` | fama | `try_int`/`try_float`/`try_bool` 安全转换，返回 `ok`/`value` |
| `string-to-int-demo/` | 裸 `.ms` | `string` → `int` 最小演示 |
| `template-string-test/` | fama | `${}` 模板插值全量：多插值、表达式、不同类型、列表元素、嵌套表达式、空串 |
| `template-string-llvm.ms` | 裸 `.ms` | 模板字符串最小验证（原先散落在 msceshi 根目录） |

### `modules/` — 模块导入与依赖链

| 用例 | 原名 | 覆盖点 |
| --- | --- | --- |
| `import-basic/` | `yanzheng1` | 本地文件导入：`import jisuan {jisuan}` 并调用 |
| `spec004-core/` | `spec004core_tmp` | 被依赖的基础包，导出 `pub func core_add` |
| `spec004-lib/` | `spec004e2elib_tmp` | 依赖 core；含 `src/utils/math.ms`，由 `src/index.ms` 再导出 |
| `spec004-app/` | `spec004e2eapp_tmp` | 依赖 lib；同时验证**包导入**与 `local.` **本地模块导入**。预期输出 `3` 和 `103` |

⚠️ **这三个是相对路径依赖链**（`path: ../spec004-core`），必须保持同级目录。
整理时只改了**目录名**并同步了 `mod.yml` / `mod-lock.yml` 里的 `path:`；包名（`name:` 字段）仍是历史命名，未改动。

跑法：`cd modules/spec004-app && fama install && fama run`

### `misc/` — 其他

| 用例 | 覆盖点 |
| --- | --- |
| `turing_check.ms` | 递归阶乘 + 二寄存器机，验证函数自调用、调用栈与返回路径 |
| `spec004-empty-home/` | 空 home + 坏配置的**负例夹具**。⚠️ 文件名是 `setting.toml`（不是 `settings.toml`），内容是故意损坏的 `[compiler`——**不要"修好"它** |

---

## 已知问题（整理时保留原状，未擅自修改）

这些都是**样例本身的问题**，改动会抹掉它们正在记录的编译器行为，所以只标注不修复：

1. `syntax/hello/src/main.ms` 把入口函数写成 `func mian()`，编译通过但运行无输出。
2. `syntax/main-typo/` 同样是把 `main` 拼成 `mian`，文件名也是 `mian.ms`。
3. `types-strings/type-convert-test/` 缺 `mod.yml`，无法用 fama 跑。
4. `types-strings/safe-try-convert-test/mod.yml` 内容是 TOML 风格（`name = "..."`）而不是 YAML，同目录 `settings.toml` 还有重复的顶层 `name`/`version`。目前 fama 仍能跑通。
5. `misc/spec004-empty-home/setting.toml` 文件名少了个 `s`，内容语法不完整——负例夹具，勿动。
6. `modules/spec004-*/` 的目录名与新式命名一致了，但内部 `name:` 仍是 `spec004core_tmp` 这类历史名。

---

## 保持整洁

派生内容跑完就删：

```
pwsh -File clean.ps1
```

约定：

- 新增样例放进对应的主题目录；裸 `.ms` 和 fama 工程都接受。
- 不要把 `build/`、`mod_module/`、`target/`、`*.exe`、`*.pdb` 提交进版本库（见 `.gitignore`）。
