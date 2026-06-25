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
- `debian/tests/control` - Autopkgtest control files
- `debian/upstream/metadata` - DEP-12 upstream metadata files
- `debian/rules` - Package build rules (Makefile)
- `debian/patches/series` - List of patches applied by dpkg-source
- `debian/source/lintian-overrides` - Lintian tag overrides for the source package
- `debian/<package>.lintian-overrides` - Lintian tag overrides for binary packages

**debhelper files:**
- `debian/dirs` - Directories to create in the package build directory
- `debian/<package>.dirs` - Per-package directory lists

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

**debian/tests/control:**
- Field name completions for autopkgtest control fields (Tests, Test-Command, Depends, Restrictions, Features, Classes, Tests-Directory, Architecture)
- Package name completions for the Depends field using the system package cache, including the `@`, `@builddeps@`, and `@recommends@` substitution variables
- Value completions for Restrictions and Features (from the autopkgtest/DEP-8 spec) and architecture fields
- Test script completions for the Tests field (executable files in the tests directory, respecting Tests-Directory)
- Directory completions for the Tests-Directory field

**debian/upstream/metadata:**
- Field name completions for all DEP-12 fields (Repository, Bug-Database, Contact, etc.)

**debian/rules:**
- Target name completions for standard Debian Policy targets (clean, build, binary, etc.) and debhelper override/execute targets
- Variable name completions for common build variables (DEB_BUILD_OPTIONS, DEB_HOST_MULTIARCH, etc.)
- Excludes already-defined targets from completions

**debian/patches/series:**
- Patch name completions based on files present in the `debian/patches/` directory
- Package name completions for patch entries, excluding already listed patches
- Option value completions for patch application flags (`-p0`, `-p1`, `-p2`, etc.)

**debian/source/lintian-overrides and debian/.lintian-overrides:**
- Package name completions from `debian/control` (source and binary package names)
- Architecture completions from the known architecture list, including negations (e.g. `!amd64`)
- Package type completions (`source`, `binary`, `udeb`)
- Lintian tag name completions from `lintian-explain-tags`

**debian/dirs:**
- Common directory prefix completions (`usr/share/`, `usr/lib/`, `usr/bin/`, `etc/`, `var/lib/`, etc.)
- Excludes already-listed directories from suggestions

### Diagnostics

- Field casing validation (e.g. `source` instead of `Source`)
- Parse error reporting with position information

**debian/dirs:**
- Duplicate entries -> warning

### Code Actions

- **Fix field casing** - automatically correct field names to canonical casing
- **Wrap and sort** - wrap long fields to 79 characters and sort dependency lists (control and copyright files)
- **Add changelog entry** - create a new changelog entry with incremented version, UNRELEASED distribution, and auto-populated maintainer
- **Mark for upload** - replace UNRELEASED with the target distribution
- **Fix dirs issues** - remove duplicate directory entries

### Hover

**debian/tests/control:**
- Field descriptions for autopkgtest control fields

**debian/source/lintian-overrides and debian/.lintian-overrides:**
- Lintian tag descriptions fetched from `lintian-explain-tags`
- Package names show whether the package is defined in `debian/control` or not found
- Architecture restrictions show the architecture name, with a note for negations (e.g. `!amd64` excludes `amd64`)
- Package type keywords (`source`, `binary`, `udeb`) show a short description

### Go to Definition

**debian/tests/control:**
- Test names in the `Tests` field jump to the corresponding test script in the tests directory (respecting `Tests-Directory`)
- Package names in relationship fields (`Depends`) jump to the matching binary package paragraph in `debian/control`
- Paths in the `Tests-Directory` field jump to the directory on disk

**debian/source/lintian-overrides and debian/.lintian-overrides:**
- Package names jump to the matching `Package:` or `Source:` paragraph in `debian/control`

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
(deb822 format) files. debian/dirs entries are sorted alphabetically, with blank lines
removed.

### Semantic Highlighting

Custom token types for syntax highlighting of Debian-specific constructs:
- Control/copyright/watch/tests-control/upstream-metadata/source-options/rules files: field names, unknown fields, values, comments
- Changelog files: package name, version, distribution, urgency, maintainer, timestamp
- Lintian-overrides files: lintian tag names, package names, package types, architecture restrictions, info text, comments

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
        \ 'allowlist': ['debcontrol', 'debcopyright', 'debchangelog', 'debsources', 'debsourceoptions', 'debwatch', 'debupstream', 'autopkgtest', 'debrules', 'debpatches', 'debdirs'],
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
  autocmd BufNewFile,BufRead */debian/patches/series setfiletype debpatches
  autocmd BufNewFile,BufRead */debian/dirs setfiletype debdirs
  autocmd BufNewFile,BufRead */debian/*.dirs setfiletype debdirs
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
    '*/debian/patches/series',
    '*/debian/dirs',
    '*/debian/*.dirs',
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
