# Publishing

## Automated Publishing

Create a git tag and push it:

```bash
git tag v0.1.4
git push origin v0.1.4
```

The GitHub Actions workflow will build and publish the extension.

## Registry Setup

Add these as GitHub repository secrets:

- `OPENVSX_TOKEN` - Get from https://open-vsx.org/user-settings/tokens (sign in with GitHub)
- `VSCE_TOKEN` - Get from Azure DevOps (optional, for VS Code Marketplace)

The workflow publishes to whichever registries have tokens configured.

## Manual Publishing

```bash
npm run package
npx ovsx publish -p YOUR_TOKEN      # Open VSX
npx vsce publish -p YOUR_TOKEN      # VS Code Marketplace
```

## Manual Installation

Users can install the `.vsix` from GitHub Releases:

```bash
code --install-extension vscode-debian-0.1.4.vsix
```
