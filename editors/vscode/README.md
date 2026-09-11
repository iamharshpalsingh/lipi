# LiPi for Visual Studio Code

<img src="images/icon.png" width="64" alt="LiPi">

Language support for **LiPi**, the Unified Development Language.

- **Errors and warnings as you type**, with the same codes and hints as `lipi check` and `lipi lint`
- **Completion** for keywords, the standard library (`math.`, `json.`, `server.`...) and your own names
- **Hover documentation** for built-ins and your functions (the comments above them)
- **Go to definition**, including into files loaded with `use`, plus **find references** and the **outline**
- **Formatting** with `lipi format` (on save by default)
- Syntax highlighting, `{…}` interpolation, indentation rules, comment toggling and the LiPi file icon

## Requirements

The `lipi` program must be installed. The extension runs `lipi lsp`. If
`lipi` isn't on your PATH, set **LiPi: Path** (`lipi.path`) in settings to the
full path of `lipi` / `lipi.exe`.

## Install

Install the packaged `lipi-0.2.0.vsix` from the Extensions view (**…** → *Install from VSIX…*), or run:

```sh
code --install-extension lipi-0.2.0.vsix
```

To build the package yourself:

```sh
npm install
npx @vscode/vsce package --skip-license --allow-missing-repository
```
