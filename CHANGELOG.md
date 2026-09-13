# Changelog

## 2.0.0

Everything a real web app needed and LiPi had no answer for.

### Breaking: `for` loops bind their variables afresh each round

A loop kept one binding for its variables, so a function made inside the body —
every button in a list — saw whatever the last round left there. Both engines
now give each round its own binding:

```lipi
adders = []
for n in 1 to 3
    adders.push(x => x + n)
show adders.map(f => f(10))     # [11, 12, 13], was [13, 13, 13]
```

The loop's variables belong to the loop and aren't visible after it; what the
body assigns still belongs to the function around it, so a running total is
unaffected. Reading one after its loop is LIP1002, found by `lipi check` with a
hint saying where it went — that is the only thing to change when moving from
1.x, and [docs/STABILITY.md](docs/STABILITY.md) walks through it.

### New

**Form elements.** `select value, options: [...] with chosen` is a dropdown
(an option is a value or `{value, label}`); `field note, lines: 4` is a box
several lines tall; `upload "Add a photo", accept: "image/*" with files` hands
its block each chosen file as `{name, type, size, dataUrl, text}`, already read.

**`action:` on any element.** It makes a card, a row or anything else
clickable, and adds `role="button"` and `tabindex="0"` so Enter and Space work
for someone not using a mouse — no more transparent button stretched over a
tile because `button` only takes text.

```lipi
for brand in brands
    card class: "tile", action: () => pick(brand)
        text brand.name
```

**Every page gets its own address and its own HTML file.** A page can name
itself, and `lipi build` writes `dist/price/index.html` for each one, carrying
its `<title>`, description and Open Graph tags, plus `404.html`:

```lipi
page "/price", title: "Price — Fixy", description: "The exact price, before you book."
```

`--site https://your.site` adds canonical addresses, `sitemap.xml` and
`robots.txt`. Served over http(s) the app uses real addresses (`/price`) and
moves between pages without reloading; opened from a file it falls back to the
`#/path` form, so a build still works from disk. `lipi dev` serves the same
addresses.

**Timers keep running with a web server.** `time.after` and `time.every` used
to run only after the main program finished, which with `server.start` meant
never. Background jobs — sweeps, reminders, retries — now belong in the same
program as the routes, taking their turns on the same thread as the handlers.

## 1.2.0

JavaScript's everyday features, in both engines (`lipi run` and `lipi build`),
with the same results:

- Spread and rest: `[...a, ...b]`, `{...defaults, color: "red"}`,
  `f(...args)` and `total(first, ...others)`.
- Patterns: `{name, age: years, ...others} = user`, `[first, _, ...more] = items`,
  `[a, b] = [b, a]`, and `for {name, age} in people`.
- `type Admin extends User`, with `super.method()` and subtypes accepted
  wherever the parent type is expected.
- The `regex` module (`test find findAll replace split`, with `ignoreCase:`
  and `multiline:`) and the `encoding` module (Base64, URL encoding, hex).
- Timers: `time.after(ms)` and `time.every(ms)` with a block, like
  setTimeout and setInterval.
- New methods: `findIndex findLast flatMap shift unshift groupBy lastIndexOf`,
  `sort((a, b) => ...)`, `lastIndexOf` on Strings and `toFixed` on numbers;
  `math.trunc` and 64-bit bit operations (`bitAnd bitOr bitXor bitNot
  shiftLeft shiftRight`).
- In 'single quotes', a backslash that isn't a known escape stays as it is,
  so regex patterns read naturally: `'\d+'`.
- Using a JavaScript name (`includes`, `forEach`, `some`, `toUpperCase`,
  `new`, `class`, `setTimeout`...) gives a hint with the LiPi way.
- Course chapter 13 shows the JavaScript way and the LiPi way side by side.
- The LiPi Playground: write LiPi in a browser and see it run, with nothing
  to install. The compiler runs in the page as WebAssembly (the new `web`
  crate) and programs run in a sandboxed frame. It has examples, error
  messages with hints, and share links (the code goes in the link).
  Build it with `scripts\build-playground.ps1`; the result is one file,
  `dist\lipi-playground.html`, that can be opened directly or hosted anywhere.

## 1.1.2

- Credits: LiPi was created by Harsh Pal Singh (@iamharshpalsingh). It's shown
  by `lipi --version`, `lipi help` and the interactive prompt, and in the
  README, license, specification, course and VS Code extension.

## 1.1.1

- "did you mean" suggestions: when two names are equally close, the one that
  starts with the same letter wins, then the one closest in length (`nmae`
  now suggests `naam`, not `image`).
- The LiPi Paathshala course (`docs/learn`, in Hinglish): 13 chapters with
  exercises, every example checked by the test suite.

## 1.1.0

- LiPi UI: every element takes `hover:`, `focus:`, `mobile:` (screens up to
  720 px) and `desktop:` style options, so responsive layouts and hover
  effects need no CSS file: `button "Save", hover: "background: #B83A22;"`.
- Buttons and links animate their colour changes.
- The installer updates LiPi even while VS Code or `lipi dev` is using it.

## 1.0.3

- `lipi dev` with a file that doesn't exist reports it right away. Before,
  it started a server that couldn't notice when the file was created.
- The project folder is found correctly even when the file doesn't exist yet.
- "couldn't read main.lipi" now points to src\main.lipi when that's where the
  file is, and suggests `lipi run`.
- The VS Code extension includes its MIT license (the Marketplace requires it).

## 1.0.2

- The Windows installer registers `.lipi` files for the current user: they
  show the LiPi logo in File Explorer and open for editing (in VS Code if it's
  installed, otherwise Notepad). `install.ps1 -Uninstall` removes this again.

## 1.0.1

- LiPi UI: components accept `key:` (`Row(item, key: item.id)`), so their
  `state` follows the item when a list is filtered or reordered. Duplicate
  keys are reported (LIP5008).
- `lipi format` keeps each file's line endings and UTF-8 byte-order mark, so
  files saved with Windows (CRLF) line endings or a BOM are no longer
  reported as needing formatting.

## 1.0.0

The first stable release. Programs written for 1.0 keep working across all 1.x
releases ([docs/STABILITY.md](docs/STABILITY.md)).

- **Stable language:** the grammar is frozen (SPEC §18), with a conformance
  suite that runs every program with both `lipi run` and `lipi build` and
  covers every syntax form. `enum`, `trait` and `yield` are reserved for the future.
- **Faster interpreter:** variables are resolved to slots before a program
  runs, common Integer operations take a fast path, and mimalloc is the memory
  allocator: 1.6–2.1× faster than 0.8.
- **Installers:** Windows (`install.cmd`, no administrator rights) and
  Linux/macOS (`install.sh`), with the VS Code extension included. CI and
  release workflows cover Windows, Linux and macOS.
- **Docs:** [getting started](docs/GETTING_STARTED.md); MIT license.

## 0.8

- `lipi build`: compiles to JavaScript for the browser (`--target web`) or
  Node.js (`--target node`), with the same results and error messages as
  `lipi run`. Browser builds refuse server-only code (LIP6001).
- LiPi UI: `page`, `component`, `state`, events, and elements such as `card`,
  `row`, `heading`, `text`, `button`, `field`, `checkbox` and `link`.
- JavaScript interop through the `js` module.
- `lipi dev`: a development server that rebuilds and reloads the page on every save.

## 0.5

- `lipi format`, `lipi lint`, packages (`lipi install`/`remove`/`update`/`publish`)
  with `lipi.lock`, the language server (`lipi lsp`) and the VS Code extension.

## 0.3

- Web server (routes, middleware, cookies, WebSockets), `crypto`, and a
  database layer for SQLite and PostgreSQL.

## 0.1 – 0.2

- The core language: values, Integer/Decimal numbers, strict Booleans,
  functions, types, errors with codes and hints, modules, async/await, the
  standard library, tests and the REPL.
