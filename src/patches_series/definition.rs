// patches_series/definition.rs

use tower_lsp_server::ls_types::{Location, Position, Range, Uri};

use crate::position::Source;

/// Try to resolve go-to-definition for a patch entry in a patches/series file.
///
/// Returns a `Location` pointing to the patch file in `debian/patches/`,
/// if the cursor is on a patch name that exists on the filesystem.
pub fn goto_definition(
    parsed: &patchkit::edit::Parse<patchkit::edit::series::lossless::SeriesFile>,
    src: Source<'_>,
    position: Position,
    uri: &Uri,
) -> Option<Location> {
    let offset = src.try_position_to_offset(position)?;

    // Find the patch entry whose name token spans the cursor offset.
    let patch_entry = parsed.tree().patch_entries().find(|p| {
        if let Some(token) = p.name_token() {
            let range = token.text_range();
            range.start() <= offset && offset < range.end()
        } else {
            false
        }
    })?;

    let patch_name = patch_entry.name()?;

    // Resolve the patch file path relative to debian/patches/.
    let series_path = uri.to_file_path()?;
    let patches_dir = series_path.parent()?;
    let patch_path = patches_dir.join(patch_name);

    if !patch_path.exists() {
        return None;
    }

    let patch_uri = Uri::from_file_path(&patch_path)?;

    Some(Location {
        uri: patch_uri,
        range: Range::new(Position::new(0, 0), Position::new(0, 0)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;

    fn test_uri() -> Uri {
        if cfg!(windows) {
            Uri::from_file_path("C:\\tmp\\debian\\patches\\series").unwrap()
        } else {
            Uri::from_file_path("/tmp/debian/patches/series").unwrap()
        }
    }

    #[test]
    fn test_goto_definition_on_comment_returns_none() {
        let text = "# this is a comment\nfix-build-flags.patch -p1\n";
        let parsed = patchkit::edit::series::parse(text);
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);

        let result = goto_definition(&parsed, src, Position::new(0, 5), &test_uri());
        assert!(result.is_none());
    }

    #[test]
    fn test_goto_definition_on_option_returns_none() {
        let text = "fix-build-flags.patch -p1\n";
        let parsed = patchkit::edit::series::parse(text);
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);

        // col 22 = sur "-p1"
        let result = goto_definition(&parsed, src, Position::new(0, 22), &test_uri());
        assert!(result.is_none());
    }

    #[test]
    fn test_goto_definition_empty_file() {
        let text = "";
        let parsed = patchkit::edit::series::parse(text);
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);

        let result = goto_definition(&parsed, src, Position::new(0, 0), &test_uri());
        assert!(result.is_none());
    }

    #[test]
    fn test_goto_definition_existing_patch() {
        let dir = tempfile::tempdir().unwrap();
        let patches_dir = dir.path().join("debian").join("patches");
        std::fs::create_dir_all(&patches_dir).unwrap();
        let patch_file = patches_dir.join("fix-spelling.patch");
        std::fs::write(&patch_file, "--- a/foo\n+++ b/foo\n").unwrap();
        let series_path = patches_dir.join("series");

        let text = "fix-spelling.patch -p1\n";
        let parsed = patchkit::edit::series::parse(text);
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let uri = Uri::from_file_path(&series_path).unwrap();

        let result = goto_definition(&parsed, src, Position::new(0, 5), &uri);
        assert!(result.is_some(), "Should find the existing patch");
        let loc = result.unwrap();
        assert_eq!(loc.uri, Uri::from_file_path(&patch_file).unwrap());
        assert_eq!(loc.range.start.line, 0);
        assert_eq!(loc.range.start.character, 0);
    }

    #[test]
    fn test_goto_definition_missing_patch_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let patches_dir = dir.path().join("debian").join("patches");
        std::fs::create_dir_all(&patches_dir).unwrap();
        let series_path = patches_dir.join("series");

        let text = "missing-patch.patch -p1\n";
        let parsed = patchkit::edit::series::parse(text);
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let uri = Uri::from_file_path(&series_path).unwrap();

        let result = goto_definition(&parsed, src, Position::new(0, 5), &uri);
        assert!(result.is_none(), "Should return none");
    }

    #[test]
    fn test_goto_definition_second_entry() {
        let dir = tempfile::tempdir().unwrap();
        let patches_dir = dir.path().join("debian").join("patches");
        std::fs::create_dir_all(&patches_dir).unwrap();
        let patch_file = patches_dir.join("0002-disable-tests.patch");
        std::fs::write(&patch_file, "--- a/foo\n+++ b/foo\n").unwrap();
        let series_path = patches_dir.join("series");

        let text = "fix-build-flags.patch -p1\n0002-disable-tests.patch\n";
        let parsed = patchkit::edit::series::parse(text);
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let uri = Uri::from_file_path(&series_path).unwrap();

        let result = goto_definition(&parsed, src, Position::new(1, 5), &uri);
        assert!(result.is_some(), "Has to find second patch");
        let loc = result.unwrap();
        assert_eq!(loc.uri, Uri::from_file_path(&patch_file).unwrap());
    }
}
