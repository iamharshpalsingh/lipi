# LiPi 1.0 stability promise

LiPi 1.0 freezes the language. From 1.0 on, **a program that works with LiPi
1.0 keeps working, with the same results, with every 1.x release.** You can
build on LiPi without worrying that an upgrade will break your code.

## What is stable

- **The grammar**: everything in [SPEC §18](SPEC.md#18-grammar-ebnf-v10),
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
- **`lipi.json` and `lipi.lock`**: both formats stay readable by later 1.x releases.

## What may change in 1.x

- **New things**: new functions, methods, modules, options, commands and
  error codes. New syntax may be added only where the same text was
  previously an error.
- **Messages**: the wording of error messages and hints can improve. Codes don't change.
- **Warnings**: `lipi lint` may learn new warnings. They are never errors,
  except when you choose `--strict`.
- **Speed and memory use**, and the text shown for values that have no fixed
  form, such as `<function add>` or `<task running>`.
- **Security fixes**: a program that relies on a security hole may stop
  working, and release notes will say so.
- **Defect fixes**: where both engines agreed with each other but contradicted
  this spec, the spec wins and the behaviour is corrected. Release notes name
  the change, show the code shape it affects, and the compiler explains it
  where it can. This has happened once:

  | Release | Was | Is |
  |---|---|---|
  | 1.3 | A `for` loop kept one binding for its variables, so a function made in the body saw the last item, and the variable stayed readable after the loop | Each round binds them afresh (SPEC §5), and they aren't visible after the loop — using one there is LIP1002 with a hint |

  These two are the only exceptions.

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
  replacement. It's removed only in a new major version (2.0).
- **Minimum version**: the `"lipi"` field in `lipi.json` records the lowest
  LiPi version a project needs.
- **Package versions**: packages use semantic versioning, and `lipi.lock`
  keeps every build reproducible.
