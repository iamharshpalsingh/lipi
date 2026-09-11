# LiPi for Visual Studio Code

Language support for **LiPi**, the Unified Development Language, created by
**Harsh Pal Singh** ([@iamharshpalsingh](https://github.com/iamharshpalsingh)).

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

The LiPi installer adds this extension automatically when VS Code is
installed. To add it by hand, use the `lipi-<version>.vsix` file from the LiPi
release: in the Extensions view choose **…** → *Install from VSIX…*, or run
`code --install-extension lipi-<version>.vsix`.

To build the package yourself:

```sh
npm install
npx @vscode/vsce package
```
