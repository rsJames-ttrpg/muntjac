import tomli
data = tomli.loads("a = 1")
print(data)
assert data == {"a": 1}, f"unexpected: {data!r}"
