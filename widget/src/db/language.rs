//! The catalogue language selection is one required language and an optional
//! second language. A single code always means a single displayed language.

fn language_code(value: &str) -> Option<&'static str> {
    match value.trim() {
        "zh" => Some("zh"),
        "en" => Some("en"),
        "ko" => Some("ko"),
        _ => None,
    }
}

fn selection(lang: &str) -> (&'static str, Option<&'static str>) {
    let (primary, secondary) = lang.trim().split_once('-').unwrap_or((lang, ""));
    let primary = language_code(primary).unwrap_or("zh");
    let secondary = language_code(secondary).filter(|secondary| *secondary != primary);
    (primary, secondary)
}

fn format_selection(primary: &str, secondary: Option<&str>) -> String {
    match secondary {
        Some(secondary) => format!("{primary}-{secondary}"),
        None => primary.to_string(),
    }
}

/// Return a valid selection, defaulting a blank/invalid primary to Chinese and
/// omitting a blank/invalid or duplicate secondary language.
pub fn normalize_lang(lang: &str) -> String {
    let (primary, secondary) = selection(lang);
    format_selection(primary, secondary)
}

pub fn primary_lang(lang: &str) -> &'static str {
    selection(lang).0
}

pub fn secondary_lang(lang: &str) -> Option<&'static str> {
    selection(lang).1
}

pub fn with_primary_lang(lang: &str, primary: &str) -> String {
    let primary = language_code(primary).unwrap_or("zh");
    let secondary = secondary_lang(lang).filter(|secondary| *secondary != primary);
    format_selection(primary, secondary)
}

pub fn with_secondary_lang(lang: &str, secondary: &str) -> String {
    let primary = primary_lang(lang);
    let secondary = language_code(secondary).filter(|secondary| *secondary != primary);
    format_selection(primary, secondary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_distinct_language_pair_and_single_language_round_trips() {
        for primary in ["zh", "en", "ko"] {
            assert_eq!(normalize_lang(primary), primary);
            assert_eq!(primary_lang(primary), primary);
            assert_eq!(secondary_lang(primary), None);
            for secondary in ["zh", "en", "ko"] {
                let input = format!("{primary}-{secondary}");
                if primary == secondary {
                    assert_eq!(normalize_lang(&input), primary);
                    assert_eq!(secondary_lang(&input), None);
                } else {
                    assert_eq!(normalize_lang(&input), input);
                    assert_eq!(secondary_lang(&input), Some(secondary));
                }
            }
        }
    }

    #[test]
    fn malformed_selections_keep_a_valid_required_language() {
        for invalid in ["", " ", "fr", "zh-fr", "zh-", "zh-zh", "zh-en-ko"] {
            assert_eq!(normalize_lang(invalid), "zh", "{invalid}");
        }
        assert_eq!(normalize_lang("fr-en"), "zh-en");
        assert_eq!(normalize_lang(" en - ko "), "en-ko");
        assert_eq!(normalize_lang("ko-invalid"), "ko");
    }

    #[test]
    fn changing_either_language_deduplicates_and_can_clear_secondary() {
        assert_eq!(with_primary_lang("zh-en", "ko"), "ko-en");
        assert_eq!(with_primary_lang("zh-en", "en"), "en");
        assert_eq!(with_primary_lang("ko-en", ""), "zh-en");
        assert_eq!(with_secondary_lang("ko-en", ""), "ko");
        assert_eq!(with_secondary_lang("ko-en", "ko"), "ko");
        assert_eq!(with_secondary_lang("ko", "zh"), "ko-zh");
        assert_eq!(with_secondary_lang("ko", "fr"), "ko");
    }
}
