# mascript-lang

`mascript-lang` 为 VS Code 提供 Mascript 语言支持。

当前版本只覆盖编译器已经实现的语法能力，不会把设计文档中的未来特性提前加入高亮和补全。

## 支持内容

- `.ms` 文件识别
- TextMate 语法高亮
- 基础语言配置
  - `//` 行注释
  - `()` 自动闭合
  - 以 `:` 结尾的代码块缩进
- 轻上下文补全
  - 关键字：`var` `const` `func` `if` `elif` `else` `while` `for` `in` `break` `continue` `return` `and` `or` `not`
  - 类型：`int` `float` `bool` `string`
  - 内建：`print`
  - 布尔字面量：`true` `false`
  - 代码骨架：`func` `if` `elif` `else` `while` `for` `main`

## 当前语法范围

本扩展只按当前编译器真实支持的语法高亮和补全，主要包括：

- 变量与常量声明
- 函数定义与调用
- `if / elif / else`
- `while`
- `for ... in start..end`
- `break / continue / return`
- 基础类型 `int / float / bool / string`
- 字符串、整数、浮点数、布尔值
- 运算符 `= + - * / % %/% += -= *= /= %= == != < <= > >= .. ->`

以下设计文档中的内容本版本不纳入扩展能力范围：

- `import`
- `list / array / map`
- 字符串模板
- 标准库模块补全
- LSP 语义分析能力

## 示例

```ms
func add(a:int, b:int) -> int :
    return a + b

func main() :
    var result :int = add(3, 4)
    if result >= 7 :
        print("ok")
```

## 说明

- 扩展不会修改或依赖编译器实现。
- 当前不提供格式化、诊断、跳转定义或语义高亮。
