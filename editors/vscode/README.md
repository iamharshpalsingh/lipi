# LiPi for Visual Studio Code

<img src="images/icon.png" width="64" alt="LiPi">

Language support for [LiPi](../../README.md), the Unified Development Language.

- Syntax highlighting for `.lipi` files, including `{…}` interpolation
- `.lipi` files show the LiPi mark in the explorer
- Automatic indentation after `if`, `for`, function definitions, `with` blocks and so on
- Comment toggling (`#`), bracket matching, auto-closing quotes and indentation-based folding

## Try it locally

Copy or link this folder into your VS Code extensions directory, then restart VS Code:

```sh
# Windows (PowerShell)
New-Item -ItemType Junction -Path "$env:USERPROFILE\.vscode\extensions\lipi-lang.lipi-0.1.0" -Target (Resolve-Path editors\vscode)
# macOS / Linux
ln -s "$(pwd)/editors/vscode" ~/.vscode/extensions/lipi-lang.lipi-0.1.0
```

Or package it with `npx @vscode/vsce package` and install the `.vsix`.

Diagnostics, autocomplete and go-to-definition will arrive with the LiPi
language server (`lipi lsp`), which is part of the 0.5 roadmap.
