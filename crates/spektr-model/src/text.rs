//! Unicode helpers shared by the normalizers.

use icu_casemap::CaseMapper;
use icu_normalizer::ComposingNormalizerBorrowed;

/// Unicode NFKC.
pub fn nfkc(s: &str) -> String {
    ComposingNormalizerBorrowed::new_nfkc()
        .normalize(s)
        .into_owned()
}

/// Full Unicode case folding (not just lowercasing: `ß` folds to `ss`).
pub fn casefold(s: &str) -> String {
    CaseMapper::new().fold_string(s).into_owned()
}

/// Trim, then collapse every run of whitespace to a single space.
pub fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn has_control(s: &str) -> bool {
    s.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(nfkc("ＪＭａｒｌｏｗ"), "JMarlow");
        assert_eq!(casefold("Straße"), "strasse");
        assert_eq!(collapse_ws("  a \t b\n c "), "a b c");
    }
}
