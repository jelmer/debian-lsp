/// A dpkg-source option definition.
pub struct SourceOption {
    /// The long option name (without leading --)
    pub name: &'static str,
    /// Description of the option
    pub description: &'static str,
    /// Whether the option takes a value
    pub takes_value: bool,
    /// Whether the option is allowed in debian/source/options (some are local-options only)
    pub allowed_in_options: bool,
}

/// Valid long options for debian/source/options and debian/source/local-options.
///
/// These are the options that can be specified in the options files.
/// The leading `--` should be stripped in the file.
pub const SOURCE_OPTIONS: &[SourceOption] = &[
    SourceOption {
        name: "compression",
        description: "Select compression to use (supported: bzip2, gzip, lzma, xz)",
        takes_value: true,
        allowed_in_options: true,
    },
    SourceOption {
        name: "compression-level",
        description: "Compression level to use (1-9, best, fast)",
        takes_value: true,
        allowed_in_options: true,
    },
    SourceOption {
        name: "threads-max",
        description: "Use at most this many threads with the compressor",
        takes_value: true,
        allowed_in_options: true,
    },
    SourceOption {
        name: "diff-ignore",
        description: "Perl regex to filter out files from diff generation",
        takes_value: true,
        allowed_in_options: true,
    },
    SourceOption {
        name: "extend-diff-ignore",
        description: "Extend the default diff-ignore regex with additional pattern",
        takes_value: true,
        allowed_in_options: true,
    },
    SourceOption {
        name: "tar-ignore",
        description: "Pattern passed to tar's --exclude when generating tarballs",
        takes_value: true,
        allowed_in_options: true,
    },
    // Format 3.0 (quilt) build options
    SourceOption {
        name: "single-debian-patch",
        description: "Use debian/patches/debian-changes as automatic patch",
        takes_value: false,
        allowed_in_options: true,
    },
    SourceOption {
        name: "create-empty-orig",
        description: "Create an empty original tarball if missing and format permits",
        takes_value: false,
        allowed_in_options: true,
    },
    SourceOption {
        name: "no-unapply-patches",
        description: "Do not unapply patches after build",
        takes_value: false,
        allowed_in_options: true,
    },
    SourceOption {
        name: "unapply-patches",
        description: "Unapply patches after build (default)",
        takes_value: false,
        allowed_in_options: true,
    },
    // Format 3.0 (quilt) options only in local-options
    SourceOption {
        name: "abort-on-upstream-changes",
        description: "Fail if an automatic patch has been generated",
        takes_value: false,
        allowed_in_options: false,
    },
    SourceOption {
        name: "auto-commit",
        description: "Automatically record generated patches in the quilt series",
        takes_value: false,
        allowed_in_options: true,
    },
    // Generic build options
    SourceOption {
        name: "include-removal",
        description: "Include removed files in the diff (format 1.0)",
        takes_value: false,
        allowed_in_options: true,
    },
    SourceOption {
        name: "include-timestamp",
        description: "Include file timestamps in the diff (format 1.0)",
        takes_value: false,
        allowed_in_options: true,
    },
    SourceOption {
        name: "include-binaries",
        description: "Include binary files in the debian tarball",
        takes_value: false,
        allowed_in_options: true,
    },
    SourceOption {
        name: "no-preparation",
        description: "Do not prepare the build tree",
        takes_value: false,
        allowed_in_options: true,
    },
    SourceOption {
        name: "no-check",
        description: "Do not check signature and checksums before unpacking",
        takes_value: false,
        allowed_in_options: true,
    },
    SourceOption {
        name: "no-copy",
        description: "Do not copy original tarballs near the source package",
        takes_value: false,
        allowed_in_options: true,
    },
    SourceOption {
        name: "no-overwrite-dir",
        description: "Do not overwrite the extraction directory if it exists",
        takes_value: false,
        allowed_in_options: true,
    },
    SourceOption {
        name: "require-valid-signature",
        description: "Abort if the package does not have a valid signature",
        takes_value: false,
        allowed_in_options: true,
    },
    SourceOption {
        name: "require-strong-checksums",
        description: "Abort if the package contains no strong checksums",
        takes_value: false,
        allowed_in_options: true,
    },
    SourceOption {
        name: "ignore-bad-version",
        description: "Allow bad source package versions",
        takes_value: false,
        allowed_in_options: true,
    },
    SourceOption {
        name: "skip-debianization",
        description: "Do not apply debian diff to upstream sources (format 1.0/3.0 quilt)",
        takes_value: false,
        allowed_in_options: true,
    },
    SourceOption {
        name: "skip-patches",
        description: "Do not apply patches at the end of extraction (format 3.0 quilt)",
        takes_value: false,
        allowed_in_options: true,
    },
];

/// Valid values for the --compression option
pub const COMPRESSION_VALUES: &[(&str, &str)] = &[
    ("xz", "XZ compression (default)"),
    ("gzip", "Gzip compression"),
    ("bzip2", "Bzip2 compression"),
    ("lzma", "LZMA compression"),
];

/// Valid values for the --compression-level option
pub const COMPRESSION_LEVEL_VALUES: &[(&str, &str)] = &[
    ("1", "Fastest compression"),
    ("2", "Level 2 compression"),
    ("3", "Level 3 compression"),
    ("4", "Level 4 compression"),
    ("5", "Level 5 compression"),
    ("6", "Default compression level"),
    ("7", "Level 7 compression"),
    ("8", "Level 8 compression"),
    ("9", "Best compression"),
    ("best", "Best compression (alias for 9)"),
    ("fast", "Fastest compression (alias for 1)"),
];
