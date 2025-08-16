# debian-lsp

[![CI](https://github.com/jelmer/debian-lsp/actions/workflows/ci.yml/badge.svg)](https://github.com/jelmer/debian-lsp/actions/workflows/ci.yml)
[![Tests](https://github.com/jelmer/debian-lsp/actions/workflows/test.yml/badge.svg)](https://github.com/jelmer/debian-lsp/actions/workflows/test.yml)

Language Server Protocol implementation for Debian control files.

At the moment this is fairly basic, but the goal is to provide a useful LSP server for editing Debian control files (`debian/control`) with features like:
- Field name completion
- Common package name suggestions
- Diagnostics for common issues
- Quick fixes for common issues
- Integration with lintian-brush, apply-multiarch-hints, etc

## Features

- Field name completion for Debian control files
- Common package name suggestions
- Works with debian/control files
- Diagnostic analysis for Debian control files
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

Add the following configuration to your VS Code `settings.json`:

```json
{
  "languageServerProtocols.debian-lsp.command": [
    "/path/to/debian-lsp/target/release/debian-lsp"
  ],
  "languageServerProtocols.debian-lsp.filetypes": [
    "debcontrol"
  ],
  "files.associations": {
    "control": "debcontrol",
    "**/debian/control": "debcontrol"
  }
}
```

Alternatively, you can use the generic LSP client extension:

1. Install the "Generic LSP Client" extension
2. Add to your `settings.json`:

```json
{
  "genericLanguageServer.configurations": {
    "debian-lsp": {
      "command": ["/path/to/debian-lsp/target/release/debian-lsp"],
      "filePatterns": ["**/debian/control", "control"],
      "languageId": "debcontrol"
    }
  }
}
```

### Using with Vim/Neovim

#### coc.nvim

1. Build the coc plugin:
   ```bash
   cd coc-debian
   npm install
   npm run build
   ```

2. Install the plugin in Vim with coc.nvim:
   ```vim
   :CocInstall /path/to/debian-lsp/coc-debian
   ```

3. Configure the LSP path in your coc-settings.json:
   ```json
   {
     "debian.serverPath": "/path/to/debian-lsp/target/release/debian-lsp"
   }
   ```

#### ALE

Add the following configuration to your `.vimrc` or `init.vim`:

```vim
" Register debian-lsp with ALE
let g:ale_linters = get(g:, 'ale_linters', {})
let g:ale_linters.debcontrol = ['debian-lsp']

" Configure the debian-lsp executable
call ale#linter#Define('debcontrol', {
\   'name': 'debian-lsp',
\   'lsp': 'stdio',
\   'executable': expand('~/src/debian-lsp/target/release/debian-lsp'),
\   'command': '%e',
\   'project_root': function('ale#handlers#lsp#GetProjectRoot'),
\})
```

Note: Adjust the `executable` path to match your installation location. You can trigger code actions in ALE with `:ALECodeAction` when your cursor is on a diagnostic.

#### Native Neovim LSP

Add the following configuration to your Neovim config (init.lua):

```lua
-- Configure debian-lsp
vim.api.nvim_create_autocmd({'BufEnter', 'BufWinEnter'}, {
  pattern = {'*/debian/control', 'control'},
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
      filetypes = {'debcontrol'},
      root_dir = lspconfig.util.root_pattern('debian/control', '.git'),
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
