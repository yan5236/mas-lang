// 测试短路逻辑在右侧带有危险列表访问时不会被错误提前求值。
// 如果 LLVM 后端把右侧表达式错误地提前执行，这个程序会因为越界访问直接崩溃。
func guarded_and(flag:bool, values:list<int>) -> bool:
    return flag and values[10] == 1

// 通过 `or` 的真值短路覆盖另一条控制流路径。
func guarded_or(flag:bool, values:list<int>) -> bool:
    return flag or values[10] == 1

// 再额外覆盖字符串列表参与短路比较的路径，确保字符串列表读取不会被错误抢跑。
func choose_text(flag:bool, values:list<string>) -> int:
    if flag or values[8] == "boom":
        return 1
    else:
        return 0

// 主函数输出固定值，证明所有危险分支都被正确短路，没有触发越界运行时错误。
func main():
    var numbers:list<int> = [1, 2, 3]
    var texts:list<string> = ["safe"]
    print(guarded_and(false, numbers))
    print(guarded_or(true, numbers))
    print(choose_text(true, texts))

    if false and numbers[99] == 1:
        print(999)
    else:
        print(7)

    if true or texts[99] == "panic":
        print(8)
    else:
        print(999)
