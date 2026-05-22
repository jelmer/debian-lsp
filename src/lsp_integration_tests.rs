use super::*;
#[cfg(feature = "lintian-brush")]
use futures::StreamExt;
use serde_json::json;
use tower_lsp_server::jsonrpc::Request;
use tower_lsp_server::LspService;
use tower_service::Service;

async fn setup_server() -> (LspService<Backend>, tower_lsp_server::ClientSocket) {
    let package_cache = package_cache::new_shared_cache();
    let architecture_list = architecture::new_shared_list();
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
        package_cache,
        architecture_list,
        bug_cache,
        maintainer_cache,
        vcswatch_cache,
        popcon_cache,
        rdeps_cache,
        git_file_cache: copyright::code_lens::new_shared_git_file_cache(),
        lintian_tag_cache: Arc::new(tokio::sync::RwLock::new(
            lintian_overrides::LintianTagCache::new(),
        )),
        upstream_cache: upstream_metadata::upstream_cache::new_shared(),
        #[cfg(feature = "multiarch-hints")]
        multiarch_hints_store: multiarch_hints::hints::HintsStore::default(),
        settings: Arc::new(Mutex::new(Settings::default())),
    });
    (service, socket)
}

#[tokio::test]
async fn test_initialize() {
    let (mut service, _) = setup_server().await;

    let req: Request = serde_json::from_value(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "capabilities": {}
        }
    }))
    .unwrap();

    let response = service.call(req).await.unwrap();
    assert!(response.is_some());
    let res = serde_json::to_value(response.unwrap()).unwrap();
    assert_eq!(res["id"], 1);
    assert!(res["result"]["capabilities"].is_object());
}

#[tokio::test]
async fn test_wrap_and_sort_code_action() {
    let (mut service, _) = setup_server().await;

    // Initialize
    let init_req: Request = serde_json::from_value(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "capabilities": {}
        }
    }))
    .unwrap();
    let _ = service.call(init_req).await.unwrap();

    // didOpen
    let open_req: Request = serde_json::from_value(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///debian/control",
                "languageId": "debcontrol",
                "version": 1,
                "text": "Source: test-pkg\nBuild-Depends: a, c, b\n"
            }
        }
    }))
    .unwrap();
    let _ = service.call(open_req).await.unwrap();

    // codeAction
    let ca_req: Request = serde_json::from_value(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": { "uri": "file:///debian/control" },
            "range": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 22 }
            },
            "context": { "diagnostics": [] }
        }
    }))
    .unwrap();
    let response = service.call(ca_req).await.unwrap();
    let res = serde_json::to_value(response.unwrap()).unwrap();

    let actions = res["result"].as_array().expect("result should be an array");
    assert!(!actions.is_empty());
    assert!(actions
        .iter()
        .any(|a| a["title"].as_str() == Some("Wrap and sort")));
}

#[tokio::test]
#[cfg(feature = "lintian-brush")]
async fn test_lintian_brush_diagnostics_integration() {
    use tokio::time::{timeout, Duration};

    let temp = tempfile::tempdir().unwrap();
    let debian_dir = temp.path().join("debian");
    std::fs::create_dir(&debian_dir).unwrap();

    // Create debian/changelog so lintian-brush can identify the package
    let changelog_path = debian_dir.join("changelog");
    let changelog_text = "test-pkg (1.0-1) unstable; urgency=low\n\n  * Initial release.\n\n -- Alice <alice@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n";
    std::fs::write(&changelog_path, changelog_text).unwrap();

    let control_path = debian_dir.join("control");
    let uri = Uri::from_file_path(&control_path).unwrap();

    let (mut service, mut socket) = setup_server().await;

    // Start a background task to collect messages from the socket
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(msg) = socket.next().await {
            let _ = tx.send(msg);
        }
    });

    // Initialize
    let _ = service
        .call(
            serde_json::from_value(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": { "capabilities": {} }
            }))
            .unwrap(),
        )
        .await
        .unwrap();

    // didOpen
    // Intentional lowercase 'source' to trigger built-in diagnostic
    // and old Standards-Version for lintian-brush
    let text = "source: test-pkg\nStandards-Version: 3.9.8\nMaintainer: Alice <alice@example.com>\n\nPackage: test-pkg\nArchitecture: all\nDescription: test\n";
    let _ = service
        .call(
            serde_json::from_value(json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": uri.clone(),
                        "languageId": "debcontrol",
                        "version": 1,
                        "text": text
                    }
                }
            }))
            .unwrap(),
        )
        .await
        .unwrap();

    // Wait for the diagnostic with a timeout. Collect the lintian-brush
    // diagnostics so they can be fed back into the codeAction request
    // the way a real editor does.
    let mut found_builtin = false;
    let mut lb_diags: Vec<serde_json::Value> = Vec::new();

    let _ = timeout(Duration::from_secs(15), async {
        while let Some(msg) = rx.recv().await {
            let msg_json = serde_json::to_value(msg).unwrap();
            if msg_json["method"] == "textDocument/publishDiagnostics" {
                let diags = msg_json["params"]["diagnostics"].as_array().unwrap();
                if diags
                    .iter()
                    .any(|d| d["message"].as_str().unwrap_or("").contains("Source"))
                {
                    found_builtin = true;
                }
                let lb: Vec<_> = diags
                    .iter()
                    .filter(|d| d["source"].as_str() == Some("lintian-brush"))
                    .cloned()
                    .collect();
                if !lb.is_empty() {
                    lb_diags = lb;
                }
                if found_builtin && !lb_diags.is_empty() {
                    return;
                }
            }
        }
    })
    .await;

    assert!(found_builtin, "Did not receive built-in diagnostics");
    assert!(
        !lb_diags.is_empty(),
        "Did not receive lintian-brush diagnostics"
    );

    // Each published lintian-brush diagnostic must carry its fix plans
    // on the `data` field — that is what lets codeAction reconstruct the
    // quick fix without re-running detectors.
    for d in &lb_diags {
        assert!(
            !d["data"].is_null(),
            "lintian-brush diagnostic should carry fix data, got {d}"
        );
    }

    // Request code actions over the Standards-Version line, feeding the
    // published lintian-brush diagnostic back in `context.diagnostics`
    // the way a real editor does. The handler reconstructs the fix from
    // the diagnostic's `data`.
    let ca_req: Request = serde_json::from_value(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 24 }
            },
            "context": { "diagnostics": lb_diags }
        }
    }))
    .unwrap();
    let response = service.call(ca_req).await.unwrap();
    let res = serde_json::to_value(response.unwrap()).unwrap();

    let actions = res["result"]
        .as_array()
        .unwrap_or_else(|| panic!("result should be an array, got {res}"));
    assert!(
        actions
            .iter()
            .any(|a| a["kind"].as_str() == Some("quickfix") && a["diagnostics"].is_array()),
        "expected a lintian-brush quickfix reconstructed from diagnostic data, got {actions:?}"
    );
}

#[tokio::test]
#[cfg(feature = "lintian-brush")]
async fn test_binary_package_fix_targets_correct_paragraph() {
    use tokio::time::{timeout, Duration};

    let temp = tempfile::tempdir().unwrap();
    let debian_dir = temp.path().join("debian");
    std::fs::create_dir(&debian_dir).unwrap();

    let changelog_path = debian_dir.join("changelog");
    let changelog_text = "mypkg (1.0-1) unstable; urgency=low\n\n  * Initial release.\n\n -- Alice <alice@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n";
    std::fs::write(&changelog_path, changelog_text).unwrap();

    let control_path = debian_dir.join("control");
    // Source is ALREADY optional. Binary is extra.
    let text = "Source: mypkg\nPriority: optional\nMaintainer: Alice <alice@example.com>\n\nPackage: binary-pkg\nArchitecture: all\nPriority: extra\nDescription: test\n";
    std::fs::write(&control_path, text).unwrap();
    let uri = Uri::from_file_path(&control_path).unwrap();

    let (mut service, mut socket) = setup_server().await;

    // Start a background task to collect messages from the socket
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(msg) = socket.next().await {
            let _ = tx.send(msg);
        }
    });

    // Initialize
    let _ = service
        .call(
            serde_json::from_value(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": { "capabilities": {} }
            }))
            .unwrap(),
        )
        .await
        .unwrap();

    // didOpen
    let _ = service
        .call(
            serde_json::from_value(json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": uri.clone(),
                        "languageId": "debcontrol",
                        "version": 1,
                        "text": text
                    }
                }
            }))
            .unwrap(),
        )
        .await
        .unwrap();

    // Wait for the diagnostics
    let mut lb_diags = Vec::new();
    let _ = timeout(Duration::from_secs(15), async {
        while let Some(msg) = rx.recv().await {
            let msg_json = serde_json::to_value(msg).unwrap();
            if msg_json["method"] == "textDocument/publishDiagnostics" {
                let diags = msg_json["params"]["diagnostics"].as_array().unwrap();
                for d in diags {
                    if d["source"].as_str() == Some("lintian-brush") {
                        lb_diags.push(d.clone());
                    }
                }
                // We only expect ONE diagnostic now (the one in the binary package)
                if !lb_diags.is_empty() {
                    return;
                }
            }
        }
    })
    .await;

    let binary_diag = lb_diags
        .iter()
        .find(|d| d["range"]["start"]["line"] == 6)
        .expect("Should have diagnostic at line 6");

    // Request code actions at line 6
    let ca_req: Request = serde_json::from_value(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": 6, "character": 0 },
                "end": { "line": 6, "character": 15 }
            },
            "context": { "diagnostics": [binary_diag] }
        }
    }))
    .unwrap();
    let response = service.call(ca_req).await.unwrap();
    let res = serde_json::to_value(response.unwrap()).unwrap();

    let actions = res["result"].as_array().expect("result should be an array");

    let fix_action = actions
        .iter()
        .find(|a| a["title"].as_str() == Some("Change priority extra to priority optional."))
        .expect("Should have priority fix action");

    let edit = &fix_action["edit"]["documentChanges"][0]["edits"][0];
    assert_eq!(
        edit["range"]["start"]["line"], 6,
        "Edit should target line 6, but targeted line {}",
        edit["range"]["start"]["line"]
    );
}
