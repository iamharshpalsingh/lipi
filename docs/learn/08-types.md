# 8. Apne types banana (type)

**Is chapter me seekhoge:** apna khud ka type (jaise Student, Product) banana, uske fields aur methods, aur `self`.

---

## Kyun apna type?

Objects (`{naam: "Asha"}`) achhe hain, par har student ka object alag-alag likhne me galti ho sakti hai (koi `naam` likhe, koi `name`). `type` ek **saancha (template)** hai: batata hai har Student me kya-kya hoga.

```lipi
type Student
    naam: String
    marks: Integer = 0

asha = Student("Asha", 95)
ravi = Student(naam: "Ravi")
show asha
show ravi
show asha.naam, ravi.marks
```

```output
Student {naam: "Asha", marks: 95}
Student {naam: "Ravi", marks: 0}
Asha 0
```

- `naam: String`: har Student me naam hoga, text hoga.
- `marks: Integer = 0`: marks na do to 0.
- Banane ke liye type ka naam function jaisa bulao: `Student("Asha", 95)`.

---

## Methods: type ke apne functions

Type ke andar function likho. Andar `self` matlab "ye wala object":

```lipi
type Student
    naam: String
    marks: Integer = 0

    grade()
        if self.marks >= 90
            return "A"
        else if self.marks >= 75
            return "B"
        return "C"

    report()
        return "{self.naam} ko grade {self.grade()} mila"

asha = Student("Asha", 95)
ravi = Student("Ravi", 80)
show asha.report()
show ravi.report()
```

```output
Asha ko grade A mila
Ravi ko grade B mila
```

---

## Galat value se bachaav

Type ne bataya ki `marks` Integer hai. Galat value dene pe LiPi turant pakadta hai:

```lipi
type Product
    naam: String
    price: Integer

p = Product("Pen", 10)
p.price = json.parse('"das"')
```

```output
ERROR LIP2002: "price" should be an Integer, but this is a String

main.lipi:6:11
    p.price = json.parse('"das"')
              ^^^^^^^^^^^^^^^^^^^

Hint: "price" was declared as Integer.
```

---

## Example: bank account

```lipi
type Account
    owner: String
    balance: Integer = 0

    deposit(amount)
        self.balance += amount

    withdraw(amount)
        if amount > self.balance
            throw "Balance kam hai"
        self.balance -= amount

acc = Account("Asha")
acc.deposit(1000)
acc.withdraw(300)
show acc.owner, "ka balance:", acc.balance
try
    acc.withdraw(5000)
catch error
    show error.message
```

```output
Asha ka balance: 700
Balance kam hai
```

---

## Khud karo

1. `Book` type banao (title, author, pages). Do books banao aur dikhao.
2. `Rectangle` type banao (lambai, chaudai) with `area()` aur `perimeter()` methods.
3. `Counter` type banao (count = 0) with `increase()` aur `reset()`.

<details>
<summary>Jawab dekho</summary>

```lipi
# 1
type Book
    title: String
    author: String
    pages: Integer = 100

show Book("Godaan", "Premchand", 312)
show Book(title: "Gitanjali", author: "Tagore")

# 2
type Rectangle
    lambai: Integer
    chaudai: Integer

    area()
        return self.lambai * self.chaudai

    perimeter()
        return 2 * (self.lambai + self.chaudai)

r = Rectangle(10, 4)
show r.area(), r.perimeter()

# 3
type Counter
    count: Integer = 0

    increase()
        self.count += 1

    reset()
        self.count = 0

c = Counter()
c.increase()
c.increase()
show c.count
c.reset()
show c.count
```

</details>

---

## Yaad rakho

| Cheez | Example |
|---|---|
| type banana | `type Student` + fields |
| field | `naam: String`, `marks: Integer = 0` |
| object banana | `Student("Asha", 95)` |
| method | type ke andar function |
| `self` | method me "ye object" |

> **Teacher tip:** `type` ko "form ka format" bolo: school ka admission form sabke liye same hota hai, bas bhari hui values alag hoti hain.
