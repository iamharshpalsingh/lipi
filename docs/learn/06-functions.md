# 6. Functions: apna kaam ek naam se

**Is chapter me seekhoge:** function banana, value lena (parameters), value lautana (`return`), default values, naam se arguments, aur chhote functions (`=>`).

---

## Function kya hai?

Function code ka ek tukda hai jise naam de dete ho. Jab chaho naam se bula lo, baar-baar code likhne ki zaroorat nahi.

```lipi
greet()
    show "Namaste!"
    show "LiPi me swagat hai"

greet()
greet()
```

```output
Namaste!
LiPi me swagat hai
Namaste!
LiPi me swagat hai
```

Function ka naam, `()`, aur neeche indent kiya hua code. Bas.

---

## Parameters: function ko value dena

```lipi
greet(naam)
    show "Namaste, {naam}!"

greet("Asha")
greet("Ravi")
```

```output
Namaste, Asha!
Namaste, Ravi!
```

`naam` parameter hai: bulate waqt jo doge, wo `naam` me aa jayega.

---

## return: jawab lautana

```lipi
add(a, b)
    return a + b

jawab = add(10, 20)
show jawab
show add(5, 7) * 2
```

```output
30
24
```

`return` function se value **bahar bhejta** hai. Us value ko variable me rakh sakte ho ya seedha use kar sakte ho.

---

## Default values aur naam se bulana

```lipi
function area(lambai, chaudai = 1) -> Integer
    return lambai * chaudai

show area(5, 3)
show area(4)
show area(chaudai: 2, lambai: 10)
```

```output
15
4
20
```

- `chaudai = 1`: agar chaudai na do to 1 maan lo.
- `area(chaudai: 2, lambai: 10)`: naam se bulao, order ki tension nahi.
- `function` word aur `-> Integer` (kya lautayega) **optional** hain. Bade programs me saaf rehte hain.

---

## Chhote functions: =>

Ek line wale function ke liye:

```lipi
double = x => x * 2
jod = (a, b) => a + b
show double(21), jod(3, 4)
show [1, 2, 3].map(x => x * x)
```

```output
42 7
[1, 4, 9]
```

---

## Function ke andar ke variables

Function ke andar banaya variable bahar nahi dikhta. Par agar bahar wala variable pehle se hai, to function use badal sakta hai:

```lipi
total = 0

addMarks(n)
    total += n

addMarks(40)
addMarks(35)
show total
```

```output
75
```

---

## Function bahut kaam aate hain: example

```lipi
grade(marks)
    if marks >= 90
        return "A"
    else if marks >= 75
        return "B"
    else if marks >= 33
        return "C"
    return "Fail"

report(naam, marks)
    show "{naam}: {marks} marks, grade {grade(marks)}"

report("Asha", 95)
report("Ravi", 78)
report("Kabir", 20)
```

```output
Asha: 95 marks, grade A
Ravi: 78 marks, grade B
Kabir: 20 marks, grade Fail
```

Ek function doosre function ko bula sakta hai. Bade program aise hi chhote-chhote functions se bante hain.

---

## Common galti: argument bhoolna

```lipi
greet(naam)
    return "Namaste, {naam}"

show greet()
```

```output
ERROR LIP2003: missing argument "naam" for "greet"

main.lipi:4:6
    show greet()
         ^^^^^^^

Hint: It is defined as greet(naam).
```

LiPi batata hai function ko kya chahiye tha.

---

## Khud karo

1. `square(n)` banao jo n ka square lautaaye.
2. `isEven(n)` banao jo `true`/`false` lautaaye.
3. `celsiusToF(c)` banao (formula: c * 9 / 5 + 32). 0, 37, 100 ke liye chalao.
4. `bill(amount, tip = 10)` banao jo amount + tip% lautaaye. Tip de ke aur bina tip ke chalao.

<details>
<summary>Jawab dekho</summary>

```lipi
# 1
square(n)
    return n * n

show square(9)

# 2
isEven(n)
    return n % 2 == 0

show isEven(4), isEven(7)

# 3
celsiusToF(c)
    return c * 9 / 5 + 32

for c in [0, 37, 100]
    show c, "C =", celsiusToF(c), "F"

# 4
bill(amount, tip = 10)
    return amount + amount * tip / 100

show bill(500)
show bill(500, tip: 20)
```

</details>

---

## Yaad rakho

| Cheez | Example |
|---|---|
| banana | `greet(naam)` + indented body |
| bulana | `greet("Asha")` |
| lautana | `return value` |
| default | `f(x, y = 1)` |
| naam se | `f(y: 2, x: 5)` |
| chhota | `x => x * 2` |

> **Teacher tip:** Function ko "recipe" bolo: ingredients (parameters) do, dish (return value) milti hai. Ek hi recipe se kitni baar bhi bana sakte ho.
