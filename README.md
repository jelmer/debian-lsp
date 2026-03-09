# debian-lsp

[![CI](https://github.com/jelmer/debian-lsp/actions/workflows/ci.yml/badge.svg)](https://github.com/jelmer/debian-lsp/actions/workflows/ci.yml)
[![Tests](https://github.com/jelmer/debian-lsp/actions/workflows/test.yml/badge.svg)](https://github.com/jelmer/debian-lsp/actions/workflows/test.yml)

Language Server Protocol implementation for Debian packaging files.

At the moment this is fairly basic, but the goal is to provide a useful LSP server for editing Debian packaging files with features like:
- Field name completion
- Common package name suggestions
- Diagnostics for common issues
- Quick fixes for common issues
- Integration with lintian-brush, apply-multiarch-hints, etc

## Supported Files

- `debian/control` - Package control files
- `debian/copyright` - DEP-5 copyright files
- `debian/watch` - Upstream watch files
- `debian/tests/control` - Autopkgtest control files (basic support)

## Features

- Field name completion for Debian packaging files
- Common package name suggestions for dependencies
- Works with `debian/control`, `debian/copyright`, `debian/watch`, and `debian/tests/control`
- Diagnostic analysis for control and copyright files
- Quick fixes for common issues

### Diagnostics

The LSP provides the following diagnostic capabilities:

- **Field casing validation**: Detects incorrectly cased field names (e.g., `source` instead of `Source`)
- **Parse error reporting**: Reports parsing errors in control file syntax

### Quick Fixes

The LSP offers automatic fixes for detected issues:

- **Field casing corrections**: Automatically fix field names to use proper Debian control file casing
  - Example: `source` → `Source`, `maintainer` → `Maintainer`
  - Available as code actions in your editor

## Installation

### Building the LSP server

```bash
cargo build --release
```

The binary will be available at `target/release/debian-lsp`.

### Using with VS Code

A dedicated VS Code extension is available in the `vscode-debian` directory. See [vscode-debian/README.md](vscode-debian/README.md) for installation and configuration instructions.

### Using with Vim/Neovim

#### coc.nvim

A coc.nvim extension is available in the `coc-debian` directory. See [coc-debian/README.md](coc-debian/README.md) for installation and configuration instructions.

#### ALE

Source the provided configuration file in your `.vimrc` or `init.vim`:

```vim
source /path/to/debian-lsp/ale-debian-lsp.vim
```

By default, the configuration will look for the `debian-lsp` executable in the same directory as the vim file. To use a custom path, set `g:debian_lsp_executable` before sourcing:

```vim
let g:debian_lsp_executable = '/custom/path/to/debian-lsp'
source /path/to/debian-lsp/ale-debian-lsp.vim
```

This configuration enables debian-lsp for all supported file types:
- `debian/control` (debcontrol filetype)
- `debian/copyright` (debcopyright filetype)
- `debian/changelog` (debchangelog filetype)
- `debian/source/format` (debsources filetype)
- `debian/watch` (make filetype)
- `debian/tests/control` (make filetype)

You can trigger code actions in ALE with `:ALECodeAction` when your cursor is on a diagnostic.

#### Native Neovim LSP

Add the following configuration to your Neovim config (init.lua):

```lua
-- Configure debian-lsp
vim.api.nvim_create_autocmd({'BufEnter', 'BufWinEnter'}, {
  pattern = {
    '*/debian/control',
    '*/debian/copyright',
    '*/debian/changelog',
    '*/debian/source/format',
    '*/debian/watch',
    '*/debian/tests/control',
  },
  callback = function()
    vim.lsp.start({
      name = 'debian-lsp',
      cmd = {vim.fn.expand('~/src/debian-lsp/target/release/debian-lsp')},
      root_dir = vim.fn.getcwd(),
    })
  end,
})
```

Or if you prefer using lspconfig:

```lua
local lspconfig = require('lspconfig')
local configs = require('lspconfig.configs')

-- Define the debian-lsp configuration
if not configs.debian_lsp then
  configs.debian_lsp = {
    default_config = {
      cmd = {vim.fn.expand('~/src/debian-lsp/target/release/debian-lsp')},
      filetypes = {
        'debcontrol',
        'debcopyright',
        'debchangelog',
        'debsources',
        'make',
      },
      root_dir = lspconfig.util.root_pattern('debian', '.git'),
      settings = {},
    },
  }
end

-- Enable debian-lsp
lspconfig.debian_lsp.setup{}
```

Note: Adjust the `cmd` path to match your installation location.

## Usage

Open any `debian/control` or `control` file in your configured editor. The LSP will automatically provide completions for:
- Field names (Source, Package, Depends, etc.)
- Common package names

## Development

To run the LSP in development mode:
```bash
cargo run
```

To watch and rebuild the coc plugin:
```bash
cd coc-debian
npm run watch
```
