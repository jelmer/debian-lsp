---@brief
---
--- https://github.com/jelmer/debian-lsp
---
--- Language Server Protocol implementation for Debian packaging files.
---
--- Supports debian/control, debian/copyright, debian/changelog, debian/watch,
--- debian/source/format, debian/source/options, debian/source/local-options,
--- debian/tests/control, and debian/upstream/metadata.
---
--- Features include completions, diagnostics, code actions (wrap and sort,
--- fix field casing, add changelog entry), on-type formatting, folding ranges,
--- inlay hints, and semantic highlighting.
---
--- `debian-lsp` can be installed via `cargo`:
--- ```sh
--- cargo install debian-lsp
--- ```
---
--- To enable inlay hints (e.g. distribution -> suite mapping in changelog):
--- ```lua
--- vim.lsp.inlay_hint.enable()
--- ```
---
--- To enable on-type formatting (auto-insert space after `:` and continuation
--- line indentation):
--- ```lua
--- vim.lsp.on_type_formatting.enable()
--- ```
---
--- To use LSP-based folding:
--- ```lua
--- vim.o.foldmethod = 'expr'
--- vim.o.foldexpr = 'v:lua.vim.lsp.foldexpr()'
--- ```

---@type vim.lsp.Config
return {
  cmd = { 'debian-lsp' },
  filetypes = { 'debcontrol', 'debcopyright', 'debchangelog', 'debsources', 'debsourceoptions', 'debwatch', 'debupstream', 'autopkgtest' },
  root_markers = { 'debian/control', 'debian' },
}
