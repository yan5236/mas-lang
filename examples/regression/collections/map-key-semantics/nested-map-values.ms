// 演示字典的值可以声明为列表，空字典从标注推导完整类型。
func main():
    var table:map<string,list<int>> = {}
    print(len(table))
