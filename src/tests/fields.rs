use crate::deb822::completion::FieldInfo;

/// All available Debian debian/tests/control file fields
pub const TESTS_FIELDS: &[FieldInfo] = &[
    FieldInfo::new("Tests", "Test script names in the test directory"),
    FieldInfo::new(
        "Test-Command",
        "Inline shell command (mutually exclusive with Tests)",
    ),
    FieldInfo::new("Depends", "Packages required to run the tests"),
    FieldInfo::new("Restrictions", "Restrictions on how the test can be run"),
    FieldInfo::new("Features", "Additional capabilities of the tests"),
    FieldInfo::new("Classes", "Abstract class names for CI infrastructure"),
    FieldInfo::new(
        "Tests-Directory",
        "Replaces debian/tests as the test scripts directory",
    ),
    FieldInfo::new("Architecture", "Limit test to specific architectures"),
];

/// Restrictions values defined by the autopkgtest spec (DEP-8).
/// Each entry is (value, description).
pub const TESTS_RESTRICTIONS_VALUES: &[(&str, &str)] = &[
    ("allow-stderr", "stderr output is not considered a failure"),
    ("breaks-testbed", "Test may break the testbed"),
    ("build-needed", "Test must run from a built source tree"),
    ("flaky", "Test may fail intermittently"),
    (
        "hint-testsuite-triggers",
        "Hint for Testsuite-Triggers only; test is not run",
    ),
    ("isolation-container", "Test requires its own container"),
    ("isolation-machine", "Test requires its own VM"),
    (
        "needs-internet",
        "Test requires unrestricted internet access",
    ),
    ("needs-reboot", "Test reboots the machine"),
    (
        "needs-recommends",
        "Deprecated: use @recommends@ in Depends instead",
    ),
    ("needs-root", "Test must be run as root"),
    ("needs-sudo", "Test requires passwordless sudo access"),
    (
        "rw-build-tree",
        "Test requires write access to the build tree",
    ),
    (
        "skip-foreign-architecture",
        "Skip test on foreign architectures",
    ),
    (
        "skip-not-installable",
        "Deprecated: use the Architecture field instead",
    ),
    (
        "skippable",
        "Test may exit with status 77 to skip at runtime",
    ),
    ("superficial", "Test provides only weak coverage"),
];

/// Features values defined by the autopkgtest spec (DEP-8).
/// Each entry is (value, description).
pub const TESTS_FEATURES_VALUES: &[(&str, &str)] =
    &[("test-name", "Set explicit name for a Test-Command test")];

/// Substitution variables for the Depends field in debian/tests/control.
/// Each entry is (value, description).
pub const TESTS_DEPENDS_SUBSTITUTION_VALUES: &[(&str, &str)] = &[
    ("@", "Replaced by each binary package from the source"),
    (
        "@builddeps@",
        "Replaced by Build-Depends, Build-Depends-Indep, Build-Depends-Arch and build-essential",
    ),
    (
        "@recommends@",
        "Replaced by Recommends of all binary packages in debian/control",
    ),
];
