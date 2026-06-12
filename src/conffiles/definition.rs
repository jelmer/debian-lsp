use tower_lsp_server::ls_types::{Location, Position, Range, Uri};

use crate::position::Source;

/// Try to resolve go-to-definition for a path entry in a debian/conffiles file.
///
/// Returns a `Location` pointing to the file in the debhelper staging directory
/// (debian/<package>/etc/...) if the cursor is on a path that exists on the filesystem.
pub fn goto_definition(src: Source<'_>, position: Position, uri: &Uri) -> Option<Location> {
    let current_line = src
        .text
        .lines()
        .nth(position.line as usize)
        .unwrap_or("")
        .trim();

    // Extract the path, strip remove-on-upgrade flag if present
    let path_str = if let Some(rest) = current_line.strip_prefix("remove-on-upgrade ") {
        rest.trim()
    } else {
        current_line
    };

    // Must be an absolute path
    if !path_str.starts_with('/') {
        return None;
    }

    // Resolve against debhelper staging directories: debian/<package>/etc/...
    let conffiles_path = uri.to_file_path()?;
    let debian_dir = conffiles_path.parent()?;

    // Strip leading / to get relative path (e.g. etc/myapp/config.conf)
    let rel = path_str.trim_start_matches('/');

    let Ok(entries) = std::fs::read_dir(debian_dir) else {
        return None;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let staging_path = path.join(rel);
        if staging_path.exists() {
            let file_uri = Uri::from_file_path(&staging_path)?;
            return Some(Location {
                uri: file_uri,
                range: Range::new(Position::new(0, 0), Position::new(0, 0)),
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;

    fn write_conffiles(dir: &std::path::Path, content: &str) -> Uri {
        let debian_dir = dir.join("debian");
        std::fs::create_dir_all(&debian_dir).unwrap();
        let path = debian_dir.join("conffiles");
        std::fs::write(&path, content).unwrap();
        Uri::from_file_path(&path).unwrap()
    }

    fn write_staging_file(dir: &std::path::Path, package: &str, rel: &str) {
        let path = dir.join("debian").join(package).join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "").unwrap();
    }

    #[test]
    fn test_goto_existing_path() {
        let dir = tempfile::tempdir().unwrap();
        write_staging_file(dir.path(), "myapp", "etc/myapp/config.conf");
        let content = "/etc/myapp/config.conf\n";
        let uri = write_conffiles(dir.path(), content);
        let idx = LineIndex::new(content);
        let src = Source::new(content, &idx);

        let result = goto_definition(src, Position::new(0, 5), &uri);
        assert!(result.is_some());
    }

    #[test]
    fn test_goto_remove_on_upgrade() {
        let dir = tempfile::tempdir().unwrap();
        write_staging_file(dir.path(), "myapp", "etc/myapp/old.conf");
        let content = "remove-on-upgrade /etc/myapp/old.conf\n";
        let uri = write_conffiles(dir.path(), content);
        let idx = LineIndex::new(content);
        let src = Source::new(content, &idx);

        let result = goto_definition(src, Position::new(0, 5), &uri);
        assert!(result.is_some());
    }

    #[test]
    fn test_goto_missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let content = "/etc/myapp/missing.conf\n";
        let uri = write_conffiles(dir.path(), content);
        let idx = LineIndex::new(content);
        let src = Source::new(content, &idx);

        let result = goto_definition(src, Position::new(0, 5), &uri);
        assert!(result.is_none());
    }

    #[test]
    fn test_goto_relative_path_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let content = "etc/myapp/config.conf\n";
        let uri = write_conffiles(dir.path(), content);
        let idx = LineIndex::new(content);
        let src = Source::new(content, &idx);

        let result = goto_definition(src, Position::new(0, 5), &uri);
        assert!(result.is_none());
    }

    #[test]
    fn test_goto_empty_line_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let content = "\n/etc/myapp/config.conf\n";
        let uri = write_conffiles(dir.path(), content);
        let idx = LineIndex::new(content);
        let src = Source::new(content, &idx);

        let result = goto_definition(src, Position::new(0, 0), &uri);
        assert!(result.is_none());
    }
}
