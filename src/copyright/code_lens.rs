//! Code lenses for debian/copyright files.
//!
//! Shows the number of Files paragraphs that reference each standalone
//! License paragraph:
//! - `License: MIT` → "used by 3 Files paragraphs"

use std::collections::HashMap;

use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{CodeLens, Command};

use crate::position::text_range_to_lsp_range;

/// Generate code lenses for copyright license paragraphs.
///
/// For each standalone License paragraph, counts how many Files paragraphs
/// reference that license name and displays the count as a code lens.
pub fn generate_code_lenses(
    parsed: &debian_copyright::lossless::Parse,
    source_text: &str,
) -> Vec<CodeLens> {
    let copyright = parsed.to_copyright();
    let mut lenses = Vec::new();

    // Count how many Files paragraphs reference each individual license name.
    // A Files paragraph with "License: GPL-2+ or MIT" counts as a reference
    // to both "GPL-2+" and "MIT".
    let mut license_usage: HashMap<String, usize> = HashMap::new();
    for files_para in copyright.iter_files() {
        if let Some(license) = files_para.license() {
            if let Some(expr) = license.expr() {
                for name in expr.license_names() {
                    *license_usage.entry(name.to_lowercase()).or_insert(0) += 1;
                }
            }
        }
    }

    // Add lenses to standalone License paragraphs
    for license_para in copyright.iter_licenses() {
        let para = license_para.as_deb822();
        let para_range = para.syntax().text_range();

        let Some(name) = license_para.name() else {
            continue;
        };

        let key = name.to_lowercase();
        let count = license_usage.get(&key).copied().unwrap_or(0);

        let title = match count {
            0 => "unused".to_string(),
            1 => "used by 1 Files paragraph".to_string(),
            n => format!("used by {n} Files paragraphs"),
        };

        // Find the License field entry to get its range
        let entry_range = if let Some(entry) = para
            .entries()
            .find(|e| e.key().is_some_and(|k| k.eq_ignore_ascii_case("License")))
        {
            text_range_to_lsp_range(source_text, entry.text_range())
        } else {
            text_range_to_lsp_range(source_text, para_range)
        };

        lenses.push(CodeLens {
            range: entry_range,
            command: Some(Command {
                title,
                command: "debian-lsp.noop".to_string(),
                arguments: None,
            }),
            data: None,
        });
    }

    lenses
}

#[cfg(test)]
mod tests {
    use super::*;
    use debian_copyright::lossless::Parse;

    fn parse(text: &str) -> Parse {
        Parse::parse_relaxed(text)
    }

    #[test]
    fn test_license_used_by_multiple_files() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/*
Copyright: 2024 Alice
License: MIT

Files: lib/*
Copyright: 2024 Bob
License: MIT

Files: debian/*
Copyright: 2024 Carol
License: GPL-2+

License: MIT
 Permission is hereby granted...

License: GPL-2+
 This program is free software...
";
        let parsed = parse(text);
        let lenses = generate_code_lenses(&parsed, text);

        assert_eq!(lenses.len(), 2);
        assert_eq!(
            lenses[0].command.as_ref().unwrap().title,
            "used by 2 Files paragraphs"
        );
        assert_eq!(
            lenses[1].command.as_ref().unwrap().title,
            "used by 1 Files paragraph"
        );
    }

    #[test]
    fn test_unused_license() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Test
License: MIT

License: MIT
 Permission is hereby granted...

License: Apache-2.0
 Licensed under the Apache License...
";
        let parsed = parse(text);
        let lenses = generate_code_lenses(&parsed, text);

        assert_eq!(lenses.len(), 2);
        assert_eq!(
            lenses[0].command.as_ref().unwrap().title,
            "used by 1 Files paragraph"
        );
        assert_eq!(lenses[1].command.as_ref().unwrap().title, "unused");
    }

    #[test]
    fn test_no_standalone_licenses() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Test
License: MIT
 Permission is hereby granted...
";
        let parsed = parse(text);
        let lenses = generate_code_lenses(&parsed, text);

        assert_eq!(lenses.len(), 0);
    }

    #[test]
    fn test_case_insensitive_matching() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Test
License: mit

License: MIT
 Permission is hereby granted...
";
        let parsed = parse(text);
        let lenses = generate_code_lenses(&parsed, text);

        assert_eq!(lenses.len(), 1);
        assert_eq!(
            lenses[0].command.as_ref().unwrap().title,
            "used by 1 Files paragraph"
        );
    }

    #[test]
    fn test_empty_copyright() {
        let text = "";
        let parsed = parse(text);
        let lenses = generate_code_lenses(&parsed, text);

        assert_eq!(lenses.len(), 0);
    }

    #[test]
    fn test_or_expression_counts_individual_licenses() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/*
Copyright: 2024 Alice
License: GPL-2+ or MIT

License: GPL-2+
 This program is free software...

License: MIT
 Permission is hereby granted...
";
        let parsed = parse(text);
        let lenses = generate_code_lenses(&parsed, text);

        assert_eq!(lenses.len(), 2);
        assert_eq!(
            lenses[0].command.as_ref().unwrap().title,
            "used by 1 Files paragraph"
        );
        assert_eq!(
            lenses[1].command.as_ref().unwrap().title,
            "used by 1 Files paragraph"
        );
    }
}
