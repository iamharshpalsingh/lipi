# 11. Backend: server, API aur database

**Is chapter me seekhoge:** apna server banana, API (jo JSON data deti hai), URL se values lena, data receive karna, aur database me data save karna.

---

## Backend kya hai?

Website (frontend) wo hai jo user dekhta hai. **Backend** wo hai jo peeche data sambhalta hai: students ki list, login, orders... Frontend backend se data maangta hai, backend **JSON** me jawab deta hai.

LiPi me dono ek hi language me banate ho.

---

## Pehla server

`server.lipi`:

```lipi
server.start 3000

get "/"
    return {message: "Namaste from LiPi!", version: 1}
```

Chalao:

```
lipi server.lipi
```

Browser me **http://localhost:3000/** kholo. Dikhega:

```
{"message":"Namaste from LiPi!","version":1}
```

- `server.start 3000`: port 3000 pe server shuru.
- `get "/"`: jab koi `/` address maange, ye block chalo.
- Object `return` karo, LiPi use JSON bana deta hai.

Rokne ke liye terminal me **Ctrl + C**.

---

## URL se values lena

```lipi
server.start 3000

get "/hello/:naam" with request
    return {message: "Namaste, {request.params.naam}!"}

get "/square/:n" with request
    n = toNumber(request.params.n)
    if n == null
        return server.respond(400, {error: "number chahiye"})
    return {number: n, square: n * n}
```

- **http://localhost:3000/hello/Asha** → `{"message":"Namaste, Asha!"}`
- **http://localhost:3000/square/9** → `{"number":9,"square":81}`
- **http://localhost:3000/square/abc** → error 400

`:naam` URL ka badalne wala hissa hai, `request.params.naam` me milta hai. `server.respond(status, data)` status code ke saath jawab deta hai (400 = galat request, 404 = nahi mila, 201 = ban gaya).

---

## Data receive karna (POST)

`get` data **deta** hai, `post` data **leta** hai:

```lipi
server.start 3000

notes = []

get "/notes"
    return notes

post "/notes" with request
    data = request.json()
    if data.get("text", "") == ""
        return server.respond(400, {error: "text zaroori hai"})
    note = {id: notes.length + 1, text: data.text}
    notes.push(note)
    return server.respond(201, note)
```

Test karo (naye terminal me):

```
curl -X POST localhost:3000/notes -d "{\"text\":\"LiPi seekhna\"}"
curl localhost:3000/notes
```

`request.json()` bheja hua JSON object deta hai.

---

## Database: data hamesha ke liye

Upar wale `notes` server band hote hi mit jaate hain. **Database** me data file me save rehta hai. LiPi me SQLite built-in hai, kuch install nahi karna:

```lipi
db = database.open("school.db")
db.students.create({naam: "Asha", marks: 92})
db.students.create({naam: "Ravi", marks: 67})
show db.students.count()
for s in db.students.all()
    show s.id, s.naam, s.marks
```

```output
2
1 Asha 92
2 Ravi 67
```

- `database.open("school.db")` file wala database kholta hai (nahi hai to bana deta hai).
- `db.students` ek table hai. Pehli baar data daalte hi apne aap ban jaati hai.
- Har row ko apne aap ek `id` milta hai.

(Dobara chalaoge to data phir se jud jaayega, kyunki ab wo file me save hai. Shuru se karna ho to `school.db` file delete kar do.)

| Kaam | Code |
|---|---|
| jodna | `db.students.create({naam: "Asha", marks: 92})` |
| sab lena | `db.students.all()` |
| id se ek | `db.students.find(1)` |
| filter | `db.students.where(naam: "Ravi")` |
| badalna | `db.students.update(1, {marks: 95})` |
| hatana | `db.students.delete(1)` |
| ginti | `db.students.count()` |

PostgreSQL chahiye to bas `database.open("postgres://user:password@host/school")`. Baaki code same rehta hai.

---

## Poori API: students ka CRUD

CRUD = Create, Read, Update, Delete. Har asli app ka base:

```lipi
db = database.open("school.db")

server.start 3000

get "/students"
    return db.students.all()

get "/students/:id" with request
    student = db.students.find(toInteger(request.params.id))
    if student == null
        return server.respond(404, {error: "student nahi mila"})
    return student

post "/students" with request
    data = request.json()
    if data.get("naam", "") == ""
        return server.respond(400, {error: "naam zaroori hai"})
    return server.respond(201, db.students.create({naam: data.naam, marks: data.get("marks", 0)}))

put "/students/:id" with request
    id = toInteger(request.params.id)
    if db.students.find(id) == null
        return server.respond(404, {error: "student nahi mila"})
    db.students.update(id, request.json())
    return db.students.find(id)

delete "/students/:id" with request
    db.students.delete(toInteger(request.params.id))
    return {deleted: true}
```

Ab tumhare paas ek asli backend hai jise koi bhi website ya app use kar sakti hai.

---

## Password safe rakhna

Password kabhi seedha save mat karo. `crypto` module use karo:

```lipi
hash = crypto.hashPassword("mera-secret-123")
show crypto.verifyPassword("mera-secret-123", hash)
show crypto.verifyPassword("galat-password", hash)
```

```output
true
false
```

`hash` ko database me save karo, asli password ko nahi. Pura login example LiPi repository me `examples\api_server.lipi` me hai.

---

## Background kaam (time.every)

Har app me kuch kaam aisa hota hai jo kisi request se nahi, **ghadi** se chalta
hai: purane order chase karna, reminder bhejna, raat ko cleanup. Wo bas server
ke saath likh do:

```lipi fragment
server.start 3000

get "/orders"
    return db.orders.all()

# Har minute: jo order 24 ghante se pending hai, us par dhyan do
time.every 60000
    for o in db.orders.where(status: "pending")
        show "pending: {o.id}"
```

Timer ka block aur request handler dono ek hi thread pe, ek-ek kar ke chalte
hain — matlab ek dusre ko aadha-adhura kabhi nahi dekhte. Timer me error aaye
to wo timer ruk jaata hai, message dikhta hai, server chalta rehta hai.

---

## Khud karo

1. Ek server banao jisme `/time` address current time de (`time.iso()`).
2. `/add/:a/:b` banao jo dono numbers ka jod JSON me de.
3. Books ki API banao (title, author) database ke saath: list, add, delete.

<details>
<summary>Jawab dekho</summary>

```lipi
# 1 aur 2
server.start 3000

get "/time"
    return {now: time.iso()}

get "/add/:a/:b" with request
    a = toNumber(request.params.a)
    b = toNumber(request.params.b)
    if a == null or b == null
        return server.respond(400, {error: "dono numbers chahiye"})
    return {a: a, b: b, sum: a + b}
```

```lipi
# 3
db = database.open("library.db")

server.start 3000

get "/books"
    return db.books.all()

post "/books" with request
    data = request.json()
    return server.respond(201, db.books.create({title: data.title, author: data.author}))

delete "/books/:id" with request
    db.books.delete(toInteger(request.params.id))
    return {deleted: true}
```

</details>

---

## Yaad rakho

| Cheez | Example |
|---|---|
| server | `server.start 3000` |
| data dena | `get "/path"` + `return {...}` |
| URL value | `get "/user/:id" with request` → `request.params.id` |
| data lena | `post "/path" with request` → `request.json()` |
| status | `server.respond(404, {...})` |
| database | `db = database.open("app.db")` |
| table | `db.students.create/all/find/where/update/delete` |
| password | `crypto.hashPassword` / `verifyPassword` |

> **Teacher tip:** Browser me JSON dekhna students ko magic lagta hai. Pehle `get "/"` wala server banwao, phir har student apna `/about-me` route banaye jo apni info JSON me de.
