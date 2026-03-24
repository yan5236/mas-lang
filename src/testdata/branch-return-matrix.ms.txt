// 测试嵌套条件分支、elif 链和循环中的提前 return 是否会让 LLVM 后端留下脏控制流。
// 重点覆盖之前出现过的“所有分支都 return 之后仍然异常崩溃”这一类高风险组合。
func classify(value:int) -> int:
    if value < 0:
        return -1
    elif value == 0:
        return 0
    else:
        return 1

// 通过多层 if/elif/else 嵌套返回，验证每一层分支汇合块都能被正确处理。
func pick_branch(seed:int) -> int:
    if seed < 5:
        if seed % 2 == 0:
            return 10
        else:
            return 11
    elif seed < 10:
        if seed == 7:
            return 20
        else:
            return 21
    else:
        return 30

// 在循环里交错使用 continue、return 和循环后的兜底 return，验证控制流终结器拼接是否稳定。
func loop_pick(limit:int) -> int:
    var index:int = 0
    while index < limit:
        if index == 0:
            index += 1
            continue
        elif index == 3:
            return 300
        else:
            index += 1
    return 999

// 主函数输出固定结果，供自动化测试直接比对。
func main():
    print(classify(-7))
    print(classify(0))
    print(classify(9))
    print(pick_branch(2))
    print(pick_branch(3))
    print(pick_branch(7))
    print(pick_branch(8))
    print(pick_branch(10))
    print(loop_pick(2))
    print(loop_pick(5))
