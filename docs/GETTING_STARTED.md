# Getting started with LiPi

This guide takes you from installing LiPi to a command-line tool, a web API
with a database, and a web app, in about half an hour.

## 1. Install

**From a release:** download the archive for your system from
[github.com/iamharshpalsingh/lipi/releases](https://github.com/iamharshpalsingh/lipi/releases),
unzip it, then:

- **Windows:** double-click `install.cmd` (or run `.\install.ps1`). No
  administrator rights are needed.
- **Linux / macOS:** run `./install.sh` in a terminal.

Open a new terminal and check:

```sh
lipi --version
lipi doctor
```

**From source:** install [Rust](https://rustup.rs), then run
`git clone https://github.com/iamharshpalsingh/lipi`, and in that folder
`cargo install --path cli`. Node.js is only needed to run
`lipi build --target node` output.

**Editor:** the release includes the VS Code extension, and the installer adds
it when VS Code is present. It gives highlighting, errors as you type,
completion, hover help and formatting on save.

## 2. Your first program

Create `hello.lipi`:

```lipi
name = "Asha"
show "Hello, {name}!"

age = 20
if age >= 18
    show "You can vote."
```

Run it:

```sh
lipi hello.lipi
```

Blocks are the lines indented by 4 spaces. `show` prints. Double quotes put
values inside text with `{...}`.

Try the interactive prompt too: run `lipi`, type `1 + 2`, and press Enter.

## 3. Mistakes are explained

Change `name` to `nmae` in the last line and run it again:

```
ERROR LIP1002: undefined variable "nmae"

hello.lipi:2:15
    show "Hello, {nmae}!"
                  ^^^^

Hint: did you mean "name"?
```

Every error has a code (see the [spec](SPEC.md#16-diagnostics)), the exact
place, and a hint. `lipi check file.lipi` finds mistakes without running anything.

## 4. A project with tests

```sh
lipi new todo
cd todo
lipi run
lipi test
```

`lipi new` creates `lipi.json`, `src/main.lipi` and `tests/`. Replace
`src/main.lipi` with a small to-do tool:

```lipi
todos = []

add(title)
    todos.push({title: title, done: false})

finish(title)
    for todo in todos
        if todo.title == title
            todo.done = true

add("Buy milk")
add("Write code")
finish("Buy milk")

for todo in todos
    mark = "x" if todo.done else " "
    show "[{mark}] {todo.title}"

export add, finish, todos
```

And `tests/main_test.lipi`:

```lipi
use "../src/main.lipi" as app

test "finishing marks a to-do as done"
    app.add("Test")
    app.finish("Test")
    assertEqual(app.todos.last.done, true)
```

`lipi format` tidies every file, and `lipi lint` points out unused variables
and code that never runs.

## 5. A web API with a database

```lipi
db = database.open("shop.db")

server.start 3000

get "/products"
    return db.products.all()

post "/products" with request
    product = db.products.create(request.json())
    return server.respond(201, product)

get "/products/:id" with request
    product = db.products.find(toInteger(request.params.id))
    if product == null
        return server.respond(404, {error: "not found"})
    return product
```

Run it with `lipi run`, then try `curl http://localhost:3000/products`.
The table is created from the data you store. Use
`database.open("postgres://user:password@host/shop")` for PostgreSQL.

## 6. A web app

```lipi
state items = []
state draft = ""

page "/"
    heading "Shopping list", level: 1
    row
        field draft, placeholder: "Add an item" with value
            draft = value
        button "Add", disabled: draft == ""
            items.push(draft)
            draft = ""
    for item in items
        card
            text item
```

```sh
lipi dev app.lipi       # open http://localhost:3000 and edit the file: the page reloads
lipi build app.lipi     # dist/index.html + dist/app.js, ready to host anywhere
```

Components, pages with parameters, links and more are in
[SPEC §15.3](SPEC.md#153-web-ui-lipi-ui). JavaScript libraries and browser
APIs are reachable through the `js` module ([SPEC §15.4](SPEC.md#154-javascript-interop)).

## 7. Where next

- [The language specification](SPEC.md): everything, with examples
- [`examples/`](../examples): a CLI, an API server, a notes app with a
  database, a web shop
- [The stability promise](STABILITY.md): what stays the same across 1.x releases
