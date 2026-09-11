# 13. LiPi vs JavaScript: kya fark hai?

**Is chapter me dekhoge:** same kaam LiPi aur JavaScript me kaise dikhta hai, LiPi seekhna kyun aasaan hai, aur LiPi JavaScript ki poori taakat kaise use karta hai.

---

## Sabse badi baat: LiPi, JavaScript ki taakat ke saath

LiPi ka web code **JavaScript me compile** hota hai (`lipi build`). Iska matlab:

- Jo tum LiPi me banate ho, wo **har browser** me chalta hai: Chrome, Edge, Firefox, Safari, Android, iPhone.
- `js` module se tum **JavaScript ki libraries aur browser ki har cheez** LiPi se use kar sakte ho.
- Aur LiPi sirf frontend nahi: **backend (server), database, tests, formatter, editor support**, sab ek hi install me.

Yaani: **seekho aasaan language, pao JavaScript jitni pahunch.**

---

## 1. Kam likhna, saaf padhna

Ek list me se 80 se zyada marks wale students ke naam:

**JavaScript:**

```js
const students = [
  { name: "Asha", marks: 92 },
  { name: "Ravi", marks: 67 },
  { name: "Meera", marks: 85 },
];
const toppers = students.filter((s) => s.marks > 80).map((s) => s.name);
console.log(toppers);
```

**LiPi:**

```lipi
students = [
    {name: "Asha", marks: 92},
    {name: "Ravi", marks: 67},
    {name: "Meera", marks: 85},
]
show students.filter(s => s.marks > 80).map(s => s.name)
```

```output
["Asha", "Meera"]
```

LiPi me `const`/`let`/`var` ki tension nahi, `;` nahi, `console.log` ki jagah seedha `show`.

---

## 2. Chupke se galat jawab nahi

JavaScript me `"5" + 3` ka jawab `"53"` aata hai, bina kisi warning ke. Beginners ke liye ye bahut badi pareshani hai.

**JavaScript:**

```js
console.log("5" + 3); // "53" (koi error nahi, bas galat jawab)
```

**LiPi:**

```lipi
show "5" + 3
```

```output
ERROR LIP2001: cannot add String and Integer

main.lipi:1:6
    show "5" + 3
         ^^^^^^^

Hint: To put a value inside text, use interpolation, for example: "Total: {total}"
```

LiPi galti **chalne se pehle** pakad leta hai, aur batata hai kaise theek karein.

---

## 3. == wahi karta hai jo lagta hai

**JavaScript:**

```js
console.log([1, 2] == [1, 2]); // false  (confusing!)
console.log(0 == "");          // true   (aur bhi confusing)
```

**LiPi:**

```lipi
show [1, 2] == [1, 2]
show {naam: "Asha"} == {naam: "Asha"}
```

```output
true
true
```

LiPi me `==` andar ki values compare karta hai. `==` aur `===` ka jhanjhat hi nahi.

---

## 4. if me sirf true/false

JavaScript me khaali list `[]` ko bhi "true" maana jaata hai, isliye `if (items)` hamesha chal jaata hai, chahe list khaali ho. LiPi ye galti hone hi nahi deta:

```lipi
items = []
if items
    show "list me kuch hai"
```

```output
ERROR LIP2005: expected a Boolean (true or false), but this is an Array

main.lipi:2:4
    if items
       ^^^^^

Hint: Check for items explicitly, for example: if not items.isEmpty()
```

---

## 5. Bade numbers bilkul sahi

JavaScript bade whole numbers ko chupke se badal deta hai:

```js
console.log(9007199254740993); // 9007199254740992  (1 kam!)
```

LiPi me Integer poore 64-bit range tak **exact** rehta hai, aur range se bahar jaaye to chupke se galat nahi hota, saaf error deta hai:

```lipi
show 9007199254740993
big = 9223372036854775807
show big + 1
```

```output
9007199254740993
ERROR LIP5009: this Integer calculation overflowed

main.lipi:3:6
    show big + 1
         ^^^^^^^

Hint: Integers go up to 9223372036854775807. For bigger values, use Decimals (for example 1.0 * x).
```

Paise, bank, marks ke hisaab me ye bahut zaroori hai.

---

## 6. Error jo sikhaaye

JavaScript me missing field ka error:

```
TypeError: Cannot read properties of undefined (reading 'city')
```

LiPi me:

```lipi
user = {naam: "Asha"}
show user.address.city
```

```output
ERROR LIP5004: this Object has no field "address"

main.lipi:2:11
    show user.address.city
              ^^^^^^^

Hint: If the field may be missing, use obj.get("address") or obj["address"], which give null instead of an error.
```

Aur agar field shayad na ho, to `?.` se safe tareeka:

```lipi
user = {naam: "Asha"}
show user.address?.city ?? "address nahi diya"
```

```output
address nahi diya
```

---

## 7. Ek install, sab kuch

JavaScript me ek poora web app banane ke liye aam taur pe chahiye:

| Kaam | JavaScript | LiPi |
|---|---|---|
| language chalana | Node.js | `lipi` |
| packages | npm | `lipi install` (built-in) |
| website / UI | React / Vue + build tool | `page`, `component`, `state` (built-in) |
| server / API | Express | `server.start`, `get "/..."` (built-in) |
| database | ORM / driver package | `database.open(...)` (built-in) |
| tests | Jest / Vitest | `test "..."` + `lipi test` (built-in) |
| formatting | Prettier | `lipi format` (built-in) |
| galtiyan pakadna | ESLint / TypeScript | `lipi check`, `lipi lint` (built-in) |
| editor support | extensions | LiPi VS Code extension |

LiPi me **ek hi install**, ek hi tareeka, ek hi language: frontend, backend, database, sab.

---

## Poora example: same app, dono me

Ek counter button wala web page:

**JavaScript (React ke saath, ek hisse ka code):**

```js
import { useState } from "react";

export default function App() {
  const [count, setCount] = useState(0);
  return (
    <div>
      <h1>Counter</h1>
      <p>Count: {count}</p>
      <button onClick={() => setCount(count + 1)}>+1</button>
    </div>
  );
}
```

Upar se React install, build setup, bundler...

**LiPi (poora app, bas itna):**

```lipi
state count = 0

page "/"
    heading "Counter", level: 1
    text "Count: {count}"
    button "+1"
        count += 1
```

`lipi dev app.lipi` aur browser me chal raha hai.

---

## Sach baat: JavaScript abhi kahan aage hai

LiPi nayi language hai, aur JavaScript 30 saal purani. Kuch cheezon me JavaScript abhi aage hai, aur ye jaanna achha hai:

- **Libraries:** npm pe laakhon packages hain. LiPi me `js` module se inhe use kar sakte ho, aur LiPi ke apne packages badh rahe hain.
- **Jobs aur community:** JavaScript ki community bahut badi hai. LiPi seekhne ke baad JavaScript samajhna bhi aasaan ho jaata hai, kyunki concepts (variables, functions, lists, objects, pages, APIs) wahi hain.
- **Speed:** bahut bhaari calculations ke liye `lipi build --target node` wala JavaScript output tez chalta hai. LiPi ka faster engine roadmap me hai.
- **Mobile/desktop apps:** LiPi ke roadmap me hain (Android, iOS, desktop).

**Iska matlab:** LiPi se shuru karo, jaldi seekho, asli apps banao, aur zaroorat pade to JavaScript ki duniya bhi tumhare haath me hai.

---

## Yaad rakho

| LiPi me | Kyun behtar hai seekhne ke liye |
|---|---|
| kam symbols | code padhna aasaan |
| `"5" + 3` pe error | chupke galat jawab nahi |
| `==` values compare | confusion nahi |
| `if` me sirf true/false | chhupi galtiyan nahi |
| exact Integers | paise ka hisaab sahi |
| error + hint | galti se seekhna |
| ek install me sab | setup me time barbaad nahi |
| JavaScript me compile | har browser me chalta hai |

> **Teacher tip:** Ye chapter pehli class me dikhane layak hai. Board pe `"5" + 3` likho, poochho jawab kya hoga. JavaScript ka "53" dikhao, phir LiPi ka saaf error. Students ko turant samajh aata hai LiPi kyun banaya gaya.
