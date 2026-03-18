/// A known debian/rules target.
pub struct RulesTarget {
    pub name: &'static str,
    pub description: &'static str,
    pub required: bool,
}

impl RulesTarget {
    pub const fn new(name: &'static str, description: &'static str, required: bool) -> Self {
        Self {
            name,
            description,
            required,
        }
    }
}

/// A known debian/rules variable.
pub struct RulesVariable {
    pub name: &'static str,
    pub description: &'static str,
}

impl RulesVariable {
    pub const fn new(name: &'static str, description: &'static str) -> Self {
        Self { name, description }
    }
}

/// Standard debian/rules targets as defined by Debian Policy.
pub const RULES_TARGETS: &[RulesTarget] = &[
    RulesTarget::new("clean", "Clean up the build tree", true),
    RulesTarget::new("build", "Build the package", true),
    RulesTarget::new("build-arch", "Build architecture-dependent files", true),
    RulesTarget::new("build-indep", "Build architecture-independent files", true),
    RulesTarget::new("binary", "Build all binary packages", true),
    RulesTarget::new(
        "binary-arch",
        "Build architecture-dependent binary packages",
        true,
    ),
    RulesTarget::new(
        "binary-indep",
        "Build architecture-independent binary packages",
        true,
    ),
    RulesTarget::new(
        "install",
        "Install files into the package build directory",
        false,
    ),
    RulesTarget::new(
        "get-orig-source",
        "Get the original upstream source tarball",
        false,
    ),
    RulesTarget::new(
        "override_dh_auto_configure",
        "Override dh_auto_configure step",
        false,
    ),
    RulesTarget::new(
        "override_dh_auto_build",
        "Override dh_auto_build step",
        false,
    ),
    RulesTarget::new("override_dh_auto_test", "Override dh_auto_test step", false),
    RulesTarget::new(
        "override_dh_auto_install",
        "Override dh_auto_install step",
        false,
    ),
    RulesTarget::new(
        "override_dh_auto_clean",
        "Override dh_auto_clean step",
        false,
    ),
    RulesTarget::new("override_dh_install", "Override dh_install step", false),
    RulesTarget::new(
        "override_dh_gencontrol",
        "Override dh_gencontrol step",
        false,
    ),
    RulesTarget::new("override_dh_shlibdeps", "Override dh_shlibdeps step", false),
    RulesTarget::new("override_dh_strip", "Override dh_strip step", false),
    RulesTarget::new(
        "execute_before_dh_auto_configure",
        "Execute before dh_auto_configure step",
        false,
    ),
    RulesTarget::new(
        "execute_after_dh_auto_configure",
        "Execute after dh_auto_configure step",
        false,
    ),
    RulesTarget::new(
        "execute_before_dh_auto_build",
        "Execute before dh_auto_build step",
        false,
    ),
    RulesTarget::new(
        "execute_after_dh_auto_build",
        "Execute after dh_auto_build step",
        false,
    ),
    RulesTarget::new(
        "execute_before_dh_auto_test",
        "Execute before dh_auto_test step",
        false,
    ),
    RulesTarget::new(
        "execute_after_dh_auto_test",
        "Execute after dh_auto_test step",
        false,
    ),
    RulesTarget::new(
        "execute_before_dh_auto_install",
        "Execute before dh_auto_install step",
        false,
    ),
    RulesTarget::new(
        "execute_after_dh_auto_install",
        "Execute after dh_auto_install step",
        false,
    ),
    RulesTarget::new(
        "execute_before_dh_auto_clean",
        "Execute before dh_auto_clean step",
        false,
    ),
    RulesTarget::new(
        "execute_after_dh_auto_clean",
        "Execute after dh_auto_clean step",
        false,
    ),
];

/// Common debian/rules variables.
pub const RULES_VARIABLES: &[RulesVariable] = &[
    RulesVariable::new(
        "DEB_BUILD_OPTIONS",
        "Build options (nocheck, nostrip, parallel, etc.)",
    ),
    RulesVariable::new(
        "DEB_BUILD_MAINT_OPTIONS",
        "Maintainer build options (hardening, reproducible, etc.)",
    ),
    RulesVariable::new(
        "DEB_HOST_GNU_TYPE",
        "GNU system type for the host architecture",
    ),
    RulesVariable::new(
        "DEB_BUILD_GNU_TYPE",
        "GNU system type for the build architecture",
    ),
    RulesVariable::new(
        "DEB_HOST_MULTIARCH",
        "Multiarch triplet for the host architecture",
    ),
    RulesVariable::new(
        "DPKG_EXPORT_BUILDFLAGS",
        "Export dpkg build flags to the environment",
    ),
    RulesVariable::new("DEB_BUILD_HARDENING", "Enable hardening build flags"),
];

/// Check if a target name is a known standard target.
pub fn is_known_target(name: &str) -> bool {
    if RULES_TARGETS.iter().any(|t| t.name == name) {
        return true;
    }
    // Also recognize override_ and execute_{before,after}_ prefixed targets
    name.starts_with("override_dh_")
        || name.starts_with("execute_before_dh_")
        || name.starts_with("execute_after_dh_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_targets() {
        assert!(is_known_target("clean"));
        assert!(is_known_target("build"));
        assert!(is_known_target("binary"));
        assert!(is_known_target("binary-arch"));
        assert!(is_known_target("binary-indep"));
        assert!(is_known_target("override_dh_auto_build"));
        assert!(is_known_target("override_dh_fixperms"));
        assert!(is_known_target("execute_before_dh_auto_test"));
        assert!(is_known_target("execute_after_dh_auto_install"));
    }

    #[test]
    fn test_unknown_targets() {
        assert!(!is_known_target("my-custom-target"));
        assert!(!is_known_target("foo"));
    }

    #[test]
    fn test_rules_targets_not_empty() {
        assert!(!RULES_TARGETS.is_empty());
        for target in RULES_TARGETS {
            assert!(!target.name.is_empty());
            assert!(!target.description.is_empty());
        }
    }

    #[test]
    fn test_rules_variables_not_empty() {
        assert!(!RULES_VARIABLES.is_empty());
        for var in RULES_VARIABLES {
            assert!(!var.name.is_empty());
            assert!(!var.description.is_empty());
        }
    }
}
