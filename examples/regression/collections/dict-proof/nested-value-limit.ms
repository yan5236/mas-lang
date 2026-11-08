// 演示语法可以写出嵌套容器类型，但当前语义阶段还没有完全放开这类组合。
func main():
    prove_nested_value_limit()

// 演示 `map<string,list<int>>` 这类递归写法会在语义检查阶段被拒绝。
func prove_nested_value_limit():
    var table:map<string,list<int>> = {}
    print(len(table))
