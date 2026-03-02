/// Valid source format values for debian/source/format
pub const SOURCE_FORMATS: &[(&str, &str)] = &[
    (
        "3.0 (quilt)",
        "Source format with quilt-based patches (recommended)",
    ),
    (
        "3.0 (native)",
        "Source format for native Debian packages (no upstream)",
    ),
    ("1.0", "Legacy source format"),
    ("3.0 (git)", "Source format using git repository"),
    ("3.0 (custom)", "Custom source format"),
];

/// Check if a format string is valid
pub fn is_valid_format(format: &str) -> bool {
    SOURCE_FORMATS.iter().any(|(fmt, _)| *fmt == format.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_format() {
        assert!(is_valid_format("3.0 (quilt)"));
        assert!(is_valid_format("3.0 (native)"));
        assert!(is_valid_format("1.0"));
        assert!(is_valid_format("3.0 (git)"));
        assert!(is_valid_format("3.0 (custom)"));
        assert!(!is_valid_format("2.0"));
        assert!(!is_valid_format("invalid"));
        // Test with whitespace
        assert!(is_valid_format(" 3.0 (quilt) "));
    }
}
