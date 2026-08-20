use once_cell::sync::Lazy;
use regex::Regex;
use unicode_normalization::UnicodeNormalization;

static INVALID_CHARS: Lazy<Regex> = Lazy::new(|| Regex::new(r"[^\w/\-]").unwrap());
static LEADING_NON_ALNUM: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[\W_]+").unwrap());

const QUOTES: &[char] = &[
    '`', '\'', '"', '\u{201C}', '\u{201D}', '\u{2018}', '\u{2019}', '\u{00AB}', '\u{00BB}',
];

/// Convert arbitrary string to a valid Obsidian tag (lowercase convention).
pub fn sanitize_tag(raw: &str) -> String {
    let nfc: String = raw.trim().nfc().collect();
    let mut tag = nfc.to_lowercase().replace(' ', "-");
    tag = INVALID_CHARS.replace_all(&tag, "").into_owned();
    tag = LEADING_NON_ALNUM.replace(&tag, "").into_owned();
    tag
}

pub fn sanitize_tags(raw_tags: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for raw in raw_tags {
        let tag = sanitize_tag(raw);
        if !tag.is_empty() && seen.insert(tag.clone()) {
            result.push(tag);
        }
    }
    result
}

fn trailing_closer_unmatched(s: &str) -> bool {
    let last = match s.chars().last() {
        Some(c) => c,
        None => return false,
    };
    let opener = match last {
        ')' => '(',
        ']' => '[',
        _ => return false,
    };
    let closer = last;
    let mut balance = 0i32;
    let end = s.len() - last.len_utf8();
    for ch in s[..end].chars() {
        if ch == opener {
            balance += 1;
        } else if ch == closer {
            balance = (balance - 1).max(0);
        }
    }
    balance == 0
}

fn leading_opener_unmatched(s: &str) -> bool {
    let first = match s.chars().next() {
        Some(c) => c,
        None => return false,
    };
    let closer = match first {
        '(' => ')',
        '[' => ']',
        _ => return false,
    };
    let opener = first;
    let mut balance = 0i32;
    let start = first.len_utf8();
    for ch in s[start..].chars().rev() {
        if ch == closer {
            balance += 1;
        } else if ch == opener {
            balance = (balance - 1).max(0);
        }
    }
    balance == 0
}

/// Strip dangling/unbalanced edge brackets from a human-readable name.
pub fn clean_display_name(name: &str) -> String {
    let original = name.trim().to_string();
    let mut cleaned = original.trim_matches(QUOTES).trim().to_string();
    loop {
        let next = cleaned.trim_matches(QUOTES).trim().to_string();
        if next == cleaned {
            break;
        }
        cleaned = next;
    }

    loop {
        if cleaned.is_empty() {
            break;
        }
        let before = cleaned.clone();
        if let Some(last) = cleaned.chars().last() {
            if matches!(last, ')' | ']') && trailing_closer_unmatched(&cleaned) {
                let new_len = cleaned.len() - last.len_utf8();
                cleaned = cleaned[..new_len].trim_end().to_string();
            }
        }
        if let Some(first) = cleaned.chars().next() {
            if matches!(first, '(' | '[') && leading_opener_unmatched(&cleaned) {
                let start = first.len_utf8();
                cleaned = cleaned[start..].trim_start().to_string();
            }
        }
        if cleaned == before {
            break;
        }
    }

    if cleaned.is_empty() {
        original
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spaces_to_hyphens() {
        assert_eq!(sanitize_tag("quantum computing"), "quantum-computing");
    }

    #[test]
    fn multiple_spaces() {
        assert_eq!(
            sanitize_tag("machine learning basics"),
            "machine-learning-basics"
        );
    }

    #[test]
    fn cpp_special_chars_removed() {
        assert_eq!(sanitize_tag("C++ programming"), "c-programming");
    }

    #[test]
    fn leading_hash_stripped() {
        assert_eq!(sanitize_tag("#my-tag"), "my-tag");
    }

    #[test]
    fn valid_tag_passthrough() {
        assert_eq!(sanitize_tag("physics"), "physics");
    }

    #[test]
    fn garbage_returns_empty() {
        assert_eq!(sanitize_tag("!!!"), "");
    }

    #[test]
    fn slashes_preserved() {
        assert_eq!(sanitize_tag("science/physics"), "science/physics");
    }

    #[test]
    fn pure_numbers_valid() {
        assert_eq!(sanitize_tag("2024"), "2024");
    }

    #[test]
    fn underscores_preserved() {
        assert_eq!(sanitize_tag("my_tag"), "my_tag");
    }

    #[test]
    fn hyphens_preserved() {
        assert_eq!(sanitize_tag("already-hyphenated"), "already-hyphenated");
    }

    #[test]
    fn mixed_case_lowercased() {
        assert_eq!(sanitize_tag("MachineLearning"), "machinelearning");
    }

    #[test]
    fn uppercase_tag_lowercased() {
        assert_eq!(sanitize_tag("AI"), "ai");
    }

    #[test]
    fn leading_hyphen_stripped() {
        assert_eq!(sanitize_tag("-bad-start"), "bad-start");
    }

    #[test]
    fn leading_underscore_stripped() {
        assert_eq!(sanitize_tag("_bad-start"), "bad-start");
    }

    #[test]
    fn whitespace_only_returns_empty() {
        assert_eq!(sanitize_tag("   "), "");
    }

    #[test]
    fn empty_string_returns_empty() {
        assert_eq!(sanitize_tag(""), "");
    }

    #[test]
    fn at_symbol_removed() {
        assert_eq!(sanitize_tag("tag@user"), "taguser");
    }

    #[test]
    fn sanitize_tags_dedup() {
        let tags = vec!["AI".into(), "ai".into(), "ML".into(), "!!!".into()];
        assert_eq!(sanitize_tags(&tags), vec!["ai", "ml"]);
    }

    #[test]
    fn unmatched_trailing_paren() {
        assert_eq!(clean_display_name("Phase II)"), "Phase II");
    }

    #[test]
    fn unmatched_leading_paren() {
        assert_eq!(clean_display_name("(draft"), "draft");
    }

    #[test]
    fn balanced_paren_kept() {
        assert_eq!(
            clean_display_name("Extreme Programming (XP)"),
            "Extreme Programming (XP)"
        );
    }

    #[test]
    fn yahoo_bang_kept() {
        assert_eq!(clean_display_name("Yahoo!"), "Yahoo!");
        assert_eq!(clean_display_name("Yahoo!)"), "Yahoo!");
    }

    #[test]
    fn nested_unmatched() {
        assert_eq!(clean_display_name("Phase II))"), "Phase II");
    }

    #[test]
    fn fx_kept() {
        assert_eq!(clean_display_name("f(x)"), "f(x)");
    }
}
