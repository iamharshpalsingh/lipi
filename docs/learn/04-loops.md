# 4. Loops: kaam baar-baar karwana

**Is chapter me seekhoge:** `for` loop, number ranges (`1 to 10`), `while`, `repeat`, aur `break` / `continue`.

---

## Kyun chahiye loop?

Maan lo 1 se 5 tak numbers dikhane hain. Paanch baar `show` likh sakte ho, par 1000 tak? Loop se ek baar likho, computer baar-baar chalayega.

---

## for: range ke saath

```lipi
for i in 1 to 5
    show i
```

```output
1
2
3
4
5
```

`1 to 5` me 1 aur 5 **dono shaamil** hain. `i` har baar agla number leta hai.

Ulta ya jump karke bhi chal sakte ho (`step`):

```lipi
for i in 10 to 0 step -5
    show i
```

```output
10
5
0
```

**Table banao:**

```lipi
for i in 1 to 5
    show "7 x {i} = {7 * i}"
```

```output
7 x 1 = 7
7 x 2 = 14
7 x 3 = 21
7 x 4 = 28
7 x 5 = 35
```

---

## for: list ke saath

```lipi
fruits = ["aam", "kela", "seb"]
for fruit in fruits
    show "Mujhe {fruit} pasand hai"
```

```output
Mujhe aam pasand hai
Mujhe kela pasand hai
Mujhe seb pasand hai
```

Position (index) bhi chahiye? Do naam likho. **Pehla** index hota hai (0 se shuru), doosra value:

```lipi
fruits = ["aam", "kela", "seb"]
for i, fruit in fruits
    show i + 1, fruit
```

```output
1 aam
2 kela
3 seb
```

Text ke har letter pe bhi loop chal sakta hai:

```lipi
for letter in "LiPi"
    show letter
```

```output
L
i
P
i
```

---

## Loop me total banana

```lipi
marks = [78, 92, 65, 88]
total = 0
for m in marks
    total += m
show "Total:", total
show "Average:", total / marks.length
```

```output
Total: 323
Average: 80.75
```

`total += m` ka matlab `total = total + m`. Aise hi `-=`, `*=`, `/=` bhi hote hain.

---

## while: jab tak condition sach hai

```lipi
paise = 100
din = 0
while paise > 0
    paise -= 30
    din += 1
show "Paise {din} din me khatam hue. Bache:", paise
```

```output
Paise 4 din me khatam hue. Bache: -20
```

**Dhyan:** agar condition kabhi false na ho to loop kabhi nahi rukega (infinite loop). Terminal me **Ctrl + C** dabaake program rok sakte ho.

---

## repeat: bas N baar

```lipi
repeat 3
    show "Hip hip hurray!"
```

```output
Hip hip hurray!
Hip hip hurray!
Hip hip hurray!
```

---

## break aur continue

`break` = loop turant band. `continue` = ye round chhodo, agle pe jao.

```lipi
for n in 50 to 60
    if n % 7 == 0
        show "Pehla 7 ka multiple:", n
        break
```

```output
Pehla 7 ka multiple: 56
```

```lipi
for n in 1 to 8
    if n % 2 == 1
        continue
    show n
```

```output
2
4
6
8
```

---

## Khud karo

1. 1 se 100 tak ke numbers ka total nikaalo. (Jawab 5050 aana chahiye.)
2. Kisi bhi number ka table 1 se 10 tak dikhao.
3. `["Asha", "Ravi", "Meera"]` list ke har naam ke saath "Welcome, ___!" dikhao.
4. 1 se 30 tak: 3 se divide ho to "Fizz", 5 se ho to "Buzz", dono se ho to "FizzBuzz", warna number.

<details>
<summary>Jawab dekho</summary>

```lipi
# 1
total = 0
for i in 1 to 100
    total += i
show total

# 2
number = 9
for i in 1 to 10
    show "{number} x {i} = {number * i}"

# 3
for naam in ["Asha", "Ravi", "Meera"]
    show "Welcome, {naam}!"

# 4
for n in 1 to 30
    if n % 15 == 0
        show "FizzBuzz"
    else if n % 3 == 0
        show "Fizz"
    else if n % 5 == 0
        show "Buzz"
    else
        show n
```

</details>

---

## Yaad rakho

| Loop | Kab use karein |
|---|---|
| `for i in 1 to 10` | numbers ke range pe |
| `for x in list` | list ke har item pe |
| `for i, x in list` | index bhi chahiye |
| `while condition` | jab tak kuch sach hai |
| `repeat 5` | bas 5 baar |
| `break` / `continue` | rukna / skip karna |

> **Teacher tip:** FizzBuzz (exercise 4) interview ka famous sawaal hai. Students ko pehle khud try karne do, 10 minute baad solution dikhao.
