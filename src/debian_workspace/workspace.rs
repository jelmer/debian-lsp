//! Adapter from the salsa-backed [`crate::workspace::Workspace`] to the
//! `debian_workspace::Workspace` trait that detector hosts (lintian-brush,
//! multiarch-hints, ...) program against.
//!
//! Reads are served from the open editor buffers when possible, falling
//! back to disk otherwise. The adapter is read-only: the [`Editor`] /
//! write paths on the trait return `Other` errors — detectors emit
//! [`debian_workspace::action::Action`] values for hosts to translate into
//! LSP edits via [`crate::debian_workspace::translate`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ::debian_workspace::workspace::Editor;
use ::debian_workspace::{Error as FixerError, Version, Workspace as FixerWorkspace};
use debian_changelog::ChangeLog;
use debian_control::lossless::Control;
use debian_copyright::lossless::Copyright;
use debian_watch::parse::ParsedWatchFile;
use makefile_lossless::Makefile;
use tower_lsp_server::ls_types::Uri;

use crate::workspace::Workspace;

/// A [`FixerWorkspace`] backed by the LSP server's in-memory state.
pub struct LspDebianWorkspace<'a> {
    workspace: &'a Workspace,
    /// Absolute path to the package root (the directory containing
    /// `debian/`). Used to resolve relative paths to URIs.
    base_path: PathBuf,
    /// Source package name, taken from `debian/changelog`. `None` when the
    /// changelog can't be read.
    package: Option<String>,
    /// Source package version, taken from `debian/changelog`. `None` when
    /// the changelog can't be read.
    version: Option<Version>,
    /// Maps an in-editor URI to the open buffer's parsed `SourceFile`.
    /// Reads consult this first before falling back to disk.
    open_files: HashMap<Uri, crate::workspace::SourceFile>,
    /// Snapshot of each open buffer's text at construction time. The
    /// `read_file` trait method returns `Cow::Borrowed` slices into
    /// these — we own the `Arc<str>`s for the lifetime of `self`, so
    /// the borrows are valid for `&self`-bound `Cow<'_, [u8]>` returns.
    /// Without this cache we'd have to copy the buffer bytes on every
    /// `read_file` call against an open file.
    open_texts: HashMap<Uri, std::sync::Arc<str>>,
}

impl<'a> LspDebianWorkspace<'a> {
    /// Construct a new workspace.
    pub fn new(
        workspace: &'a Workspace,
        base_path: PathBuf,
        package: Option<String>,
        version: Option<Version>,
        open_files: HashMap<Uri, crate::workspace::SourceFile>,
    ) -> Self {
        // Snapshot the text for each open file once. Cloning an
        // Arc<str> is cheap (atomic refcount bump); the hot
        // read_file path then hands out borrows into these Arcs
        // rather than re-cloning bytes per call.
        let open_texts = open_files
            .iter()
            .map(|(uri, sf)| (uri.clone(), workspace.source_text(*sf)))
            .collect();
        Self {
            workspace,
            base_path,
            package,
            version,
            open_files,
            open_texts,
        }
    }

    /// Return the in-editor or on-disk text of `rel`. Used by the
    /// translator to resolve `Action::Filesystem::ReplaceText` ranges
    /// against the right text.
    ///
    /// Cheap when the file is open — clones an `Arc` rather than the
    /// full buffer. Falls back to a disk read for files we know only
    /// from disk (in which case the cost is dominated by I/O, not the
    /// allocation).
    pub fn current_text(&self, rel: &Path) -> Option<std::sync::Arc<str>> {
        let uri = Uri::from_file_path(self.base_path.join(rel))?;
        if let Some(&sf) = self.open_files.get(&uri) {
            return Some(self.workspace.source_text(sf));
        }
        std::fs::read_to_string(self.base_path.join(rel))
            .ok()
            .map(std::sync::Arc::from)
    }

    /// Resolve a package-relative path to an editor URI.
    pub fn resolve_uri(&self, rel: &Path) -> Option<Uri> {
        Uri::from_file_path(self.base_path.join(rel))
    }

    /// Absolute path to the package root (the directory containing
    /// `debian/`).
    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    /// Look up the open `SourceFile` (if any) backing a package-relative
    /// path. Used by the trait impl and the action translator to fetch the
    /// salsa-cached parse rather than reparsing.
    pub fn source_file_for(&self, rel: &Path) -> Option<crate::workspace::SourceFile> {
        let uri = self.resolve_uri(rel)?;
        self.open_files.get(&uri).copied()
    }

    /// Salsa-cached `Control` parse for `rel`, when the file is open.
    /// Returns `None` if the file isn't tracked or the parse failed.
    pub fn parsed_control_for(&self, rel: &Path) -> Option<Control> {
        let sf = self.source_file_for(rel)?;
        self.workspace.get_parsed_control(sf).to_result().ok()
    }

    /// Salsa-cached `ChangeLog` parse for `rel`, when the file is open.
    pub fn parsed_changelog_for(&self, rel: &Path) -> Option<ChangeLog> {
        let sf = self.source_file_for(rel)?;
        self.workspace.get_parsed_changelog(sf).to_result().ok()
    }

    /// Salsa-cached `YamlFile` parse for `rel`, when the file is open.
    pub fn parsed_yaml_for(&self, rel: &Path) -> Option<yaml_edit::YamlFile> {
        let sf = self.source_file_for(rel)?;
        self.workspace
            .get_parsed_upstream_metadata(sf)
            .to_result()
            .ok()
    }

    /// Salsa-cached `Copyright` parse for `rel`, when the file is open.
    pub fn parsed_copyright_for(&self, rel: &Path) -> Option<Copyright> {
        let sf = self.source_file_for(rel)?;
        self.workspace.get_parsed_copyright(sf).to_result().ok()
    }

    /// Salsa-cached `Makefile` parse for `rel`, when the file is open.
    pub fn parsed_rules_for(&self, rel: &Path) -> Option<Makefile> {
        let sf = self.source_file_for(rel)?;
        Some(self.workspace.get_parsed_rules(sf).tree())
    }

    /// Salsa-cached `ParsedWatchFile` for `rel`, when the file is open.
    pub fn parsed_watch_for(&self, rel: &Path) -> Option<ParsedWatchFile> {
        let sf = self.source_file_for(rel)?;
        Some(self.workspace.get_parsed_watch(sf).to_watch_file())
    }

    /// Salsa-cached DEP-3 header parse for `rel`, when the file is
    /// open. Returns the parsed deb822 of the header portion only,
    /// plus the byte offset where the diff body starts. `None` if the
    /// file isn't tracked.
    pub fn parsed_dep3_header_for(
        &self,
        rel: &Path,
    ) -> Option<(deb822_lossless::Parse<deb822_lossless::Deb822>, usize)> {
        let sf = self.source_file_for(rel)?;
        Some(self.workspace.get_parsed_dep3_header(sf))
    }
}

impl<'a> FixerWorkspace for LspDebianWorkspace<'a> {
    fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }

    fn current_version(&self) -> Option<&Version> {
        self.version.as_ref()
    }

    fn parsed_control(&self) -> Result<Control, FixerError> {
        // Reuse the salsa-cached parse when the file is open in the editor;
        // fall back to a one-shot parse for files we only know from disk.
        if let Some(sf) = self.source_file_for(Path::new("debian/control")) {
            return self
                .workspace
                .get_parsed_control(sf)
                .to_result()
                .map_err(|e| FixerError::Other(format!("Failed to parse debian/control: {}", e)));
        }
        let text = self
            .current_text(Path::new("debian/control"))
            .ok_or(FixerError::NotFound)?;
        text.parse().map_err(|e: deb822_lossless::ParseError| {
            FixerError::Other(format!("Failed to parse debian/control: {}", e))
        })
    }

    fn parsed_changelog(&self) -> Result<ChangeLog, FixerError> {
        if let Some(sf) = self.source_file_for(Path::new("debian/changelog")) {
            return Ok(self.workspace.get_parsed_changelog(sf).tree());
        }
        let text = self
            .current_text(Path::new("debian/changelog"))
            .ok_or(FixerError::NotFound)?;
        Ok(ChangeLog::parse_relaxed(&text))
    }

    fn parsed_copyright(&self) -> Result<Copyright, FixerError> {
        if let Some(sf) = self.source_file_for(Path::new("debian/copyright")) {
            return self
                .workspace
                .get_parsed_copyright(sf)
                .to_result()
                .map_err(|e| {
                    FixerError::Other(format!("Failed to parse debian/copyright: {:?}", e))
                });
        }
        let text = self
            .current_text(Path::new("debian/copyright"))
            .ok_or(FixerError::NotFound)?;
        text.parse()
            .map_err(|e: debian_copyright::lossless::Error| {
                FixerError::Other(format!("Failed to parse debian/copyright: {:?}", e))
            })
    }

    fn parsed_upstream_metadata(&self) -> Result<yaml_edit::YamlFile, FixerError> {
        use std::str::FromStr;
        let rel = Path::new("debian/upstream/metadata");
        if let Some(sf) = self.source_file_for(rel) {
            return self
                .workspace
                .get_parsed_upstream_metadata(sf)
                .to_result()
                .map_err(|e| {
                    FixerError::Other(format!("Failed to parse {}: {}", rel.display(), e))
                });
        }
        let text = self.current_text(rel).ok_or(FixerError::NotFound)?;
        yaml_edit::YamlFile::from_str(&text)
            .map_err(|e| FixerError::Other(format!("Failed to parse {}: {}", rel.display(), e)))
    }

    fn parsed_watch(&self) -> Result<ParsedWatchFile, FixerError> {
        let rel = Path::new("debian/watch");
        if let Some(sf) = self.source_file_for(rel) {
            return Ok(self.workspace.get_parsed_watch(sf).to_watch_file());
        }
        let text = self.current_text(rel).ok_or(FixerError::NotFound)?;
        Ok(debian_watch::parse::Parse::parse(&text).to_watch_file())
    }

    fn parsed_rules(&self) -> Result<Makefile, FixerError> {
        let rel = Path::new("debian/rules");
        if let Some(sf) = self.source_file_for(rel) {
            return Ok(self.workspace.get_parsed_rules(sf).tree());
        }
        let bytes = self.read_file(rel)?.ok_or(FixerError::NotFound)?;
        Makefile::read_relaxed(&bytes[..])
            .map_err(|e| FixerError::Other(format!("Failed to parse {}: {}", rel.display(), e)))
    }

    fn source_format(&self) -> Result<Option<String>, FixerError> {
        match self.read_file(Path::new("debian/source/format"))? {
            Some(b) => Ok(std::str::from_utf8(&b)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())),
            None => Ok(None),
        }
    }

    fn list_dir(&self, rel: &Path) -> Result<Option<Vec<String>>, FixerError> {
        let abs = self.base_path.join(rel);
        let read_dir = match std::fs::read_dir(&abs) {
            Ok(it) => it,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(FixerError::Io(e)),
        };
        let mut names = Vec::new();
        for entry in read_dir {
            let entry = entry?;
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
        Ok(Some(names))
    }

    fn file_mode(&self, rel: &Path) -> Result<Option<u32>, FixerError> {
        use std::os::unix::fs::PermissionsExt;
        let abs = self.base_path.join(rel);
        match std::fs::metadata(&abs) {
            Ok(m) => Ok(Some(m.permissions().mode())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(FixerError::Io(e)),
        }
    }

    fn control(&self) -> Result<Box<dyn Editor<Control> + '_>, FixerError> {
        // The mutable editor path isn't supported in the LSP host —
        // detectors don't use it. Returning Other here makes any caller
        // that does try to mutate fail loudly.
        Err(FixerError::Other(
            "LspDebianWorkspace does not support mutable Control editor; emit Actions instead"
                .into(),
        ))
    }

    fn changelog(&self) -> Result<Box<dyn Editor<ChangeLog> + '_>, FixerError> {
        Err(FixerError::Other(
            "LspDebianWorkspace does not support mutable ChangeLog editor; emit Actions instead"
                .into(),
        ))
    }

    fn debcargo(&self) -> Result<Option<Box<dyn Editor<toml_edit::DocumentMut> + '_>>, FixerError> {
        Err(FixerError::Other(
            "LspDebianWorkspace does not support mutable debcargo editor; emit Actions instead"
                .into(),
        ))
    }

    fn read_file(&self, rel: &Path) -> Result<Option<std::borrow::Cow<'_, [u8]>>, FixerError> {
        // Open buffers: hand back a borrow into our `open_texts`
        // cache so detectors don't pay an O(N) copy on every read of
        // an open file. Closed files fall back to a disk read, which
        // is necessarily `Cow::Owned`.
        let uri = self
            .resolve_uri(rel)
            .ok_or_else(|| FixerError::Other(format!("Cannot resolve {}", rel.display())))?;
        if let Some(text) = self.open_texts.get(&uri) {
            return Ok(Some(std::borrow::Cow::Borrowed(text.as_bytes())));
        }
        match std::fs::read(self.base_path.join(rel)) {
            Ok(bytes) => Ok(Some(std::borrow::Cow::Owned(bytes))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(FixerError::Io(e)),
        }
    }

    fn write_file(&self, _rel: &Path, _content: &[u8]) -> Result<(), FixerError> {
        Err(FixerError::Other(
            "LspDebianWorkspace is read-only; emit Actions instead".into(),
        ))
    }
}
