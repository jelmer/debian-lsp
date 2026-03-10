use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::RwLock;

/// Thread-safe shared architecture list.
pub type SharedArchitectureList = Arc<RwLock<Vec<String>>>;

/// Create a new empty shared architecture list.
pub fn new_shared_list() -> SharedArchitectureList {
    Arc::new(RwLock::new(Vec::new()))
}

/// Stream architecture names from `dpkg-architecture -L` into the shared list.
///
/// Each architecture name is inserted (in sorted order) as soon as it is
/// read, so completions are available immediately while still loading.
pub async fn stream_into(list: &SharedArchitectureList) {
    let Ok(mut child) = Command::new("dpkg-architecture")
        .arg("-L")
        .stdout(std::process::Stdio::piped())
        .spawn()
    else {
        return;
    };

    let Some(stdout) = child.stdout.take() else {
        return;
    };

    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if !line.is_empty() {
            let mut arches = list.write().await;
            let pos = arches.binary_search(&line).unwrap_or_else(|p| p);
            arches.insert(pos, line);
        }
    }
}
