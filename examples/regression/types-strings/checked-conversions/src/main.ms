func main():
    print("=== 安全类型转换 try_int 测试 ===")

    var r1 = try_int("123")
    if r1.ok:
        print("try_int(\"123\") 成功: ")
        print(r1.value)
    else:
        print("try_int(\"123\") 失败")

    var r2 = try_int("abc")
    if r2.ok:
        print("try_int(\"abc\") 成功")
    else:
        print("try_int(\"abc\") 失败: 非数字字符串转换失败")

    var r3 = try_int("")
    if r3.ok:
        print("try_int(\"\") 成功")
    else:
        print("try_int(\"\") 失败: 空字符串转换失败")

    var r1b = try_int(42)
    if r1b.ok:
        print("try_int(42) 成功: ")
        print(r1b.value)
    else:
        print("try_int(42) 失败")

    print("")
    print("=== 安全类型转换 try_float 测试 ===")

    var r4 = try_float("3.14")
    if r4.ok:
        print("try_float(\"3.14\") 成功: ")
        print(r4.value)
    else:
        print("try_float(\"3.14\") 失败")

    var r5 = try_float("abc")
    if r5.ok:
        print("try_float(\"abc\") 成功")
    else:
        print("try_float(\"abc\") 失败: 非数字字符串转换失败")

    var r6 = try_float("")
    if r6.ok:
        print("try_float(\"\") 成功")
    else:
        print("try_float(\"\") 失败: 空字符串转换失败")

    var r4b = try_float(100)
    if r4b.ok:
        print("try_float(100) 成功: ")
        print(r4b.value)
    else:
        print("try_float(100) 失败")

    print("")
    print("=== 安全类型转换 try_bool 测试 ===")

    var r7 = try_bool("true")
    if r7.ok:
        print("try_bool(\"true\") 成功: ")
        print(r7.value)
    else:
        print("try_bool(\"true\") 失败")

    var r8 = try_bool("false")
    if r8.ok:
        print("try_bool(\"false\") 成功: ")
        print(r8.value)
    else:
        print("try_bool(\"false\") 失败")

    var r9 = try_bool("abc")
    if r9.ok:
        print("try_bool(\"abc\") 成功")
    else:
        print("try_bool(\"abc\") 失败: 非布尔字符串转换失败")

    var r10 = try_bool("")
    if r10.ok:
        print("try_bool(\"\") 成功")
    else:
        print("try_bool(\"\") 失败: 非布尔字符串转换失败")

    var r7b = try_bool(1)
    if r7b.ok:
        print("try_bool(1) 成功: ")
        print(r7b.value)
    else:
        print("try_bool(1) 失败")

    var r8b = try_bool(0)
    if r8b.ok:
        print("try_bool(0) 成功: ")
        print(r8b.value)
    else:
        print("try_bool(0) 失败")

    print("")
    print("=== 测试完成 ===")