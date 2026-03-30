# Debian Language Support for VS Code

Language Server Protocol extension for Debian package files, including:
- `debian/control` - Package control files
- `debian/copyright` - DEP-5 copyright files
- `debian/watch` - Upstream watch files
- `debian/source/options` - dpkg-source options files
- `debian/source/local-options` - Local dpkg-source options files
- `debian/tests/control` - Autopkgtest control files

## Features

- **Field name completion**: Intelligent completion for Debian control file fields
- **Package name suggestions**: Common package name suggestions for dependencies
- **Diagnostics**: Real-time validation of field names and syntax
- **Quick fixes**: Automatic corrections for common issues like incorrect field casing
- **Syntax highlighting**: Support for Debian control, copyright, and watch files

## Requirements

The `debian-lsp` language server binary is bundled with the extension for supported platforms (Linux x64/arm64, macOS x64/arm64). No separate installation is needed.

If you are on an unsupported platform or want to use a different build of `debian-lsp`, you can install it manually and configure the path in settings (see Configuration below).

## Configuration

This extension contributes the following settings:

* `debian.enable`: Enable/disable the Debian language server (default: `true`)
* `debian.serverPath`: Path to the debian-lsp executable. Override this to use a custom build instead of the bundled binary (default: `"debian-lsp"`)
* `debian.trace.server`: Trace communication between VS Code and the language server (default: `"off"`)

### Example configuration

Add to your VS Code `settings.json`:

```json
{
  "debian.serverPath": "/usr/local/bin/debian-lsp",
  "debian.trace.server": "verbose"
}
```

### On-Type Formatting

Format-on-type is enabled by default for deb822-based file types (control, copyright, watch, tests/control). This automatically inserts a space after typing `:` at the end of a field name and adds continuation-line indentation when pressing Enter inside a field value.

To disable this, add to your `settings.json`:

```json
{
  "[debcontrol]": { "editor.formatOnType": false },
  "[debcopyright]": { "editor.formatOnType": false },
  "[debwatch]": { "editor.formatOnType": false },
  "[debtestscontrol]": { "editor.formatOnType": false }
}
```

## Usage

Simply open any Debian package file:
- `debian/control`
- `debian/copyright`
- `debian/watch`
- `debian/source/options`
- `debian/source/local-options`
- `debian/tests/control`

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
