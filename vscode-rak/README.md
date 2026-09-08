# Rak for VS Code

Syntax highlighting and language features for the [Rak](https://github.com/Louiml/Rak) programming language.

## Setup

1. Install `rakc` (with the `lsp` feature built):
   ```bash
   cargo build --release --features lsp
   ```
2. Make sure `rakc` is on your PATH, or configure the path in VS Code settings.

## Language server

`rakc lsp` is a stdio language server that provides diagnostics, completion, hover, and go-to-definition. VS Code launches it automatically for `.rak` files when configured. Since this extension uses the TextMate grammar only, add the following to your user `settings.json` to enable the LSP manually (or use any LSP client like `vscode-lsp`):

```json
"rak.server.path": "/path/to/rakc"
```

For Neovim (with `nvim-lspconfig`), configure `rakc lsp` as the command for `.rak` files.