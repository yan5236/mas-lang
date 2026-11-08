func main() :
    print("=== 类型转换完整测试 ===")

    var n1:int = int("123")
    print(n1)
    var n2:int = int(3.14)
    print(n2)
    var n3:int = int(true)
    print(n3)

    var f1:float = float("3.14")
    print(f1)
    var f2:float = float(42)
    print(f2)
    var f3:float = float(true)
    print(f3)

    var s1:string = string(42)
    print(s1)
    var s2:string = string(3.14)
    print(s2)
    var s3:string = string(true)
    print(s3)

    var b1:bool = bool(1)
    print(b1)
    var b2:bool = bool(0)
    print(b2)
    var b3:bool = bool("true")
    print(b3)
    var b4:bool = bool("")
    print(b4)

    print("=== 测试完成 ===")
