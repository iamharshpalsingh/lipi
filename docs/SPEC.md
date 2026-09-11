# LiPi Language Specification — v1.0

> **LiPi** — Unified Development Language. *Ancient roots. Modern code.*
> Easy to start. Hard to outgrow.

This document records what the reference implementation in this repository
actually does. It follows the *LiPi v0.1 Complete Language Specification and
Implementation Plan*. Where that plan left a choice open, the choice made here
is listed in [§19 Decisions](#19-decisions-taken-where-the-plan-left-a-choice).
Every rule here is covered by the golden tests in `tests/language/`.

---

## 1. Source files

- UTF-8 text with the `.lipi` extension.
- **Newlines and indentation are significant.** A block is the set of lines
  indented deeper than the line that opens it. Use 4 spaces per level (a tab
  counts as 4; the future formatter will normalise tabs).
- `#` starts a comment that runs to the end of the line.
- Inside `( )`, `[ ]` and `{ }`, newlines and indentation are ignored.
- A line starting with `.` continues the previous one (method chains).
- There are no semicolons and no `{ }` blocks.

## 2. Names and keywords

**Identifiers** start with an ASCII letter or `_` and continue with ASCII
letters, digits or `_`: `name`, `user_name`, `_product`, `price2`. Unicode
letters are allowed in strings and comments but not in names (v0.1 rule).

**Naming conventions:** `camelCase` for variables and functions, `PascalCase`
for types, 4-space indentation. The standard library follows these conventions.

**Keywords:**

| Keyword | Purpose |
|---|---|
| `if` `else` | conditions |
| `for` `in` `while` `repeat` `break` `continue` | loops, membership |
| `function` | optional function-definition keyword |
| `return` | return a value |
| `const` | constant binding |
| `use` `from` `as` `export` | modules |
| `async` `await` | asynchronous code |
| `try` `catch` `finally` `throw` | errors |
| `match` | pattern matching |
| `and` `or` `not` | Boolean logic |
| `true` `false` `null` | literals |
| `show` | print a line |

**Reserved for future use** (can't be used as names): `state`, `route`,
`component`, `server` (`server` is also the built-in web server module).

**Contextual words**, special only in one position: `to` and `step` (ranges),
`with` (trailing blocks), `type` (type definitions), `test` (test blocks),
`self` (inside methods), and `_` (the match wildcard).

## 3. Values and types

| Type | Examples | Notes |
|---|---|---|
| `Integer` | `10`, `-5`, `1_000_000`, `0xff` | 64-bit. Overflow is an error (LIP5009) |
| `Decimal` | `10.5`, `1e3` | 64-bit floating point. Printed with a fraction: `5.0` |
| `String` | `"hi {name}"`, `'literal'`, `"""multi-line"""` | immutable UTF-8 |
| `Boolean` | `true`, `false` | |
| `Null` | `null` | "no value" |
| `Array` | `[1, 2, 3]` | ordered, zero-based, mutable, shared by reference |
| `Object` | `{name: "A", age: 25}` | String keys, insertion order, mutable |
| `Function` | `add`, `x => x * 2` | first-class |
| `Task` | `http.get(url)`, `sleep(100)` | result of async work (§11) |
| user types | `Point(1, 2)` | §9 |

`Number` is used in annotations to mean "Integer or Decimal". `Any` accepts
everything.

### 3.1 Numeric model

- Integer `+ - * % **` Integer → Integer (checked: overflow is an error).
  `**` with a negative exponent gives a Decimal.
- **`/` always produces a Decimal**: `10 / 4` is `2.5` and `10 / 2` is `5.0`.
  For whole-number division, use `math.floor(a / b)`, which returns an Integer.
- Any operation involving a Decimal produces a Decimal.
- `%` takes the sign of the divisor: `-7 % 3 == 2`.
- Dividing by zero (`/` or `%`) is an error (LIP5002), never `infinity`.
- Integers and Decimals compare by value: `5 == 5.0` is `true`.
- Array and String positions must be Integers.

### 3.2 Conditions are strict

`if`, `while`, `not`, `and`, `or`, inline `if … else`, `match` guards and
test callbacks (`filter`, `find`, `any`, `all`, `count`) require a real
`Boolean`. `if items` is an error (LIP2005) with a hint to write
`if not items.isEmpty()`. `and` and `or` short-circuit and return a Boolean.

### 3.3 Equality

`==` compares by content: `[1, 2] == [1, 2]` and `{a: 1} == {a: 1}`.
Different types are never equal (`1 == "1"` is `false`).

## 4. Strings

- **Double quotes interpolate:** `"Total: {price * qty}"`.
- **Single quotes are literal:** `'{"a": 1}'`, which is handy for JSON.
- **Triple quotes** (`"""` / `'''`) span lines. A newline right after the
  opening quotes is dropped.
- Escapes: `\n \t \r \\ \" \' \{ \} \0 \u{1F600}`.
- `+` joins two Strings. `"Age: " + 5` is an error: use interpolation or
  `toString`.

## 5. Variables and constants

```lipi
name = "Dezy"                 # create or update
age: Integer = 25             # with a declared type, checked on every assignment
const appName = "LiPi"        # can never be reassigned (LIP1003)
count += 1                    # also -=, *=, /=
```

**Scope:** variables belong to the function (or file) that creates them.
Blocks (`if`, `for`, …) don't create a new scope. Assigning to a name updates
the nearest existing variable in an enclosing function or file. Otherwise it
creates a local variable in the current function; it never creates a hidden
global. Loop variables, parameters and `catch` names are local. Closures
capture variables by reference.

**Evaluation order** is left to right: the receiver, then the arguments in
order, then the call.

## 6. Operators

From lowest to highest precedence:

| # | Operators | Notes |
|---|---|---|
| 1 | `a if cond else b`, `x => …` | inline choice, lambda |
| 2 | `??` | `a ?? b` is `a` unless `a` is `null` |
| 3 | `or` | Booleans only |
| 4 | `and` | Booleans only |
| 5 | `not` | |
| 6 | `== != < > <= >= in` `not in` | comparisons don't chain: use `and` |
| 7 | `a to b`, `a to b step s` | inclusive Integer range |
| 8 | `+ -` | |
| 9 | `* / %` | |
| 10 | unary `-`, `await` | |
| 11 | `**` | right-associative |
| 12 | `f(x)` `a.b` `a?.b` `a[i]` | call, field, optional field, index |

`a?.b` gives `null` instead of an error when `a` is `null`, or when `a` is a
field missing from a plain Object.

## 7. Control flow

```lipi
if age >= 18
    show "Adult"
else if age >= 13
    show "Teen"
else
    show "Child"

while count < 10
    count += 1

repeat 3
    show "hip hip"

for item in items            # Array items
for i, item in items         # position and item
for letter in "hello"        # characters
for key, value in user       # Object keys and values
for n in 1 to 10             # 1 … 10
for n in 10 to 0 step -2     # 10, 8, … 0

match status
    "paid"
        show "Complete"
    "pending", "new"         # several patterns
        show "Waiting"
    1 to 9                   # Integer range pattern
        show "small"
    _ if verbose             # `_` matches anything; `if` adds a guard
        show "verbose"
    else
        show "Unknown"
```

`match` compares with `==` and runs the first case that matches.

## 8. Functions

```lipi
add(a, b)                        # compact definition
    return a + b

function sub(a, b)               # the same, with the optional keyword
    return a - b

greet(name = "friend", greeting: String = "Hello") -> String
    return "{greeting}, {name}!"

greet(greeting: "Namaste", name: "Ravi")   # named arguments
```

- `name(params)` followed by an indented block is a definition. On its own
  line, it's a call.
- Functions are hoisted within their file or function.
- Without `return`, a function returns `null`.
- Too many or too few arguments, unknown named arguments and wrong declared
  types are errors, reported before the program runs when the checker can see
  them.
- **Lambdas:** `x => x * 2`, `(a, b) => a + b`. Callbacks receive only the
  arguments they declare, so `items.map(x => …)` and `items.map((x, i) => …)`
  both work.
- **Calls without parentheses** at the start of a line: `server.start 3000`,
  `checkout payment`.
- **Trailing blocks:** a call followed by an indented block passes the block as
  a function in its last argument. `with` names the block's inputs:

  ```lipi
  get "/users/:id" with request
      return {id: request.params.id}

  payment.on "success"
      show "paid"
  ```
- Recursion is limited to 5000 nested calls (LIP5005).

## 9. Types

```lipi
type User
    name: String
    age: Integer = 0
    email: String?

    greet()
        return "Hi, I'm " + self.name

u = User("Dezy", 25)
v = User(name: "Asha")
```

A field is required unless it has a default or a nullable type (`T?`). Methods
see the instance as `self`. Setting an undeclared field is an error.
Inheritance, generics and traits aren't in v0.1.

## 10. Errors

```lipi
try
    user = database.users.find(id: 123)
catch error
    show error.message
finally
    show "done"

throw "something went wrong"
throw {message: "not found", status: 404}
```

A runtime error is a structured Object with these fields: `message`, `code`
(for example `"LIP5001"`), `category` (for example `"Runtime"`), `hint`,
`line` and `file`. If you throw an Object, `catch` receives that Object. An
unhandled error stops the program and prints a diagnostic (§16) and the call
chain.

## 11. Async / await

```lipi
async getUser(id)
    response = await http.get("https://api.example.com/users/{id}")
    return response.json()

user = await getUser(123)
```

- `await` is allowed at the top level of a file, inside `async` functions,
  and inside lambdas and trailing blocks (which take on their surroundings'
  context). Anywhere else it's error LIP4001, *await used outside async context*.
- Slow operations (`http.*`, `sleep`) return a Task right away and run in the
  background. `await task` waits for it and rethrows its errors.
- `await all([t1, t2])` waits for several tasks.
  `await timeout(task, ms)` fails with LIP4002.
  `task.cancel()` and `task.isDone()` also work.
- *Implementation note:* calling an `async` function runs its body right away
  and returns a finished Task. I/O inside it still runs in the background. A
  cooperative scheduler is planned.

## 12. Modules

```lipi
# math.lipi
add(a, b)
    return a + b
export add

# main.lipi
use math                         # binds `math`
show math.add(10, 20)

use "./lib/helpers.lipi" as h    # an explicit relative path
from math use add                # bind names directly
```

- **Members are private by default.** `export name, …` makes them public, and
  so does `export` in front of a definition (`export add(a, b)`,
  `export const limit = 3`, `export type User`). `export` is only allowed at
  the top level of a file.
- **Resolution** is deterministic. A path (anything with `/`, starting with
  `.` or ending in `.lipi`) is relative to the file that uses it. A bare name
  such as `use math` or `use utils.strings` is looked up in this order:
  1. `math.lipi` next to the current file,
  2. `src/math.lipi` in the project (the folder with `lipi.json`),
  3. the package `lipi_modules/math/`,
  4. the standard module `math`.

  A name that matches both a project file and a package is error LIP3004.
- Each module runs once, and circular `use` is error LIP3002. Using a name a
  module doesn't export is error LIP3003.
- The standard modules (`math json fs env http time process server crypto`)
  are always available with no `use`.

## 13. Gradual type system

```lipi
age = 25                        # inferred: Integer
name: String = "Dezy"           # annotated
scores: Array[Integer] = [90]   # also [Integer]
nickname: String? = null        # nullable
total(prices: Array) -> Decimal
```

Type names: `Integer Decimal Number String Boolean Null Array Object Function
Task Any`, user types, `Array[T]`/`[T]` and `T?`. Lowercase names are an error
with a hint (`type names are capitalized: "String"`).

**Before running**, `lipi run` and `lipi check` report every error they can
prove: undefined names, type mismatches in operators and assignments,
non-Boolean conditions, argument count and type errors for known functions,
constant reassignment, duplicate definitions, misplaced
`return`/`break`/`continue`/`await`/`export`, unknown members of Strings,
Arrays and numbers, and unknown type names. When it can't be sure, the checker
leaves the check to the runtime. Declared types are always enforced at runtime
as well.

## 14. Testing

Files ending in `_test.lipi` can contain:

```lipi
test "adds numbers"
    assertEqual(add(2, 3), 5)
    assert(add(1, 1) > 1, "should be bigger")
```

`lipi test [path]` runs them.

## 15. Standard library

**Global functions:** `toNumber toInteger toDecimal toString typeOf input
assert assertEqual sleep all timeout`. The route functions `get post put patch
delete` are also global.

| Module | Members |
|---|---|
| `math` | `pi e infinity sqrt abs floor ceil round(x, digits) pow log(x, base) log10 exp sin cos tan atan2 sign clamp min max random randomInt(min, max)` |
| `json` | `parse(text)`, `stringify(value, pretty: true)` |
| `fs` | `read write append exists isDir list makeDir delete` |
| `env` | `get(name, default) has all load(".env")` |
| `http` | `get(url, options) delete(url, options) post/put/patch(url, body, options) request({...})`. Options: `headers timeout query`. A response has `status ok headers body url json()` |
| `time` | `now()` (Integer ms since 1970), `date(ms)`, `iso(ms)` |
| `process` | `args platform exit(code) cwd() run(command)` |
| `server` | see §15.1 |
| `crypto` | `sha256 hmacSha256 hashPassword verifyPassword randomToken(bytes) uuid` |
| `database` | see §15.2 |

**String:** `length upper lower trim trimStart trimEnd split contains
startsWith endsWith replace indexOf slice repeat chars lines isEmpty padStart
padEnd reverse toNumber`

**Array** (the first five change the Array): `push pop insert removeAt remove`,
plus `length first last contains indexOf join map filter reduce each find any
all count sort sortBy reverse slice sum min max isEmpty copy unique flat`.
`items[-1]` is the last item.

**Object:** `keys values entries has get(key, default) remove copy isEmpty
length`. `obj.field` is an error when the field is missing; `obj["key"]` and
`obj.get("key")` return `null` instead.

**Integer/Decimal:** `round(digits) floor ceil abs toString`

### 15.1 Web server

```lipi
server.start 3000                     # listens on 127.0.0.1; host: "0.0.0.0" to expose

server.before with request            # middleware: return a response to stop early
    if request.headers.get("authorization") == null
        return server.respond(401, {error: "log in first"})

get "/users/:id" with request         # :param segments, *rest wildcard
    return {id: request.params.id}    # Objects and Arrays → JSON

post "/users" with request
    return server.respond(201, request.json())

server.static "/assets", "./public"
server.websocket "/chat" with socket
    socket.on "message" with text
        server.broadcast("/chat", text)
```

- Requests are served after the rest of the file has run.
- A request has `method path params query headers cookies form body ip json()`.
- Return values: an Object or Array is sent as JSON, a String as text (as HTML
  if it starts with `<`), `null` as `204 No Content`, and
  `server.respond(status, body, headers)` for full control.
  `server.redirect(url)` also works.
- `server.cookie(name, value, {maxAge, secure, httpOnly})` builds a cookie with
  secure defaults (`HttpOnly; SameSite=Lax; Path=/`).
- A handler error is logged and answered with `500 {"error": message}`. The
  server keeps running.
- Handlers run one at a time on the main thread; connection I/O runs on
  background threads.

### 15.2 Database (SQLite and PostgreSQL)

```lipi
db = database.open("shop.db")              # SQLite file; ":memory:" for a throwaway database
db = database.open("postgres://user:password@host:5432/shop")   # PostgreSQL (TLS when the server offers it)
users = database.users.all()               # the default database: DATABASE_URL, else lipi.db in the project

user = db.users.create({name: "Dezy", age: 25, tags: ["admin"]})   # returns the row, with its id
db.users.find(123)                         # by id, or null
db.users.find(email: "a@b.c")              # by fields
db.users.where(active: true, order: "-age", limit: 10, offset: 20)
db.users.all(order: "name")
db.users.count(active: true)
db.orders.update(order.id, {status: "paid"})   # returns the updated row
db.users.delete(3)                         # true if a row was deleted

db.query("SELECT * FROM users WHERE age > ?", [18])       # rows as Objects
db.run("UPDATE users SET age = age + 1 WHERE id = :id", {id: 1})   # {changes, lastId}
db.transaction(tx => ...)                  # commits, or rolls back when the block fails
db.migrate("001_create_products", "CREATE TABLE products (...)")   # runs once
db.tables()
```

- **The same LiPi code runs on both databases.** `?` and `:name`
  placeholders in `query`/`run` are rewritten to `$1, $2, …` for PostgreSQL.
- **Values are always parameters**, never pasted into SQL. Table and column
  names must be plain identifiers.
- PostgreSQL types map to LiPi values: integer types become Integer;
  real/double/NUMERIC become Decimal (whole NUMERICs become Integer); BOOLEAN
  becomes Boolean; JSON/JSONB become values; TIMESTAMP(TZ)/DATE become ISO
  Strings (you can also store `time.now()` Integers into timestamp columns);
  UUID becomes a String.
- **Schema grows with your data:** the first `create` makes the table (`id`
  is an auto-incrementing primary key), and new fields in `create`/`update`
  add columns. For production schemas, use `migrate`.
- Auto-created column types: Integer→INTEGER (BIGINT on PostgreSQL),
  Decimal→REAL (DOUBLE PRECISION), String→TEXT, Boolean→BOOLEAN,
  Array/Object→JSON (JSONB). All of them read back as the original LiPi values.
- Reading a table that doesn't exist yet gives `[]`, `0` or `null`.
- Database errors are LIP5011 and carry hints (missing table, unique
  violation, SQL syntax).

### 15.3 Web UI (LiPi UI)

Web apps are written in LiPi and compiled with `lipi build`:

```lipi
state cart = []                     # app-wide state

component ProductCard(product)
    card
        heading product.name
        text "₹{product.price}"
        button "Add to cart"        # the block runs on click
            cart.push(product)

component Quantity(label)
    state count = 1                 # belongs to this Quantity, kept between draws
    row
        text "{label}: {count}"
        button "+"
            count += 1

page "/"
    heading "Shop", level: 1
    for product in products
        ProductCard(product)
    link "Checkout", to: "/checkout"

page "/orders/:id" with url         # url.path, url.params.id, url.query
    heading "Order {url.params.id}"
```

- **Pages:** `page "/path"` + block, declared at the top level. Paths can
  have `:name` parts and a final `*` (in `url.params.rest`); the block's
  optional input (`with url`) holds `path`, `params` and `query`. Addresses use
  the `#/path` form, so a build works when opened straight from disk.
  `navigate("/path")` changes page from code.
- **Drawing:** a page's block runs from the top on every redraw. Each element
  call adds to the element being drawn, so `if`, `for` and function calls
  work as usual. Components are functions that draw; calling them outside
  a page is error LIP6002.
- **Redraws** happen after every event handler (again when an async handler
  finishes), when a `state` variable is assigned, and when an Array or Object
  is changed in place (`cart.push(x)`). Only what changed is updated in the
  page, so a text field keeps its cursor while you type.
- **State:** `state x = value` at the top level is app-wide. Inside a
  component it belongs to that component instance: it's created on the
  first draw and kept while the component stays in the same place. Give components in a
  list a `key:` (`Row(item, key: item.id)`) so each one's state follows its
  item when the list is filtered or reordered; two components with the same
  key are an error. Components
  can't use `await`; load data in top-level code or a button's block and
  keep it in state.
- **Elements:**

  | Element | Meaning |
  |---|---|
  | `card`, `row`, `column`, `section` + block | containers (the block draws the contents) |
  | `heading value, level: 2` | heading, levels 1–6 |
  | `text value, …` | paragraph |
  | `button label, disabled: false` + block | the block runs on click |
  | `field value, placeholder: "…", type: "text"` `with value` + block | text box; the block gets the new text |
  | `checkbox checked, label` `with checked` + block | the block gets true or false |
  | `link label, to: "/page"` | app page, or an `https:`/`http:`/`mailto:`/`tel:` address (opens in a new tab) |
  | `image source, alt: "…"` | image |
  | `element "tag", …` + block | any other element except script/style/embeds (LIP6003) |

  Every element also takes `class:`, `id:`, `style:` and `title:`, and
  four style options for things an inline style can't do:
  `hover:` (while the pointer is over it), `focus:` (when it's focused with
  the keyboard), `mobile:` (screens up to 720 px wide) and `desktop:`
  (wider screens). Each takes declarations like `style:`, for example
  `button "Save", hover: "background: #B83A22;", mobile: "width: 100%;"`.
  They take priority over `style:`. `mobile: "display: none;"` and
  `desktop: "display: none;"` show different things on phones and on
  computers. Text is
  always inserted as text, never as HTML, and `javascript:` addresses are
  refused (LIP6003). LiPi's built-in stylesheet gives every element a
  default look.
- `lipi run` on a UI program stops at the first element with LIP6002 and
  explains how to build it. Node builds refuse UI elements (LIP3006).

### 15.4 JavaScript interop

The `js` module is the explicit boundary to JavaScript, in web and Node builds
(`lipi run` stops with LIP3007):

```lipi
chart = await js.import("https://cdn.jsdelivr.net/npm/canvas-confetti@1/+esm")
chart.default(particleCount: 120)          # named arguments become one options object

title = js.global.document.title           # browser APIs through js.global
now = js.new(js.global.Date)
data = js.value(js.global.JSON.parse(text)) # plain JS data as LiPi values
```

| JavaScript | LiPi |
|---|---|
| whole number that fits exactly / other number | Integer / Decimal |
| string, boolean, null/undefined | String, Boolean, null |
| array | a new Array (a copy) |
| function | function (and a LiPi function passed to JavaScript becomes a JS function) |
| promise | Task (`await` it) |
| any other object | `JsObject`: `.field`, `obj["key"]`, `.method(...)` and assignment reach the JS object; a missing field is null |

LiPi values passed to JavaScript become numbers, strings, arrays and plain
objects. A JavaScript exception becomes a LiPi error with code LIP5012, which
`try`/`catch` can handle. `js.import` takes a URL, a file served next to
`index.html` (put it in the project's `public/` folder), or, in Node builds, a
package name. `js.typeOf(x)` gives JavaScript's `typeof`.

## 16. Diagnostics

Every diagnostic has a stable code, a plain-language message, the exact
location and, where possible, a hint:

```
ERROR LIP1002: undefined variable "usr"

main.lipi:8:10
    show usr.name
         ^^^

Hint: did you mean "user"?
```

`lipi check --json` prints diagnostics as JSON for editors and CI.

| Range | Class | Codes |
|---|---|---|
| LIP0xxx | Syntax | 0001 unexpected token · 0002 indentation · 0003 string · 0004 invalid character · 0005 missing block · 0006 invalid assignment target · 0007 number literal · 0008 habit from another language |
| LIP1xxx | Name | 1001 duplicate definition · 1002 undefined variable · 1003 constant reassigned · 1004 unknown member · 1005 reserved word · 1006 misplaced return/break/continue · 1007 unknown parameter |
| LIP2xxx | Type | 2000 other type error · 2001 operator type mismatch · 2002 declared type mismatch · 2003 argument count · 2004 argument type · 2005 condition not Boolean · 2006 unknown type · 2007 return type · 2008 not callable |
| LIP3xxx | Module | 3001 module not found · 3002 circular use · 3003 not exported · 3004 ambiguous module · 3005 invalid export · 3006 not available in JavaScript builds yet · 3007 only available in JavaScript builds |
| LIP4xxx | Async | 4001 await outside async context · 4002 timed out · 4003 cancelled · 4004 background task failed |
| LIP5xxx | Runtime | 5000 general · 5001 index out of range · 5002 division by zero · 5003 null access · 5004 missing field · 5005 recursion limit · 5006 thrown by the program · 5007 file/IO · 5008 invalid argument · 5009 Integer overflow · 5010 assertion failed · 5011 database · 5012 JavaScript error |
| LIP6xxx | Security and platform | 6001 server-only code in a browser build · 6002 UI outside a browser page · 6003 unsafe link, address or element |
| LIP7xxx | Package | reserved |

Lint warnings (from `lipi lint`) use LIP9xxx: 9001 unused variable ·
9002 unused import · 9003 shadowing · 9004 unreachable code. Package
manager errors use LIP7xxx: 7001 checksum mismatch · 7002 package not found ·
7003 no matching version · 7004 version conflict · 7005 invalid manifest ·
7006 download/IO failure · 7007 version already published.

## 17. CLI and tooling

`lipi <file>`, `lipi run [file]`, `lipi check [file] [--json]`,
`lipi test [path]`, `lipi new <name>` (creates `lipi.json`, `src/main.lipi`,
`tests/`), `lipi repl` (or plain `lipi`), `lipi doctor`, `lipi --version`.
`deploy setup` are reserved and report which release adds them.

**Development server:** `lipi dev [file] [--port 3000]` builds the web app,
serves it on `http://localhost:3000/`, and watches the project's `.lipi` files
and `public/` folder. Every save rebuilds the app and reloads the page. A
build error is printed in the terminal and shown on the page until it's fixed.
Files in `public/` are served as they are, and `lipi build` copies them into
the output folder.

**JavaScript builds:** `lipi build [file] [--target web|node] [--out dist]`
compiles a program and every file it uses into one JavaScript bundle.

- `--target web` (the default) writes `dist/index.html` and `dist/app.js`.
  `show` prints to the page and the browser console; an uncaught error is
  shown on the page in the usual format.
- `--target node` writes `dist/app.cjs`, which runs with `node dist/app.cjs`
  and also has `fs`, `env` and `process`.
- The output keeps LiPi's semantics: Integers are exact over the full 64-bit
  range, with the same overflow error. `/` gives a Decimal, conditions must be
  Booleans, and `==` compares contents. Errors have the same codes, messages,
  hints, source excerpts and call traces as `lipi run`. The test suite runs
  every golden program both ways and requires identical output.
- **Security boundary:** browser builds refuse server-only modules and
  functions (`fs`, `env`, `process`, `database`, `server`, routes, `input`,
  and password hashing) with LIP6001. File access, secrets and database
  credentials therefore can't end up in code sent to the browser. Node builds
  report features that aren't supported yet (`database`, `server`) with LIP3006.
- Known differences: an `await` inside a callback makes that callback return a
  task (so `items.map(x => await f(x))` gives tasks, like `all` expects).
  After an `await`, stack traces only show the calls made since it resumed.

**Formatter:** `lipi format [paths] [--check]` rewrites files in the one
canonical style: 4-space indentation, single spaces around operators, one
space after `,` and `:`, no padding inside brackets, blank-line runs
collapsed to one, two spaces before inline comments, and a final newline.
Strings and comments are kept exactly. Formatting is idempotent, and the
formatter refuses to write a file if its tokens would change.

**Linter:** `lipi lint [paths] [--strict]` reports every `lipi check` error
plus warnings: unused variables and imports, parameters or loop variables that
shadow an outer variable, and code after `return`/`throw`/`break`/`continue`.
`--strict` makes warnings fail the command (for CI).

**Packages:** dependencies go in `lipi.json`:

```json
{
  "name": "my-store",
  "version": "1.0.0",
  "main": "src/main.lipi",
  "registry": "file:///C:/lipi-registry",
  "dependencies": {
    "utils": "path:../utils",
    "colors": "git:https://github.com/me/lipi-colors#v1.0.0",
    "slug": "^1.2.0"
  }
}
```

- `lipi install` installs everything into `lipi_modules/` and writes
  `lipi.lock` (exact version, source and SHA-256 integrity of every package).
  With an existing lock, the same versions are installed again and any content
  change is refused (LIP7001). `lipi install --frozen` fails if the lock is
  incomplete (for CI).
- `lipi install ../utils`, `lipi install git:URL#tag` and
  `lipi install slug@^1.2` add a dependency. Without a range, the newest
  version is used as `^X.Y.Z`.
- `lipi update [name…]` upgrades within the ranges. `lipi remove name`
  removes a package and anything only it needed.
- Resolution is deterministic, installing one version per package name.
  Incompatible requirements are error LIP7004.
- A package is used by name: `use slug`. Its entry file is `main` from its
  `lipi.json`, else `main.lipi` or `src/main.lipi`.
- **Registries** are static: `<registry>/<name>/index.json` lists versions
  with their archive file, integrity and dependencies. `lipi publish` packs the
  project deterministically (`.tar.gz`) and adds it to a folder registry
  (`--registry file:///…`). Published versions can never be overwritten
  (LIP7007). Downloads are cached in `~/.lipi/cache` (or `$LIPI_HOME`) and
  re-verified on every use. The public LiPi Registry service isn't online yet.

**Language server:** `lipi lsp` speaks the Language Server Protocol over
stdio, so any LSP editor can use it. It provides:

- diagnostics on every change (`lipi check` errors plus `lipi lint` warnings)
- completion: keywords, built-ins, module members after `.`, names in the file
- hover docs for built-ins and user functions (their leading comments)
- go to definition, also across `use`/`from … use`
- find references, a document outline, and formatting

**Editor:** `editors/vscode` is the VS Code extension. It connects to
`lipi lsp` and adds syntax highlighting, indentation rules, comment toggling,
bracket matching, format-on-save and the LiPi file icon.

## 18. Grammar (EBNF, v1.0)

The grammar is frozen for LiPi 1.x (see [STABILITY.md](STABILITY.md)).
`tests/conformance` uses every form below.

```ebnf
(* Layout: a block is the lines indented 4 spaces deeper than the line that
   owns it (INDENT/DEDENT). A line that starts with "." continues the
   previous line. Comments start with # and run to the end of the line. *)
program        = { statement } ;
block          = NEWLINE INDENT statement { statement } DEDENT ;

statement      = show | const | typed_binding | state | assignment | if | while | for
               | repeat | function | component | return | break | continue | throw
               | try | match | use | from_use | export | type | test | call_statement ;

show           = "show" [ expression { "," expression } ] ;
const          = "const" IDENT [ ":" type ] "=" expression ;
typed_binding  = IDENT ":" type "=" expression ;
state          = "state" IDENT [ ":" type ] "=" expression ;
assignment     = lvalue ( "=" | "+=" | "-=" | "*=" | "/=" ) expression ;
lvalue         = IDENT | postfix "." name | postfix "[" expression "]" ;
if             = "if" expression block { "else" "if" expression block } [ "else" block ] ;
while          = "while" expression block ;
for            = "for" IDENT [ "," IDENT ] "in" expression block ;
repeat         = "repeat" expression block ;
function       = [ "async" ] [ "function" ] IDENT "(" [ params ] ")" [ "->" type ] block ;
component      = "component" IDENT "(" [ params ] ")" block ;
params         = param { "," param } ;
param          = IDENT [ ":" type ] [ "=" expression ] ;
return         = "return" [ expression ] ;
throw          = "throw" expression ;
try            = "try" block [ "catch" [ IDENT ] block ] [ "finally" block ] ;
                 (* at least one of catch and finally *)
match          = "match" expression NEWLINE INDENT { case } [ "else" block ] DEDENT ;
case           = expression { "," expression } [ "if" expression ] block ;
                 (* "_" matches anything; "a to b" matches a number in the range *)
use            = "use" module [ "as" IDENT ] ;
from_use       = "from" module "use" IDENT { "," IDENT } ;
module         = STRING | IDENT { "." name } ;
export         = "export" ( IDENT { "," IDENT } | function | component | const
               | typed_binding | state | assignment | type ) ;
type           = "type" IDENT NEWLINE INDENT { field | method } DEDENT ;
field          = IDENT [ ":" type ] [ "=" expression ] ;
method         = [ "async" ] [ "function" ] IDENT "(" [ params ] ")" [ "->" type ] block ;
test           = "test" STRING block ;
call_statement = expression [ command_args ] [ trailing_block ] ;
                 (* command arguments only follow a name or dotted name: get "/users" *)
command_args   = arg { "," arg } ;
trailing_block = [ "with" param { "," param } ] block ;
                 (* passed as the last argument, as a function *)

type           = ( IDENT [ "[" type "]" ] | "[" type "]" | "null" ) { "?" } ;

expression     = lambda | choice ;
lambda         = ( IDENT | "(" [ params ] ")" ) "=>" expression ;
choice         = coalesce [ "if" coalesce "else" expression ] ;
coalesce       = or { "??" or } ;
or             = and { "or" and } ;
and            = not { "and" not } ;
not            = "not" not | comparison ;
comparison     = range [ ( "==" | "!=" | "<" | "<=" | ">" | ">=" | "in" | "not" "in" ) range ] ;
range          = term [ "to" term [ "step" term ] ] ;
term           = factor { ( "+" | "-" ) factor } ;
factor         = unary { ( "*" | "/" | "%" ) unary } ;
unary          = ( "-" | "await" ) unary | power ;
power          = postfix [ "**" unary ] ;              (* right-associative; -2 ** 2 is -4 *)
postfix        = primary { "(" [ args ] ")" | "." name | "?." name | "[" expression "]" } ;
args           = arg { "," arg } ;
arg            = [ IDENT ":" ] expression ;          (* named arguments come last *)
primary        = INTEGER | DECIMAL | STRING | "true" | "false" | "null" | IDENT
               | "[" [ expression { "," expression } [ "," ] ] "]"
               | "{" [ entry { "," entry } [ "," ] ] "}"
               | "(" expression ")" ;
entry          = ( name | STRING | INTEGER ) ":" expression
               | IDENT ;                              (* {name} is short for {name: name} *)
name           = IDENT | KEYWORD ;                    (* keywords work after "." and as keys *)

INTEGER        = DIGIT { DIGIT | "_" } | "0x" HEX { HEX | "_" } ;
DECIMAL        = DIGITS "." DIGITS [ EXPONENT ] | DIGITS EXPONENT ;
STRING         = '"' { CHAR | ESCAPE | "{" expression "}" } '"'  (* interpolates *)
               | "'" { CHAR } "'" ;                  (* literal *)
IDENT          = ( LETTER | "_" ) { LETTER | DIGIT | "_" } ;   (* ASCII *)
```

## 19. Decisions taken where the plan left a choice

| Topic | Decision |
|---|---|
| Function syntax | Compact `add(a, b)` plus the optional `function` keyword |
| Range syntax | `1 to 10`, `10 to 0 step -2` (inclusive) |
| Division | `/` always gives a Decimal; overflow is an error |
| Truthiness | Strict Booleans everywhere (no compatibility mode) |
| Interpolation | In v0.1 already: double quotes interpolate, single quotes are literal |
| Constants | camelCase (`const appName`) |
| Object vs map | One Object type with String keys; `obj.x` is strict, `obj["x"]` is lenient |
| Module cycles | Error LIP3002 |
| Scope resolution | Decided before the program runs: an assignment updates the variable of that name in the nearest enclosing function or file that assigns it, else creates a local. `lipi run` and `lipi build` share this rule (`lipi_compiler::scope`) |
| Match syntax | Patterns directly (no `when`), `else`, `_`, guards with `if` |
| Extra keywords | `show`, `repeat`, `break`, `continue` (from the plan's examples and loop needs) |
| `type` blocks | A simple object model (fields and methods, no inheritance) |

## 20. Not yet implemented

The hosted LiPi Registry service, forms and validation helpers, source maps and minified production builds,
the debugger, regex and encoding modules, permission-aware I/O, and generics/traits.
