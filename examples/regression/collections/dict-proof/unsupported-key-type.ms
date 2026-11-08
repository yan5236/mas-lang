// 演示当前版本不会把所有“看起来可哈希”的类型都放进字典键域。
func main():
    prove_unsupported_key_type()

// 演示容器类型不能作为字典键类型，当前语义阶段只接受基础标量类型作为键。
func prove_unsupported_key_type():
    var invalid:map<list<int>,string> = {}
    print(len(invalid))
