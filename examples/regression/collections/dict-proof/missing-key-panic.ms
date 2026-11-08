// 演示直接读取缺失键时不会返回默认值，而是触发明确的运行时失败。
func main():
    prove_missing_key_panics()

// 演示字典下标访问遇到不存在的键时会立即终止程序。
func prove_missing_key_panics():
    var user:map<string,int> = {"age": 18}
    print(user["name"])
