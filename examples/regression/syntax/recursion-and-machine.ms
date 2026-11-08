// 通过递归计算阶乘，用于验证当前编译器支持函数自调用。
// 递归本身不是判定图灵完备的唯一条件，但它能证明函数调用栈与返回路径是可工作的。
func factorial(n:int) -> int :
    if n <= 1 :
        return 1
    else :
        return n * factorial(n - 1)

// 运行一个最小的二寄存器机程序。
// 二寄存器机是经典的图灵完备计算模型，这里用 Mas lang 直接编码它的指令步进逻辑。
// 指令编码约定如下：
// 0 = INC，表示寄存器加一后跳转到 jump_zero 指定的位置。
// 1 = DECJZ，表示寄存器为零时跳转到 jump_zero，否则先减一再跳转到 jump_nonzero。
// 2 = HALT，表示程序停止。
func run_two_counter_machine(initial_a:int, initial_b:int) :
    var ops:list<int> = [1, 0, 2]
    var regs:list<int> = [0, 1, 0]
    var jump_nonzero:list<int> = [1, 0, 0]
    var jump_zero:list<int> = [2, 0, 0]

    var counter_a:int = initial_a
    var counter_b:int = initial_b
    var pc:int = 0
    var steps:int = 0

    while pc >= 0 and pc < len(ops) :
        var op:int = ops[pc]

        if op == 2 :
            break

        var reg:int = regs[pc]

        if op == 0 :
            if reg == 0 :
                counter_a += 1
            else :
                counter_b += 1
            pc = jump_zero[pc]
        else :
            if reg == 0 :
                if counter_a == 0 :
                    pc = jump_zero[pc]
                else :
                    counter_a -= 1
                    pc = jump_nonzero[pc]
            else :
                if counter_b == 0 :
                    pc = jump_zero[pc]
                else :
                    counter_b -= 1
                    pc = jump_nonzero[pc]

        steps += 1

    print("factorial(5)")
    print(factorial(5))
    print("two-counter-machine counter_a")
    print(counter_a)
    print("two-counter-machine counter_b")
    print(counter_b)
    print("two-counter-machine steps")
    print(steps)

// 主函数负责触发递归测试与二寄存器机测试。
// 这里选择把寄存器 A 的 5 个计数搬运到寄存器 B，用来验证条件跳转、循环与可变状态协同工作。
func main() :
    run_two_counter_machine(5, 0)
