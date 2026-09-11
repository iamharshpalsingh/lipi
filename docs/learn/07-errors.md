# 7. Errors: galti padhna aur sambhalna

**Is chapter me seekhoge:** error message kaise padhein, common errors, `try` / `catch` / `finally`, aur khud error `throw` karna.

---

## Error message padhna

Har LiPi error ke 4 hisse hote hain:

```lipi
naam = "Asha"
show nmae
```

```output
ERROR LIP1002: undefined variable "nmae"

main.lipi:2:6
    show nmae
         ^^^^

Hint: did you mean "naam"?
```

1. **`ERROR LIP1002`**: error ka code. Har tarah ki galti ka apna code hai.
2. **Message**: kya galat hai.
3. **Jagah**: `main.lipi:2:6` matlab file main.lipi, line 2, column 6, aur `^^^^` exact jagah.
4. **Hint**: kaise theek karein.

Error codes ka pehla number batata hai kis tarah ki galti hai:

| Code | Kis tarah ki galti |
|---|---|
| LIP0xxx | likhne ka tareeka galat (syntax) |
| LIP1xxx | naam galat ya nahi mila |
| LIP2xxx | type galat (text + number, condition...) |
| LIP3xxx | module / file |
| LIP4xxx | async |
| LIP5xxx | chalte waqt galti (list se bahar, null...) |

---

## Kuch common errors

**List ke bahar ki position:**

```lipi
items = [10, 20, 30]
show items[5]
```

```output
ERROR LIP5001: index 5 is outside array length 3

main.lipi:2:12
    show items[5]
               ^

Hint: Positions start at 0, so the last one is 2. Negative positions count from the end: -1 is the last item.
```

**null ki value padhna:**

```lipi
user = json.parse("null")
show user.naam
```

```output
ERROR LIP5003: cannot read ".naam" of null

main.lipi:2:11
    show user.naam
              ^^^^

Hint: "user" is null. Use user?.naam to get null instead of an error.
```

`user?.naam` likhne se error ki jagah `null` milta hai.

---

## try / catch: galti ko sambhalna

Kuch cheezein galat ho sakti hain (user galat input de, file na mile...). `try` me risky code, `catch` me kya karna hai agar galti hui:

```lipi
try
    number = toNumber("abc")
    show number + 1
catch error
    show "Galti hui:", error.message
show "Program chalta raha"
```

```output
Galti hui: cannot add Null and Integer
Program chalta raha
```

Bina `try` ke program ruk jaata. `try` ke saath galti pakdi gayi aur program aage chala.

`error` me ye cheezein hoti hain: `error.message`, `error.code`, `error.hint`, `error.line`.

---

## throw: khud error banana

Jab tumhare program ke rules toote, khud error bhejo:

```lipi
withdraw(balance, amount)
    if amount > balance
        throw "Itne paise nahi hain"
    return balance - amount

try
    show withdraw(100, 30)
    show withdraw(100, 500)
catch error
    show "Error:", error.message, error.code
finally
    show "Transaction khatam"
```

```output
70
Error: Itne paise nahi hain LIP5006
Transaction khatam
```

- `throw "message"` error bhejta hai.
- `finally` hamesha chalta hai: galti ho ya na ho.

---

## Galti dhoondhne ke tips (debugging)

1. **Error ka Hint zaroor padho.** Aadhe se zyada baar wahi solution hota hai.
2. **`show` se check karo** ki variable me kya hai: `show "debug:", total`.
3. **`lipi check file.lipi`** program chalaye bina galtiyan dhoondhta hai.
4. **Chhote tukde me test karo.** Poora program ek saath mat likho, thoda likho, chalao, phir aage.
5. VS Code me LiPi extension **likhte waqt hi** laal line se galti dikhata hai.

---

## Khud karo

1. Ek list banao aur jaan-boojh ke galat position padho. Error ka code aur hint note karo.
2. `safeDivide(a, b)` banao jo b = 0 hone pe `"Zero se divide nahi kar sakte"` throw kare. `try` se dono case chalao.
3. `ageCheck(umar)` banao: umar 0 se kam ya 150 se zyada ho to error throw kare.

<details>
<summary>Jawab dekho</summary>

```lipi
# 2
safeDivide(a, b)
    if b == 0
        throw "Zero se divide nahi kar sakte"
    return a / b

try
    show safeDivide(10, 2)
    show safeDivide(5, 0)
catch error
    show error.message

# 3
ageCheck(umar)
    if umar < 0 or umar > 150
        throw "Umar {umar} sahi nahi hai"
    return "Theek hai"

for u in [25, -3, 200]
    try
        show ageCheck(u)
    catch error
        show "Galti:", error.message
```

</details>

---

## Yaad rakho

| Cheez | Kaam |
|---|---|
| `try` ... `catch error` | galti pakdo |
| `finally` | hamesha chalo |
| `throw "msg"` | khud error bhejo |
| `error.message` / `error.code` | galti ki detail |
| `x?.naam` | null ho to error nahi, null |
| `lipi check` | bina chalaye galti dhoondho |

> **Teacher tip:** Class me "error hunt" khelo: code me 3 galtiyan chhupa do, students error messages padh ke dhoondhein. Jo pehle theek kare wo jeete.
