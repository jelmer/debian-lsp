//! Debian Language Server Protocol implementation.

#![deny(missing_docs)]
#![deny(unsafe_code)]

use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

use clap::{Parser, Subcommand};

/// Server settings received from the client via initializationOptions.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
struct Settings {
    /// Allow the upstream-ontologist to make network requests when guessing
    /// upstream metadata values. Defaults to `false`.
    upstream_ontologist_net_access: bool,
}

mod architecture;
mod bugs;
mod changelog;
mod control;
mod copyright;
mod deb822;
mod distros;
mod lintian_overrides;
mod maintainers;
mod package_cache;
mod patches_series;
mod popcon;
mod position;
mod rdeps;
mod rules;
mod source_format;
mod source_options;
mod tests;
mod udd;
mod upstream_metadata;
mod vcswatch;
mod watch;
mod workspace;

use position::{text_range_to_lsp_range, try_lsp_range_to_text_range};
use std::collections::HashMap;
use tower_lsp_server::ls_types::notification::Notification;
use workspace::Workspace;

/// Custom notification for package status, displayed in the editor status bar.
enum PackageStatusNotification {}

/// Parameters for the package status notification.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PackageStatusParams {
    /// Source package name (from debian/changelog)
    name: String,
    /// Package version (from debian/changelog)
    version: String,
}

impl Notification for PackageStatusNotification {
    type Params = PackageStatusParams;
    const METHOD: &'static str = "debian/packageStatus";
}

/// Debian file type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileType {
    /// debian/control file
    Control,
    /// debian/copyright file
    Copyright,
    /// debian/watch file
    Watch,
    /// debian/tests/control file
    TestsControl,
    /// debian/changelog file
    Changelog,
    /// debian/source/format file
    SourceFormat,
    /// debian/source/options or debian/source/local-options file
    SourceOptions,
    /// debian/upstream/metadata file
    UpstreamMetadata,
    /// debian/rules file
    Rules,
    /// lintian overrides file (debian/source/lintian-overrides or debian/*.lintian-overrides)
    LintianOverrides,
    /// debian/patches/series file
    PatchesSeries,
}

impl FileType {
    /// Detect the file type from a URI
    fn detect(uri: &Uri) -> Option<Self> {
        if control::is_control_file(uri) {
            Some(Self::Control)
        } else if copyright::is_copyright_file(uri) {
            Some(Self::Copyright)
        } else if watch::is_watch_file(uri) {
            Some(Self::Watch)
        } else if tests::is_tests_control_file(uri) {
            Some(Self::TestsControl)
        } else if changelog::is_changelog_file(uri) {
            Some(Self::Changelog)
        } else if source_format::is_source_format_file(uri) {
            Some(Self::SourceFormat)
        } else if source_options::is_source_options_or_local_options_file(uri) {
            Some(Self::SourceOptions)
        } else if upstream_metadata::is_upstream_metadata_file(uri) {
            Some(Self::UpstreamMetadata)
        } else if rules::is_rules_file(uri) {
            Some(Self::Rules)
        } else if lintian_overrides::is_lintian_overrides_file(uri) {
            Some(Self::LintianOverrides)
        } else if patches_series::is_patches_series_file(uri) {
            Some(Self::PatchesSeries)
        } else {
            None
        }
    }
}

/// Information about an open file
#[derive(Clone, Copy)]
struct FileInfo {
    /// The workspace's source file ID
    source_file: workspace::SourceFile,
    /// The detected file type
    file_type: FileType,
}

struct Backend {
    client: Client,
    workspace: Arc<Mutex<Workspace>>,
    files: Arc<Mutex<HashMap<Uri, FileInfo>>>,
    package_cache: package_cache::SharedPackageCache,
    architecture_list: architecture::SharedArchitectureList,
    bug_cache: bugs::SharedBugCache,
    maintainer_cache: maintainers::SharedMaintainerCache,
    vcswatch_cache: vcswatch::SharedVcsWatchCache,
    popcon_cache: popcon::SharedPopconCache,
    rdeps_cache: rdeps::SharedRdepsCache,
    git_file_cache: copyright::code_lens::SharedGitFileCache,
    lintian_tag_cache: lintian_overrides::SharedLintianTagCache,
    upstream_cache: upstream_metadata::SharedUpstreamCache,
    settings: Arc<Mutex<Settings>>,
}

impl Backend {
    fn collect_diagnostics(
        source_file: workspace::SourceFile,
        file_type: FileType,
        workspace: &Workspace,
    ) -> Option<Vec<Diagnostic>> {
        match file_type {
            FileType::Control => {
                let source_text = workspace.source_text(source_file);
                let parsed = workspace.get_parsed_control(source_file);
                Some(control::diagnostics::get_diagnostics(&source_text, &parsed))
            }
            FileType::Copyright => Some(workspace.get_copyright_diagnostics(source_file)),
            FileType::PatchesSeries => {
                let source_text = workspace.source_text(source_file);
                let parsed = workspace.get_parsed_patches_series(source_file);
                Some(patches_series::diagnostics::get_diagnostics(
                    &source_text,
                    &parsed,
                ))
            }
            FileType::Watch
            | FileType::TestsControl
            | FileType::Changelog
            | FileType::SourceFormat
            | FileType::SourceOptions
            | FileType::UpstreamMetadata
            | FileType::Rules
            | FileType::LintianOverrides => None,
        }
    }

    /// Find the `debian/` directory by walking up from the given URI.
    fn find_debian_dir(uri: &Uri) -> Option<std::path::PathBuf> {
        let path = uri.to_file_path()?;
        path.ancestors()
            .find(|p| p.file_name().and_then(|n| n.to_str()) == Some("debian"))
            .map(|p| p.to_path_buf())
    }

    /// Get or load the changelog source file for the debian directory
    /// containing the given URI. If the changelog is already open, reuses the
    /// existing workspace entry; otherwise reads it from disk and inserts it
    /// into the workspace so the Salsa cache is populated.
    fn get_changelog_source_file(
        uri: &Uri,
        files: &HashMap<Uri, FileInfo>,
        workspace: &mut Workspace,
    ) -> Option<workspace::SourceFile> {
        let debian_dir = Self::find_debian_dir(uri)?;
        let changelog_path = debian_dir.join("changelog");
        let changelog_uri = Uri::from_file_path(&changelog_path)?;

        if let Some(info) = files.get(&changelog_uri) {
            return Some(info.source_file);
        }

        // Not open — read from disk and insert into the workspace
        let text = std::fs::read_to_string(&changelog_path).ok()?;
        Some(workspace.update_file(changelog_uri, text))
    }

    /// Look up the version from `debian/changelog` for the same project as the
    /// given control file URI. Checks open files first, falls back to reading
    /// from disk.
    fn get_changelog_version(
        control_uri: &Uri,
        files: &Arc<Mutex<HashMap<Uri, FileInfo>>>,
        workspace: &Workspace,
    ) -> Option<String> {
        let control_path = control_uri.to_file_path()?;
        let changelog_path = control_path.parent()?.join("changelog");
        let changelog_uri = Uri::from_file_path(&changelog_path)?;

        // Check if the changelog is open in the workspace
        let files_guard = files.try_lock().ok()?;
        let changelog_file = files_guard.get(&changelog_uri)?;
        let parsed = workspace.get_parsed_changelog(changelog_file.source_file);
        let changelog = parsed.tree();
        let entry = changelog.iter().next()?;
        Some(entry.version()?.to_string())
    }

    /// Read `*.substvars` files from the same directory as the control file
    /// and populate the map with their key=value pairs.
    fn read_substvars_files(
        control_uri: &Uri,
        map: &mut std::collections::HashMap<String, String>,
    ) {
        let control_path = control_uri.to_file_path();
        let Some(control_path) = control_path.as_deref() else {
            return;
        };
        let Some(debian_dir) = control_path.parent() else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(debian_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("substvars") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            for line in content.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    map.entry(key.to_string())
                        .or_insert_with(|| value.to_string());
                }
            }
        }
    }

    /// Spawn a background task to prefetch bug data for the source package
    /// in the given changelog, so completions are fast when the user needs them.
    fn prefetch_changelog_bugs(&self, source_file: workspace::SourceFile, workspace: &Workspace) {
        let parsed = workspace.get_parsed_changelog(source_file);
        let changelog = parsed.tree();
        let package_name = changelog.iter().next().and_then(|entry| entry.package());
        if let Some(package_name) = package_name {
            let bug_cache = self.bug_cache.clone();
            tokio::spawn(async move {
                let mut cache = bug_cache.write().await;
                cache.prefetch_bugs_for_package(&package_name).await;
                cache
                    .prefetch_launchpad_bugs_for_package(&package_name)
                    .await;
            });
        }
    }

    /// Spawn a background task to populate the upstream metadata guess cache.
    fn prefetch_upstream_guesses(&self, uri: &Uri) {
        let project_root =
            Self::find_debian_dir(uri).and_then(|d| d.parent().map(|p| p.to_path_buf()));
        if let Some(project_root) = project_root {
            let cache = self.upstream_cache.clone();
            let settings = self.settings.clone();
            tokio::spawn(async move {
                let needs_populate = !cache.read().await.is_cached(&project_root);
                if needs_populate {
                    let net_access = settings.lock().await.upstream_ontologist_net_access;
                    cache
                        .write()
                        .await
                        .populate(&project_root, net_access)
                        .await;
                }
            });
        }
    }

    /// Send a `debian/packageStatus` notification with the source package name
    /// and version extracted from `debian/changelog`.
    async fn send_package_status(&self, uri: &Uri) {
        let params = {
            let files = self.files.lock().await;
            let mut workspace = self.workspace.lock().await;

            let source_file = Self::get_changelog_source_file(uri, &files, &mut workspace);
            source_file.and_then(|sf| {
                let parsed = workspace.get_parsed_changelog(sf);
                let changelog = parsed.tree();
                let entry = changelog.iter().next()?;
                let name = entry.package()?;
                let version = entry.version()?;
                Some(PackageStatusParams {
                    name,
                    version: version.to_string(),
                })
            })
        };

        if let Some(params) = params {
            self.client
                .send_notification::<PackageStatusNotification>(params)
                .await;
        }
    }
}

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        if let Some(options) = params.initialization_options {
            match serde_json::from_value::<Settings>(options) {
                Ok(settings) => {
                    *self.settings.lock().await = settings;
                }
                Err(e) => {
                    tracing::warn!("Failed to parse initializationOptions: {e}");
                }
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::INCREMENTAL),
                        will_save_wait_until: Some(true),
                        ..Default::default()
                    },
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: None,
                    trigger_characters: Some(vec![
                        ":".to_string(),
                        " ".to_string(),
                        "(".to_string(),
                        "[".to_string(),
                        "<".to_string(),
                        "$".to_string(),
                        "=".to_string(),
                        ",".to_string(),
                        "#".to_string(),
                    ]),
                    work_done_progress_options: Default::default(),
                    all_commit_characters: None,
                    completion_item: None,
                }),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            work_done_progress_options: WorkDoneProgressOptions::default(),
                            legend: SemanticTokensLegend {
                                token_types: vec![
                                    SemanticTokenType::new("debianField"),
                                    SemanticTokenType::new("debianUnknownField"),
                                    SemanticTokenType::new("debianValue"),
                                    SemanticTokenType::new("debianComment"),
                                    SemanticTokenType::new("changelogPackage"),
                                    SemanticTokenType::new("changelogVersion"),
                                    SemanticTokenType::new("changelogDistribution"),
                                    SemanticTokenType::new("changelogUrgency"),
                                    SemanticTokenType::new("changelogMaintainer"),
                                    SemanticTokenType::new("changelogTimestamp"),
                                    SemanticTokenType::new("changelogMetadataValue"),
                                    SemanticTokenType::new("changelogBugReference"),
                                ],
                                token_modifiers: vec![SemanticTokenModifier::DECLARATION],
                            },
                            range: Some(false),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                    ),
                ),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(false),
                }),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![control::code_lens::OPEN_URL_COMMAND.to_string()],
                    ..Default::default()
                }),
                inlay_hint_provider: Some(OneOf::Left(true)),
                document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
                    first_trigger_character: ":".to_string(),
                    more_trigger_character: Some(vec!["\n".to_string(), "-".to_string()]),
                }),
                document_formatting_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: Default::default(),
                })),
                selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
                document_link_provider: Some(DocumentLinkOptions {
                    resolve_provider: Some(false),
                    work_done_progress_options: Default::default(),
                }),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Debian LSP initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.client
            .log_message(
                MessageType::INFO,
                format!("file opened: {:?}", params.text_document.uri),
            )
            .await;

        // Detect file type once
        let Some(file_type) = FileType::detect(&params.text_document.uri) else {
            return;
        };

        let mut workspace = self.workspace.lock().await;
        let source_file = workspace.update_file(
            params.text_document.uri.clone(),
            params.text_document.text.clone(),
        );

        let mut files = self.files.lock().await;
        files.insert(
            params.text_document.uri.clone(),
            FileInfo {
                source_file,
                file_type,
            },
        );

        if file_type == FileType::Changelog {
            self.prefetch_changelog_bugs(source_file, &workspace);
        }

        if file_type == FileType::UpstreamMetadata {
            self.prefetch_upstream_guesses(&params.text_document.uri);
        }

        let diagnostics = Self::collect_diagnostics(source_file, file_type, &workspace);
        drop(files);
        drop(workspace);

        if let Some(diagnostics) = diagnostics {
            self.client
                .publish_diagnostics(params.text_document.uri.clone(), diagnostics, None)
                .await;
        }

        self.send_package_status(&params.text_document.uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.client
            .log_message(
                MessageType::INFO,
                format!("file changed: {:?}", params.text_document.uri),
            )
            .await;

        // Get or detect the file type
        let mut files = self.files.lock().await;
        let file_info = files.get(&params.text_document.uri).copied();
        let file_type = file_info
            .map(|info| info.file_type)
            .or_else(|| FileType::detect(&params.text_document.uri));

        let Some(file_type) = file_type else {
            return;
        };

        if params.content_changes.is_empty() {
            return;
        }

        // Apply incremental content changes to the current text
        let mut workspace = self.workspace.lock().await;
        let mut text = file_info
            .map(|info| workspace.source_text(info.source_file))
            .unwrap_or_default();

        for change in &params.content_changes {
            if let Some(range) = &change.range {
                // Incremental change: splice the range
                if let Some(text_range) = position::try_lsp_range_to_text_range(&text, range) {
                    let start: usize = text_range.start().into();
                    let end: usize = text_range.end().into();
                    text.replace_range(start..end, &change.text);
                }
            } else {
                // Full replacement
                text = change.text.clone();
            }
        }

        let source_file = workspace.update_file(params.text_document.uri.clone(), text);
        files.insert(
            params.text_document.uri.clone(),
            FileInfo {
                source_file,
                file_type,
            },
        );

        let diagnostics = Self::collect_diagnostics(source_file, file_type, &workspace);
        drop(files);
        drop(workspace);

        if let Some(diagnostics) = diagnostics {
            self.client
                .publish_diagnostics(params.text_document.uri.clone(), diagnostics, None)
                .await;
        }

        if file_type == FileType::Changelog {
            self.send_package_status(&params.text_document.uri).await;
        }
    }

    async fn will_save_wait_until(
        &self,
        params: WillSaveTextDocumentParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let files = self.files.lock().await;
        let file_info = match files.get(&params.text_document.uri) {
            Some(info) => *info,
            None => return Ok(None),
        };

        if file_info.file_type != FileType::Changelog {
            return Ok(None);
        }

        let workspace = self.workspace.lock().await;
        let source_text = workspace.source_text(file_info.source_file);
        let parsed = workspace.get_parsed_changelog(file_info.source_file);
        let changelog = parsed.tree();

        let edit = changelog::generate_timestamp_update_edit(&changelog, &source_text);
        Ok(edit.map(|e| vec![e]))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        // Look up the file type from our cache
        let files = self.files.lock().await;
        let file_info = files
            .get(&uri)
            .map(|info| (info.file_type, info.source_file));
        drop(files); // Release the lock

        let completions = match file_info {
            Some((FileType::Control, source_file)) => {
                let workspace = self.workspace.lock().await;
                let source_text = workspace.source_text(source_file);
                let parsed = workspace.get_parsed_control(source_file);
                // Check if cursor is on a field value to try async relationship completions
                let cursor_context = deb822::completion::get_cursor_context(
                    parsed.tree().as_deb822(),
                    &source_text,
                    position,
                );
                if let Some(deb822::completion::CursorContext::FieldValue {
                    field_name,
                    value_prefix,
                }) = &cursor_context
                {
                    drop(workspace); // Release lock before async operations
                                     // Try async completions (relationship fields via package cache)
                    if let Some(async_completions) = control::get_async_field_value_completions(
                        field_name,
                        value_prefix,
                        position,
                        &self.package_cache,
                        &self.architecture_list,
                        &self.maintainer_cache,
                    )
                    .await
                    {
                        async_completions
                    } else {
                        // Fall back to sync completions (Section, Priority, etc.)
                        control::get_field_value_completions(field_name, value_prefix)
                    }
                } else {
                    // Not on a field value — get field name completions
                    // (workspace lock and parsed result already held)
                    control::get_completions(parsed.tree().as_deb822(), &source_text, position)
                }
            }
            Some((FileType::Copyright, source_file)) => {
                let workspace = self.workspace.lock().await;
                let source_text = workspace.source_text(source_file);
                let parsed = workspace.get_parsed_copyright(source_file);
                copyright::get_completions(&parsed, &source_text, position)
            }
            Some((FileType::Watch, source_file)) => {
                let workspace = self.workspace.lock().await;
                let parsed = workspace.get_parsed_watch(source_file);
                let source_text = workspace.source_text(source_file);
                let wf = parsed.to_watch_file();
                match &wf {
                    debian_watch::parse::ParsedWatchFile::LineBased(wf) => {
                        watch::get_linebased_completions(&uri, wf, &source_text, position)
                    }
                    debian_watch::parse::ParsedWatchFile::Deb822(wf) => {
                        watch::get_completions_deb822(wf.as_deb822(), &source_text, position)
                    }
                }
            }
            Some((FileType::TestsControl, _)) => tests::get_completions(&uri, position),
            Some((FileType::Changelog, source_file)) => {
                let workspace = self.workspace.lock().await;
                let source_text = workspace.source_text(source_file);
                let parsed = workspace.get_parsed_changelog(source_file);
                drop(workspace);
                if let Some(bug_completions) = changelog::get_async_bug_completions(
                    &parsed,
                    &source_text,
                    position,
                    &self.bug_cache,
                )
                .await
                {
                    bug_completions
                } else {
                    changelog::get_completions(&parsed, &source_text, position)
                }
            }
            Some((FileType::SourceFormat, _)) => source_format::get_completions(&uri, position),
            Some((FileType::LintianOverrides, source_file)) => {
                let workspace = self.workspace.lock().await;
                let source_text = workspace.source_text(source_file);
                drop(workspace);
                let mut tag_cache = self.lintian_tag_cache.write().await;
                let tags = tag_cache.get_tags().await;
                lintian_overrides::get_completions(&source_text, position, tags)
            }
            Some((FileType::SourceOptions, source_file)) => {
                let workspace = self.workspace.lock().await;
                let source_text = workspace.source_text(source_file);
                source_options::get_completions(&uri, position, &source_text)
            }
            Some((FileType::UpstreamMetadata, source_file)) => {
                let workspace = self.workspace.lock().await;
                let source_text = workspace.source_text(source_file);
                let project_root =
                    Self::find_debian_dir(&uri).and_then(|d| d.parent().map(|p| p.to_path_buf()));
                drop(workspace);
                upstream_metadata::get_completions(
                    &source_text,
                    position,
                    &self.upstream_cache,
                    project_root.as_deref(),
                )
                .await
            }
            Some((FileType::Rules, source_file)) => {
                let workspace = self.workspace.lock().await;
                let source_text = workspace.source_text(source_file);
                let parsed = workspace.get_parsed_rules(source_file);
                let makefile = parsed.tree();
                rules::get_completions(&makefile, &source_text, position)
            }
            Some((FileType::PatchesSeries, source_file)) => {
                let workspace = self.workspace.lock().await;
                let source_text = workspace.source_text(source_file);
                let parsed = workspace.get_parsed_patches_series(source_file);
                patches_series::get_completions(&uri, &parsed, &source_text, position)
            }
            None => Vec::new(),
        };

        if completions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(CompletionResponse::Array(completions)))
        }
    }

    async fn completion_resolve(&self, item: CompletionItem) -> Result<CompletionItem> {
        Ok(item)
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let workspace = self.workspace.lock().await;
        let files = self.files.lock().await;

        let file_info = match files.get(&params.text_document.uri) {
            Some(info) => info,
            None => return Ok(None),
        };

        // Only control, copyright, and changelog files support code actions for now
        match file_info.file_type {
            FileType::Control | FileType::Copyright | FileType::Changelog => {}
            _ => return Ok(None),
        }

        let source_text = workspace.source_text(file_info.source_file);

        let mut actions = Vec::new();

        let text_range = try_lsp_range_to_text_range(&source_text, &params.range);

        match file_info.file_type {
            FileType::Control => {
                let Some(text_range) = text_range else {
                    return Ok(None);
                };

                // Add wrap-and-sort action
                let parsed = workspace.get_parsed_control(file_info.source_file);
                if let Some(action) = control::get_wrap_and_sort_action(
                    &params.text_document.uri,
                    &source_text,
                    &parsed,
                    text_range,
                ) {
                    actions.push(action);
                }

                // Add binary package action
                if let Some(action) = control::get_add_binary_package_action(
                    &params.text_document.uri,
                    &source_text,
                    &parsed,
                ) {
                    actions.push(action);
                }

                // Add field casing fixes
                let issues =
                    control::diagnostics::find_field_casing_issues(&parsed, Some(text_range));
                actions.extend(control::get_field_casing_actions(
                    &params.text_document.uri,
                    &source_text,
                    issues,
                    &params.context.diagnostics,
                ));
            }
            FileType::Copyright => {
                let Some(text_range) = text_range else {
                    return Ok(None);
                };

                // Add wrap-and-sort action
                let parsed = workspace.get_parsed_copyright(file_info.source_file);
                if let Some(action) = copyright::get_wrap_and_sort_action(
                    &params.text_document.uri,
                    &source_text,
                    &parsed,
                    text_range,
                ) {
                    actions.push(action);
                }

                // Add field casing fixes
                let issues = workspace
                    .find_copyright_field_casing_issues(file_info.source_file, Some(text_range));
                actions.extend(copyright::get_field_casing_actions(
                    &params.text_document.uri,
                    &source_text,
                    issues,
                    &params.context.diagnostics,
                ));
            }
            FileType::Changelog => {
                // Add action to create a new changelog entry (file-wide action)
                let parsed = workspace.get_parsed_changelog(file_info.source_file);
                let changelog = parsed.tree();
                match changelog::generate_new_changelog_entry(&changelog) {
                    Ok(new_entry) => {
                        // Insert the new entry at the beginning of the file
                        let edit = TextEdit {
                            range: Range {
                                start: Position {
                                    line: 0,
                                    character: 0,
                                },
                                end: Position {
                                    line: 0,
                                    character: 0,
                                },
                            },
                            new_text: new_entry,
                        };

                        let workspace_edit = WorkspaceEdit {
                            changes: Some(
                                vec![(params.text_document.uri.clone(), vec![edit])]
                                    .into_iter()
                                    .collect(),
                            ),
                            ..Default::default()
                        };

                        let action = CodeAction {
                            title: "Add new changelog entry".to_string(),
                            kind: Some(CodeActionKind::SOURCE),
                            edit: Some(workspace_edit),
                            ..Default::default()
                        };

                        actions.push(CodeActionOrCommand::CodeAction(action));
                    }
                    Err(_) => {
                        // If we can't generate a new entry, don't add the action
                    }
                }

                // Check for UNRELEASED entries in the requested range and offer "Mark for upload"
                if let Some(text_range) = text_range {
                    let unreleased_entries = workspace
                        .find_unreleased_entries_in_range(file_info.source_file, text_range);

                    for info in unreleased_entries {
                        let lsp_range =
                            text_range_to_lsp_range(&source_text, info.unreleased_range);

                        let edit = TextEdit {
                            range: lsp_range,
                            new_text: info.target_distribution.clone(),
                        };

                        let workspace_edit = WorkspaceEdit {
                            changes: Some(
                                vec![(params.text_document.uri.clone(), vec![edit])]
                                    .into_iter()
                                    .collect(),
                            ),
                            ..Default::default()
                        };

                        let action = CodeAction {
                            title: format!("Mark for upload to {}", info.target_distribution),
                            kind: Some(CodeActionKind::REFACTOR),
                            edit: Some(workspace_edit),
                            ..Default::default()
                        };

                        actions.push(CodeActionOrCommand::CodeAction(action));
                    }
                }
            }
            _ => unreachable!(),
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let workspace = self.workspace.lock().await;
        let files = self.files.lock().await;

        let file_info = match files.get(&params.text_document.uri) {
            Some(info) => info,
            None => return Ok(None),
        };

        if file_info.file_type != FileType::Control {
            return Ok(None);
        }

        let source_text = workspace.source_text(file_info.source_file);
        let parsed = workspace.get_parsed_control(file_info.source_file);

        let Some(pkg) =
            control::find_package_name_at_position(&parsed, &source_text, &params.position)
        else {
            return Ok(None);
        };

        Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
            range: pkg.range,
            placeholder: pkg.name,
        }))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let workspace = self.workspace.lock().await;
        let files = self.files.lock().await;

        let uri = &params.text_document_position.text_document.uri;
        let file_info = match files.get(uri) {
            Some(info) => info,
            None => return Ok(None),
        };

        if file_info.file_type != FileType::Control {
            return Ok(None);
        }

        let source_text = workspace.source_text(file_info.source_file);
        let parsed = workspace.get_parsed_control(file_info.source_file);

        let Some(pkg) = control::find_package_name_at_position(
            &parsed,
            &source_text,
            &params.text_document_position.position,
        ) else {
            return Ok(None);
        };

        let old_name = &pkg.name;
        let new_name = &params.new_name;

        // Edit the Package: field value in debian/control
        let control_edit = TextEdit {
            range: pkg.range,
            new_text: new_name.clone(),
        };

        let mut document_changes: Vec<DocumentChangeOperation> = Vec::new();

        // Add the text edit for the control file
        document_changes.push(DocumentChangeOperation::Edit(TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: None,
            },
            edits: vec![OneOf::Left(control_edit)],
        }));

        // Determine the debian/ directory from the control file URI
        if let Some(control_path) = uri.to_file_path() {
            if let Some(debian_dir) = control_path.parent() {
                // Collect file renames for debian/<pkg>.<ext> files
                let file_renames =
                    control::collect_package_file_renames(debian_dir, old_name, new_name);
                for op in file_renames {
                    document_changes.push(DocumentChangeOperation::Op(op));
                }

                // Update references in debian/tests/control
                let tests_control_path = debian_dir.join("tests/control");
                if tests_control_path.exists() {
                    // Try to use the open file from the workspace first
                    let tests_control_uri = Uri::from_file_path(&tests_control_path);
                    let tests_text = if let Some(ref tc_uri) = tests_control_uri {
                        files
                            .get(tc_uri)
                            .map(|info| (tc_uri.clone(), workspace.source_text(info.source_file)))
                    } else {
                        None
                    };

                    // Fall back to reading from disk
                    let tests_text = tests_text.or_else(|| {
                        let text = std::fs::read_to_string(&tests_control_path).ok()?;
                        let tc_uri = Uri::from_file_path(&tests_control_path)?;
                        Some((tc_uri, text))
                    });

                    if let Some((tc_uri, text)) = tests_text {
                        let edits = control::collect_tests_control_edits(&text, old_name, new_name);
                        if !edits.is_empty() {
                            document_changes.push(DocumentChangeOperation::Edit(
                                TextDocumentEdit {
                                    text_document: OptionalVersionedTextDocumentIdentifier {
                                        uri: tc_uri,
                                        version: None,
                                    },
                                    edits: edits.into_iter().map(OneOf::Left).collect(),
                                },
                            ));
                        }
                    }
                }
            }
        }

        Ok(Some(WorkspaceEdit {
            document_changes: Some(DocumentChanges::Operations(document_changes)),
            ..Default::default()
        }))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = &params.text_document.uri;

        let workspace = self.workspace.lock().await;
        let files = self.files.lock().await;

        let file = match files.get(uri) {
            Some(f) => *f,
            None => return Ok(None),
        };
        drop(files);

        let source_text = workspace.source_text(file.source_file);

        let tokens = match file.file_type {
            FileType::Control => {
                let parsed = workspace.get_parsed_control(file.source_file);
                let control = parsed.tree();
                control::generate_semantic_tokens(&control, &source_text)
            }
            FileType::Copyright => {
                let parsed = workspace.get_parsed_copyright(file.source_file);
                let copyright = parsed.tree();
                copyright::generate_semantic_tokens(&copyright, &source_text)
            }
            FileType::Changelog => {
                let parsed = workspace.get_parsed_changelog(file.source_file);
                changelog::generate_semantic_tokens(&parsed, &source_text)
            }
            FileType::Watch => {
                let parsed = workspace.get_parsed_watch(file.source_file);
                watch::generate_semantic_tokens(&parsed, &source_text)
            }
            FileType::TestsControl => {
                let deb822_parse = workspace.get_parsed_deb822(file.source_file);
                tests::generate_semantic_tokens(&deb822_parse, &source_text)
            }
            FileType::UpstreamMetadata => {
                let parsed = workspace.get_parsed_upstream_metadata(file.source_file);
                let yaml_file = parsed.tree();
                match yaml_file.document() {
                    Some(doc) => upstream_metadata::generate_semantic_tokens(&doc, &source_text),
                    None => vec![],
                }
            }
            FileType::Rules => {
                let parsed = workspace.get_parsed_rules(file.source_file);
                let makefile = parsed.tree();
                rules::generate_semantic_tokens(&makefile, &source_text)
            }
            FileType::SourceFormat => vec![],
            FileType::SourceOptions => source_options::generate_semantic_tokens(&source_text),
            FileType::LintianOverrides => {
                let parsed = lintian_overrides::LintianOverrides::parse(&source_text);
                match parsed.ok() {
                    Ok(overrides) => {
                        lintian_overrides::generate_semantic_tokens(&overrides, &source_text)
                    }
                    Err(_) => vec![],
                }
            }
            FileType::PatchesSeries => {
                let parsed = workspace.get_parsed_patches_series(file.source_file);
                let patches_series = parsed.tree();
                patches_series::generate_semantic_tokens(&patches_series, &source_text)
            }
        };

        if tokens.is_empty() {
            Ok(None)
        } else {
            Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
                result_id: None,
                data: tokens,
            })))
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;

        let files = self.files.lock().await;
        let file = match files.get(uri) {
            Some(f) => *f,
            None => return Ok(None),
        };
        drop(files);

        let workspace = self.workspace.lock().await;
        let source_text = workspace.source_text(file.source_file);

        let symbols = match file.file_type {
            FileType::Changelog => {
                let parsed = workspace.get_parsed_changelog(file.source_file);
                changelog::generate_document_symbols(&parsed, &source_text)
            }
            FileType::Copyright => {
                let parsed = workspace.get_parsed_copyright(file.source_file);
                copyright::generate_document_symbols(&parsed, &source_text)
            }
            FileType::Control => {
                let parsed = workspace.get_parsed_control(file.source_file);
                control::generate_document_symbols(&parsed, &source_text)
            }
            _ => return Ok(None),
        };

        if symbols.is_empty() {
            Ok(None)
        } else {
            Ok(Some(DocumentSymbolResponse::Nested(symbols)))
        }
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let uri = &params.text_document.uri;

        let files = self.files.lock().await;
        let file = match files.get(uri) {
            Some(f) => *f,
            None => return Ok(None),
        };
        drop(files);

        let workspace = self.workspace.lock().await;
        let source_text = workspace.source_text(file.source_file);

        let ranges = match file.file_type {
            FileType::Control => {
                let parsed = workspace.get_parsed_control(file.source_file);
                deb822::folding::generate_folding_ranges(parsed.tree().as_deb822(), &source_text)
            }
            FileType::Copyright => {
                let parsed = workspace.get_parsed_copyright(file.source_file);
                deb822::folding::generate_folding_ranges(parsed.tree().as_deb822(), &source_text)
            }
            FileType::Changelog => {
                let parsed = workspace.get_parsed_changelog(file.source_file);
                changelog::generate_folding_ranges(&parsed, &source_text)
            }
            FileType::Watch => {
                let parsed = workspace.get_parsed_watch(file.source_file);
                watch::generate_folding_ranges(&parsed, &source_text)
            }
            FileType::TestsControl => {
                let deb822_parse = workspace.get_parsed_deb822(file.source_file);
                match deb822_parse.to_result() {
                    Ok(deb822) => deb822::folding::generate_folding_ranges(&deb822, &source_text),
                    Err(_) => return Ok(None),
                }
            }
            _ => return Ok(None),
        };

        if ranges.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ranges))
        }
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        let uri = &params.text_document.uri;

        let files = self.files.lock().await;
        let file = match files.get(uri) {
            Some(f) => *f,
            None => return Ok(None),
        };
        drop(files);

        let workspace = self.workspace.lock().await;
        let source_text = workspace.source_text(file.source_file);

        let ranges = match file.file_type {
            FileType::Control => {
                let parsed = workspace.get_parsed_control(file.source_file);
                deb822::selection_range::generate_selection_ranges(
                    parsed.tree().as_deb822(),
                    &source_text,
                    &params.positions,
                )
            }
            FileType::Copyright => {
                let parsed = workspace.get_parsed_copyright(file.source_file);
                deb822::selection_range::generate_selection_ranges(
                    parsed.tree().as_deb822(),
                    &source_text,
                    &params.positions,
                )
            }
            FileType::Changelog => {
                let parsed = workspace.get_parsed_changelog(file.source_file);
                changelog::generate_selection_ranges(&parsed, &source_text, &params.positions)
            }
            FileType::Watch => {
                let parsed = workspace.get_parsed_watch(file.source_file);
                watch::generate_selection_ranges(&parsed, &source_text, &params.positions)
            }
            FileType::TestsControl => {
                let deb822_parse = workspace.get_parsed_deb822(file.source_file);
                match deb822_parse.to_result() {
                    Ok(deb822) => deb822::selection_range::generate_selection_ranges(
                        &deb822,
                        &source_text,
                        &params.positions,
                    ),
                    Err(_) => return Ok(None),
                }
            }
            FileType::SourceOptions => {
                source_options::generate_selection_ranges(&source_text, &params.positions)
            }
            _ => return Ok(None),
        };

        if ranges.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ranges))
        }
    }

    async fn on_type_formatting(
        &self,
        params: DocumentOnTypeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let files = self.files.lock().await;
        let file = match files.get(uri) {
            Some(f) => *f,
            None => return Ok(None),
        };
        drop(files);

        let workspace = self.workspace.lock().await;
        let source_text = workspace.source_text(file.source_file);

        match file.file_type {
            FileType::Control => {
                let parsed = workspace.get_parsed_control(file.source_file);
                Ok(deb822::on_type_formatting::on_type_formatting(
                    parsed.tree().as_deb822(),
                    &source_text,
                    position,
                    &params.ch,
                ))
            }
            FileType::Copyright => {
                let parsed = workspace.get_parsed_copyright(file.source_file);
                Ok(deb822::on_type_formatting::on_type_formatting(
                    parsed.tree().as_deb822(),
                    &source_text,
                    position,
                    &params.ch,
                ))
            }
            FileType::TestsControl => {
                let deb822 = deb822_lossless::Deb822::parse(&source_text).tree();
                Ok(deb822::on_type_formatting::on_type_formatting(
                    &deb822,
                    &source_text,
                    position,
                    &params.ch,
                ))
            }
            FileType::Watch => {
                let parsed = workspace.get_parsed_watch(file.source_file);
                let wf = parsed.to_watch_file();
                match &wf {
                    debian_watch::parse::ParsedWatchFile::Deb822(_) => {
                        let deb822 = deb822_lossless::Deb822::parse(&source_text).tree();
                        Ok(deb822::on_type_formatting::on_type_formatting(
                            &deb822,
                            &source_text,
                            position,
                            &params.ch,
                        ))
                    }
                    _ => Ok(None),
                }
            }
            FileType::Changelog => {
                let parsed = workspace.get_parsed_changelog(file.source_file);
                Ok(changelog::on_type_formatting::on_type_formatting(
                    &parsed,
                    &source_text,
                    position,
                    &params.ch,
                ))
            }
            FileType::UpstreamMetadata => {
                Ok(upstream_metadata::on_type_formatting::on_type_formatting(
                    &source_text,
                    position,
                    &params.ch,
                ))
            }
            _ => Ok(None),
        }
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;

        let files = self.files.lock().await;
        let file = match files.get(uri) {
            Some(f) => *f,
            None => return Ok(None),
        };
        drop(files);

        let workspace = self.workspace.lock().await;
        let source_text = workspace.source_text(file.source_file);

        match file.file_type {
            FileType::Control => {
                let parsed = workspace.get_parsed_control(file.source_file);
                Ok(control::format_control(&source_text, &parsed))
            }
            FileType::Copyright => {
                let parsed = workspace.get_parsed_copyright(file.source_file);
                Ok(copyright::format_copyright(&source_text, &parsed))
            }
            FileType::Watch => {
                let parsed = workspace.get_parsed_watch(file.source_file);
                let wf = parsed.to_watch_file();
                match &wf {
                    debian_watch::parse::ParsedWatchFile::Deb822(_) => {
                        let deb822 = deb822_lossless::Deb822::parse(&source_text).tree();
                        let wrap_paragraph =
                            |p: &deb822_lossless::Paragraph| -> deb822_lossless::Paragraph {
                                p.wrap_and_sort(
                                    deb822_lossless::Indentation::Spaces(1),
                                    false,
                                    Some(79),
                                    None,
                                    None,
                                )
                            };
                        let formatted = deb822
                            .wrap_and_sort(None, Some(&wrap_paragraph))
                            .to_string();
                        if formatted == source_text {
                            return Ok(None);
                        }
                        let full_range = text_range_to_lsp_range(
                            &source_text,
                            text_size::TextRange::new(0.into(), (source_text.len() as u32).into()),
                        );
                        Ok(Some(vec![TextEdit {
                            range: full_range,
                            new_text: formatted,
                        }]))
                    }
                    _ => Ok(None),
                }
            }
            FileType::TestsControl => {
                let deb822 = deb822_lossless::Deb822::parse(&source_text).tree();
                let wrap_paragraph =
                    |p: &deb822_lossless::Paragraph| -> deb822_lossless::Paragraph {
                        p.wrap_and_sort(
                            deb822_lossless::Indentation::Spaces(1),
                            false,
                            Some(79),
                            None,
                            None,
                        )
                    };
                let formatted = deb822
                    .wrap_and_sort(None, Some(&wrap_paragraph))
                    .to_string();
                if formatted == source_text {
                    return Ok(None);
                }
                let full_range = text_range_to_lsp_range(
                    &source_text,
                    text_size::TextRange::new(0.into(), (source_text.len() as u32).into()),
                );
                Ok(Some(vec![TextEdit {
                    range: full_range,
                    new_text: formatted,
                }]))
            }
            _ => Ok(None),
        }
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let uri = &params.text_document.uri;

        let files = self.files.lock().await;
        let file = match files.get(uri) {
            Some(f) => *f,
            None => return Ok(None),
        };
        drop(files);

        match file.file_type {
            FileType::Control => {
                let workspace = self.workspace.lock().await;
                let source_text = workspace.source_text(file.source_file);
                let parsed = workspace.get_parsed_control(file.source_file);
                drop(workspace);

                let ctx = control::code_lens::LensContext {
                    package_cache: &self.package_cache,
                    vcswatch_cache: &self.vcswatch_cache,
                    bug_cache: &self.bug_cache,
                    popcon_cache: &self.popcon_cache,
                    rdeps_cache: &self.rdeps_cache,
                };
                let (lenses, uncached) =
                    control::generate_code_lenses(&parsed, &source_text, &ctx).await;

                if !uncached.is_empty() {
                    let client = self.client.clone();
                    let package_cache = self.package_cache.clone();
                    let vcswatch_cache = self.vcswatch_cache.clone();
                    let bug_cache = self.bug_cache.clone();
                    let popcon_cache = self.popcon_cache.clone();
                    let rdeps_cache = self.rdeps_cache.clone();
                    tokio::spawn(async move {
                        if uncached.needs_policy_version {
                            let mut cache = package_cache.write().await;
                            cache.load_versions("debian-policy").await;
                        }
                        if let Some(url) = &uncached.vcs_git_url {
                            let mut cache = vcswatch_cache.write().await;
                            cache.get_version_for_url(url).await;
                        }
                        if let Some(source) = &uncached.source_package {
                            let mut cache = bug_cache.write().await;
                            cache.prefetch_bugs_for_package(source).await;
                        }
                        for pkg in &uncached.binary_packages {
                            {
                                let mut cache = bug_cache.write().await;
                                cache.prefetch_bugs_for_binary_package(pkg).await;
                            }
                            {
                                let mut cache = popcon_cache.write().await;
                                cache.get_inst_count(pkg).await;
                            }
                            {
                                let mut cache = rdeps_cache.write().await;
                                cache.get_rdeps_count(pkg).await;
                            }
                        }
                        let _ = client.code_lens_refresh().await;
                    });
                }

                if lenses.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(lenses))
                }
            }
            FileType::Copyright => {
                let workspace = self.workspace.lock().await;
                let source_text = workspace.source_text(file.source_file);
                let parsed = workspace.get_parsed_copyright(file.source_file);
                drop(workspace);

                // Derive the source root from the copyright file URI
                // (debian/copyright -> parent is debian/ -> parent is source root)
                let source_root = uri.to_file_path().and_then(|p| {
                    p.parent()
                        .and_then(|debian| debian.parent())
                        .map(|root| root.to_path_buf())
                });

                let lenses = copyright::generate_code_lenses(
                    &parsed,
                    &source_text,
                    source_root.as_deref(),
                    &self.git_file_cache,
                )
                .await;
                if lenses.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(lenses))
                }
            }
            _ => Ok(None),
        }
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = &params.text_document.uri;

        let files = self.files.lock().await;
        let file = match files.get(uri) {
            Some(f) => *f,
            None => return Ok(None),
        };
        drop(files);

        match file.file_type {
            FileType::Changelog => {
                let workspace = self.workspace.lock().await;
                let source_text = workspace.source_text(file.source_file);
                let parsed = workspace.get_parsed_changelog(file.source_file);
                let hints = changelog::generate_inlay_hints(&parsed, &source_text, &params.range);
                if hints.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(hints))
                }
            }
            FileType::Control => {
                let workspace = self.workspace.lock().await;
                let source_text = workspace.source_text(file.source_file);
                let parsed = workspace.get_parsed_control(file.source_file);

                // Resolve substvars from changelog and .substvars files
                let resolved_substvars = {
                    let mut map = std::collections::HashMap::new();
                    if let Some(version) = Self::get_changelog_version(uri, &self.files, &workspace)
                    {
                        map.insert("binary:Version".to_string(), version.clone());
                        map.insert("source:Version".to_string(), version);
                    }
                    Self::read_substvars_files(uri, &mut map);
                    map
                };

                drop(workspace); // Release lock before async package cache access
                let ctx = control::inlay_hints::HintContext {
                    package_cache: &self.package_cache,
                    resolved_substvars: &resolved_substvars,
                };
                let (hints, uncached_packages) =
                    control::generate_inlay_hints(&parsed, &source_text, &params.range, &ctx).await;

                // Load uncached packages in the background (two batch
                // subprocess calls), then ask the editor to re-request hints.
                if !uncached_packages.is_empty() {
                    let cache = self.package_cache.clone();
                    let client = self.client.clone();
                    tokio::spawn(async move {
                        let mut c = cache.write().await;
                        c.load_versions_batch(&uncached_packages).await;
                        c.load_providers_batch(&uncached_packages).await;
                        drop(c);
                        let _ = client.inlay_hint_refresh().await;
                    });
                }

                if hints.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(hints))
                }
            }
            _ => Ok(None),
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let files = self.files.lock().await;
        let file = match files.get(uri) {
            Some(f) => *f,
            None => return Ok(None),
        };
        drop(files);

        let workspace = self.workspace.lock().await;
        let source_text = workspace.source_text(file.source_file);

        match file.file_type {
            FileType::Control => {
                let parsed = workspace.get_parsed_control(file.source_file);
                Ok(control::get_hover(
                    parsed.tree().as_deb822(),
                    &source_text,
                    position,
                ))
            }
            FileType::Copyright => {
                let parsed = workspace.get_parsed_copyright(file.source_file);
                let copyright = parsed.tree();
                Ok(copyright::get_hover(
                    copyright.as_deb822(),
                    &source_text,
                    position,
                ))
            }
            FileType::Watch => {
                let parsed = workspace.get_parsed_watch(file.source_file);
                let wf = parsed.to_watch_file();
                match &wf {
                    debian_watch::parse::ParsedWatchFile::Deb822(wf) => {
                        Ok(watch::get_hover(wf.as_deb822(), &source_text, position))
                    }
                    _ => Ok(None),
                }
            }
            FileType::Changelog => {
                let parsed = workspace.get_parsed_changelog(file.source_file);
                drop(workspace);
                Ok(changelog::get_hover(&parsed, &source_text, position, &self.bug_cache).await)
            }
            FileType::UpstreamMetadata => {
                let parsed = workspace.get_parsed_upstream_metadata(file.source_file);
                let yaml_file = parsed.tree();
                match yaml_file.document() {
                    Some(doc) => Ok(upstream_metadata::get_hover(&doc, &source_text, position)),
                    None => Ok(None),
                }
            }
            _ => Ok(None),
        }
    }

    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        let uri = &params.text_document.uri;

        let files = self.files.lock().await;
        let file = match files.get(uri) {
            Some(f) => *f,
            None => return Ok(None),
        };
        drop(files);

        match file.file_type {
            FileType::UpstreamMetadata => {
                let workspace = self.workspace.lock().await;
                let source_text = workspace.source_text(file.source_file);
                let parsed = workspace.get_parsed_upstream_metadata(file.source_file);
                let yaml_file = parsed.tree();
                match yaml_file.document() {
                    Some(doc) => Ok(Some(upstream_metadata::get_document_links(
                        &doc,
                        &source_text,
                    ))),
                    None => Ok(None),
                }
            }
            _ => Ok(None),
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let files = self.files.lock().await;
        let file = match files.get(uri) {
            Some(f) => *f,
            None => return Ok(None),
        };
        drop(files);

        match file.file_type {
            FileType::Control => {
                let workspace = self.workspace.lock().await;
                let source_text = workspace.source_text(file.source_file);
                let parsed = workspace.get_parsed_control(file.source_file);
                let location = control::goto_definition(&parsed, &source_text, position, uri);
                Ok(location.map(GotoDefinitionResponse::Scalar))
            }
            _ => Ok(None),
        }
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;

        let files = self.files.lock().await;
        let file = match files.get(uri) {
            Some(f) => *f,
            None => return Ok(None),
        };
        drop(files);

        match file.file_type {
            FileType::Control => {
                let workspace = self.workspace.lock().await;
                let source_text = workspace.source_text(file.source_file);
                let parsed = workspace.get_parsed_control(file.source_file);
                let refs = control::find_references(
                    &parsed,
                    &source_text,
                    position,
                    uri,
                    include_declaration,
                );
                if refs.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(refs))
                }
            }
            _ => Ok(None),
        }
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        if params.command == control::code_lens::OPEN_URL_COMMAND {
            if let Some(url) = params.arguments.first().and_then(|v| v.as_str()) {
                if let Ok(uri) = url.parse::<Uri>() {
                    let _ = self
                        .client
                        .show_document(ShowDocumentParams {
                            uri,
                            external: Some(true),
                            take_focus: Some(true),
                            selection: None,
                        })
                        .await;
                }
            }
        }
        Ok(None)
    }
}

/// Command-line interface for debian-lsp.
#[derive(Parser, Debug)]
#[command(name = "debian-lsp", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

/// Subcommands for debian-lsp.
#[derive(Subcommand, Debug)]
enum Command {
    /// Check Debian files and report diagnostics to stdout.
    ///
    /// Output is in gcc-style format: filename:line: severity: message.
    /// Paths can be files or directories; directories are walked recursively
    /// and any recognized Debian files are checked.
    Check {
        /// Files or directories to check.
        paths: Vec<std::path::PathBuf>,
    },
}

/// Severity level for a diagnostic, used in gcc-style output.
fn severity_label(severity: Option<DiagnosticSeverity>) -> &'static str {
    match severity {
        Some(DiagnosticSeverity::ERROR) => "error",
        Some(DiagnosticSeverity::WARNING) => "warning",
        Some(DiagnosticSeverity::INFORMATION) => "note",
        Some(DiagnosticSeverity::HINT) => "note",
        _ => "error",
    }
}

/// Directory names to skip when walking recursively.
const SKIP_DIRS: &[&str] = &[".git", ".hg", ".svn", ".bzr", "__pycache__", ".tox"];

/// Collect all regular files under `dir` recursively, skipping VCS directories.
fn collect_files_recursive(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{}: error: {}", dir.display(), e);
            return files;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if SKIP_DIRS.contains(&name) {
                    continue;
                }
            }
            files.extend(collect_files_recursive(&path));
        } else if path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    files
}

/// Run one-off diagnostics on the given paths, printing gcc-style output.
///
/// Paths can be files or directories. Directories are walked recursively
/// and any recognized Debian files found within are checked.
///
/// Returns the number of errors found (for the exit code).
fn run_check(paths: &[std::path::PathBuf]) -> i32 {
    let mut workspace = Workspace::new();
    let mut error_count: i32 = 0;

    // Expand directories into individual files, tracking which were explicit.
    let explicit_paths: std::collections::HashSet<std::path::PathBuf> =
        paths.iter().filter(|p| !p.is_dir()).cloned().collect();
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            files.extend(collect_files_recursive(path));
        } else {
            files.push(path.clone());
        }
    }

    for path in &files {
        // Determine the file type before reading, so we skip non-Debian files
        // without trying to read them (they may be binary, etc.).
        let abs_path = match std::fs::canonicalize(path) {
            Ok(p) => p,
            Err(_) => path.to_path_buf(),
        };

        let uri = match Uri::from_file_path(&abs_path) {
            Some(u) => u,
            None => {
                if explicit_paths.contains(path) {
                    eprintln!("{}: error: could not convert path to URI", path.display());
                    error_count += 1;
                }
                continue;
            }
        };

        let file_type = match FileType::detect(&uri) {
            Some(ft) => ft,
            None => {
                // Only warn for paths the user specified explicitly, not files
                // discovered by walking a directory.
                if explicit_paths.contains(path) {
                    eprintln!(
                        "{}: warning: unrecognized Debian file type, skipping",
                        path.display()
                    );
                }
                continue;
            }
        };

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{}: error: {}", path.display(), e);
                error_count += 1;
                continue;
            }
        };

        let source_file = workspace.update_file(uri, content.clone());

        let diagnostics = match Backend::collect_diagnostics(source_file, file_type, &workspace) {
            Some(d) => d,
            None => continue,
        };

        let display_path = path.display();
        for diag in &diagnostics {
            let line = diag.range.start.line + 1; // LSP lines are 0-based
            let col = diag.range.start.character + 1;
            let severity = severity_label(diag.severity);
            println!(
                "{}:{}:{}: {}: {}",
                display_path, line, col, severity, diag.message
            );
            if severity == "error" {
                error_count += 1;
            }
        }
    }

    error_count
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Check { paths }) => {
            let errors = run_check(&paths);
            std::process::exit(if errors > 0 { 1 } else { 0 });
        }
        None => {
            // Default: run as LSP server
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .with_ansi(false)
                .init();

            let stdin = tokio::io::stdin();
            let stdout = tokio::io::stdout();

            // Load package cache in background
            let package_cache = package_cache::new_shared_cache();
            let cache_for_loading = package_cache.clone();
            tokio::spawn(async move {
                package_cache::stream_packages_into(&cache_for_loading).await;
            });

            // Load architecture list in background
            let architecture_list = architecture::new_shared_list();
            let arch_for_loading = architecture_list.clone();
            tokio::spawn(async move {
                architecture::stream_into(&arch_for_loading).await;
            });

            let udd_pool = udd::shared_pool();
            let bug_cache = bugs::new_shared_bug_cache(udd_pool.clone());
            let vcswatch_cache = vcswatch::new_shared_vcswatch_cache(udd_pool.clone());
            let popcon_cache = popcon::new_shared_popcon_cache(udd_pool.clone());
            let maintainer_cache = maintainers::new_shared_maintainer_cache(udd_pool.clone());
            let rdeps_cache = rdeps::new_shared_rdeps_cache(udd_pool);

            let (service, socket) = LspService::new(|client| Backend {
                client,
                workspace: Arc::new(Mutex::new(Workspace::new())),
                files: Arc::new(Mutex::new(HashMap::new())),
                package_cache: package_cache.clone(),
                architecture_list: architecture_list.clone(),
                bug_cache: bug_cache.clone(),
                maintainer_cache: maintainer_cache.clone(),
                vcswatch_cache: vcswatch_cache.clone(),
                popcon_cache: popcon_cache.clone(),
                rdeps_cache: rdeps_cache.clone(),
                git_file_cache: copyright::code_lens::new_shared_git_file_cache(),
                lintian_tag_cache: Arc::new(tokio::sync::RwLock::new(
                    lintian_overrides::LintianTagCache::new(),
                )),
                upstream_cache: upstream_metadata::upstream_cache::new_shared(),
                settings: Arc::new(Mutex::new(Settings::default())),
            });

            Server::new(stdin, stdout, socket).serve(service).await;
        }
    }
}

#[cfg(test)]
mod main_tests {
    use super::*;

    #[test]
    fn test_completion_returns_control_completions() {
        let text = "Source: test\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = control::get_completions(&deb822, text, Position::new(0, 3));
        assert!(!completions.is_empty());
        assert!(completions.iter().any(|c| c.label == "Source"));
    }

    #[test]
    fn test_workspace_integration() {
        // Test that the workspace can parse control files
        let mut workspace = workspace::Workspace::new();
        let url = str::parse("file:///debian/control").unwrap();
        let content = "source: test-package\nMaintainer: Test <test@example.com>\n";

        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_control(file);

        // Should parse correctly
        assert!(parsed.errors().is_empty());

        if let Ok(control) = parsed.to_result() {
            let mut field_names = Vec::new();
            for paragraph in control.as_deb822().paragraphs() {
                for entry in paragraph.entries() {
                    if let Some(name) = entry.key() {
                        field_names.push(name);
                    }
                }
            }
            assert!(field_names.contains(&"source".to_string()));
            assert!(field_names.contains(&"Maintainer".to_string()));
        }
    }

    #[test]
    fn test_field_casing_detection() {
        // Test that we can detect incorrect field casing
        use control::get_standard_field_name;

        // Test correct casing - should return the same
        assert_eq!(get_standard_field_name("Source"), Some("Source"));
        assert_eq!(get_standard_field_name("Package"), Some("Package"));
        assert_eq!(get_standard_field_name("Maintainer"), Some("Maintainer"));

        // Test incorrect casing - should return the standard form
        assert_eq!(get_standard_field_name("source"), Some("Source"));
        assert_eq!(get_standard_field_name("package"), Some("Package"));
        assert_eq!(get_standard_field_name("maintainer"), Some("Maintainer"));
        assert_eq!(get_standard_field_name("MAINTAINER"), Some("Maintainer"));

        // Test unknown fields - should return None
        assert_eq!(get_standard_field_name("UnknownField"), None);
        assert_eq!(get_standard_field_name("random"), None);
    }

    #[test]
    fn test_changelog_action_generation() {
        // Test that we can generate a new changelog entry
        let changelog_content = r#"test-package (1.0-1) unstable; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;

        let parsed = debian_changelog::ChangeLog::parse(changelog_content);
        let changelog = parsed.tree();

        let result = changelog::generate_new_changelog_entry(&changelog);
        assert!(result.is_ok(), "Should successfully generate entry");

        let new_entry = result.unwrap();

        // Parse the lines to verify exact structure
        let lines: Vec<&str> = new_entry.lines().collect();
        assert!(lines.len() >= 5, "Should have at least 5 lines");

        // Check the header line exactly (version is incremented, uses UNRELEASED)
        assert_eq!(
            lines[0], "test-package (1.0-2) UNRELEASED; urgency=medium",
            "First line should be header with incremented version and UNRELEASED"
        );

        // Check empty line after header
        assert_eq!(lines[1], "", "Second line should be empty");

        // Check bullet point line
        assert_eq!(lines[2], "  * ", "Third line should be bullet point");

        // Check empty line before signature
        assert_eq!(lines[3], "", "Fourth line should be empty");

        // Check signature line starts with proper format
        assert!(
            lines[4].starts_with(" -- "),
            "Fifth line should start with signature marker, got: {}",
            lines[4]
        );
    }

    #[test]
    fn test_changelog_version_increment_multiple_revisions() {
        // Test the version increment logic with different versions
        let changelog_text = r#"mypackage (2.5-3) unstable; urgency=low

  * Some changes.

 -- Jane Smith <jane@example.com>  Tue, 15 Feb 2025 10:30:00 +0000
"#;

        let parsed = debian_changelog::ChangeLog::parse(changelog_text);
        let changelog = parsed.tree();

        let result = changelog::generate_new_changelog_entry(&changelog);
        assert!(result.is_ok(), "Should successfully generate entry");

        let new_entry = result.unwrap();
        let lines: Vec<&str> = new_entry.lines().collect();

        // Check exact version increment (3 -> 4) with UNRELEASED
        assert_eq!(
            lines[0], "mypackage (2.5-4) UNRELEASED; urgency=medium",
            "Should increment debian revision from 3 to 4 with UNRELEASED"
        );
    }

    #[test]
    fn test_changelog_file_type_detection() {
        // Test that we correctly detect changelog files
        let changelog_uri: Uri = str::parse("file:///path/to/debian/changelog").unwrap();
        let control_uri: Uri = str::parse("file:///path/to/debian/control").unwrap();

        assert_eq!(FileType::detect(&changelog_uri), Some(FileType::Changelog));
        assert_eq!(FileType::detect(&control_uri), Some(FileType::Control));
    }

    #[test]
    fn test_incremental_edit_apply() {
        // Simulate applying an incremental edit like did_change does
        let mut text = "Source: test\nMaintainer: Alice\n".to_string();
        let range = Range::new(Position::new(0, 8), Position::new(0, 12));
        let text_range = position::try_lsp_range_to_text_range(&text, &range).unwrap();
        let start: usize = text_range.start().into();
        let end: usize = text_range.end().into();
        text.replace_range(start..end, "hello");
        assert_eq!(text, "Source: hello\nMaintainer: Alice\n");
    }

    #[test]
    fn test_incremental_edit_insert() {
        let mut text = "Source: test\n".to_string();
        // Insert at end of line 0
        let range = Range::new(Position::new(0, 12), Position::new(0, 12));
        let text_range = position::try_lsp_range_to_text_range(&text, &range).unwrap();
        let start: usize = text_range.start().into();
        let end: usize = text_range.end().into();
        text.replace_range(start..end, "-pkg");
        assert_eq!(text, "Source: test-pkg\n");
    }

    #[test]
    fn test_incremental_edit_delete() {
        let mut text = "Source: test-pkg\n".to_string();
        let range = Range::new(Position::new(0, 8), Position::new(0, 16));
        let text_range = position::try_lsp_range_to_text_range(&text, &range).unwrap();
        let start: usize = text_range.start().into();
        let end: usize = text_range.end().into();
        text.replace_range(start..end, "");
        assert_eq!(text, "Source: \n");
    }

    #[test]
    fn test_incremental_edit_multiline() {
        let mut text = "Source: test\nMaintainer: Alice\nPriority: optional\n".to_string();
        // Replace entire second line
        let range = Range::new(Position::new(1, 0), Position::new(2, 0));
        let text_range = position::try_lsp_range_to_text_range(&text, &range).unwrap();
        let start: usize = text_range.start().into();
        let end: usize = text_range.end().into();
        text.replace_range(start..end, "Maintainer: Bob\n");
        assert_eq!(text, "Source: test\nMaintainer: Bob\nPriority: optional\n");
    }

    #[test]
    fn test_workspace_update_file_reuses_input() {
        let mut workspace = workspace::Workspace::new();
        let url: Uri = str::parse("file:///debian/control").unwrap();

        let file1 = workspace.update_file(url.clone(), "Source: a\n".to_string());
        let file2 = workspace.update_file(url.clone(), "Source: b\n".to_string());

        // Should reuse the same SourceFile input
        assert_eq!(file1, file2);
        // Text should be updated
        assert_eq!(workspace.source_text(file2), "Source: b\n");
    }

    #[test]
    fn test_upstream_metadata_file_type_detection() {
        let metadata_uri: Uri = str::parse("file:///path/to/debian/upstream/metadata").unwrap();
        let non_metadata_uri: Uri = str::parse("file:///path/to/upstream/metadata").unwrap();

        assert_eq!(
            FileType::detect(&metadata_uri),
            Some(FileType::UpstreamMetadata)
        );
        assert_eq!(FileType::detect(&non_metadata_uri), None);
    }

    #[test]
    fn test_source_options_file_type_detection() {
        let options_uri: Uri = str::parse("file:///path/to/debian/source/options").unwrap();
        let local_options_uri: Uri =
            str::parse("file:///path/to/debian/source/local-options").unwrap();
        let non_options_uri: Uri = str::parse("file:///path/to/debian/options").unwrap();

        assert_eq!(
            FileType::detect(&options_uri),
            Some(FileType::SourceOptions)
        );
        assert_eq!(
            FileType::detect(&local_options_uri),
            Some(FileType::SourceOptions)
        );
        assert_eq!(FileType::detect(&non_options_uri), None);
    }
}
