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
