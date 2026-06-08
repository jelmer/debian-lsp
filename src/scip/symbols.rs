//! Construction of SCIP symbol identifiers for Debian entities.
//!
//! All symbols use the `scip-debian` scheme. The protobuf `Package` field
//! identifies the source package (`manager="debian"`, `name=<src>`,
//! `version=<changelog-version-or-empty>`).
//!
//! Descriptor layout:
//!
//! - Source package: `<src>` with suffix `Namespace`.
//! - Binary package: `<src>/<bin>` — namespace then `Type`.
//! - Field on a stanza: `<src>/<bin?>/<field>` — terminated by `Term`.
//! - Changelog version: `<src>/changelog/<version>` — `Namespace/Meta`.
//! - License short-name (DEP-5): `<src>/license/<short>` — `Namespace/Type`.
//!
//! External symbols (a referenced source/binary defined in a different index)
//! use the same scheme but with `version=""` in the `Package`, so they remain
//! resolvable across archive snapshots.

use scip::symbol::format_symbol;
use scip::types::descriptor::Suffix;
use scip::types::{Descriptor, Package, Relationship, Symbol};

/// Scheme used for all symbols emitted by `scip-debian`.
pub const SCHEME: &str = "scip-debian";

/// Scheme used for Debian BTS bug references.
pub const BTS_SCHEME: &str = "scip-debian-bts";

/// Scheme used for Launchpad bug references.
pub const LP_SCHEME: &str = "scip-launchpad-bug";

/// Scheme used for CVE references.
pub const CVE_SCHEME: &str = "scip-cve";

/// Manager string identifying Debian source packages.
pub const MANAGER: &str = "debian";

/// Manager string identifying the Debian BTS.
pub const BTS_MANAGER: &str = "debian-bts";

/// Manager string identifying Launchpad.
pub const LP_MANAGER: &str = "launchpad";

/// Manager string identifying CVE references.
pub const CVE_MANAGER: &str = "cve";

/// Build a `Descriptor` with the given name and suffix.
fn desc(name: &str, suffix: Suffix) -> Descriptor {
    Descriptor {
        name: name.to_owned(),
        suffix: suffix.into(),
        ..Default::default()
    }
}

/// Build a `Package` for a source package, with an optional version.
fn pkg(source: &str, version: Option<&str>) -> Package {
    Package {
        manager: MANAGER.to_owned(),
        name: source.to_owned(),
        version: version.unwrap_or("").to_owned(),
        ..Default::default()
    }
}

/// Format a [`Symbol`] into its canonical string form.
///
/// Panics if the symbol contains characters that cannot be escaped — this
/// should not happen for any input produced by this crate.
fn fmt(sym: Symbol) -> String {
    format_symbol(sym)
}

/// Symbol for the source package itself.
pub fn source_package(name: &str, version: Option<&str>) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(pkg(name, version)).into(),
        descriptors: vec![desc(name, Suffix::Namespace)],
        ..Default::default()
    })
}

/// Symbol for a binary package, scoped to its source.
pub fn binary_package(source: &str, version: Option<&str>, binary: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(pkg(source, version)).into(),
        descriptors: vec![desc(source, Suffix::Namespace), desc(binary, Suffix::Type)],
        ..Default::default()
    })
}

/// Symbol for a field on the source stanza.
pub fn source_field(source: &str, version: Option<&str>, field: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(pkg(source, version)).into(),
        descriptors: vec![desc(source, Suffix::Namespace), desc(field, Suffix::Term)],
        ..Default::default()
    })
}

/// Symbol for a field on a binary stanza.
pub fn binary_field(source: &str, version: Option<&str>, binary: &str, field: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(pkg(source, version)).into(),
        descriptors: vec![
            desc(source, Suffix::Namespace),
            desc(binary, Suffix::Type),
            desc(field, Suffix::Term),
        ],
        ..Default::default()
    })
}

/// Symbol for a single changelog entry version.
pub fn changelog_version(source: &str, version: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(pkg(source, Some(version))).into(),
        descriptors: vec![
            desc(source, Suffix::Namespace),
            desc("changelog", Suffix::Namespace),
            desc(version, Suffix::Meta),
        ],
        ..Default::default()
    })
}

/// Symbol for a license short-name in a DEP-5 copyright file.
pub fn license(source: &str, version: Option<&str>, short_name: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(pkg(source, version)).into(),
        descriptors: vec![
            desc(source, Suffix::Namespace),
            desc("license", Suffix::Namespace),
            desc(short_name, Suffix::Type),
        ],
        ..Default::default()
    })
}

/// Symbol for an external reference to another Debian binary package.
///
/// The package version is left empty so the reference resolves to the
/// current version of that package in whichever index aggregates it.
pub fn external_binary(name: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(pkg(name, None)).into(),
        descriptors: vec![desc(name, Suffix::Namespace), desc(name, Suffix::Type)],
        ..Default::default()
    })
}

/// Symbol for a source-package upstream metadata field (`debian/upstream/metadata`).
pub fn upstream_metadata_field(source: &str, version: Option<&str>, key: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(pkg(source, version)).into(),
        descriptors: vec![
            desc(source, Suffix::Namespace),
            desc("upstream", Suffix::Namespace),
            desc(key, Suffix::Term),
        ],
        ..Default::default()
    })
}

/// Symbol for a key in `debian/debcargo.toml`.
///
/// `scope` names the table the key belongs to (`""` for the top level,
/// `"source"` for `[source]`, or a package name for `[packages.NAME]`), so
/// keys with the same name in different tables get distinct symbols.
pub fn debcargo_key(source: &str, version: Option<&str>, scope: &str, key: &str) -> String {
    let mut descriptors = vec![
        desc(source, Suffix::Namespace),
        desc("debcargo", Suffix::Namespace),
    ];
    if !scope.is_empty() {
        descriptors.push(desc(scope, Suffix::Namespace));
    }
    descriptors.push(desc(key, Suffix::Term));
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(pkg(source, version)).into(),
        descriptors,
        ..Default::default()
    })
}

/// Symbol for a `[packages.NAME]` package name in `debian/debcargo.toml`.
pub fn debcargo_package(source: &str, version: Option<&str>, name: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(pkg(source, version)).into(),
        descriptors: vec![
            desc(source, Suffix::Namespace),
            desc("debcargo", Suffix::Namespace),
            desc("packages", Suffix::Namespace),
            desc(name, Suffix::Type),
        ],
        ..Default::default()
    })
}

/// Symbol for a `debian/source/format` value.
///
/// Cross-package: the same format string maps to the same symbol across the
/// archive, enabling searches like "all packages using `3.0 (quilt)`".
pub fn source_format(format: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(Package {
            manager: MANAGER.to_owned(),
            name: "source-format".to_owned(),
            ..Default::default()
        })
        .into(),
        descriptors: vec![desc(format, Suffix::Type)],
        ..Default::default()
    })
}

/// Symbol for a web URL referenced from packaging metadata (a `Homepage`,
/// `Vcs-Browser`, copyright `Format`, upstream `Repository`, etc.).
///
/// Cross-package and keyed on the URL itself, so the same URL collects under one
/// symbol across the archive and the SCIP consumer renders it as a navigable,
/// clickable link via the markdown documentation attached to the symbol.
pub fn web_url(url: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        descriptors: vec![desc("url", Suffix::Namespace), desc(url, Suffix::Term)],
        ..Default::default()
    })
}

/// Markdown documentation for a [`web_url`] symbol: a clickable link to the URL.
pub fn web_url_doc(url: &str) -> String {
    format!("[{url}]({url})")
}

/// Symbol for a build profile name (e.g. `nocheck`, `noudeb`).
///
/// Cross-package, so all uses of a given profile collect under one symbol. A
/// build profile belongs to no package, so the package field is left empty and
/// the kind is carried by a leading `build-profile` namespace descriptor.
pub fn build_profile(name: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        descriptors: vec![
            desc("build-profile", Suffix::Namespace),
            desc(name, Suffix::Type),
        ],
        ..Default::default()
    })
}

/// Symbol for a quilt patch in `debian/patches/`.
pub fn patch(source: &str, version: Option<&str>, patch_name: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(pkg(source, version)).into(),
        descriptors: vec![
            desc(source, Suffix::Namespace),
            desc("patches", Suffix::Namespace),
            desc(patch_name, Suffix::Type),
        ],
        ..Default::default()
    })
}

/// Symbol for a `Files:` paragraph glob in `debian/copyright`.
pub fn copyright_files_glob(source: &str, version: Option<&str>, glob: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(pkg(source, version)).into(),
        descriptors: vec![
            desc(source, Suffix::Namespace),
            desc("copyright", Suffix::Namespace),
            desc(glob, Suffix::Meta),
        ],
        ..Default::default()
    })
}

/// Symbol for a person (identity), keyed by email address.
///
/// Cross-package: a maintainer's symbol is the same across every package they
/// touch, enabling "all packages by X" searches.
pub fn identity(email: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        // A person belongs to no package, so leave the package field empty and
        // carry the kind in a leading `identity` namespace descriptor.
        descriptors: vec![
            desc("identity", Suffix::Namespace),
            desc(email, Suffix::Term),
        ],
        ..Default::default()
    })
}

/// Symbol for a target in `debian/rules`, scoped to its source package.
pub fn rules_target(source: &str, version: Option<&str>, target: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(pkg(source, version)).into(),
        descriptors: vec![
            desc(source, Suffix::Namespace),
            desc("rules", Suffix::Namespace),
            desc(target, Suffix::Method),
        ],
        ..Default::default()
    })
}

/// Symbol for a variable assignment in `debian/rules`, scoped to its source package.
pub fn rules_variable(source: &str, version: Option<&str>, name: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(pkg(source, version)).into(),
        descriptors: vec![
            desc(source, Suffix::Namespace),
            desc("rules", Suffix::Namespace),
            desc(name, Suffix::Term),
        ],
        ..Default::default()
    })
}

/// Symbol for a debhelper command (e.g. `dh_install`).
///
/// Cross-package: every reference to `dh_install` across the archive resolves
/// to the same symbol.
pub fn debhelper_command(name: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(Package {
            manager: MANAGER.to_owned(),
            name: "debhelper".to_owned(),
            ..Default::default()
        })
        .into(),
        descriptors: vec![desc(name, Suffix::Method)],
        ..Default::default()
    })
}

/// Symbol for an upstream file path referenced from a patch's diff body.
pub fn upstream_path(source: &str, version: Option<&str>, path: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(pkg(source, version)).into(),
        descriptors: vec![
            desc(source, Suffix::Namespace),
            desc("upstream", Suffix::Namespace),
            desc(path, Suffix::Meta),
        ],
        ..Default::default()
    })
}

/// Symbol for an autopkgtest test name, scoped to its source package.
///
/// Each name in a `Tests:` field of `debian/tests/control` corresponds to a
/// test script under the tests directory.
pub fn autopkgtest_test(source: &str, version: Option<&str>, name: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(pkg(source, version)).into(),
        descriptors: vec![
            desc(source, Suffix::Namespace),
            desc("tests", Suffix::Namespace),
            desc(name, Suffix::Method),
        ],
        ..Default::default()
    })
}

/// Symbol for an autopkgtest restriction (e.g. `needs-root`, `allow-stderr`).
///
/// Cross-package: every use of a given restriction collects under one symbol.
pub fn autopkgtest_restriction(name: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(Package {
            manager: MANAGER.to_owned(),
            name: "autopkgtest-restriction".to_owned(),
            ..Default::default()
        })
        .into(),
        descriptors: vec![desc(name, Suffix::Type)],
        ..Default::default()
    })
}

/// Symbol for an autopkgtest feature (e.g. `test-name`).
///
/// Cross-package, like [`autopkgtest_restriction`].
pub fn autopkgtest_feature(name: &str) -> String {
    fmt(Symbol {
        scheme: SCHEME.to_owned(),
        package: Some(Package {
            manager: MANAGER.to_owned(),
            name: "autopkgtest-feature".to_owned(),
            ..Default::default()
        })
        .into(),
        descriptors: vec![desc(name, Suffix::Type)],
        ..Default::default()
    })
}

/// Symbol for a Debian BTS bug number.
pub fn bts_bug(number: &str) -> String {
    fmt(Symbol {
        scheme: BTS_SCHEME.to_owned(),
        package: Some(Package {
            manager: BTS_MANAGER.to_owned(),
            ..Default::default()
        })
        .into(),
        descriptors: vec![desc(number, Suffix::Meta)],
        ..Default::default()
    })
}

/// Recover the bug number from a symbol produced by [`bts_bug`].
///
/// Returns `None` for any symbol that is not a Debian BTS bug reference.
pub fn parse_bts_bug(symbol: &str) -> Option<u32> {
    let parsed = scip::symbol::parse_symbol(symbol).ok()?;
    if parsed.scheme != BTS_SCHEME {
        return None;
    }
    parsed.descriptors.first()?.name.parse().ok()
}

/// Static documentation for a Debian BTS bug, used when no live BTS data is
/// available (offline mode, or a lookup that returned nothing).
pub fn bts_bug_static_doc(number: u32) -> String {
    format!("**[Debian Bug #{number}](https://bugs.debian.org/{number})**")
}

/// Symbol for a Launchpad bug number.
pub fn lp_bug(number: &str) -> String {
    fmt(Symbol {
        scheme: LP_SCHEME.to_owned(),
        package: Some(Package {
            manager: LP_MANAGER.to_owned(),
            ..Default::default()
        })
        .into(),
        descriptors: vec![desc(number, Suffix::Meta)],
        ..Default::default()
    })
}

/// Recover the bug number from a symbol produced by [`lp_bug`].
///
/// Returns `None` for any symbol that is not a Launchpad bug reference.
pub fn parse_lp_bug(symbol: &str) -> Option<u32> {
    let parsed = scip::symbol::parse_symbol(symbol).ok()?;
    if parsed.scheme != LP_SCHEME {
        return None;
    }
    parsed.descriptors.first()?.name.parse().ok()
}

/// Static documentation for a Launchpad bug, used when no live data is
/// available (offline mode, the `launchpad` feature disabled, or a lookup that
/// returned nothing).
pub fn lp_bug_static_doc(number: u32) -> String {
    format!("**[Launchpad Bug #{number}](https://bugs.launchpad.net/bugs/{number})**")
}

/// Symbol for a CVE identifier.
///
/// Cross-package: every reference to a given CVE across the archive resolves to
/// the same symbol. CVEs belong to no source package, so the package field is
/// left empty.
pub fn cve(id: &str) -> String {
    fmt(Symbol {
        scheme: CVE_SCHEME.to_owned(),
        package: Some(Package {
            manager: CVE_MANAGER.to_owned(),
            ..Default::default()
        })
        .into(),
        descriptors: vec![desc(id, Suffix::Meta)],
        ..Default::default()
    })
}

/// Recover the CVE identifier from a symbol produced by [`cve`].
///
/// Returns `None` for any symbol that is not a CVE reference.
pub fn parse_cve(symbol: &str) -> Option<String> {
    let parsed = scip::symbol::parse_symbol(symbol).ok()?;
    if parsed.scheme != CVE_SCHEME {
        return None;
    }
    Some(parsed.descriptors.first()?.name.clone())
}

/// Static documentation for a CVE, used when no live security tracker data is
/// available (offline mode, or a lookup that returned nothing).
pub fn cve_static_doc(id: &str) -> String {
    crate::cve::link_markdown(id)
}

/// A [`Relationship`] declaring that the owning symbol is a reference of
/// `target`.
///
/// Use this for "this symbol points at that one" edges where "Find references"
/// on `target` should surface the owner (e.g. a binary package referencing its
/// source package).
pub fn rel_reference(target: String) -> Relationship {
    Relationship {
        symbol: target,
        is_reference: true,
        ..Default::default()
    }
}

/// A [`Relationship`] declaring that the owning symbol implements `target`.
///
/// Use this for "Find implementations" edges, e.g. an `override_dh_*` rules
/// target implementing the corresponding debhelper command.
pub fn rel_implementation(target: String) -> Relationship {
    Relationship {
        symbol: target,
        is_implementation: true,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_package_symbol_round_trips() {
        let s = source_package("hello", Some("2.10-3"));
        let parsed = scip::symbol::parse_symbol(&s).unwrap();
        assert_eq!(parsed.scheme, SCHEME);
        assert_eq!(parsed.package.name, "hello");
        assert_eq!(parsed.package.version, "2.10-3");
        assert_eq!(parsed.descriptors.len(), 1);
        assert_eq!(parsed.descriptors[0].name, "hello");
    }

    #[test]
    fn package_less_symbols_have_no_package() {
        // identity and build_profile belong to no package, so their package
        // field is empty -- they must not look like they're "from" a package.
        for s in [identity("jelmer@debian.org"), build_profile("nocheck")] {
            let parsed = scip::symbol::parse_symbol(&s).unwrap();
            assert!(
                parsed.package.name.is_empty(),
                "expected empty package name in {s}"
            );
        }
        // The descriptor path still distinguishes them and stays stable.
        let id = scip::symbol::parse_symbol(&identity("jelmer@debian.org")).unwrap();
        let names: Vec<_> = id.descriptors.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["identity", "jelmer@debian.org"]);
    }

    #[test]
    fn binary_field_distinct_from_source_field() {
        assert_ne!(
            source_field("hello", None, "Depends"),
            binary_field("hello", None, "hello", "Depends")
        );
    }

    #[test]
    fn bts_bug_parses() {
        let s = bts_bug("123456");
        let parsed = scip::symbol::parse_symbol(&s).unwrap();
        assert_eq!(parsed.scheme, BTS_SCHEME);
        assert_eq!(parsed.descriptors[0].name, "123456");
    }

    #[test]
    fn bts_bug_round_trips_through_parse() {
        assert_eq!(parse_bts_bug(&bts_bug("123456")), Some(123456));
    }

    #[test]
    fn parse_bts_bug_rejects_other_symbols() {
        assert_eq!(parse_bts_bug(&source_package("hello", None)), None);
        assert_eq!(parse_bts_bug("not a symbol"), None);
        // Debian and Launchpad bugs use distinct schemes and don't cross-parse.
        assert_eq!(parse_bts_bug(&lp_bug("123456")), None);
    }

    #[test]
    fn bts_bug_static_doc_links_to_tracker() {
        assert_eq!(
            bts_bug_static_doc(123456),
            "**[Debian Bug #123456](https://bugs.debian.org/123456)**"
        );
    }

    #[test]
    fn lp_bug_round_trips_through_parse() {
        assert_eq!(parse_lp_bug(&lp_bug("2002003")), Some(2002003));
        assert_eq!(parse_lp_bug(&bts_bug("2002003")), None);
    }

    #[test]
    fn lp_bug_static_doc_links_to_tracker() {
        assert_eq!(
            lp_bug_static_doc(2002003),
            "**[Launchpad Bug #2002003](https://bugs.launchpad.net/bugs/2002003)**"
        );
    }

    #[test]
    fn cve_round_trips_through_parse() {
        assert_eq!(
            parse_cve(&cve("CVE-2024-1234")),
            Some("CVE-2024-1234".to_string())
        );
    }

    #[test]
    fn parse_cve_rejects_other_symbols() {
        assert_eq!(parse_cve(&bts_bug("123456")), None);
        assert_eq!(parse_cve(&source_package("hello", None)), None);
        assert_eq!(parse_cve("not a symbol"), None);
    }

    #[test]
    fn cve_belongs_to_no_package() {
        let parsed = scip::symbol::parse_symbol(&cve("CVE-2024-1234")).unwrap();
        assert!(parsed.package.name.is_empty());
    }

    #[test]
    fn cve_static_doc_links_to_tracker() {
        assert_eq!(
            cve_static_doc("CVE-2024-1234"),
            "**[CVE-2024-1234](https://security-tracker.debian.org/tracker/CVE-2024-1234)**"
        );
    }
}
