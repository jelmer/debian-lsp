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
- `debian/source/options` - dpkg-source options files
- `debian/source/local-options` - Local dpkg-source options files
- `debian/tests/control` - Autopkgtest control files (basic support)
- `debian/upstream/metadata` - DEP-12 upstream metadata files
- `debian/rules` - Package build rules (Makefile)

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

**debian/source/options and debian/source/local-options:**
- Option name completions for all dpkg-source long options (compression, single-debian-patch, etc.)
- Value completions for compression and compression-level options
- Filters options by file type (some options are local-options only)

**debian/upstream/metadata:**
- Field name completions for all DEP-12 fields (Repository, Bug-Database, Contact, etc.)

**debian/rules:**
- Target name completions for standard Debian Policy targets (clean, build, binary, etc.) and debhelper override/execute targets
- Variable name completions for common build variables (DEB_BUILD_OPTIONS, DEB_HOST_MULTIARCH, etc.)
- Excludes already-defined targets from completions

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

### Inlay Hints

**debian/control:**
- Archive versions per suite for packages in dependency fields
- Providers for virtual packages
- Resolved values for substitution variables (`${shlibs:Depends}`, etc.)

**debian/changelog:**
- Distribution-to-suite mappings (e.g. `unstable = sid`, `UNRELEASED -> unstable`)

### Code Lenses

**debian/control:**
- Standards-Version: shows the latest version when outdated
- debhelper-compat: shows stable and maximum compat levels (via `dh_assistant`)
- Vcs-Git: shows the packaged version from UDD vcswatch

### Document Symbols

- **debian/control** - source and binary package paragraphs
- **debian/copyright** - header, files, and license paragraphs
- **debian/changelog** - changelog entries

### Folding Ranges

Paragraph-level folding for deb822-based files (control, copyright, watch,
tests/control) and entry-level folding for changelog files.

### Document Formatting

Wrap-and-sort formatting for debian/control, debian/copyright, and debian/watch
(deb822 format) files.

### Semantic Highlighting

Custom token types for syntax highlighting of Debian-specific constructs:
- Control/copyright/watch/upstream-metadata/source-options/rules files: field names, unknown fields, values, comments
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

#### vim-lsp

Add the following configuration to your `.vimrc` or `init.vim`:

```vim
" Configure vim-lsp for debian-lsp
function! s:config_debian_lsp()
  if executable('debian-lsp')
    augroup debian_lsp
      autocmd!
      autocmd User lsp_setup call lsp#register_server({
        \ 'name': 'debian-lsp',
        \ 'cmd': {server_info -> ['debian-lsp']},
        \ 'allowlist': ['debcontrol', 'debcopyright', 'debchangelog', 'debsources', 'debsourceoptions', 'debwatch', 'debupstream', 'autopkgtest', 'debrules'],
        \ 'blocklist': [],
        \ 'enabled': 1,
        \ })
    augroup END
  endif
endfunction

call s:config_debian_lsp()

" Set filetypes for Debian packaging files (if not already set by ftdetect)
augroup debian_filetypes
  autocmd!
  autocmd BufNewFile,BufRead */debian/control setfiletype debcontrol
  autocmd BufNewFile,BufRead */debian/copyright setfiletype debcopyright
  autocmd BufNewFile,BufRead */debian/changelog setfiletype debchangelog
  autocmd BufNewFile,BufRead */debian/changelog.dch setfiletype debchangelog
  autocmd BufNewFile,BufRead */debian/source/format setfiletype debsources
  autocmd BufNewFile,BufRead */debian/source/options setfiletype debsourceoptions
  autocmd BufNewFile,BufRead */debian/source/local-options setfiletype debsourceoptions
  autocmd BufNewFile,BufRead */debian/watch setfiletype debwatch
  autocmd BufNewFile,BufRead */debian/upstream/metadata setfiletype debupstream
  autocmd BufNewFile,BufRead */debian/rules setfiletype debrules
augroup END
```

Replace `debian-lsp` with the full path to the executable if it's not on your PATH.

You can then use vim-lsp commands like:
- `:LspDocumentDiagnostics` - Show diagnostics
- `:LspCodeAction` - Show code actions
- `:LspDefinition` - Go to definition
- `:LspHover` - Show hover information

#### Neovim 0.11+ with bundled config

A bundled LSP config is provided in the `nvim-lspconfig/` directory. Copy it to your Neovim config:

```sh
mkdir -p ~/.config/nvim/lsp
cp nvim-lspconfig/lsp/debian_lsp.lua ~/.config/nvim/lsp/
```

Then enable it in your `init.lua`:

```lua
vim.lsp.enable('debian_lsp')
```

To use a custom path to the `debian-lsp` binary:

```lua
vim.lsp.config('debian_lsp', {
  cmd = { '/path/to/debian-lsp' },
})
vim.lsp.enable('debian_lsp')
```

#### Native Neovim LSP (without nvim-lspconfig)

If you don't use nvim-lspconfig, add the following to your `init.lua`:

```lua
vim.api.nvim_create_autocmd({'BufEnter', 'BufWinEnter'}, {
  pattern = {
    '*/debian/control',
    '*/debian/copyright',
    '*/debian/changelog',
    '*/debian/changelog.dch',
    '*/debian/source/format',
    '*/debian/source/options',
    '*/debian/source/local-options',
    '*/debian/watch',
    '*/debian/tests/control',
    '*/debian/upstream/metadata',
    '*/debian/rules',
  },
  callback = function()
    vim.lsp.start({
      name = 'debian-lsp',
      cmd = {'debian-lsp'},
      root_dir = vim.fn.getcwd(),
    })
  end,
})
```
### Using with Helix

See [helix-lspconfig/README.md](helix-lspconfig/README.md) for installation and configuration instructions.

### Using with Emacs

See [emacs-lspconfig/README.md](emacs-lspconfig/README.md) for installation and configuration instructions.

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
