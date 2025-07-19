# coc-debian

A coc.nvim extension that provides Language Server Protocol support for Debian control files.

## Features

- Auto-completion for Debian control file field names
- Package name suggestions
- Automatic activation for `debian/control` files

## Installation

### Prerequisites

- [coc.nvim](https://github.com/neoclide/coc.nvim) installed in Vim/Neovim
- Node.js and npm
- The debian-lsp server built and available

### Local Installation

1. **Build the debian-lsp server first:**
   ```bash
   cd /path/to/debian-lsp
   cargo build --release
   ```

2. **Install and build the coc extension:**
   ```bash
   cd coc-debian
   npm install
   npm run build
   ```

3. **Install the extension in coc.nvim:**
   ```vim
   :CocInstall file:///absolute/path/to/debian-lsp/coc-debian
   ```
   
   Or alternatively, create a symlink in your coc extensions directory:
   ```bash
   ln -s /absolute/path/to/debian-lsp/coc-debian ~/.config/coc/extensions/node_modules/coc-debian
   ```

4. **Configure the LSP server path:**
   
   Add the following to your coc-settings.json (`:CocConfig` in Vim):
   ```json
   {
     "debian.enable": true,
     "debian.serverPath": "/absolute/path/to/debian-lsp/target/release/debian-lsp"
   }
   ```

### Verify Installation

1. Open a Debian control file (`debian/control` or any file named `control`)
2. Try typing field names and you should see completions
3. Check `:CocList extensions` to see if `coc-debian` is listed and active

## Configuration

Available settings in coc-settings.json:

- `debian.enable` (boolean, default: true) - Enable/disable the extension
- `debian.serverPath` (string, default: "debian-lsp") - Path to the debian-lsp executable

## Development

To work on the extension:

```bash
# Watch for changes and rebuild
npm run watch

# After making changes, restart coc
:CocRestart
```

## Troubleshooting

- **LSP not starting:** Check that the `debian.serverPath` points to the correct executable
- **No completions:** Verify the file is named `control` or is in a `debian/` directory
- **Extension not loading:** Check `:CocList extensions` and look for any error messages