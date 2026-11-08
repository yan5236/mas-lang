// 测试第二阶段所有功能

// ===================== 2.1 条件判断 =====================
func test_if() :
    print("=== 测试 if/elif/else ===")
    var age :int = 18
    if age >= 18 :
        print("成年")
    elif age >= 12 :
        print("青少年")
    else :
        print("未成年")

func test_if2() :
    var score :int = 85
    if score >= 90 :
        print("A")
    elif score >= 80 :
        print("B")
    elif score >= 70 :
        print("C")
    elif score >= 60 :
        print("D")
    else :
        print("F")

func test_condition() :
    print("=== 测试条件表达式求值 ===")
    var a :int = 10
    var b :int = 20
    if a < b :
        print("a < b")
    if a != b :
        print("a != b")
    if a == 10 :
        print("a == 10")

// ===================== 2.2 循环 =====================
func test_while() :
    print("=== 测试 while 循环 ===")
    var i :int = 0
    while i < 5 :
        print(i)
        i += 1

func test_for() :
    print("=== 测试 for 循环 ===")
    for j in 0..5 :
        print(j)

func test_break() :
    print("=== 测试 break ===")
    var k :int = 0
    while k < 10 :
        if k == 5 :
            break
        print(k)
        k += 1

func test_continue() :
    print("=== 测试 continue ===")
    for m in 0..5 :
        if m == 2 :
            continue
        print(m)

// ===================== 2.3 函数 =====================
func add(a:int, b:int) -> int :
    return a + b

func greet(name:string) :
    print("Hello " + name)

func sum_range(start:int, end:int) -> int :
    var total :int = 0
    for i in start..end :
        total += i
    return total

// ===================== 2.4 运算符 =====================
func test_const() :
    print("=== 测试常量 ===")
    const app_name = "Mas lang"
    const max_users :int = 42
    print(app_name)
    print(max_users)
    print(max_users + 8)

func test_mod() :
    print("=== 测试取模运算符 % ===")
    var r1 :int = 7 % 3
    print(r1)
    var r2 :int = 10 % 4
    print(r2)

func test_int_div() :
    print("=== 测试整数除法 %/% ===")
    var d1 :int = 7 %/% 2
    print(d1)
    var d2 :int = 10 %/% 3
    print(d2)

func test_compound_assign() :
    print("=== 测试复合赋值 ===")
    var x :int = 10
    x += 5
    print(x)
    x -= 3
    print(x)
    x *= 2
    print(x)
    x = x %/% 4
    print(x)
    x = 10
    x %= 3
    print(x)

func test_comparison() :
    print("=== 测试比较运算符 ===")
    var a :int = 10
    var b :int = 20
    if a == 10 :
        print("== true")
    if a != b :
        print("!= true")
    if a < b :
        print("< true")
    if b > a :
        print("> true")
    if a <= 10 :
        print("<= true")
    if b >= 20 :
        print(">= true")

func test_logic() :
    print("=== 测试逻辑运算符 ===")
    var t :bool = true
    var f :bool = false
    if t and f :
        print("and true")
    else :
        print("and false")
    if t or f :
        print("or true")
    if not f :
        print("not false")

func test_short_circuit() :
    print("=== 测试短路求值 ===")
    var result :int = 0
    var condition :bool = false
    if condition and (result == 1) :
        print("should not reach")
    else :
        print("short-circuit and works")
    
    condition = true
    if condition or (result == 1) :
        print("short-circuit or works")

// ===================== main 函数入口 =====================
func main() :
    test_if()
    test_if2()
    test_condition()
    
    test_while()
    test_for()
    test_break()
    test_continue()
    
    var sum :int = add(3, 4)
    print(sum)
    greet("Mas lang")
    var total :int = sum_range(1, 5)
    print(total)
    
    test_const()
    test_mod()
    test_int_div()
    test_compound_assign()
    test_comparison()
    test_logic()
    test_short_circuit()
    
    print("All tests passed!")
