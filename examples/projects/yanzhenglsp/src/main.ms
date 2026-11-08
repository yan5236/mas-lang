import time { now_unix }

const app_name:string = "LSP验证"

func greet(name:string) -> string:
    return "你好，" + name

func add_score(base:int, bonus:int) -> int:
    var total:int = base + bonus
    return total

func build_message(score:int) -> string:
    var timestamp:int = now_unix()
    return app_name + ":" + string(score) + "@" + string(timestamp)

func main():
    var count:int = 3
    var score:int = add_score(count, 7)
    print(greet("mas-lang"))
    print(build_message(score))
