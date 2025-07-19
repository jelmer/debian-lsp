# debian-lsp

[![CI](https://github.com/jelmer/debian-lsp/actions/workflows/ci.yml/badge.svg)](https://github.com/jelmer/debian-lsp/actions/workflows/ci.yml)
[![Tests](https://github.com/jelmer/debian-lsp/actions/workflows/test.yml/badge.svg)](https://github.com/jelmer/debian-lsp/actions/workflows/test.yml)

Language Server Protocol implementation for Debian control files.

## Features

- Field name completion for Debian control files
- Common package name suggestions
- Works with debian/control files

## Installation

### Building the LSP server

```bash
cargo build --release
```

The binary will be available at `target/release/debian-lsp`.

### Installing the coc.nvim plugin

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

## Usage

Open any `debian/control` or `control` file in Vim with coc.nvim installed. The LSP will automatically provide completions for:
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