// 测试字符串列表的空列表初始化、追加、索引、切片、contains/index/remove/pop/sort/reverse。
// 重点覆盖字符串元素走特殊 ABI 返回路径时，常见列表操作是否还能稳定协同工作。
func print_slice(items:list<string>):
    print(items[0])
    print(items[1])

// 主函数通过固定输出验证字符串列表在多种操作串联下不会出现类型错乱或空指针问题。
func main():
    var words:list<string> = []
    words.append("gamma")
    words.append("alpha")
    words.append("beta")
    print(len(words))
    print(words[0])
    print(words.contains("alpha"))
    print(words.index("beta"))

    var middle:list<string> = words[1:3]
    print_slice(middle)

    print(words.pop())
    words.append("delta")
    words.remove("alpha")
    print(len(words))

    words.sort()
    print(words[0])
    print(words[1])

    words.reverse()
    print(words[0])
    print(words.pop(1))

    for word in words:
        print(word)
