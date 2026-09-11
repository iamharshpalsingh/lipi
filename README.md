# Lipi

**Easy to start. Hard to outgrow.**

Lipi (लिपि, "script, writing system") is a programming language with a
beginner-friendly surface and a professional ceiling. This repository holds
the reference implementation: a Rust compiler front end (lexer, parser,
gradual type checker), an interpreter with a standard library, and the `lipi`
command-line tool.

```lipi
name = "Dezy"
age = 25

if age >= 18
    show "Adult"

add(a, b)
    return a + b

user = await http.get("https://api.github.com/users/octocat")
show user.json().name
```

## Quick start

You need [Rust](https://rustup.rs) (stable). Then:

```sh
cargo build --release
./target/release/lipi examples/hello.lipi
```

Put `target/release` on your `PATH` (or copy `lipi`/`lipi.exe` somewhere that
is) so you can type `lipi` anywhere.

```sh
lipi hello.lipi          # run a file
lipi                     # interactive prompt
lipi new my-app          # create a project (lipi.json, main.lipi, tests/)
cd my-app && lipi run    # run the project's main file
lipi test                # run test blocks in *_test.lipi files
lipi check main.lipi     # find mistakes without running
lipi doctor              # check your setup
```

## What it looks like when things go wrong

Lipi explains problems in plain language and points at the exact spot:

```
ERROR: expected a number
  --> main.lipi:2:9
   |
 2 | price = age + 10
   |         ^^^
Hint: "age" is a string. Convert it with to_number(age), or use a numeric value.
```

It also recognises habits from other languages:

```
ERROR: I don't know what `print` is
  --> hello.lipi:1:1
   |
 1 | print("hello")
   | ^^^^^
Hint: To print something in Lipi, write: show "Hello"
```

## The language in one screen

```lipi
# Variables, optional types, constants
count = 0
name: string = "Asha"
const LIMIT = 10

# Text interpolation ("double") and literal text ('single')
show "Hello {name}, you have {count + 1} message(s)"

# Lists and objects
items = [3, 1, 2]
items.push(4)
user = {name: "Dezy", tags: ["admin"]}
show user.tags[0], items.sort(), items.map(x => x * 2)

# Loops
for i in 1 to 5
    show i
for key, value in user
    show "{key} = {value}"

# Functions: no keyword, defaults, named arguments
greet(who = "friend")
    return "Hi, {who}!"
show greet(who: "Ravi")

# Inline choice and nil handling
label = "big" if count > 100 else "small"
city = user.address?.city ?? "unknown"

# Types with methods
type Point
    x: number
    y: number = 0
    length()
        return math.sqrt(self.x ** 2 + self.y ** 2)
show Point(3, 4).length()

# Errors
try
    data = json.parse(fs.read("config.json"))
catch error
    show "Couldn't load config: {error.message}"

# Async: run requests at the same time
pages = await all([http.get(url1), http.get(url2)])

# Pattern matching
match status
    when 200 to 299
        show "ok"
    when 404
        show "not found"
    else
        show "error"
```

The full reference is in [docs/SPEC.md](docs/SPEC.md).

## Repository layout

```
compiler/   lexer, parser, AST, semantic checker, diagnostics  (crate lipi_compiler)
runtime/    interpreter, values, tasks, standard library      (crate lipi_runtime)
cli/        the `lipi` command, REPL, golden tests             (crate lipi)
tests/language/   .lipi programs with expected output (.out)
examples/   runnable example programs
docs/       language specification
```

## Development

```sh
cargo test                                    # unit, golden and CLI tests
LIPI_BLESS=1 cargo test -p lipi --test golden # accept new golden output
```

Each `tests/language/*.lipi` program is run by the real binary and compared
with its `.out` file. Error messages are tested the same way.

## Status

Lipi is at **0.2**. The roadmap from the implementation plan:

| Version | Scope | Status |
|---|---|---|
| 0.1 | variables, strings, numbers, booleans, lists, objects, if/else, loops, functions, interpreter | ✅ done |
| 0.2 | modules, errors, JSON, files, HTTP, async/await, gradual types, tests, REPL, `lipi new/run/check/test` | ✅ done |
| 0.3 | HTTP server (`server`, routes), SQLite/PostgreSQL, WebSockets, authentication | next |
| 0.5 | package manager and registry, formatter, linter, VS Code extension, LSP | planned |
| 0.8 | JavaScript target, WASM exploration, browser APIs, JS interop | planned |
| 1.0 | stable syntax, runtime, stdlib, debugger, production builds | planned |
| later | native backend, Android, iOS, desktop | planned |

### Known limitations in 0.2

- Execution uses a tree-walking interpreter: fine for scripts, tools and
  learning, but not yet fast. A bytecode VM or IR is the planned next step for
  performance, followed by the JavaScript target.
- HTTP uses the system `curl` program as its transport (built into Windows 10+,
  macOS and most Linux systems). This keeps the build free of native C
  dependencies. `lipi doctor` checks for it.
- Calling an `async` function runs its body immediately. Background I/O
  (`http`, `sleep`) does overlap, but there's no cooperative scheduler yet.
- Closures hold their scope with reference counting, so long-running programs
  that create many self-referencing closures can leak memory. Cycle collection
  comes with the VM.
- There's no permission model yet for `fs`, `process.run` or the network.
