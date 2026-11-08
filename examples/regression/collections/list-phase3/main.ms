func main() :
    var items :list<int> = [3, 1, 2]

    print(len(items))
    print(items[0])

    items.append(4)
    print(len(items))

    var part :list<int> = items[1:3]
    print(part[0])
    print(part[1])

    print(items.contains(2))
    print(items.index(4))

    items.remove(1)
    print(len(items))

    print(items.pop())

    items.append(1)
    items.sort()
    print(items[0])
    print(items[2])

    items.reverse()
    print(items[0])
    print(items.pop(1))

    for item in items :
        print(item)
