func main():
    var name = "Alice"
    var age = 25
    var height = 1.75
    var is_student = false

    print("=== 基础模板字符串测试 ===")
    print("Hello ${name}")
    print("Name=${name}")

    print("=== 多重插值测试 ===")
    print("Hello ${name}, you are ${age} years old")

    print("=== 表达式插值测试 ===")
    print("Age + 5 = ${age + 5}")
    print("Height * 2 = ${height * 2}")

    print("=== 混合文本和表达式 ===")
    print("Name is ${name} and age is ${age}")
    print("Result: ${name} + ${age} = ${name + string(age)}")

    print("=== 不同类型插值 ===")
    print("Int: ${age}")
    print("Float: ${height}")
    print("Bool: ${is_student}")
    print("Bool string: ${string(is_student)}")

    print("=== 列表模板 ===")
    var items = ["apple", "banana", "cherry"]
    print("First item: ${items[0]}")
    print("List length: ${len(items)}")

    print("=== 嵌套表达式 ===")
    var x = 10
    var y = 20
    print("(${x} + ${y}) * 2 = ${(x + y) * 2}")

    print("=== 函数调用模板 ===")
    print("Max of 3 and 5 is ${3 + 5}")

    print("=== 空模板表达式 ===")
    var empty = ""
    print("Empty string: '${empty}'")

    print("=== 完成 ===")
