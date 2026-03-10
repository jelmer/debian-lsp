//! Debian Language Server Protocol implementation.

#![deny(missing_docs)]
#![deny(unsafe_code)]

use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

mod architecture;
mod changelog;
mod control;
mod copyright;
mod deb822;
mod package_cache;
mod position;
mod source_format;
mod tests;
mod upstream_metadata;
mod watch;
mod workspace;

use position::{text_range_to_lsp_range, try_lsp_range_to_text_range};
use std::collections::HashMap;
use workspace::Workspace;

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
    /// debian/upstream/metadata file
    UpstreamMetadata,
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
        } else if upstream_metadata::is_upstream_metadata_file(uri) {
            Some(Self::UpstreamMetadata)
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

/// Check if two LSP ranges overlap
fn range_overlaps(a: &Range, b: &Range) -> bool {
    // Check if range a starts before b ends and b starts before a ends
    (a.start.line < b.end.line
        || (a.start.line == b.end.line && a.start.character <= b.end.character))
        && (b.start.line < a.end.line
            || (b.start.line == a.end.line && b.start.character <= a.end.character))
}

struct Backend {
    client: Client,
    workspace: Arc<Mutex<Workspace>>,
    files: Arc<Mutex<HashMap<Uri, FileInfo>>>,
    package_cache: package_cache::SharedPackageCache,
    architecture_list: architecture::SharedArchitectureList,
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
            FileType::Watch
            | FileType::TestsControl
            | FileType::Changelog
            | FileType::SourceFormat
            | FileType::UpstreamMetadata => None,
        }
    }
}

impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![
                        ":".to_string(),
                        " ".to_string(),
                        "(".to_string(),
                        "[".to_string(),
                        "<".to_string(),
                        "$".to_string(),
                    ]),
                    work_done_progress_options: Default::default(),
                    all_commit_characters: None,
                    completion_item: None,
                }),
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
                                ],
                                token_modifiers: vec![SemanticTokenModifier::DECLARATION],
                            },
                            range: Some(false),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                    ),
                ),
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

        if let Some(diagnostics) = Self::collect_diagnostics(source_file, file_type, &workspace) {
            drop(workspace);
            self.client
                .publish_diagnostics(params.text_document.uri.clone(), diagnostics, None)
                .await;
        }
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
        let file_type = files
            .get(&params.text_document.uri)
            .map(|info| info.file_type)
            .or_else(|| FileType::detect(&params.text_document.uri));

        let Some(file_type) = file_type else {
            return;
        };

        // Apply the content changes
        let Some(changes) = params.content_changes.first() else {
            return;
        };

        let mut workspace = self.workspace.lock().await;
        let source_file =
            workspace.update_file(params.text_document.uri.clone(), changes.text.clone());
        files.insert(
            params.text_document.uri.clone(),
            FileInfo {
                source_file,
                file_type,
            },
        );

        if let Some(diagnostics) = Self::collect_diagnostics(source_file, file_type, &workspace) {
            drop(workspace);
            self.client
                .publish_diagnostics(params.text_document.uri.clone(), diagnostics, None)
                .await;
        }
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
                drop(workspace);
                if let Some(deb822::completion::CursorContext::FieldValue {
                    field_name,
                    value_prefix,
                }) = &cursor_context
                {
                    // Try async completions (relationship fields via package cache)
                    if let Some(async_completions) = control::get_async_field_value_completions(
                        field_name,
                        value_prefix,
                        &self.package_cache,
                        &self.architecture_list,
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
                    let workspace = self.workspace.lock().await;
                    let source_text = workspace.source_text(source_file);
                    let parsed = workspace.get_parsed_control(source_file);
                    control::get_completions(parsed.tree().as_deb822(), &source_text, position)
                }
            }
            Some((FileType::Copyright, source_file)) => {
                let workspace = self.workspace.lock().await;
                let source_text = workspace.source_text(source_file);
                let parsed = workspace.get_parsed_copyright(source_file);
                let copyright = parsed.to_copyright();
                copyright::get_completions(copyright.as_deb822(), &source_text, position)
            }
            Some((FileType::Watch, _)) => watch::get_completions(&uri, position),
            Some((FileType::TestsControl, _)) => tests::get_completions(&uri, position),
            Some((FileType::Changelog, source_file)) => {
                let workspace = self.workspace.lock().await;
                let source_text = workspace.source_text(source_file);
                let parsed = workspace.get_parsed_changelog(source_file);
                changelog::get_completions(&parsed, &source_text, position)
            }
            Some((FileType::SourceFormat, _)) => source_format::get_completions(&uri, position),
            Some((FileType::UpstreamMetadata, source_file)) => {
                let workspace = self.workspace.lock().await;
                let source_text = workspace.source_text(source_file);
                upstream_metadata::get_completions(&source_text, position)
            }
            None => Vec::new(),
        };

        if completions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(CompletionResponse::Array(completions)))
        }
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

        // Check for field casing issues - only process fields in the requested range
        let Some(text_range) = try_lsp_range_to_text_range(&source_text, &params.range) else {
            return Ok(None);
        };

        match file_info.file_type {
            FileType::Control => {
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
                // Add action to create a new changelog entry
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
                            kind: Some(CodeActionKind::REFACTOR),
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
                let unreleased_entries =
                    workspace.find_unreleased_entries_in_range(file_info.source_file, text_range);

                for info in unreleased_entries {
                    let lsp_range = text_range_to_lsp_range(&source_text, info.unreleased_range);

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
            _ => unreachable!(),
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
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
            FileType::TestsControl => tests::generate_semantic_tokens(&source_text),
            FileType::UpstreamMetadata => upstream_metadata::generate_semantic_tokens(&source_text),
            // TODO: Implement semantic tokens for other file types
            _ => vec![],
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
}

#[tokio::main]
async fn main() {
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

    let (service, socket) = LspService::new(|client| Backend {
        client,
        workspace: Arc::new(Mutex::new(Workspace::new())),
        files: Arc::new(Mutex::new(HashMap::new())),
        package_cache: package_cache.clone(),
        architecture_list: architecture_list.clone(),
    });

    Server::new(stdin, stdout, socket).serve(service).await;
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
    fn test_upstream_metadata_file_type_detection() {
        let metadata_uri: Uri = str::parse("file:///path/to/debian/upstream/metadata").unwrap();
        let non_metadata_uri: Uri = str::parse("file:///path/to/upstream/metadata").unwrap();

        assert_eq!(
            FileType::detect(&metadata_uri),
            Some(FileType::UpstreamMetadata)
        );
        assert_eq!(FileType::detect(&non_metadata_uri), None);
    }
}
