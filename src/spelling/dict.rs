//! Minimal [`typos::Dictionary`] backed by the `typos-dict` correction list.
//!
//! `typos-cli` ships a richer `BuiltIn` dictionary, but depending on it pulls
//! in the whole CLI (clap, ignore, sysinfo, ...). We only need the word list,
//! so we wrap `typos_dict::WORD` directly. Variant handling (British vs
//! American spellings from `typos-vars`) is intentionally omitted for now;
//! `typos-dict` alone already covers the common misspellings lintian flags.

use typos::tokens::{Case, Identifier, Word};
use typos::{Dictionary, Status};
use unicase::UniCase;

/// Correction dictionary over the `typos-dict` word list.
#[derive(Default)]
pub struct WordListDictionary;

impl Dictionary for WordListDictionary {
    fn correct_ident<'s>(&'s self, _ident: Identifier<'_>) -> Option<Status<'s>> {
        // Identifier-level corrections need the CLI's hand-maintained
        // allow-list (O_WRONLY, dBA, ...); we only check word tokens.
        None
    }

    fn correct_word<'s>(&'s self, word: Word<'_>) -> Option<Status<'s>> {
        // Case::None covers all-caps acronyms and the like; skip them to
        // avoid flagging things the dictionary can't meaningfully correct.
        if word.case() == Case::None {
            return None;
        }

        let corrections = typos_dict::WORD
            .find(&UniCase::new(word.token()))
            .copied()?;
        if corrections.is_empty() {
            return Some(Status::Invalid);
        }

        let mut status = Status::Corrections(
            corrections
                .iter()
                .map(|c| std::borrow::Cow::Borrowed(*c))
                .collect(),
        );
        // Match the casing of the original word so e.g. "Recieve" suggests
        // "Receive", not "receive".
        for s in status.corrections_mut() {
            case_correct(s, word.case());
        }
        Some(status)
    }
}

/// Re-case a correction to match the casing of the misspelled word.
///
/// Lifted from `typos-cli`'s `case_correct`: the dictionary stores
/// lowercase corrections, so a `Title`-cased typo gets a `Title`-cased fix.
fn case_correct(correction: &mut std::borrow::Cow<'_, str>, case: Case) {
    match case {
        Case::Lower | Case::None => {}
        Case::Title => {
            let mut chars = correction.chars();
            if let Some(first) = chars.next() {
                let title = first.to_uppercase().chain(chars).collect::<String>();
                *correction = std::borrow::Cow::Owned(title);
            }
        }
        Case::Upper => {
            *correction = std::borrow::Cow::Owned(correction.to_uppercase());
        }
    }
}
