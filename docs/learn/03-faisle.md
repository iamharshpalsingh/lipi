# 3. Comparison aur faisle (if / else)

**Is chapter me seekhoge:** do values ko compare karna, `and` / `or` / `not`, aur program se faisle karwana: `if`, `else if`, `else`, aur `match`.

---

## Compare karna

Comparison ka jawab hamesha `true` (haan) ya `false` (na) hota hai:

```lipi
show 5 > 3, 5 < 3, 5 == 5, 5 != 5
show 10 >= 10, 7 <= 6
show "aam" == "aam", "aam" == "kela"
```

```output
true false true false
true false
true false
```

| Symbol | Matlab |
|---|---|
| `==` | barabar hai? |
| `!=` | barabar nahi hai? |
| `>` / `<` | bada / chhota |
| `>=` / `<=` | bada ya barabar / chhota ya barabar |

Yaad rakho: `=` value **rakhta** hai, `==` **check** karta hai.

---

## and, or, not

```lipi
umar = 20
hasId = true
show umar >= 18 and hasId
show umar < 18 or hasId
show not hasId
```

```output
true
true
false
```

- `and`: dono sach hon tab `true`
- `or`: koi ek bhi sach ho to `true`
- `not`: ulta kar do

---

## if: faisla karna

Agar condition sach hai, to andar wali (indent ki hui) lines chalti hain:

```lipi
temperature = 38
if temperature > 35
    show "Bahut garmi hai! Paani piyo."
show "Ye line hamesha chalegi"
```

```output
Bahut garmi hai! Paani piyo.
Ye line hamesha chalegi
```

**Indentation zaroori hai:** `if` ke andar ki lines **4 spaces** aage honi chahiye. LiPi me `{ }` ya `:` nahi lagte.

---

## if / else if / else

```lipi
marks = 82
if marks >= 90
    show "Grade A"
else if marks >= 75
    show "Grade B"
else if marks >= 33
    show "Grade C"
else
    show "Fail, dobara koshish karo"
```

```output
Grade B
```

Upar se neeche check hota hai. Jo pehli condition sach mili, wahi chalti hai, baaki skip.

---

## Ek line me choice (inline if)

Chhote faisle ek line me:

```lipi
marks = 45
result = "Pass" if marks >= 33 else "Fail"
show result
```

```output
Pass
```

---

## LiPi me condition sirf true/false hoti hai

Kuch languages me `if 5` chal jaata hai. LiPi me nahi, kyunki isse galtiyan chhup jaati hain. Condition hamesha `true`/`false` honi chahiye:

```lipi
count = 5
if count
    show "hai"
```

```output
ERROR LIP2005: expected a Boolean (true or false), but this is an Integer

main.lipi:2:4
    if count
       ^^^^^

Hint: Compare it explicitly, for example: if count > 0
```

Theek tareeka: `if count > 0`.

---

## match: bahut saare options

Jab ek value ke kai possible options hon, `match` saaf rehta hai:

```lipi
day = "sunday"
match day
    "saturday", "sunday"
        show "Chhutti ka din!"
    "monday"
        show "Naya hafta shuru"
    else
        show "Kaam ka din"
```

```output
Chhutti ka din!
```

`match` numbers ke range bhi samajhta hai:

```lipi
umar = 15
match umar
    0 to 12
        show "Bachcha"
    13 to 19
        show "Teenager"
    else
        show "Bada"
```

```output
Teenager
```

---

## Khud karo

1. `number = 7`. Batao number even hai ya odd. (Hint: `number % 2 == 0`)
2. `umar` aur `hasTicket` variables banao. Movie tabhi dekh sakte hain jab umar 13 ya zyada ho **aur** ticket ho.
3. `bill = 1200`. 1000 se zyada bill pe 10% discount, warna koi discount nahi. Final amount dikhao.
4. `match` se din ka number (1-7) leke din ka naam dikhao.

<details>
<summary>Jawab dekho</summary>

```lipi
# 1
number = 7
if number % 2 == 0
    show number, "even hai"
else
    show number, "odd hai"

# 2
umar = 14
hasTicket = true
if umar >= 13 and hasTicket
    show "Movie enjoy karo!"
else
    show "Sorry, entry nahi"

# 3
bill = 1200
discount = bill * 0.1 if bill > 1000 else 0
show "Final amount:", bill - discount

# 4
dayNumber = 3
match dayNumber
    1
        show "Monday"
    2
        show "Tuesday"
    3
        show "Wednesday"
    4
        show "Thursday"
    5
        show "Friday"
    6, 7
        show "Weekend!"
    else
        show "Galat number"
```

</details>

---

## Yaad rakho

| Cheez | Example |
|---|---|
| compare | `a == b`, `a != b`, `a > b` |
| jodna | `and`, `or`, `not` |
| faisla | `if` / `else if` / `else` |
| ek line | `x if condition else y` |
| kai options | `match value` |
| indent | andar ki lines 4 spaces aage |

> **Teacher tip:** Real life ke faisle se shuru karo: "Agar baarish ho rahi hai to chhata lo, warna nahi." Pehle board pe hindi me likho, phir LiPi me.
