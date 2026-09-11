# LiPi

**Unified Development Language** · *Ancient roots. Modern code.* · Easy to start. Hard to outgrow.

LiPi (लिपि, "script, writing system") is a beginner-friendly, general-purpose
language with a single readable syntax and one toolchain for scripts, CLIs,
backends, APIs and, later, web, desktop and mobile. This repository holds the
reference implementation: a Rust compiler front end (lexer, parser, name
resolution, gradual type checker, diagnostics), an interpreter with a standard
library and web server, and the `lipi` CLI.

```lipi
name = "Dezy"
age = 25

if age >= 18
    show "Adult"

add(a, b)
    return a + b

server.start 3000

get "/users/:id" with request
    return {id: request.params.id, name: name}
```

## Quick start

You need [Rust](https://rustup.rs) (stable). On Windows with the GNU toolchain,
also install a MinGW GCC (for example `winget install BrechtSanders.WinLibs.POSIX.UCRT`),
which the SQLite driver needs.

```sh
cargo build --release
./target/release/lipi examples/hello.lipi      # Hello, LiPi!
```

```sh
lipi hello.lipi            # run a file
lipi                       # interactive prompt
lipi new my-app            # lipi.json, src/main.lipi, tests/
cd my-app && lipi run      # run the project
lipi test                  # run test blocks in *_test.lipi files
lipi check src/main.lipi   # find mistakes without running (--json for editors/CI)
lipi doctor                # check your setup
```

## Errors that teach

```
ERROR LIP2001: cannot add String and Integer

main.lipi:2:9
    price = age + 10
            ^^^

Hint: "age" is a String. Convert it to a number or use a numeric value.
```

Every diagnostic has a stable code (see [SPEC §16](docs/SPEC.md#16-diagnostics)),
the exact location and a hint. LiPi also recognises habits from other
languages (`print`, `let`, `&&`, `elif`, `import`, `nil`, curly quotes) and
shows the LiPi way.

## The language in one screen

```lipi
count = 0                          # Integer
price = 19.99                      # Decimal (10 / 4 == 2.5; overflow is an error)
name: String = "Asha"              # optional type annotation
const appName = "LiPi"

show "Hello {name}, welcome to {appName}"   # interpolation; 'single quotes' are literal

items = [3, 1, 2]
items.push(4)
user = {name: "Dezy", tags: ["admin"]}
show items.sort(), items.map(x => x * 2), user.tags[0]

for i in 1 to 5
    show i
for key, value in user
    show "{key} = {value}"

greet(who = "friend")              # or: function greet(who = "friend")
    return "Hi, " + who + "!"
show greet(who: "Ravi")

label = "big" if count > 100 else "small"
city = user.address?.city ?? "unknown"
if not items.isEmpty()             # conditions must be true/false
    show items.first

type Point
    x: Integer
    y: Integer = 0
    length()
        return math.sqrt(self.x ** 2 + self.y ** 2)

match status
    "paid"
        show "Complete"
    "pending", "new"
        show "Waiting"
    else
        show "Unknown"

try
    config = json.parse(fs.read("config.json"))
catch error
    show error.code, error.message

async getUser(id)
    response = await http.get("https://api.example.com/users/{id}")
    return response.json()

use math                           # math.lipi next to this file (export add in it)
from "./lib/tax.lipi" use gst

db = database.open("shop.db")      # SQLite, or "postgres://user:pw@host/shop"; tables grow with your data
db.orders.create({item: "tea", qty: 2})
paid = db.orders.where(status: "paid", order: "-id")
db.orders.update(order.id, {status: "paid"})
```

The full reference is [docs/SPEC.md](docs/SPEC.md).

## Repository layout

```
compiler/   lexer, parser, AST, checker (name resolution + types), diagnostics
runtime/    interpreter, values, tasks, stdlib, HTTP client/server, crypto
cli/        the `lipi` command, REPL, golden + server integration tests
tests/      language/*.lipi with expected output (.out); server/ test app
examples/   hello, todo CLI, shapes, word count, GitHub API, JSON API server
docs/       language specification
```

## Development

```sh
cargo test                                      # unit, golden, CLI and server tests
LIPI_BLESS=1 cargo test -p lipi --test golden   # accept new golden output
LIPI_TEST_POSTGRES_URL=postgres://postgres@localhost:5432/postgres cargo test -p lipi --test postgres
```

## Status

| Release | Target | Status |
|---|---|---|
| 0.1 | core executable language | ✅ |
| 0.2 | usability, type system, modules, errors, JSON, files, HTTP client, async, tests, REPL | ✅ |
| 0.3 | application APIs: HTTP server, middleware, cookies, WebSockets, auth foundations (crypto), database layer on SQLite and PostgreSQL | ✅ |
| 0.5 | packages, registry, lipi.lock, formatter, linter, LSP, VS Code | planned |
| 0.8 | web platform: LiPi UI, JS target, JS interop | planned |
| 1.0 | stable language and ecosystem | planned |
| 1.x | WASM, native, desktop, Android, iOS | planned |

### Known limitations

- It's a tree-walking interpreter: fine for tools, APIs and learning, but not
  yet fast. An IR/bytecode VM is planned before the JS/WASM/native backends.
- The HTTP client uses the system `curl` for its transport (`lipi doctor`
  checks for it).
- Calling an `async` function runs it right away. Background I/O overlaps,
  but there's no cooperative scheduler yet.
- There's no permission model yet for `fs`, `process.run` or the network.
- The formatter, `lipi build` and the package manager are not implemented yet.
