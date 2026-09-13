# LiPi stability promise

**A program that works with a LiPi release keeps working, with the same
results, with every later release of the same major version.** You can build on
LiPi without worrying that an upgrade will break your code. That promise held
for all of 1.x, and it holds again from 2.0 on.

Breaking a program is what a new major version is for, and 2.0 has exactly one
such change — see [Moving from 1.x to 2.0](#moving-from-1x-to-20) below.

## What is stable

- **The grammar**: everything in [SPEC §18](SPEC.md#18-grammar-ebnf-v20),
  with the keywords and reserved words listed below.
- **The meaning of programs**: the rules in the spec for values, numbers
  (Integer and Decimal), strict Boolean conditions, equality, scope, errors,
  async, modules, types, LiPi UI and JavaScript interop.
- **The standard library**: the names, parameters and behaviour of built-in
  functions, methods and modules.
- **The command line**: `lipi` commands, their options and exit codes.
- **Error codes**: a code such as `LIP2001` keeps meaning the same thing, so
  editors, CI checks and `catch` blocks can rely on it
  (see [SPEC §16](SPEC.md#16-diagnostics)).
- **Formatting**: `lipi format` output only changes to fix a bug.
- **`lipi.json` and `lipi.lock`**: both formats stay readable by later releases.

## What may change in 2.x

- **New things**: new functions, methods, modules, options, commands and
  error codes. New syntax may be added only where the same text was
  previously an error.
- **Messages**: the wording of error messages and hints can improve. Codes don't change.
- **Warnings**: `lipi lint` may learn new warnings. They are never errors,
  except when you choose `--strict`.
- **Speed and memory use**, and the text shown for values that have no fixed
  form, such as `<function add>` or `<task running>`.
- **Security fixes**: a program that relies on a security hole may stop
  working. This is the only exception, and release notes will say so.

## Moving from 1.x to 2.0

One change, and the compiler points at every place it affects.

**A `for` loop's variables belong to the loop, and each round binds them
afresh.** A loop used to keep one binding, so a function made inside the body
saw whatever the last round left there, and the variable stayed readable after
the loop.

```lipi
adders = []
for n in 1 to 3
    adders.push(x => x + n)
show adders.map(f => f(10))     # 2.0: [11, 12, 13]   1.x: [13, 13, 13]
```

A function, or a UI event block, written inside a loop now acts on that round's
item. What the body *assigns* still belongs to the function around it, so a
running total is unaffected:

```lipi
total = 0
for price in prices
    total += price      # `total` is the function's, as before
show total              # fine; `show price` here is now LIP1002
```

**What to change in your code:** anything that reads a loop's variable after
the loop has ended. `lipi check` finds every one of them and its hint says
where the variable went; keep what you need in a variable defined before the
loop. Nothing else about `for` changed, and no other program shape is affected.

## Keywords and reserved words

Keywords: `if else for in while repeat break continue return and or not
true false null const use from as export function async await try catch
finally throw match show`.

Words with a meaning in some places: `to` and `step` (ranges), `with`
(trailing blocks), `type`, `extends` (after a type's name), `super` (in the
methods of a type that extends another), `test`, `component`, `state`.

Reserved for future features (they can't be used as names): `route`,
`server` (also the web server module), `enum`, `trait`, `yield`.

## How changes are made

- **Tested behaviour**: [`tests/conformance`](../tests/conformance) is the
  executable form of this promise. Every program in it must give the same
  output with `lipi run` and with `lipi build`, and the suite must use every
  syntax form in the language. A change that breaks a conformance program is
  a breaking change.
- **Deprecation**: when something needs to go, it first produces a warning
  (a LIP9xxx code) for at least one minor release, with a hint showing the
  replacement. It's removed only in a new major version.
- **Major versions**: a change that breaks a working program waits for one,
  is listed in the release notes with the code shape it affects, and is
  explained by the compiler wherever it can be.
- **Minimum version**: the `"lipi"` field in `lipi.json` records the lowest
  LiPi version a project needs.
- **Package versions**: packages use semantic versioning, and `lipi.lock`
  keeps every build reproducible.
