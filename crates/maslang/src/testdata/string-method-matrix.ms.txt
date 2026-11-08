func main():
    var text = "  你好，世界  "
    var cleaned = text.trim()
    print(cleaned)
    print(cleaned.contains("好"))
    print(cleaned.starts_with("你好"))
    print(cleaned.ends_with("世界"))
    print(cleaned.replace("世界", "Mas lang"))
    print("中文".replace("", "X"))
