# Lipi Language Specification — Draft 0.2

> Easy to start. Hard to outgrow.

This is the working specification for Lipi 0.x. It describes what the
reference implementation (this repository) actually does. 0.x versions may
still change; the grammar freezes at 1.0 with a compatibility policy.

---

## 1. Source files

- Extension `.lipi`, UTF-8 text.
- **Indentation defines blocks.** Use 4 spaces per level (a tab counts as 4).
  A line that is more indented than the one before starts a block; returning
  to an earlier indentation ends it.
- Comments start with `#` and run to the end of the line.
- Inside `( )`, `[ ]` and `{ }`, newlines and indentation are ignored, so
  long lists, objects and calls can span several lines.
- A line starting with `.` continues the previous line (method chains):

  ```lipi
  names = people
      .filter(p => p.age >= 18)
      .map(p => p.name)
  ```

- No semicolons and no `{ }` blocks.

## 2. Names and keywords

Names (identifiers) start with a letter or `_` and contain letters, digits and
`_`. Any Unicode letter works, including Devanagari: `नाम = "Lipi"`.
Convention: `snake_case` for variables and functions, `PascalCase` for types,
`UPPER_CASE` for constants.

**Reserved keywords:**

```
if else for in while repeat break continue return
and or not true false nil const import async await
try catch finally throw match show
```

**Contextual words** (only special in one position, usable as names elsewhere):
`to`, `step` (ranges), `as`, `from` (imports), `when` (match), `type`
(type definitions), `test` (test blocks), `self` (inside methods), `_`
(match wildcard).

## 3. Values

| Type       | Examples                        | Notes |
|------------|---------------------------------|-------|
| `nil`      | `nil`                           | "no value" |
| `bool`     | `true`, `false`                 | |
| `number`   | `42`, `3.14`, `1_000_000`, `1e6`, `0xff` | 64-bit floating point. Whole numbers print without `.0` |
| `string`   | `"Hi {name}"`, `'literal'`, `"""multi-line"""` | Immutable Unicode text |
| `list`     | `[1, 2, 3]`                     | Ordered, mutable, shared by reference |
| `object`   | `{name: "Dezy", age: 25}`       | Keys keep their order. Mutable, shared by reference |
| `function` | `add`, `x => x * 2`             | First-class values |
| `task`     | result of `http.get(...)`, `sleep(100)`, an `async` call | See §11 |
| user types | `Point(1, 2)`                   | See §9 |

**Truthiness:** only `nil` and `false` are false. `0`, `""` and `[]` are true.
Check emptiness explicitly with `.is_empty()`.

**Equality** (`==`) compares by content: `[1, 2] == [1, 2]` is `true`.
Values of different types are never equal (`1 == "1"` is `false`). Nothing is
converted behind your back.

## 4. Strings

- **Double quotes interpolate:** `"Total: {price * qty}"`. Any expression can
  go inside `{ }`.
- **Single quotes are literal:** `'{"a": 1}'`, which is handy for JSON.
- **Triple quotes** (`"""` or `'''`) span lines. A newline right after the
  opening quotes is dropped.
- Escapes: `\n \t \r \\ \" \' \{ \} \0 \u{1F600}`.

## 5. Variables

```lipi
name = "Dezy"              # create or update
age: number = 25           # with a declared type (checked from then on)
const PI = 3.14159         # can never be reassigned
count += 1                 # also -=, *=, /=
```

**Scope rules:**

- Variables belong to the **function** (or file) that creates them. `if`,
  `for`, `while` and other blocks do not create a new scope.
- Assigning to a name **updates the nearest existing variable** in an
  enclosing function or file. Otherwise it **creates** a new variable in the
  current function. That is why closures can update captured variables:

  ```lipi
  make_counter()
      count = 0
      increment()
          count += 1
          return count
      return increment
  ```

- Loop variables, parameters and `catch` names are always local.
- Built-in names (`show`, `math`, ...) can be shadowed but never overwritten
  globally.

## 6. Operators

From lowest to highest precedence:

| Precedence | Operators | Notes |
|---|---|---|
| 1 | `a if cond else b`, `x => ...` | inline choice, lambda |
| 2 | `??` | `a ?? b` is `a` unless `a` is `nil` |
| 3 | `or` | returns the first true operand (or the last one) |
| 4 | `and` | returns the first false operand (or the last one) |
| 5 | `not` | |
| 6 | `== != < > <= >= in` `not in` | comparisons don't chain: write `0 < x and x < 10` |
| 7 | `a to b`, `a to b step s` | inclusive range |
| 8 | `+ -` | `+` also joins two strings or two lists |
| 9 | `* / %` | `%` has the sign of the divisor (`-7 % 3 == 2`) |
| 10 | unary `-`, `await` | |
| 11 | `**` | power, right-associative |
| 12 | `f(x)`, `a.b`, `a?.b`, `a[i]` | call, field, optional field, index |

- Arithmetic works on numbers only. `"a" + 1` is an error with a hint to use
  interpolation or `to_number`.
- Dividing by zero is an error, not `infinity`.
- `in` checks list membership, substrings, and object keys.
- `a?.b` gives `nil` instead of an error when `a` is `nil` (or when `a` is a
  field missing from a plain object).

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

for item in items            # list items
for i, item in items         # position and item
for letter in "hello"        # characters
for key in user              # object keys
for key, value in user       # object keys and values
for n in 1 to 10             # 1, 2, ... 10 (inclusive)
for n in 10 to 0 step -2     # 10, 8, ... 0

break / continue             # inside loops

match command
    when "start", "go"
        run()
    when 1 to 9              # numeric range
        show "small"
    when _ if verbose        # `if` after the patterns adds a guard
        show "verbose mode"
    when _                   # anything
        show "other"
    else
        show "no case matched"
```

`match` compares with `==` and runs the first matching `when`.

## 8. Functions

A function is defined by its name, its parameters and an indented body. There
is no keyword.

```lipi
add(a, b)
    return a + b

greet(name = "friend", greeting: string = "Hello") -> string
    return "{greeting}, {name}!"

greet()                                  # "Hello, friend!"
greet("Asha")                            # positional
greet(greeting: "Namaste", name: "Ravi") # named arguments
```

- **Definition or call?** `name(params)` followed by an indented block is a
  definition. On its own line it's a call.
- Functions are **hoisted**: you can call a function defined later in the same
  file or function.
- Without `return`, a function returns `nil`.
- Parameters may have defaults (evaluated at call time) and declared types.
  A return type is written as `-> type`.
- Calling with too many or too few arguments, or with an unknown named
  argument, is an error.
- **Lambdas:** `x => x * 2`, `(a, b) => a + b`, `() => 42`. Their body is a
  single expression.
- **Callbacks** passed to built-ins receive only the arguments they declare:
  `items.map(x => x * 2)` and `items.map((x, i) => x * i)` both work.
- Recursion is limited to 5000 nested calls, and running out reports the
  function that recursed.

## 9. Types

```lipi
type User
    name: string
    age: number = 0
    email: string? 

    greet()
        return "Hi, I'm {self.name}"

u = User("Dezy", 25)
v = User(name: "Asha")        # age defaults to 0, email to nil
show u.greet()
```

- Fields: `name: type`, `name = default` or `name: type = default`. A field
  with an optional type (`string?`) defaults to `nil`. Other fields without a
  default are required.
- Create an instance by calling the type with positional (field order) or
  named arguments.
- Methods see the instance as `self`.
- Setting an undeclared field, or a value of the wrong declared type, is an
  error.
- Generics, interfaces/traits and inheritance are future work (see the roadmap).

## 10. Errors

```lipi
try
    data = json.parse(text)
catch error
    show "Bad input: {error.message}"
finally
    show "done"

throw "something went wrong"          # error with this message
throw {message: "not found", code: 404}
```

- A caught error is an object with `message`, `hint`, `line` and `file`. If
  you throw an object, you catch that same object.
- An uncaught error stops the program and prints the message, the source line
  with a marker, a hint and the chain of calls that led there.

## 11. Async

```lipi
async load_user(id)
    response = await http.get("https://api.example.com/users/{id}")
    return response.json()

user = await load_user(7)
```

- Slow operations (`http.*`, `sleep`) return a **task** immediately and run in
  the background. `await task` waits for the result and rethrows any error.
- Start several tasks, then wait for all of them together:
  `results = await all([task1, task2])`.
- `await timeout(task, 5000)` fails with "timed out" after 5 seconds.
- `task.cancel()` cancels a task and `task.is_done()` checks it without waiting.
- `await` works at the top of a file and inside any function.
- *0.2 behaviour:* calling an `async` function runs its body right away and
  returns a finished task. Background I/O inside it still overlaps with other
  tasks. A full cooperative scheduler is planned.

## 12. Modules

```lipi
import "./math_utils.lipi"               # available as math_utils
import "./math_utils.lipi" as mu
from "./math_utils.lipi" import add, PI
import payments                          # package from lipi_modules/
```

- Paths are relative to the importing file, and `.lipi` is optional.
- A module exports every top-level name that doesn't start with `_`.
- Each module runs once. Later imports reuse the result, and circular imports
  are reported as errors.
- The standard modules (`math`, `json`, `fs`, `env`, `http`, `time`,
  `process`) are always available with no import.

## 13. Gradual typing

Types are optional. Add them where they help:

```lipi
age = 25                      # inferred
name: string = "Dezy"         # declared
scores: list[number] = [90, 85]
nickname: string? = nil       # optional: string or nil
total(prices: list) -> number
```

Type names: `number string bool nil any list object function task`, a user
type name, `list[T]` (or `[T]`) and `T?`.

**Before running**, `lipi run` and `lipi check` report mistakes that are
certain to fail:

- unknown names (with "did you mean")
- operations on the wrong types, such as `"twenty" + 10`
- a value that doesn't match a declared type
- wrong argument counts, unknown named arguments and argument types for known
  functions
- reassigning a constant
- `return` outside a function, `break` outside a loop
- unknown members of text, lists and numbers (`items.lenght`)
- unknown type names

The checker is conservative: when it can't be sure, it leaves the check to
runtime. Declared types are always enforced at runtime too.

## 14. Testing

Files ending in `_test.lipi` can contain test blocks:

```lipi
test "adds numbers"
    assert_equal(add(2, 3), 5)
    assert(add(1, 1) > 1, "should be bigger")
```

`lipi test` finds and runs them and reports passes and failures.

## 15. Standard library

**Global functions:** `to_number(x)` (returns `nil` if the text isn't a number),
`to_string(x)`, `type_of(x)`, `input(prompt)`, `assert(cond, message)`,
`assert_equal(actual, expected)`, `sleep(ms)`, `all(tasks)`,
`timeout(task, ms)`.

| Module | Members |
|---|---|
| `math` | `pi e infinity sqrt abs floor ceil round(x, digits) pow log(x, base) log10 exp sin cos tan atan2 sign clamp min max random random_int(min, max)` |
| `json` | `parse(text)`, `stringify(value, pretty: true)` |
| `fs` | `read write append exists is_dir list make_dir delete` |
| `env` | `get(name, default)`, `has(name)`, `all()`, `load(".env")` |
| `http` | `get(url, options)`, `delete(url, options)`, `post/put/patch(url, body, options)`, `request({...})`. Options: `headers`, `timeout` (ms), `query`. The response has `status ok headers body url json()` |
| `time` | `now()` (ms since 1970), `date(ms)`, `iso(ms)` (UTC) |
| `process` | `args`, `platform`, `exit(code)`, `cwd()`, `run(command)` → `{code, output, error}` |

**Text:** `length upper lower trim trim_start trim_end split(sep) contains
starts_with ends_with replace(old, new) index_of slice(start, end) repeat(n)
chars lines is_empty pad_start(width, fill) pad_end reverse to_number`

**Lists** (the first five change the list, the rest return new values):
`push pop insert(pos, item) remove_at(pos) remove(item)`, `length first last
contains index_of join(sep) map filter reduce(f, start) each find any all
count sort sort_by(f) reverse slice sum min max is_empty copy unique flat`.
Negative positions count from the end: `items[-1]` is the last item.

**Objects:** `keys values entries has(key) get(key, default) remove(key) copy
is_empty length`. `obj.field` is an error if the field is missing (this
catches typos). `obj["key"]` and `obj.get("key")` give `nil` instead.

**Numbers:** `round(digits) floor ceil abs to_string`

## 16. Error-message philosophy

Every error message:

1. says **what went wrong** in plain words ("expected a number", not "TypeError"),
2. shows the **exact line** with a marker under the problem,
3. offers a **safe suggestion** in a `Hint:` line,
4. recognises habits from other languages (`print`, `let`, `&&`, `elif`,
   `null`, `++`, `//` comments, `===`, curly quotes) and shows the Lipi way.

```
ERROR: expected a number
  --> main.lipi:2:9
   |
 2 | price = age + 10
   |         ^^^
Hint: "age" is a string. Convert it with to_number(age), or use a numeric value.
```

## 17. Not yet specified (planned)

Generics and interfaces/traits, pattern destructuring, a cooperative async
scheduler with cancellation tokens, a module visibility keyword, the
client/server boundary for web code (`page`, `server`), secrets handling, a
package manifest schema beyond `name`, `version`, `main` and `dependencies`,
and a formatter's exact layout rules.
