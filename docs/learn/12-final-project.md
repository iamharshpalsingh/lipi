# 12. Final project: Class Marks Manager

**Is chapter me banaoge:** ek poora web app, jisme ab tak seekhi har cheez aati hai: state, list of objects, functions, components, pages, navbar, styling, aur mobile support.

App kya karega:
- students aur unke marks add karna
- har student ka grade apne aap
- class ka average, topper, pass/fail count
- teen pages: Dashboard, Students, About
- phone aur computer dono pe achha dikhe

---

## Step 1: Data aur functions

Ek nayi file banao `marks.lipi`. Sabse pehle data (state) aur hisaab wale functions:

```lipi
state students = [
    {naam: "Asha", marks: 92},
    {naam: "Ravi", marks: 67},
    {naam: "Meera", marks: 85},
    {naam: "Kabir", marks: 28},
]
state newName = ""
state newMarks = ""

grade(marks)
    if marks >= 90
        return "A"
    else if marks >= 75
        return "B"
    else if marks >= 33
        return "C"
    return "F"

average()
    if students.isEmpty()
        return 0
    return (students.map(s => s.marks).sum() / students.length).round(1)

topper()
    return students.sortBy(s => s.marks).last

passCount()
    return students.filter(s => s.marks >= 33).length
```

**Samjho:** `sortBy(s => s.marks)` marks ke hisaab se sort karta hai, `.last` sabse zyada wala.

---

## Step 2: Styles

Saare styles ek jagah, naam ke saath:

```lipi
const barStyle = "background: #17120E; padding: 10px 16px; border-radius: 16px; margin-bottom: 24px; justify-content: space-between; flex-wrap: nowrap;"
const brandStyle = "color: white; font-weight: 800; font-size: 18px; text-decoration: none;"
const linkStyle = "color: #DDDDDD; text-decoration: none; padding: 6px 14px; border-radius: 999px;"
const activeStyle = "color: #17120E; background: white; text-decoration: none; padding: 6px 14px; border-radius: 999px; font-weight: 600;"
const statStyle = "flex: 1; min-width: 150px;"
const rowStyle = "background: white; border: 1px solid #EEE7DD; border-radius: 12px; padding: 10px 14px; justify-content: space-between; margin: 6px 0;"
```

---

## Step 3: Navbar

```lipi
state menuOpen = false

const barStyle = "background: #17120E; padding: 10px 16px; border-radius: 16px; margin-bottom: 24px; justify-content: space-between; flex-wrap: nowrap;"
const linkStyle = "color: #DDDDDD; text-decoration: none; padding: 6px 14px; border-radius: 999px;"
const activeStyle = "color: #17120E; background: white; text-decoration: none; padding: 6px 14px; border-radius: 999px; font-weight: 600;"

component NavLink(label, path, current)
    link label, to: path, style: activeStyle if current == path else linkStyle, hover: "" if current == path else "background: #333333;"

component Navbar(current)
    row style: barStyle
        link "Marks Manager", to: "/", style: "color: white; font-weight: 800; text-decoration: none;"
        row mobile: "display: none;"
            NavLink("Dashboard", "/", current)
            NavLink("Students", "/students", current)
            NavLink("About", "/about", current)
        button "✕" if menuOpen else "☰", desktop: "display: none;"
            menuOpen = not menuOpen
    if menuOpen
        column desktop: "display: none;"
            button "Dashboard"
                menuOpen = false
                navigate("/")
            button "Students"
                menuOpen = false
                navigate("/students")
            button "About"
                menuOpen = false
                navigate("/about")
```

Computer pe links, phone pe ☰ menu. (Chapter 10 me detail me samjha tha.)

---

## Step 4: Chhote components

```lipi fragment
component Stat(label, value)
    card style: "flex: 1; min-width: 150px;"
        text label
        heading "{value}", level: 2

component StudentRow(student, index)
    row style: "background: white; border: 1px solid #EEE7DD; border-radius: 12px; padding: 10px 14px; justify-content: space-between; margin: 6px 0;"
        text "{student.naam}: {student.marks}"
        row
            text "Grade {grade(student.marks)}"
            button "Remove"
                students.removeAt(index)
```

`StudentRow` ko `index` diya hai taaki "Remove" sahi student hataaye.

---

## Step 5: Pages

```lipi fragment
page "/" with url
    Navbar(url.path)
    heading "Dashboard", level: 1
    row mobile: "flex-direction: column; align-items: stretch;"
        Stat("Students", students.length)
        Stat("Average", average())
        Stat("Pass", passCount())
    if not students.isEmpty()
        text "Topper: {topper().naam} ({topper().marks} marks)"
    link "Sab students dekho", to: "/students"

page "/students" with url
    Navbar(url.path)
    heading "Students", level: 1
    row
        field newName, placeholder: "Naam" with value
            newName = value
        field newMarks, placeholder: "Marks", type: "number" with value
            newMarks = value
        button "Add", disabled: newName == "" or toNumber(newMarks) == null
            students.push({naam: newName, marks: toNumber(newMarks)})
            newName = ""
            newMarks = ""
    for i, s in students
        StudentRow(s, i)

page "/about" with url
    Navbar(url.path)
    heading "About", level: 1
    text "Ye app LiPi se bana hai: sirf LiPi, koi HTML/CSS/JavaScript file nahi."
```

---

## Step 6: Sab jodo aur chalao

Step 1 se 5 tak ka saara code ek file `marks.lipi` me (upar se neeche, isi order me) rakho. Styles aur navbar sirf ek baar likhne hain. Phir:

```
lipi check marks.lipi
lipi dev marks.lipi
```

Browser me **http://localhost:3000** kholo. Try karo:
1. Students page pe naya student add karo, Dashboard pe average badal jaayega.
2. Kisi ko Remove karo.
3. Browser chhota karo (ya Ctrl + Shift + M), navbar ☰ ban jaayega.

Online daalna ho to `lipi build marks.lipi` aur `dist` folder upload karo.

---

## Aage badhao (challenges)

1. **Search:** Students page pe field jo naam se filter kare.
2. **Sort buttons:** "Marks se sort" aur "Naam se sort".
3. **Edit:** marks badalne ka button.
4. **Subjects:** har student ke 3 subjects ke marks, aur total.
5. **Backend:** students ko server + database me save karo (Chapter 11), taaki refresh ke baad bhi rahein.

> **Teacher tip:** Final project ko 2-3 classes me baanto: pehli me Step 1-2, doosri me 3-4, teesri me 5-6 aur challenges. Sabse achha project class ke saamne present karwao.
