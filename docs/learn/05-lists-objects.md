# 5. Lists (Arrays) aur Objects

**Is chapter me seekhoge:** bahut saari values ek saath rakhna (list), naam wali values (object), aur unke useful methods.

---

## List (Array): values ki line

```lipi
fruits = ["aam", "kela", "seb"]
show fruits
show fruits[0], fruits[1], fruits[-1]
show fruits.length
```

```output
["aam", "kela", "seb"]
aam kela seb
3
```

- Position **0 se** shuru hoti hai: `fruits[0]` pehla item.
- `fruits[-1]` aakhri item.
- `.length` = kitne items.

---

## List me add / remove

```lipi
fruits = ["aam", "kela"]
fruits.push("seb")
fruits.insert(0, "angoor")
show fruits
last = fruits.pop()
show last, fruits
fruits.remove("kela")
show fruits
```

```output
["angoor", "aam", "kela", "seb"]
seb ["angoor", "aam", "kela"]
["angoor", "aam"]
```

| Method | Kaam |
|---|---|
| `push(x)` | end me jodo |
| `insert(i, x)` | position i pe jodo |
| `pop()` | aakhri nikaalo |
| `remove(x)` | x hatao |
| `removeAt(i)` | position i wala hatao |

---

## Useful list methods

```lipi
marks = [78, 92, 65, 88]
show marks.sum(), marks.max(), marks.min()
show marks.sort()
show marks.contains(92), marks.indexOf(65)
show marks.first, marks.last
```

```output
323 92 65
[65, 78, 88, 92]
true 2
78 88
```

`sort()` ek **nayi** sorted list deta hai, purani list waisi hi rehti hai.

---

## map aur filter: har item pe kaam

`map` har item ko badal ke nayi list deta hai. `filter` sirf wo items rakhta hai jo condition pass karein:

```lipi
marks = [78, 92, 65, 88]
show marks.map(m => m + 5)
show marks.filter(m => m >= 80)
```

```output
[83, 97, 70, 93]
[92, 88]
```

`m => m + 5` ek chhota function hai: "har m ke liye m + 5 do". (Functions Chapter 6 me.)

Aur bhi:

```lipi
names = ["Ravi", "Asha", "Meera"]
show names.join(", ")
show names.find(n => n.startsWith("A"))
show names.any(n => n.length > 4), names.all(n => n.length > 2)
```

```output
Ravi, Asha, Meera
Asha
true true
```

---

## Object: naam wali values

Object me har value ka ek naam (key) hota hai. Jaise ek form:

```lipi
student = {naam: "Asha", umar: 20, city: "Pune"}
show student.naam
show student["city"]
student.umar = 21
student.email = "asha@example.com"
show student
```

```output
Asha
Pune
{naam: "Asha", umar: 21, city: "Pune", email: "asha@example.com"}
```

- `student.naam` se value lo, `student.umar = 21` se badlo.
- Nayi key likh do to wo jud jaati hai.

---

## Object ke methods

```lipi
student = {naam: "Asha", umar: 20}
show student.keys()
show student.values()
show student.has("phone"), student.get("phone", "nahi hai")
for key, value in student
    show key, "=", value
```

```output
["naam", "umar"]
["Asha", 20]
false nahi hai
naam = Asha
umar = 20
```

Jo key shayad na ho, uske liye `get("key", default)` safe hai.

---

## List of objects: asli data aisa dikhta hai

```lipi
students = [
    {naam: "Asha", marks: 92},
    {naam: "Ravi", marks: 67},
    {naam: "Meera", marks: 85},
]
toppers = students.filter(s => s.marks >= 80)
show toppers.map(s => s.naam)
for s in students
    show "{s.naam}: {s.marks}"
average = students.map(s => s.marks).sum() / students.length
show "Class average:", average.round(1)
```

```output
["Asha", "Meera"]
Asha: 92
Ravi: 67
Meera: 85
Class average: 81.3
```

---

## Text ke methods bhi dekh lo

```lipi
text = "  Namaste Duniya  "
show text.trim()
show text.trim().upper(), text.trim().lower()
show "a,b,c".split(",")
show "Hello".length, "Hello".contains("ell"), "Hello".replace("l", "L")
```

```output
Namaste Duniya
NAMASTE DUNIYA namaste duniya
["a", "b", "c"]
5 true HeLLo
```

---

## Khud karo

1. 5 doston ke naam ki list banao. Pehla, aakhri naam aur total count dikhao.
2. `prices = [120, 450, 80, 999, 300]`. Total, sabse mehnga, aur 200 se sasti cheezein dikhao.
3. Ek `book` object banao (title, author, price). Price 10% badha do aur poora object dikhao.
4. 3 products ki list (naam, price) banao aur sirf naam ki list dikhao.

<details>
<summary>Jawab dekho</summary>

```lipi
# 1
dost = ["Ravi", "Asha", "Meera", "Kabir", "Zoya"]
show dost[0], dost[-1], dost.length

# 2
prices = [120, 450, 80, 999, 300]
show "Total:", prices.sum()
show "Sabse mehnga:", prices.max()
show "200 se saste:", prices.filter(p => p < 200)

# 3
book = {title: "Godaan", author: "Premchand", price: 250}
book.price = book.price * 1.1
show book

# 4
products = [
    {naam: "Pen", price: 10},
    {naam: "Copy", price: 40},
    {naam: "Bag", price: 700},
]
show products.map(p => p.naam)
```

</details>

---

## Yaad rakho

| Kaam | List | Object |
|---|---|---|
| banana | `[1, 2, 3]` | `{naam: "A", umar: 5}` |
| value lena | `list[0]` | `obj.naam` / `obj["naam"]` |
| jodna | `list.push(x)` | `obj.nayi = x` |
| ginti | `list.length` | `obj.length` |
| loop | `for x in list` | `for key, value in obj` |

> **Teacher tip:** List ko "train ke dabbe" (0 se number) aur object ko "ID card" (har cheez ka naam) bol ke samjhao.
