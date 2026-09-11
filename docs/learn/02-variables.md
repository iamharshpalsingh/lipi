# 2. Variables aur values

**Is chapter me seekhoge:** variable kya hai, number/text/true-false kaise rakhte hain, text ke andar value kaise daalte hain, aur `null`.

---

## Variable kya hai?

Variable ek **dabba** hai jiska ek naam hota hai, aur usme koi value rakhte hain. Baad me naam se value nikaal sakte ho.

```lipi
naam = "Asha"
umar = 20
show naam
show umar
```

```output
Asha
20
```

`naam = "Asha"` ka matlab: "naam" naam ke dabbe me "Asha" rakh do. `=` ka matlab **rakhna** hai (barabar check karna nahi, uske liye `==` hota hai).

Value badal bhi sakte ho:

```lipi
score = 10
score = 25
show score
```

```output
25
```

**Naam ke rules:** letters, numbers aur `_` use karo, number se shuru mat karo. LiPi me style hai `camelCase`: jaise `totalMarks`, `studentName`.

---

## Values ke types

Har value ka ek **type** hota hai. `typeOf()` batata hai value kis type ki hai:

```lipi
umar = 20
height = 5.4
naam = "Asha"
student = true
pata = null
show typeOf(umar), typeOf(height), typeOf(naam), typeOf(student), typeOf(pata)
```

```output
Integer Decimal String Boolean Null
```

| Type | Kya hai | Example |
|---|---|---|
| **Integer** | poora number | `20`, `-5`, `1_000_000` |
| **Decimal** | point wala number | `5.4`, `99.99` |
| **String** | text | `"Asha"` |
| **Boolean** | haan ya na | `true`, `false` |
| **Null** | koi value nahi | `null` |
| **Array** | list | `[1, 2, 3]` (Chapter 5) |
| **Object** | naam wali values | `{naam: "Asha"}` (Chapter 5) |

---

## Numbers ka hisaab

```lipi
show 7 + 3, 10 - 4, 6 * 7
show 10 / 4
show 10 / 2
show 17 % 5
show 2 ** 10
```

```output
10 6 42
2.5
5.0
2
1024
```

- `/` (divide) ka jawab **hamesha Decimal** hota hai: `10 / 2` = `5.0`. Isse galat hisaab nahi hota.
- `%` = bacha hua (remainder): 17 ko 5 se divide karo, 2 bachta hai.
- `**` = power: 2 ki power 10 = 1024.

---

## Text ke andar value daalna (interpolation)

Double quotes `"..."` ke andar `{ }` me variable ya hisaab likh do, uski value text me aa jaati hai:

```lipi
naam = "Asha"
umar = 20
show "Mera naam {naam} hai aur main {umar} saal ki hoon"
show "Agle saal main {umar + 1} saal ki ho jaaungi"
```

```output
Mera naam Asha hai aur main 20 saal ki hoon
Agle saal main 21 saal ki ho jaaungi
```

**Single quotes** `'...'` me `{ }` kaam nahi karta, text bilkul waisa ka waisa rehta hai:

```lipi
show 'Ye {naam} waise ka waisa dikhega'
```

```output
Ye {naam} waise ka waisa dikhega
```

Text ko jodne ke liye `+` bhi use kar sakte ho (dono text hon tab):

```lipi
pehla = "Lipi"
doosra = " Language"
show pehla + doosra
```

```output
Lipi Language
```

---

## Common galti: text + number

Text aur number ko `+` se nahi jod sakte. LiPi rok deta hai aur bataata hai kya karein:

```lipi
umar = 20
show "Umar: " + umar
```

```output
ERROR LIP2001: cannot add String and Integer

main.lipi:2:6
    show "Umar: " + umar
         ^^^^^^^^^^^^^^^

Hint: To put a value inside text, use interpolation, for example: "Total: {total}"
```

Theek tareeka: `show "Umar: {umar}"` ya `show "Umar:", umar`.

---

## Type badalna (conversion)

User se jo aata hai wo aksar text hota hai. Number chahiye to `toNumber()`:

```lipi
text = "42"
number = toNumber(text)
show number + 8
show toNumber("abc")
show toString(5) + " aam"
```

```output
50
null
5 aam
```

`toNumber("abc")` ka jawab `null` hai, kyunki "abc" number nahi hai.

---

## null: jab koi value na ho

`null` matlab "abhi kuch nahi hai". `??` bolta hai: agar left wala `null` hai to right wala use karo.

```lipi
phone = null
show phone ?? "phone number nahi diya"
city = "Pune"
show city ?? "pata nahi"
```

```output
phone number nahi diya
Pune
```

---

## const: jo kabhi na badle

Kuch values kabhi nahi badalni chahiye. Unhe `const` se banao:

```lipi
const school = "LiPi Academy"
show school
```

```output
LiPi Academy
```

Agar baad me `school = "Kuch aur"` likha to LiPi error dega (`LIP1003`).

---

## Type likh ke batana (optional)

Chaho to bata sakte ho ki variable me kaunsi type rahegi. Galat value daali to LiPi turant pakad lega:

```lipi
marks: Integer = 90
naam: String = "Ravi"
email: String? = null
show marks, naam, email
```

```output
90 Ravi null
```

`String?` ka matlab: String ya `null`.

---

## Khud karo

1. `naam`, `umar`, `city` variables banao aur ek sentence dikhao: "Main Asha hoon, 20 saal, Pune se" (interpolation use karo).
2. Ek rectangle ki lambai 12 aur chaudai 5 hai. Area aur perimeter dikhao.
3. `"250"` aur `"150"` text hain. Inko number bana ke jodo aur jawab dikhao.
4. `price = 499.0` aur `quantity = 3`. Total aur 18% GST ke saath total dikhao.

<details>
<summary>Jawab dekho</summary>

```lipi
# 1
naam = "Asha"
umar = 20
city = "Pune"
show "Main {naam} hoon, {umar} saal, {city} se"

# 2
lambai = 12
chaudai = 5
show "Area:", lambai * chaudai
show "Perimeter:", 2 * (lambai + chaudai)

# 3
a = toNumber("250")
b = toNumber("150")
show a + b

# 4
price = 499.0
quantity = 3
total = price * quantity
show "Total:", total
show "GST ke saath:", total * 1.18
```

</details>

---

## Yaad rakho

| Cheez | Matlab |
|---|---|
| `x = 5` | x me 5 rakho |
| `"Hi {naam}"` | text me value daalo |
| `10 / 4` | hamesha Decimal (2.5) |
| `a ?? b` | a null hai to b |
| `const x = 1` | kabhi na badalne wala |
| `toNumber("5")` | text se number |
| `typeOf(x)` | x ka type |

> **Teacher tip:** Variable ko "naam likha hua dabba" bol ke samjhao. Board pe dabbe bana ke unme values likho, phir value badal ke dikhao ki purani value mit jaati hai.
