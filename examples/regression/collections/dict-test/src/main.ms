func main():
    test_dict_literal()
    test_dict_type_declaration()
    test_dict_access()
    test_dict_operations()
    test_dict_methods()
    test_dict_traversal()

func test_dict_literal():
    print("=== 测试字典字面量 ===")
    var user = {"name": "张三", "age": "18"}
    print(user["name"])
    print(user["age"])

func test_dict_type_declaration():
    print("=== 测试字典类型声明 ===")
    var user :map<string,int> = {"name": 1, "age": 18}
    print(len(user))

func test_dict_access():
    print("=== 测试键值访问 ===")
    var m :map<string,int> = {"a": 1, "b": 2}
    print(m["a"])
    print(m["b"])

func test_dict_operations():
    print("=== 测试键值操作 ===")
    var m :map<string,int> = {"name": 1}
    m["age"] = 18
    m["name"] = 2
    print(len(m))
    print(m["name"])
    print(m["age"])
    m.remove("age")
    print(len(m))

func test_dict_methods():
    print("=== 测试字典方法 ===")
    var m :map<string,int> = {"a": 1, "b": 2, "c": 3}
    print(m.contains("a"))
    print(m.contains("x"))
    var keys = m.keys()
    var values = m.values()
    print(len(keys))
    print(len(values))

func test_dict_traversal():
    print("=== 测试字典遍历 ===")
    var m :map<string,int> = {"x": 10, "y": 20}
    for key, value in m:
        print(key)
        print(value)