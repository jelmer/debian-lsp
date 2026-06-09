# SCIP output

This module emits a [SCIP](https://github.com/sourcegraph/scip) index for a
Debian packaging tree. The entry point is `indexer::Indexer`, which walks a
`debian/` directory and produces a `scip::types::Index` (write it with
`scip::write_message_to_file`). A consumer such as Sourcegraph, or the
[debian-codegraph](https://github.com/jelmer/debian-codegraph) browser, renders
the index as a navigable, cross-referenced view of the packaging.

Each `debian/` file becomes one SCIP `Document`, holding three kinds of data:

- **symbol occurrences** -- ranges tagged with a symbol string, the unit of
  navigation (go-to-definition, find-references, hover).
- **syntax-highlighting occurrences** -- symbol-less ranges carrying only a
  `syntax_kind`, so a consumer can colour the file (see `highlight.rs`).
- **symbol information** -- per-symbol metadata: kind, display name, documentation
  (markdown), and relationship edges.

Documents embed their own source text, so the index is self-contained.

## Symbols

Every symbol uses one of four schemes:

| scheme | for |
| --- | --- |
| `scip-debian` | packaging entities (packages, fields, files, ...) |
| `scip-debian-bts` | Debian BTS bug references |
| `scip-launchpad-bug` | Launchpad bug references |
| `scip-cve` | CVE references |

`scip-debian` symbols carry a protobuf `Package` (`manager="debian"`,
`name=<src>`, `version`) followed by a descriptor chain that names the entity.

The `version` field encodes the navigation model, so it is worth being precise:

- Entities that *belong to one upload* -- the source package, a control/copyright
  field, a license short-name, a quilt patch, a changelog entry -- are pinned to
  the source's changelog version (`name=dulwich, version=1.2.5-1`). The index
  hosts many versions of a package at once, and pinning keeps each version's
  symbols distinct, so navigating within `1.2.5-1` never lands in `1.2.5-2`.
- A *binary package* is named version-lessly, by the binary name alone. The same
  symbol is the definition at its `Package:` stanza and the reference from every
  other package's relation fields, so a `Depends: foo` resolves to the
  `Package: foo` line that defines it, in whichever version the consuming index
  hosts. `binary_package` works this way; so do `Provides:` entries, which define
  the same symbol for the (often virtual) packages they declare.
- Archive-wide vocabularies carry no package at all (see below).

Putting the version in the `Package` is the spec's intent -- the grammar is
`<manager> <package-name> <version>` -- and matches how other generators behave
(rust-analyzer pins a crate's symbols to its semver). We use the source's Debian
version (`1.2.5-1`).

The descriptor suffix classifies the trailing token, which is also how a consumer
colours it without language-specific knowledge:

- `Namespace` -- a container (a source package, a `file`/`changelog`/`license`
  grouping, a module path).
- `Type` -- a binary package, a license short-name, a debcargo package.
- `Term` -- a field name or a scalar key.
- `Meta` -- a free-form value (a changelog version, a file path, a glob, a bug
  number) that is not an identifier.
- `Method` -- a debhelper command or a rules target.

See `symbols.rs` for the exact construction of each; the main ones:

- **packages** -- `source_package`, `binary_package`. A binary package has a
  relationship edge back to its source.
- **fields** -- `source_field` / `binary_field` (`debian/control`),
  `copyright_field`, `autopkgtest_field`, `watch_field`, `patch_field`,
  `upstream_metadata_field`, `debcargo_key`. Field-name tokens are documented
  with the field's description, so a consumer shows the same hover the editor
  does.
- **values** -- `license` (DEP-5 short-name), `copyright_files_glob`,
  `changelog_version`, `source_format`, `rules_target` / `rules_variable`,
  `debcargo_package`, `autopkgtest_test`, `upstream_path`, `patch`.
- **identities** -- `identity(email)` for a maintainer/uploader, package-less and
  keyed on the email, so the same person collects across the archive.
- **file references** -- `file_ref(path)`, package-less and keyed on a repo-relative
  path. Emitted for a `debian/changelog` mention of another packaging file
  (`d/control`, `d/patches/foo.patch`). It carries no definition; its
  documentation is a markdown link to the relative path, which a consumer reads
  back and resolves within the package to jump to the file.
- **resource links** -- `web_url(url)` for a URL-valued field (`Homepage`,
  `Vcs-Browser`, copyright `Format`, ...). Its documentation is a markdown link
  to the URL.

### Cross-package vocabularies

Some symbols are package-less so the same value collects across the whole
archive, enabling queries like "all packages using `3.0 (quilt)`":
`build_profile`, `debhelper_command`, `source_format`,
`autopkgtest_restriction`, `autopkgtest_feature`, `identity`, `file_ref`,
`web_url`.

### External symbols

Things referenced from a tree but defined elsewhere -- another source package, or
an archive-wide vocabulary (build profiles, autopkgtest restrictions, bug/CVE
references) -- are emitted as index-level `external_symbols` carrying their
documentation, so those references render with hover text rather than bare.

## Per-file indexers

| file | module | notable output |
| --- | --- | --- |
| `debian/changelog` | `changelog.rs` | entry-version defs; maintainer `identity` refs; bug/CVE refs; `file_ref` mentions of other files |
| `debian/control` | `control.rs` | source/binary package defs; documented field names; relation-field refs to other binaries; URL field links |
| `debian/copyright` | `copyright.rs` | DEP-5 `License` defs and `Files` glob defs/refs; documented field names; `Format` URL link |
| `debian/rules` | `rules.rs` | target and variable defs; `debhelper_command` refs |
| `debian/watch` | `watch.rs` | documented field/option names (deb822 v5 and line-based) |
| `debian/upstream/metadata` | `upstream_metadata.rs` | documented field names; URL field links |
| `debian/source/format` | `source_format.rs` | the format value as a cross-package symbol |
| `debian/patches/*` | `patches.rs` | series entries, patch names, patch-header fields, embedded URLs |
| `debian/tests/control` | `autopkgtest.rs` | test names; documented fields; `Restrictions`/`Features` refs |
| `debian/debcargo.toml` | `debcargo.rs` | documented top-level/source/package keys |

`links.rs` is shared: it scans a deb822 document's URL-valued and prose fields
and emits `web_url` link occurrences. `fields.rs` is shared: it documents a
deb822 file's known field-name keys from the same tables the LSP hover uses.

## Bug, CVE and live enrichment

`debian/changelog` (and patch headers) reference bugs and CVEs:
`Closes: #NNN` -> `bts_bug`, `LP: #NNN` -> `lp_bug`, `CVE-YYYY-NNNN` -> `cve`.
Each is emitted with a static markdown link to its tracker. When run online,
`bug_info::attach` upgrades these to a richer live summary (title, status),
reusing the LSP's caches; offline, the static link stands.

## Index metadata

The index records `tool_info` (name `debian-lsp`, version, and the invocation
arguments) and a `project_root` URI, and uses UTF-8 position encoding.
