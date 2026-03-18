# nvim-lspconfig configuration for debian-lsp

This directory contains configuration for using debian-lsp with Neovim 0.11+.

## Installation

Copy (or symlink) `lsp/debian_lsp.lua` to your Neovim LSP config directory:

```sh
mkdir -p ~/.config/nvim/lsp
cp lsp/debian_lsp.lua ~/.config/nvim/lsp/
```

Then enable the LSP in your Neovim config:

```lua
vim.lsp.enable('debian_lsp')
```

## File

- `lsp/debian_lsp.lua` - LSP config for Neovim 0.11+ (`vim.lsp.config`/`vim.lsp.enable`)
