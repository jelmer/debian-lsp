use super::Indexer;
use std::fs;
use tempfile::tempdir;

fn write_tree(dir: &std::path::Path) {
    let debian = dir.join("debian");
    fs::create_dir_all(&debian).unwrap();
    fs::write(
        debian.join("changelog"),
        "hello (2.10-3) unstable; urgency=medium\n\n  \
        * Fix bug. (Closes: #777111)\n  \
        * Fix another. (LP: #2002003)\n  \
        * Fix security issue (CVE-2024-12345).\n  \
        * d/control: Add a dependency.\n  \
        * d/patches/fix-segfault.patch: Refresh.\n  \
        * Drop obsolete patch fix-segfault.patch.\n\n \
        -- Test User <test@example.org>  Tue, 27 May 2026 12:00:00 +0000\n",
    )
    .unwrap();
    fs::write(
        debian.join("control"),
        "Source: hello\n\
        Maintainer: Test User <test@example.org>\n\
        Build-Depends: debhelper-compat (= 13), pytest <!nocheck>\n\n\
        Package: hello\n\
        Architecture: any\n\
        Depends: libfoo1\n\
        Description: example\n short\n",
    )
    .unwrap();
    fs::write(
        debian.join("copyright"),
        "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\n\
        Files: *\n\
        Copyright: 2026 Test User\n\
        License: GPL-2+\n\n\
        License: GPL-2+\n full text here\n",
    )
    .unwrap();
    fs::write(
        debian.join("watch"),
        "version=4\nhttps://example.org/hello/ hello-(.+)\\.tar\\.gz\n",
    )
    .unwrap();
    fs::create_dir_all(debian.join("upstream")).unwrap();
    fs::write(
        debian.join("upstream").join("metadata"),
        "Repository: https://github.com/example/hello\nBug-Database: https://github.com/example/hello/issues\n",
    )
    .unwrap();
    fs::create_dir_all(debian.join("source")).unwrap();
    fs::write(debian.join("source").join("format"), "3.0 (quilt)\n").unwrap();
    fs::write(
        debian.join("rules"),
        "#!/usr/bin/make -f\nDEB_BUILD_OPTIONS = nocheck\n\n%:\n\tdh $@\n\noverride_dh_auto_test:\n\tdh_auto_test --no-act\n",
    )
    .unwrap();
    let patches = debian.join("patches");
    fs::create_dir_all(&patches).unwrap();
    fs::write(patches.join("series"), "fix-segfault.patch\n").unwrap();
    fs::write(
        patches.join("fix-segfault.patch"),
        "From: Jane <jane@example.org>\nSubject: Fix segfault\nBug-Debian: https://bugs.debian.org/123456\n\n--- a/foo\n+++ b/foo\n",
    )
    .unwrap();
    fs::create_dir_all(debian.join("tests")).unwrap();
    fs::write(
        debian.join("tests").join("control"),
        "Tests: smoke\nDepends: @, python3-foo\nRestrictions: needs-root\n",
    )
    .unwrap();
    fs::write(
        debian.join("debcargo.toml"),
        "overlay = \".\"\n\n[source]\nhomepage = \"https://example.org\"\n\n[packages.lib]\nsummary = \"Example\"\n",
    )
    .unwrap();
}

#[test]
fn full_tree_round_trip() {
    let dir = tempdir().unwrap();
    write_tree(dir.path());

    let index = Indexer::new(dir.path()).build();

    let paths: Vec<&str> = index
        .documents
        .iter()
        .map(|d| d.relative_path.as_str())
        .collect();
    assert!(paths.contains(&"debian/changelog"));
    assert!(paths.contains(&"debian/control"));
    assert!(paths.contains(&"debian/copyright"));
    assert!(paths.contains(&"debian/watch"));
    assert!(paths.contains(&"debian/upstream/metadata"));
    assert!(paths.contains(&"debian/source/format"));
    assert!(paths.contains(&"debian/rules"));
    assert!(paths.contains(&"debian/patches/series"));
    assert!(paths.contains(&"debian/patches/fix-segfault.patch"));
    assert!(paths.contains(&"debian/tests/control"));
    assert!(paths.contains(&"debian/debcargo.toml"));

    let meta = index.metadata.as_ref().expect("metadata set");
    assert_eq!(meta.tool_info.name, "debian-lsp");
    assert!(!meta.tool_info.version.is_empty());

    // Every document embeds its own source text so the index is self-contained.
    let control_text = index
        .documents
        .iter()
        .find(|d| d.relative_path == "debian/control")
        .map(|d| d.text.as_str())
        .expect("control document");
    assert!(control_text.contains("Source: hello"));
    assert!(
        index.documents.iter().all(|d| !d.text.is_empty()),
        "all documents should embed their text"
    );

    let ext_syms: Vec<&str> = index
        .external_symbols
        .iter()
        .map(|s| s.symbol.as_str())
        .collect();
    assert!(
        ext_syms.iter().any(|s| s.contains("debhelper-compat")),
        "ext = {ext_syms:?}"
    );
    assert!(
        ext_syms.iter().any(|s| s.contains("libfoo1")),
        "ext = {ext_syms:?}"
    );
    assert!(
        ext_syms.iter().any(|s| s.contains("python3-foo")),
        "ext = {ext_syms:?}"
    );

    // Cross-package vocabularies are emitted as documented external symbols.
    let nocheck = index
        .external_symbols
        .iter()
        .find(|s| s.symbol.contains("build-profile") && s.symbol.contains("nocheck"))
        .expect("build-profile external symbol");
    assert_eq!(nocheck.documentation, vec!["Skip test suites"]);

    let needs_root = index
        .external_symbols
        .iter()
        .find(|s| s.symbol.contains("autopkgtest-restriction") && s.symbol.contains("needs-root"))
        .expect("restriction external symbol");
    assert_eq!(needs_root.documentation, vec!["Test must be run as root"]);

    // The changelog's Closes bug is an external symbol with static link
    // documentation (live BTS enrichment happens later, only when online).
    let bug = index
        .external_symbols
        .iter()
        .find(|s| s.symbol == super::symbols::bts_bug("777111"))
        .expect("bug external symbol");
    assert_eq!(
        bug.documentation,
        vec!["**[Debian Bug #777111](https://bugs.debian.org/777111)**"]
    );

    // Launchpad bugs are indexed the same way, with their own static link.
    let lp_bug = index
        .external_symbols
        .iter()
        .find(|s| s.symbol == super::symbols::lp_bug("2002003"))
        .expect("launchpad bug external symbol");
    assert_eq!(
        lp_bug.documentation,
        vec!["**[Launchpad Bug #2002003](https://bugs.launchpad.net/bugs/2002003)**"]
    );

    // CVEs are indexed the same way, with a static link to the Security Tracker.
    let cve = index
        .external_symbols
        .iter()
        .find(|s| s.symbol == super::symbols::cve("CVE-2024-12345"))
        .expect("cve external symbol");
    assert_eq!(
        cve.documentation,
        vec!["**[CVE-2024-12345](https://security-tracker.debian.org/tracker/CVE-2024-12345)**"]
    );

    // Relationship edges are assembled into the per-document symbols. The
    // `hello` binary package references the `hello` source package.
    let control_doc = index
        .documents
        .iter()
        .find(|d| d.relative_path == "debian/control")
        .expect("control document");
    let bin_sym = control_doc
        .symbols
        .iter()
        .find(|s| s.symbol == super::symbols::binary_package("hello", Some("2.10-3"), "hello"))
        .expect("binary symbol info");
    assert!(
        bin_sym.relationships.iter().any(|r| r.is_reference
            && r.symbol == super::symbols::source_package("hello", Some("2.10-3"))),
        "expected binary->source relationship, got {:?}",
        bin_sym.relationships
    );

    // Syntax-highlighting occurrences (symbol-less, with a syntax_kind) are
    // present across the indexed documents.
    let unspecified = scip::types::SyntaxKind::UnspecifiedSyntaxKind.into();
    let has_highlight = |path: &str| {
        index
            .documents
            .iter()
            .find(|d| d.relative_path == path)
            .map(|d| {
                d.occurrences
                    .iter()
                    .any(|o| o.symbol.is_empty() && o.syntax_kind != unspecified)
            })
            .unwrap_or(false)
    };
    for path in [
        "debian/control",
        "debian/copyright",
        "debian/rules",
        "debian/changelog",
        "debian/watch",
    ] {
        assert!(
            has_highlight(path),
            "expected highlight occurrences in {path}"
        );
    }

    // autopkgtest test definitions carry the Test role.
    let tests_doc = index
        .documents
        .iter()
        .find(|d| d.relative_path == "debian/tests/control")
        .expect("tests/control document");
    assert!(
        tests_doc.occurrences.iter().any(|o| {
            (o.symbol_roles & scip::types::SymbolRole::Test as i32) != 0
                && o.symbol.contains("/tests/smoke")
        }),
        "expected a Test-role occurrence for the smoke test"
    );

    let out = dir.path().join("index.scip");
    scip::write_message_to_file(&out, index).expect("write");
    let bytes = fs::read(&out).unwrap();
    assert!(!bytes.is_empty());
}

#[test]
fn changelog_file_mentions_resolve_cross_file() {
    let dir = tempdir().unwrap();
    write_tree(dir.path());

    let index = Indexer::new(dir.path()).build();

    let changelog = index
        .documents
        .iter()
        .find(|d| d.relative_path == "debian/changelog")
        .expect("changelog document");

    // The changelog references the `debian_file` symbol for each mentioned
    // file, scoped to the topmost source/version.
    for path in ["debian/control", "debian/patches/fix-segfault.patch"] {
        let sym = super::symbols::debian_file("hello", Some("2.10-3"), path);
        assert!(
            changelog.occurrences.iter().any(|o| o.symbol == sym
                && (o.symbol_roles & scip::types::SymbolRole::Definition as i32) == 0),
            "expected changelog reference to {path}"
        );

        // ...and the referenced document defines that same symbol, so the
        // reference resolves to it.
        let target = index
            .documents
            .iter()
            .find(|d| d.relative_path == path)
            .unwrap_or_else(|| panic!("missing document {path}"));
        assert!(
            target.occurrences.iter().any(|o| o.symbol == sym
                && (o.symbol_roles & scip::types::SymbolRole::Definition as i32) != 0),
            "expected {path} to define its debian_file symbol"
        );
    }
}

#[test]
fn records_tool_info_arguments() {
    let dir = tempdir().unwrap();
    write_tree(dir.path());

    let index = Indexer::new(dir.path())
        .with_arguments(vec!["--offline".to_owned()])
        .build();

    let meta = index.metadata.as_ref().expect("metadata set");
    assert_eq!(meta.tool_info.arguments, vec!["--offline".to_owned()]);
}
