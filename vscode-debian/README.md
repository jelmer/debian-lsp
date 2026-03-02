# Debian Language Support for VS Code

Language Server Protocol extension for Debian package files, including:
- `debian/control` files
- `debian/copyright` files
- `debian/watch` files

## Features

- **Field name completion**: Intelligent completion for Debian control file fields
- **Package name suggestions**: Common package name suggestions for dependencies
- **Diagnostics**: Real-time validation of field names and syntax
- **Quick fixes**: Automatic corrections for common issues like incorrect field casing
- **Syntax highlighting**: Support for Debian control, copyright, and watch files

## Requirements

The `debian-lsp` language server must be installed and available in your PATH, or you can configure the path to the executable in settings.

### Installing debian-lsp

1. Clone and build the language server:
   ```bash
   git clone https://github.com/jelmer/debian-lsp
   cd debian-lsp
   cargo build --release
   ```

2. Either:
   - Copy `target/release/debian-lsp` to a directory in your PATH, or
   - Configure the path in VS Code settings (see Configuration below)

## Configuration

This extension contributes the following settings:

* `debian.enable`: Enable/disable the Debian language server (default: `true`)
* `debian.serverPath`: Path to the debian-lsp executable (default: `"debian-lsp"`)
* `debian.trace.server`: Trace communication between VS Code and the language server (default: `"off"`)

### Example configuration

Add to your VS Code `settings.json`:

```json
{
  "debian.serverPath": "/usr/local/bin/debian-lsp",
  "debian.trace.server": "verbose"
}
```

## Usage

Simply open any Debian package file:
- `debian/control`
- `debian/copyright`
- `debian/watch`

The extension will automatically activate and provide language features.

## Development

### Building the extension

```bash
npm install
npm run compile
```

### Packaging the extension

```bash
npm run package
```

This creates a `.vsix` file that can be installed in VS Code.

### Installing the extension locally

```bash
code --install-extension vscode-debian-*.vsix
```

## License

Apache-2.0+
