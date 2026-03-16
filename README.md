# debian-lsp

[![CI](https://github.com/jelmer/debian-lsp/actions/workflows/ci.yml/badge.svg)](https://github.com/jelmer/debian-lsp/actions/workflows/ci.yml)
[![Tests](https://github.com/jelmer/debian-lsp/actions/workflows/test.yml/badge.svg)](https://github.com/jelmer/debian-lsp/actions/workflows/test.yml)

Language Server Protocol implementation for Debian packaging files.

## Supported Files

- `debian/control` - Package control files
- `debian/copyright` - DEP-5 copyright files
- `debian/watch` - Upstream watch files (v1-4 line-based and v5 deb822 formats)
- `debian/changelog` - Package changelog files
- `debian/source/format` - Source format declaration files
- `debian/tests/control` - Autopkgtest control files (basic support)
- `debian/upstream/metadata` - DEP-12 upstream metadata files

## Features

### Completions

**debian/control:**
- Field name completions for all standard source and binary package fields
- Package name completions for relationship fields (Depends, Build-Depends, Recommends, etc.) using the system package cache
- Value completions for Section (all Debian sections including area-qualified), Priority, and architecture fields

**debian/copyright:**
- Field name completions for header, files, and license paragraphs
- Value completions for Format and License (from `/usr/share/common-licenses`)

**debian/watch:**
- Field name completions for watch file fields
- Version number completions
- Option value completions (compression, mode, pgpmode, searchmode, gitmode, gitexport, component)

**debian/changelog:**
- Distribution completions (unstable, stable, testing, experimental, UNRELEASED, plus release codenames)
- Urgency level completions (low, medium, high, critical, emergency)

**debian/source/format:**
- Format value completions (3.0 (quilt), 3.0 (native), 3.0 (git), 1.0, etc.)

**debian/upstream/metadata:**
- Field name completions for all DEP-12 fields (Repository, Bug-Database, Contact, etc.)

### Diagnostics

- Field casing validation (e.g. `source` instead of `Source`)
- Parse error reporting with position information

### Code Actions

- **Fix field casing** - automatically correct field names to canonical casing
- **Wrap and sort** - wrap long fields to 79 characters and sort dependency lists (control and copyright files)
- **Add changelog entry** - create a new changelog entry with incremented version, UNRELEASED distribution, and auto-populated maintainer
- **Mark for upload** - replace UNRELEASED with the target distribution

### On-Type Formatting

For deb822-based files (control, copyright, watch, tests/control), the server provides on-type formatting:
- Automatically inserts a space after typing `:` at the end of a field name
- Inserts continuation-line indentation after pressing Enter inside a field value

This requires the editor to have format-on-type enabled:

- **VS Code**: Enabled by default via the extension's `configurationDefaults`
- **coc.nvim**: Set `"coc.preferences.formatOnType": true` in your coc-settings.json (`:CocConfig`)
- **Native Neovim LSP**: Pass `on_type_formatting = true` in your client capabilities, or call `vim.lsp.buf.format()` manually
- **ALE**: Not supported (ALE does not handle `textDocument/onTypeFormatting`)

### Semantic Highlighting

Custom token types for syntax highlighting of Debian-specific constructs:
- Control/copyright/watch/upstream-metadata files: field names, unknown fields, values, comments
- Changelog files: package name, version, distribution, urgency, maintainer, timestamp

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
    '*/debian/upstream/metadata',
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
        'yaml',
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
