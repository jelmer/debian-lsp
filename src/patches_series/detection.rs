use std::collections::HashSet;
use std::fs;
use std::path::Path;
use tower_lsp_server::ls_types::Uri;

/// Check if a given URL represents a Debian patches/series file
pub fn is_patches_series_file(uri: &Uri) -> bool {
    let path = uri.as_str();
    path.ends_with("/debian/patches/series")
}

// Get all files in a debian/patches folder
pub fn list_patch_files(uri: &Uri) -> HashSet<String> {
    let Some(path) = uri.to_file_path() else {
        return HashSet::new();
    };
    let Some(patches_dir) = path.parent() else {
        return HashSet::new();
    };
    let patches_dir = patches_dir.to_path_buf();
    let mut result = HashSet::new();
    collect_patches(&patches_dir, &patches_dir, &mut result);
    result
}

fn collect_patches(base: &Path, dir: &Path, result: &mut HashSet<String>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_patches(base, &path, result);
            } else {
                if path.file_name().and_then(|n| n.to_str()) == Some("series") {
                    continue;
                }
                if let Ok(relative) = path.strip_prefix(base) {
                    if let Some(s) = relative.to_str() {
                        result.insert(s.to_string());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_patches_series_file() {
        let tests_patches_series_paths = vec![
            "file:///path/to/debian/patches/series",
            "file:///project/debian/patches/series",
        ];
        let non_tests_patches_series_paths = vec![
            "file:///path/to/other.txt",
            "file:///path/to/debian/control",
            "file:///path/to/debian/copyright",
            "file:///path/to/debian/watch",
            "file:///path/to/patches/series", // Not in debian/ directory
            "file:///path/to/debian/tests/control.backup",
        ];
        for path in tests_patches_series_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(
                is_patches_series_file(&uri),
                "Should detect tests/patches/series file: {}",
                path
            );
        }
        for path in non_tests_patches_series_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(
                !is_patches_series_file(&uri),
                "Should not detect as tests/patches/series file: {}",
                path
            );
        }
    }

    #[test]
    fn test_collect_patches_excludes_series_file() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        std::fs::write(base.join("fix-arm.patch"), "").unwrap();
        std::fs::write(base.join("fix-mips.diff"), "").unwrap();
        std::fs::write(base.join("fix-no-extension"), "").unwrap();
        std::fs::write(base.join("series"), "").unwrap(); // doit être exclu

        let mut result = std::collections::HashSet::new();
        collect_patches(base, base, &mut result);

        assert!(result.contains("fix-arm.patch"));
        assert!(result.contains("fix-mips.diff"));
        assert!(result.contains("fix-no-extension"));
        assert!(!result.contains("series")); // series exclu ✓
    }

    #[test]
    fn test_collect_patches_recursive() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        std::fs::create_dir(base.join("upstream")).unwrap();
        std::fs::write(base.join("fix-arm.patch"), "").unwrap();
        std::fs::write(base.join("upstream").join("fix-leak.patch"), "").unwrap();
        std::fs::write(base.join("upstream").join("fix-mem.diff"), "").unwrap();

        let mut result = std::collections::HashSet::new();
        collect_patches(base, base, &mut result);

        assert!(result.contains("fix-arm.patch"));
        assert!(result.contains(
            &std::path::Path::new("upstream")
                .join("fix-leak.patch")
                .to_string_lossy()
                .to_string()
        ));
        assert!(result.contains(
            &std::path::Path::new("upstream")
                .join("fix-mem.diff")
                .to_string_lossy()
                .to_string()
        ));
    }

    #[test]
    fn test_collect_patches_accepts_diff_extension() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        std::fs::write(base.join("fix-arm.diff"), "").unwrap();

        let mut result = std::collections::HashSet::new();
        collect_patches(base, base, &mut result);

        assert!(result.contains("fix-arm.diff"));
    }

    #[test]
    fn test_collect_patches_accepts_no_extension() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        std::fs::write(base.join("001-fix-build"), "").unwrap();

        let mut result = std::collections::HashSet::new();
        collect_patches(base, base, &mut result);

        assert!(result.contains("001-fix-build"));
    }

    #[test]
    fn test_collect_patches_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        let mut result = std::collections::HashSet::new();
        collect_patches(base, base, &mut result);

        assert!(result.is_empty());
    }
}
