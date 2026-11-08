// 演示当前字典键支持的基础类型，以及遍历会保留插入顺序。
func main():
    prove_string_key()
    prove_int_key()
    prove_bool_key()
    prove_insertion_order()

// 演示字符串键不是唯一可用的键类型，字符串键可以正常读取与查询。
func prove_string_key():
    print("== string key ==")
    var names:map<string,int> = {"alice": 10, "bob": 20}
    print(names["alice"])
    print(names.contains("bob"))

// 演示整数键同样可以作为字典键使用，而不是只支持字符串键。
func prove_int_key():
    print("== int key ==")
    var numbers:map<int,string> = {1: "one", 2: "two"}
    print(numbers[1])
    print(numbers.contains(2))

// 演示布尔键也能作为字典键使用，证明键类型范围至少覆盖全部基础标量类型。
func prove_bool_key():
    print("== bool key ==")
    var flags:map<bool,string> = {true: "yes", false: "no"}
    print(flags[true])
    print(flags[false])

// 演示 keys() 和 for-each 遍历都会按插入顺序返回条目，而不是随机顺序。
func prove_insertion_order():
    print("== insertion order ==")
    var scores:map<string,int> = {"c": 3, "a": 1, "b": 2}
    var keys = scores.keys()
    print(keys[0])
    print(keys[1])
    print(keys[2])
    for key, value in scores:
        print(key)
        print(value)
