# Changelog

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
