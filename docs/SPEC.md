# LiPi Language Specification — v0.1 baseline (implementation 0.3-dev)

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

### 15.2 Database (SQLite; PostgreSQL next)

```lipi
db = database.open("shop.db")              # ":memory:" for a throwaway database
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

- **Values are always parameters**, never pasted into SQL. Table and column
  names must be plain identifiers.
- **Schema grows with your data:** the first `create` makes the table (`id`
  is an auto-incrementing primary key), and new fields in `create`/`update`
  add columns. For production schemas, use `migrate`.
- Storage types: Integer→INTEGER, Decimal→REAL, String→TEXT, Boolean→BOOLEAN
  (read back as `true`/`false`), Array/Object→JSON (read back as values).
- Reading a table that doesn't exist yet gives `[]`, `0` or `null`.
- Database errors are LIP5011 and carry hints (missing table, unique
  violation, SQL syntax).

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
| LIP2xxx | Type | 2001 operator type mismatch · 2002 declared type mismatch · 2003 argument count · 2004 argument type · 2005 condition not Boolean · 2006 unknown type · 2007 return type · 2008 not callable |
| LIP3xxx | Module | 3001 module not found · 3002 circular use · 3003 not exported · 3004 ambiguous module · 3005 invalid export |
| LIP4xxx | Async | 4001 await outside async context · 4002 timed out · 4003 cancelled · 4004 background task failed |
| LIP5xxx | Runtime | 5000 general · 5001 index out of range · 5002 division by zero · 5003 null access · 5004 missing field · 5005 recursion limit · 5006 thrown by the program · 5007 file/IO · 5008 invalid argument · 5009 Integer overflow · 5010 assertion failed · 5011 database |
| LIP6xxx | Security | reserved |
| LIP7xxx | Package | reserved |

## 17. CLI

`lipi <file>`, `lipi run [file]`, `lipi check [file] [--json]`,
`lipi test [path]`, `lipi new <name>` (creates `lipi.json`, `src/main.lipi`,
`tests/`), `lipi repl` (or plain `lipi`), `lipi doctor`, `lipi --version`.
`build dev format lint install remove update publish deploy setup` are
reserved and report which release adds them.

## 18. Grammar (EBNF, v0.1 core)

```ebnf
program     = { statement } ;
statement   = show | binding | const | assignment | if | while | for | repeat
            | function | return | break | continue | throw | try | match
            | use | export | type | test | expression [ trailing_block ] ;
binding     = IDENT [ ":" type ] "=" expression ;
const       = "const" IDENT [ ":" type ] "=" expression ;
assignment  = lvalue ( "=" | "+=" | "-=" | "*=" | "/=" ) expression ;
if          = "if" expression block { "else" "if" expression block } [ "else" block ] ;
function    = [ "async" ] [ "function" ] IDENT "(" [ params ] ")" [ "->" type ] block ;
match       = "match" expression NEWLINE INDENT { patterns [ "if" expression ] block }
              [ "else" block ] DEDENT ;
use         = "use" module [ "as" IDENT ] | "from" module "use" IDENT { "," IDENT } ;
export      = "export" ( IDENT { "," IDENT } | function | const | binding | type ) ;
block       = NEWLINE INDENT { statement } DEDENT ;
trailing_block = [ "with" IDENT { "," IDENT } ] block ;
expression  = lambda | choice ;
choice      = coalesce [ "if" coalesce "else" expression ] ;
coalesce    = or { "??" or } ;
or          = and { "or" and } ;
and         = not { "and" not } ;
not         = "not" not | comparison ;
comparison  = range [ ( "==" | "!=" | "<" | "<=" | ">" | ">=" | "in" | "not" "in" ) range ] ;
range       = term [ "to" term [ "step" term ] ] ;
term        = factor { ( "+" | "-" ) factor } ;
factor      = unary { ( "*" | "/" | "%" ) unary } ;
unary       = ( "-" | "await" ) unary | power ;
power       = postfix [ "**" unary ] ;
postfix     = primary { "(" [ args ] ")" | "." IDENT | "?." IDENT | "[" expression "]" } ;
primary     = INTEGER | DECIMAL | STRING | "true" | "false" | "null" | IDENT
            | "[" [ expression { "," expression } ] "]"
            | "{" [ key ":" expression { "," key ":" expression } ] "}"
            | "(" expression ")" ;
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
| Match syntax | Patterns directly (no `when`), `else`, `_`, guards with `if` |
| Extra keywords | `show`, `repeat`, `break`, `continue` (from the plan's examples and loop needs) |
| `type` blocks | A simple object model (fields and methods, no inheritance) |

## 20. Not yet implemented

PostgreSQL driver (next in 0.3), formatter, linter, `lipi build` with the
JavaScript target, LSP, debugger, package manager and lockfile, regex and
encoding modules, UI components and `state`, client/server secret boundaries,
permission-aware I/O, and generics/traits.
