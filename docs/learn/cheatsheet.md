# LiPi Cheat Sheet (ek page me sab)

Print karke apne paas rakho.

## Basics

```lipi
# comment
show "Namaste", 5 + 3            # dikhao
naam = "Asha"                    # variable
umar = 20
const school = "LiPi Academy"    # kabhi na badle
marks: Integer = 90              # type ke saath
show "Hi {naam}, umar {umar}"    # text me value
show 10 / 4, 17 % 5, 2 ** 3      # 2.5 2 8
show toNumber("42"), toString(7), typeOf(3.5)
phone = null
show phone ?? "nahi diya"
```

## Faisle

```lipi
marks = 82
if marks >= 90
    show "A"
else if marks >= 75
    show "B"
else
    show "C"

result = "Pass" if marks >= 33 else "Fail"

match marks
    90 to 100
        show "Top"
    else
        show "Theek"
```

Compare: `==  !=  >  <  >=  <=` · Jodna: `and  or  not`

## Loops

```lipi
for i in 1 to 5
    show i
for i in 10 to 0 step -2
    show i
for fruit in ["aam", "kela"]
    show fruit
for i, fruit in ["aam", "kela"]
    show i, fruit
n = 0
while n < 3
    n += 1
repeat 2
    show "hi"
```

`break` = ruko · `continue` = agla round

## Lists aur Objects

```lipi
list = [3, 1, 2]
list.push(4)
show list[0], list[-1], list.length
show list.sort(), list.sum(), list.max()
show list.map(x => x * 2), list.filter(x => x > 1)
show list.contains(3), list.join("-")

user = {naam: "Asha", umar: 20}
user.city = "Pune"
show user.naam, user["city"], user.keys()
show user.get("phone", "nahi")
for key, value in user
    show key, value
```

## Functions

```lipi
add(a, b)
    return a + b

function greet(naam = "dost") -> String
    return "Namaste, {naam}"

double = x => x * 2
show add(2, 3), greet(), greet(naam: "Ravi"), double(4)
```

## Errors

```lipi
try
    throw "kuch galat"
catch error
    show error.message, error.code
finally
    show "hamesha"
```

## Types

```lipi
type Student
    naam: String
    marks: Integer = 0

    grade()
        return "A" if self.marks >= 90 else "B"

s = Student("Asha", 95)
show s.naam, s.grade()

type Topper extends Student      # Student ki sab cheezein + apni
    grade()
        return "A+ ({super.grade()})"
```

## JavaScript jaise features

```lipi
student = {naam: "Asha", marks: 95, city: "Pune"}
{naam, marks, ...baaki} = student     # object se nikaalo
[a, b, ...rest] = [1, 2, 3, 4]        # array se nikaalo
sab = [...rest, 5]                    # spread
naya = {...student, marks: 99}

jod(...nums)                          # kitne bhi arguments
    return nums.sum()

show jod(1, 2, 3), 2.5.toFixed(1), naya.marks
show regex.findAll('\d+', "a1 b22").map(m => m.text)
show encoding.base64Encode("hi"), [3, 1, 2].sort((x, y) => y - x)
```

`time.after(ms)` / `time.every(ms)` + block = setTimeout / setInterval

## Web app (LiPi UI)

```lipi
state count = 0

component Card(title)
    card
        heading title, level: 3

page "/" with url
    heading "Home", level: 1
    text "Count: {count}"
    button "+1", style: "border-radius: 999px;", hover: "background: #B83A22;"
        count += 1
    Card("Ek card")
    link "About", to: "/about"
    text "Sirf phone pe", desktop: "display: none;"

page "/about"
    heading "About"
```

Elements: `heading text card row column section button link image field checkbox element`
Style options: `style:` `hover:` `focus:` `mobile:` `desktop:` `class:` `id:`

## Backend

```lipi
db = database.open("app.db")

server.start 3000

get "/items"
    return db.items.all()

post "/items" with request
    return server.respond(201, db.items.create(request.json()))
```

## Commands

| Command | Kaam |
|---|---|
| `lipi file.lipi` | file chalao |
| `lipi` | interactive prompt |
| `lipi new naam` | naya project |
| `lipi run` | project chalao |
| `lipi test` | tests chalao |
| `lipi check file.lipi` | galti dhoondho |
| `lipi format` | code saaf karo |
| `lipi lint` | warnings |
| `lipi dev app.lipi` | website + live reload |
| `lipi build app.lipi` | website banao (`dist`) |
| `lipi install pkg` | package lagao |
