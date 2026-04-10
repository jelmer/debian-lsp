# Helix

Add the following to your Helix language configuration file:

- **Linux / Mac**: `~/.config/helix/languages.toml`
- **Windows**: `%AppData%\helix\languages.toml`

> **Note:** This goes in `languages.toml`, not `config.toml`.

If `debian-lsp` is not on your `PATH`, replace `command = "debian-lsp"` with
the full path to the binary (e.g. `command = "/path/to/debian-lsp"`). 

Add the following to `languages.toml`:

```toml
[language-server.debian-lsp]
command = "debian-lsp"

[[language]]
name = "debcontrol"
grammar = "debian"
scope = "text.debian.control"
file-types = [{ glob = "debian/control" }]
language-servers = ["debian-lsp"]
comment-tokens = "#"

[[language]]
name = "debcopyright"
scope = "text.debian.copyright"
file-types = [{ glob = "debian/copyright" }]
language-servers = ["debian-lsp"]
comment-tokens = "#"

[[language]]
name = "debwatch"
scope = "text.debian.watch"
file-types = [{ glob = "debian/watch" }]
language-servers = ["debian-lsp"]
comment-tokens = "#"

[[language]]
name = "debchangelog"
scope = "text.debian.changelog"
file-types = [
  { glob = "debian/changelog" },
  { glob = "debian/changelog.dch" }
]
language-servers = ["debian-lsp"]

[[language]]
name = "debsources"
scope = "text.debian.source.format"
file-types = [{ glob = "debian/source/format" }]
language-servers = ["debian-lsp"]
comment-tokens = "#"

[[language]]
name = "debsourceoptions"
scope = "text.debian.source.options"
file-types = [
  { glob = "debian/source/options" },
  { glob = "debian/source/local-options" }
]
language-servers = ["debian-lsp"]
comment-tokens = "#"

[[language]]
name = "autopkgtest"
scope = "text.debian.tests.control"
file-types = [{ glob = "debian/tests/control" }]
language-servers = ["debian-lsp"]
comment-tokens = "#"

[[language]]
name = "debupstream"
scope = "text.debian.upstream"
file-types = [{ glob = "debian/upstream/metadata" }]
language-servers = ["debian-lsp"]
comment-tokens = "#"

[[language]]
name = "debrules"
scope = "text.debian.rules"
file-types = [{ glob = "debian/rules" }]
language-servers = ["debian-lsp"]
comment-tokens = "#"

[[language]]
name = "debseries"
scope = "text.debian.series"
file-types = [{ glob = "debian/patches/series"}]
language-servers = ["debian-lsp"]
comment-tokens = "#"


[[language]]
name = "debian"
language-servers = ["debian-lsp"]
```
