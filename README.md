# debian-lsp

[![CI](https://github.com/jelmer/debian-lsp/actions/workflows/ci.yml/badge.svg)](https://github.com/jelmer/debian-lsp/actions/workflows/ci.yml)
[![Tests](https://github.com/jelmer/debian-lsp/actions/workflows/test.yml/badge.svg)](https://github.com/jelmer/debian-lsp/actions/workflows/test.yml)

Language Server Protocol implementation for Debian packaging files.

## Supported Files

Most files under `debian/` are supported, including `control`, `copyright`,
`changelog`, `watch`, `rules`, `source/format`, `source/options`,
`source/local-options`, `tests/control`, `upstream/metadata`, `patches/series`,
`conffiles`, `lintian-overrides`, and their per-package variants.

## Features

The server implements the standard LSP surface across these files:

- **Completions** for field names, package names (from the system package
  cache and `debian/control`), and enumerated values (sections, distributions,
  architectures, licenses, dpkg-source options, autopkgtest restrictions,
  lintian tags, etc.)
- **Diagnostics** for parse errors, field casing, and file-specific problems
  (invalid paths and flags in `conffiles`, duplicate entries, and similar)
- **Code actions** including fix field casing, wrap-and-sort, add changelog
  entry, mark for upload, and fixes for `conffiles` issues
- **Hover** with field descriptions, lintian tag explanations (via
  `lintian-explain-tags`), and context for architectures and package types
- **Go to definition** from test names, package references, and directory
  paths to their targets in the source tree
- **Inlay hints** for archive versions, virtual package providers,
  substitution variables, and distribution-to-suite mappings
- **Code lenses** on `Standards-Version`, `debhelper-compat`, and `Vcs-Git`
  in `debian/control`
- **Document symbols** for paragraphs and changelog entries
- **Folding ranges** for deb822 paragraphs and changelog entries
- **Document formatting** (wrap-and-sort) for deb822 files
- **Semantic highlighting** with Debian-specific token types
- **On-type formatting** for deb822 files (space after `:`, continuation-line
  indentation on Enter)

On-type formatting requires the editor to have format-on-type enabled:

- **VS Code**: enabled by default via the extension
- **coc.nvim**: set `"coc.preferences.formatOnType": true`
- **Native Neovim LSP**: pass `on_type_formatting = true` in client capabilities
- **ALE**: not supported

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
        \ 'allowlist': ['debcontrol', 'debcopyright', 'debchangelog', 'debsources', 'debsourceoptions', 'debwatch', 'debupstream', 'autopkgtest', 'debrules', 'debpatches', 'debconffiles'],
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
  autocmd BufNewFile,BufRead */debian/conffiles setfiletype debconffiles
  autocmd BufNewFile,BufRead */debian/*.conffiles setfiletype debconffiles
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
    '*/debian/conffiles',
    '*/debian/*.conffiles',
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
