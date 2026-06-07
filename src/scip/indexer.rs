//! Top-level indexer: discover `debian/` files and assemble a SCIP [`Index`].

use super::{
    autopkgtest, changelog, control, copyright, patches, rules, source_format, symbols,
    upstream_metadata, watch,
};
use scip::types::{Index, Metadata, SymbolInformation, ToolInfo};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Build a SCIP index from a Debian source tree.
///
/// The tree is expected to contain a `debian/` subdirectory. Files outside
/// `debian/` are ignored.
pub struct Indexer {
    root: PathBuf,
    project_root: Option<String>,
    arguments: Vec<String>,
}

impl Indexer {
    /// Create a new indexer rooted at `root` (a directory containing `debian/`).
    pub fn new<P: AsRef<Path>>(root: P) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            project_root: None,
            arguments: Vec::new(),
        }
    }

    /// Override the `project_root` URI recorded in the index metadata.
    ///
    /// Defaults to `file://<absolute path to root>`.
    pub fn with_project_root(mut self, project_root: String) -> Self {
        self.project_root = Some(project_root);
        self
    }

    /// Record the invocation arguments in the index metadata's `tool_info`,
    /// so the index documents how it was produced.
    pub fn with_arguments(mut self, arguments: Vec<String>) -> Self {
        self.arguments = arguments;
        self
    }

    /// Walk `debian/` and produce a SCIP [`Index`].
    pub fn build(self) -> Index {
        let debian = self.root.join("debian");
        let mut documents = Vec::new();
        let mut external_binaries: HashSet<String> = HashSet::new();
        let mut build_profiles: HashSet<String> = HashSet::new();
        let mut restrictions: HashSet<String> = HashSet::new();
        let mut features: HashSet<String> = HashSet::new();
        let mut bug_numbers: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();

        // Step 1: changelog first, to learn the source name and current version.
        let changelog_text = std::fs::read_to_string(debian.join("changelog")).ok();
        let (source_name, version) = if let Some(text) = changelog_text.as_deref() {
            let idx = changelog::index(text, "debian/changelog");
            let src = idx.source_name.clone();
            let ver = idx.topmost_version.clone();
            bug_numbers.extend(idx.bug_numbers);
            documents.push(idx.document);
            (src, ver)
        } else {
            (None, None)
        };

        // Step 2: control.
        if let Ok(text) = std::fs::read_to_string(debian.join("control")) {
            let idx = control::index(&text, "debian/control", version.as_deref());
            external_binaries.extend(idx.external_binaries);
            build_profiles.extend(idx.build_profiles);
            documents.push(idx.document);
        }

        // Step 3: copyright.
        if let Ok(text) = std::fs::read_to_string(debian.join("copyright")) {
            let src = source_name.as_deref().unwrap_or("unknown");
            let idx = copyright::index(&text, "debian/copyright", src, version.as_deref());
            documents.push(idx.document);
        }

        let src = source_name.as_deref().unwrap_or("unknown");

        // Step 4: watch.
        if let Ok(text) = std::fs::read_to_string(debian.join("watch")) {
            let idx = watch::index(&text, "debian/watch", src, version.as_deref());
            documents.push(idx.document);
        }

        // Step 5: upstream/metadata.
        if let Ok(text) = std::fs::read_to_string(debian.join("upstream").join("metadata")) {
            let idx = upstream_metadata::index(
                &text,
                "debian/upstream/metadata",
                src,
                version.as_deref(),
            );
            documents.push(idx.document);
        }

        // Step 6: source/format.
        if let Ok(text) = std::fs::read_to_string(debian.join("source").join("format")) {
            let idx = source_format::index(&text, "debian/source/format");
            documents.push(idx.document);
        }

        // Step 7: rules (Makefile).
        if let Ok(text) = std::fs::read_to_string(debian.join("rules")) {
            let idx = rules::index(&text, "debian/rules", src, version.as_deref());
            documents.push(idx.document);
        }

        // Step 8: patches.
        let patches_idx = patches::index(&self.root, src, version.as_deref());
        if let Some(doc) = patches_idx.series_document {
            documents.push(doc);
        }
        documents.extend(patches_idx.patch_documents);

        // Step 9: tests/control (autopkgtest).
        if let Ok(text) = std::fs::read_to_string(debian.join("tests").join("control")) {
            let idx = autopkgtest::index(&text, "debian/tests/control", src, version.as_deref());
            external_binaries.extend(idx.external_binaries);
            restrictions.extend(idx.restrictions);
            features.extend(idx.features);
            documents.push(idx.document);
        }

        // External symbols carry hover information for things referenced from
        // this index but defined elsewhere (other source packages) or drawn
        // from an archive-wide vocabulary (build profiles, autopkgtest
        // restrictions and features).
        let mut external_symbols: Vec<SymbolInformation> = external_binaries
            .iter()
            .map(|name| SymbolInformation {
                symbol: symbols::external_binary(name),
                kind: scip::types::symbol_information::Kind::Package.into(),
                ..Default::default()
            })
            .collect();
        external_symbols.extend(build_profiles.iter().map(|name| {
            SymbolInformation {
                symbol: symbols::build_profile(name),
                kind: scip::types::symbol_information::Kind::Type.into(),
                documentation: crate::control::relation_completion::build_profile_description(name)
                    .map(str::to_owned)
                    .into_iter()
                    .collect(),
                ..Default::default()
            }
        }));
        external_symbols.extend(restrictions.iter().map(|name| {
            SymbolInformation {
                symbol: symbols::autopkgtest_restriction(name),
                kind: scip::types::symbol_information::Kind::Type.into(),
                documentation: crate::tests::fields::restriction_description(name)
                    .map(str::to_owned)
                    .into_iter()
                    .collect(),
                ..Default::default()
            }
        }));
        external_symbols.extend(features.iter().map(|name| {
            SymbolInformation {
                symbol: symbols::autopkgtest_feature(name),
                kind: scip::types::symbol_information::Kind::Type.into(),
                documentation: crate::tests::fields::feature_description(name)
                    .map(str::to_owned)
                    .into_iter()
                    .collect(),
                ..Default::default()
            }
        }));
        // BTS bugs referenced from the changelog. Static documentation (a link
        // to the bug page); `run_scip` upgrades these to live BTS summaries
        // when not running offline.
        external_symbols.extend(bug_numbers.iter().map(|&n| SymbolInformation {
            symbol: symbols::bts_bug(&n.to_string()),
            kind: scip::types::symbol_information::Kind::Constant.into(),
            display_name: format!("#{n}"),
            documentation: vec![symbols::bts_bug_static_doc(n)],
            ..Default::default()
        }));

        let project_root = self.project_root.unwrap_or_else(|| {
            let abs = self
                .root
                .canonicalize()
                .unwrap_or_else(|_| self.root.clone());
            format!("file://{}", abs.display())
        });

        Index {
            metadata: Some(Metadata {
                version: scip::types::ProtocolVersion::UnspecifiedProtocolVersion.into(),
                tool_info: Some(ToolInfo {
                    name: "debian-lsp".to_owned(),
                    version: env!("CARGO_PKG_VERSION").to_owned(),
                    arguments: self.arguments,
                    ..Default::default()
                })
                .into(),
                project_root,
                text_document_encoding: scip::types::TextEncoding::UTF8.into(),
                ..Default::default()
            })
            .into(),
            documents,
            external_symbols,
            ..Default::default()
        }
    }
}
