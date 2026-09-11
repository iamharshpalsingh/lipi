# 1. Shuruaat: pehla LiPi program

**Is chapter me seekhoge:** LiPi kya hai, pehla program kaise likhte aur chalate hain, `show`, comments, aur galti (error) kaise padhte hain.

---

## LiPi kya hai?

LiPi ek programming language hai. Programming language matlab computer se baat karne ka tareeka. Tum computer ko kuch kaam bataate ho (jaise "ye message dikhao", "ye hisaab lagao"), aur computer wo karta hai.

LiPi ko **Harsh Pal Singh** ([@iamharshpalsingh](https://github.com/iamharshpalsingh)) ne banaya hai. Naam "लिपि" (lipi) ka matlab hai "script, likhne ka tareeka".

LiPi ko **aasaan** banaya gaya hai: kam symbols, saaf words, aur jab galti ho to computer batata hai ki galti **kahan** hai aur **kaise theek** karein.

LiPi se tum bana sakte ho:
- terminal wale programs (calculator, to-do list, marks report)
- websites aur web apps (buttons, pages, navbar)
- backend / API (server, database)

---

## Step 1: Check karo LiPi installed hai

Terminal kholo (Windows key dabao, **PowerShell** likho, Enter) aur likho:

```
lipi --version
```

Kuch aisa dikhna chahiye: `lipi 1.1.0`. Agar "not recognized" aaye to LiPi dobara install karo (`install.cmd`).

---

## Step 2: Pehla program

Ek nayi file banao `hello.lipi` (Notepad ya VS Code me) aur usme likho:

```lipi
show "Namaste, LiPi!"
```

Save karo, aur terminal me usi folder me jaake chalao:

```
lipi hello.lipi
```

Output:

```output
Namaste, LiPi!
```

Badhai ho! Tumhara pehla program chal gaya.

**Samjho:** `show` ka matlab hai "screen pe dikhao". Double quotes `"..."` ke andar jo likha hai wo **text** hai, jaisa likha waisa hi dikhega.

---

## show se kai cheezein ek saath

`show` ke baad comma `,` laga ke kai cheezein ek line me dikha sakte ho. Beech me apne aap space aa jaata hai.

```lipi
show "Mera naam", "Asha"
show "2 + 3 =", 2 + 3
show "Pehli line"
show "Doosri line"
```

```output
Mera naam Asha
2 + 3 = 5
Pehli line
Doosri line
```

Dhyan do: `2 + 3` quotes ke bahar hai, isliye computer hisaab lagata hai aur `5` dikhata hai. `"2 + 3 ="` quotes ke andar hai, isliye wo text jaisa dikhta hai.

---

## Comments: apne liye notes

`#` ke baad jo bhi likho, computer use **ignore** karta hai. Ye insaanon ke liye note hota hai.

```lipi
# Ye program greeting dikhata hai
show "Suprabhat!"   # line ke end me bhi comment likh sakte ho
```

```output
Suprabhat!
```

Achhe programmer comments se samjhaate hain ki code **kyun** likha hai.

---

## Interactive prompt (turant try karo)

Terminal me sirf `lipi` likho aur Enter dabao. Ab tum ek-ek line likh ke turant result dekh sakte ho:

```
> 10 + 20
30
> "Hello".upper()
"HELLO"
> exit
```

Naye cheezein try karne ka ye sabse fast tareeka hai. Bahar aane ke liye `exit` likho.

---

## Project banana

Chhote programs ek file me chal jaate hain. Bade programs ke liye **project** banao:

```
lipi new mera-project
cd mera-project
lipi run
```

Project me `src\main.lipi` tumhara main code hai, aur `lipi run` use chalata hai. (Project ke baare me Chapter 9 me detail me padhenge.)

---

## Galti (error) hone par kya hota hai?

Galtiyan sabse hoti hain. LiPi batata hai **kya** galat hai, **kahan** hai, aur **kaise** theek karein. Maan lo `show` ki spelling galat ho gayi:

```lipi
shw "Namaste"
```

```output
ERROR LIP1002: undefined variable "shw"

main.lipi:1:1
    shw "Namaste"
    ^^^

Hint: did you mean "show"?
```

Error ko aise padho:
1. **Pehli line:** kya galat hai (`undefined variable "shw"` matlab "shw" naam ki koi cheez nahi hai). `LIP1002` error ka code hai.
2. **`main.lipi:1:1`:** file ka naam, line number 1, column 1.
3. **`^^^`:** exact jagah jahan galti hai.
4. **Hint:** theek kaise karein. Yahan: "kya tumhara matlab `show` tha?"

---

## Khud karo (Exercises)

1. Apna naam aur apne shehar ka naam ek line me dikhao.
2. `show` se 3 lines ki chhoti si kavita dikhao.
3. `125 + 375` ka jawab dikhao, aage likha ho `"Jawab:"`.
4. Jaan-boojh ke `show` ki spelling galat karo aur error padho. Hint kya bolta hai?

<details>
<summary>Jawab dekho</summary>

```lipi
# 1
show "Main Asha hoon,", "Pune se"

# 2
show "Chanda mama door ke,"
show "Puye pakaaye boor ke,"
show "Aap khaaye thaali me."

# 3
show "Jawab:", 125 + 375
```

4 ka jawab: Hint bolega `did you mean "show"?`

</details>

---

## Yaad rakho

| Cheez | Matlab |
|---|---|
| `show "text"` | screen pe dikhao |
| `show a, b` | kai cheezein ek line me (beech me space) |
| `# note` | comment, computer ignore karta hai |
| `lipi file.lipi` | file chalao |
| `lipi` | interactive prompt |
| `lipi new naam` | naya project |

> **Teacher tip:** Pehli class me sirf `show` aur comments karao. Students ko jaan-boojh ke galti karne do aur error ko zor se padhne ko bolo. Isse unka error se darr nikal jaata hai.
