use once_cell::sync::Lazy;
use regex::Regex;
use unicode_normalization::UnicodeNormalization;

use crate::sanitize::clean_display_name;

static PAREN_ABBR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?P<base>.+?)\s*\((?P<abbr>[A-ZА-Я0-9][A-ZА-Я0-9.+-]{1,8})\)$").unwrap()
});

const QUOTES: &[char] = &[
    '`', '\'', '"', '\u{201C}', '\u{201D}', '\u{2018}', '\u{2019}', '\u{00AB}', '\u{00BB}',
];

fn strip_surrounding_quotes(text: &str) -> String {
    let mut s = text.to_string();
    loop {
        let trimmed = s.trim_matches(QUOTES);
        if trimmed.len() == s.len() {
            break;
        }
        s = trimmed.to_string();
    }
    s.trim().to_string()
}

pub fn clean_concept_text(text: &str) -> String {
    let nfkc: String = text.nfkc().collect();
    let text = nfkc.trim();
    let text = strip_surrounding_quotes(text);
    let text = clean_display_name(&text);
    let collapsed = Regex::new(r"\s+").unwrap().replace_all(&text, " ");
    collapsed.into_owned()
}

pub fn concept_key(text: &str) -> String {
    let text = caseless::default_case_fold_str(&clean_concept_text(text));
    static PUNCT: Lazy<Regex> = Lazy::new(|| Regex::new(r"[_\-/:]+").unwrap());
    static NONWORD: Lazy<Regex> = Lazy::new(|| Regex::new(r"[^\w\s]+").unwrap());
    static SPACE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
    let text = PUNCT.replace_all(&text, " ");
    let text = NONWORD.replace_all(&text, " ");
    SPACE.replace_all(text.trim(), " ").trim().to_string()
}

pub fn base_concept_name(text: &str) -> String {
    let cleaned = clean_concept_text(text);
    if let Some(caps) = PAREN_ABBR_RE.captures(&cleaned) {
        let abbr = caps.name("abbr").unwrap().as_str();
        if abbr
            .chars()
            .all(|c| c.is_uppercase() || c.is_ascii_digit() || matches!(c, '.' | '+' | '-'))
            && abbr.chars().any(|c| c.is_uppercase())
            && abbr
                .chars()
                .filter(|c| c.is_alphabetic())
                .all(|c| c.is_uppercase())
        {
            // Python abbr.isupper() is true for "XP" and "A1" (digits ignored); false if any
            // lowercase letter is present. Match that: all cased chars must be uppercase.
            if abbr.chars().any(|c| c.is_lowercase()) {
                return cleaned;
            }
            return caps.name("base").unwrap().as_str().trim().to_string();
        }
        if !abbr.chars().any(|c| c.is_lowercase()) && abbr.chars().any(|c| c.is_uppercase()) {
            return caps.name("base").unwrap().as_str().trim().to_string();
        }
        return cleaned;
    }
    cleaned
}

fn fold_token(token: &str) -> String {
    if token.chars().count() < 4 {
        return token.to_string();
    }
    if token.chars().all(|c| !c.is_lowercase()) && token.chars().any(|c| c.is_uppercase()) {
        return token.to_string();
    }
    if token.ends_with("ies") && token.chars().count() > 4 {
        let end = token.len() - 3;
        return format!("{}y", &token[..end]);
    }
    if token.ends_with("sses") {
        let end = token.len() - 2;
        return token[..end].to_string();
    }
    let char_len = token.chars().count();
    if token.ends_with('s')
        && !(token.ends_with("ss") || token.ends_with("us") || token.ends_with("is"))
        && char_len.saturating_sub(1) >= 4
    {
        return token[..token.len() - 1].to_string();
    }
    token.to_string()
}

pub fn match_key(text: &str) -> String {
    let base = concept_key(text);
    base.split_whitespace()
        .map(fold_token)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_folds_case_and_punct() {
        assert_eq!(
            concept_key("Gradient-Descent"),
            concept_key("gradient descent")
        );
    }

    #[test]
    fn match_key_users() {
        assert_eq!(match_key("Users"), match_key("User"));
    }

    #[test]
    fn match_key_does_not_fold_lens() {
        assert_ne!(match_key("Lens"), match_key("Len"));
    }

    #[test]
    fn base_strips_xp() {
        assert_eq!(
            base_concept_name("Extreme Programming (XP)"),
            "Extreme Programming"
        );
    }

    #[test]
    fn base_keeps_lowercase_paren() {
        assert_eq!(base_concept_name("Foo (bar)"), "Foo (bar)");
    }
}
