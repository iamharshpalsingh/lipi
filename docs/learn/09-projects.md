# 9. Projects, files aur tests

**Is chapter me seekhoge:** project banana, code ko kai files me baantna (`use` / `export`), tests likhna, aur code saaf rakhna (`format`, `lint`).

---

## Project banana

```
lipi new school-app
cd school-app
```

Ye banata hai:

```
school-app\
├── lipi.json          ← project ki settings
├── src\
│   └── main.lipi      ← main code
└── tests\
    └── main_test.lipi ← tests
```

| Command | Kaam |
|---|---|
| `lipi run` | project chalao (src\main.lipi) |
| `lipi test` | saare tests chalao |
| `lipi check` | galtiyan dhoondho |
| `lipi format` | code ko saaf format me likho |
| `lipi lint` | faaltu variables, bekaar code dhoondho |

---

## Code ko files me baantna

Bade program ek file me mushkil ho jaate hain. Kaam ke hisaab se files banao.

`src\maths.lipi`:

```lipi
export add(a, b)
    return a + b

export average(list)
    return list.sum() / list.length

export const pi = 3.14159
```

`export` batata hai kaunsi cheezein doosri files use kar sakti hain. Bina `export` wali cheezein us file ki **private** rehti hain.

`src\main.lipi`:

```lipi
use "./maths.lipi"

show maths.add(2, 3)
show maths.average([70, 80, 90])
```

Ya sirf kuch cheezein seedhe naam se:

```lipi
from "./maths.lipi" use add, pi

show add(10, 20), pi
```

---

## Built-in modules

LiPi ke saath kuch modules pehle se aate hain:

```lipi
show math.sqrt(144), math.round(3.14159, 2), math.max(4, 9, 2)
show json.stringify({naam: "Asha", marks: [90, 85]})
data = json.parse('{"city": "Pune", "pin": 411001}')
show data.city, data.pin
```

```output
12.0 3.14 9
{"naam":"Asha","marks":[90,85]}
Pune 411001
```

| Module | Kaam |
|---|---|
| `math` | sqrt, round, max, min, random... |
| `json` | JSON banana / padhna |
| `time` | time.now(), time.iso() |
| `fs` | files padhna/likhna |
| `http` | internet se data lena |
| `crypto` | password hash, random token |
| `server`, `database` | backend (Chapter 11) |

---

## Tests: apna code khud check karo

Test ek chhota check hai: "ye function ye jawab dega". `tests\main_test.lipi`:

```lipi
use "../src/maths.lipi" as maths

test "add do numbers jodta hai"
    assertEqual(maths.add(2, 3), 5)

test "average sahi hai"
    assertEqual(maths.average([10, 20, 30]), 20.0)
```

`lipi test` chalao:

```
tests\main_test.lipi
  ✓ add do numbers jodta hai
  ✓ average sahi hai

2 passed
```

Jab baad me code badlo, `lipi test` bata dega kuch toota to nahi.

---

## Code saaf rakhna

```
lipi format    # sab files ko ek jaise saaf format me
lipi lint      # warning: unused variable, bekaar code...
```

VS Code me save karte hi format apne aap ho jaata hai.

---

## Khud karo

1. `lipi new calculator` banao. `src\calc.lipi` me `add`, `sub`, `mul`, `div` export karo, aur `main.lipi` me use karo.
2. Char functions ke liye tests likho aur `lipi test` chalao.
3. `lipi lint` chala ke dekho koi warning aati hai kya.

<details>
<summary>Jawab dekho</summary>

`src\calc.lipi`:

```lipi
export add(a, b)
    return a + b

export sub(a, b)
    return a - b

export mul(a, b)
    return a * b

export div(a, b)
    if b == 0
        throw "Zero se divide nahi"
    return a / b
```

`tests\calc_test.lipi`:

```lipi
use "../src/calc.lipi" as calc

test "add"
    assertEqual(calc.add(2, 3), 5)

test "div"
    assertEqual(calc.div(10, 4), 2.5)
```

</details>

---

## Yaad rakho

| Cheez | Example |
|---|---|
| naya project | `lipi new naam` |
| export | `export add(a, b)` |
| poora module | `use "./maths.lipi"` |
| kuch naam | `from "./maths.lipi" use add` |
| test | `test "naam"` + `assertEqual(a, b)` |
| saaf code | `lipi format`, `lipi lint` |

> **Teacher tip:** Group project karao: har student ek file (module) banaye, phir sab milke ek app banayein. Isse teamwork aur `export` dono samajh aata hai.
