use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn content_hash(body: &str) -> String {
    sha256_hex(body.as_bytes())
}

pub fn hash8(value: &str) -> String {
    sha256_hex(value.as_bytes())[..8].to_string()
}

pub fn normalize_question(question: &str) -> String {
    let nfkc: String = question.nfkc().collect();
    let mut normalized = nfkc.trim().to_lowercase();
    let collapsed = {
        let mut out = String::new();
        let mut prev_space = false;
        for ch in normalized.chars() {
            if ch.is_whitespace() {
                if !prev_space {
                    out.push(' ');
                    prev_space = true;
                }
            } else {
                out.push(ch);
                prev_space = false;
            }
        }
        out
    };
    normalized = collapsed;
    if let Some(stripped) = normalized.strip_suffix('?') {
        normalized = stripped.trim_end().to_string();
    }
    normalized
}

pub fn question_hash(question: &str) -> String {
    sha256_hex(normalize_question(question).as_bytes())[..16].to_string()
}

pub fn relation_id(subject_key: &str, predicate: &str, object_key: &str) -> String {
    sha256_hex(format!("{subject_key}:{predicate}:{object_key}").as_bytes())[..16].to_string()
}

pub fn generate_article_id() -> String {
    ulid::Ulid::new().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn question_hash_stable() {
        assert_eq!(
            question_hash("What is a qubit?"),
            question_hash("what is a qubit")
        );
        assert_eq!(question_hash("What is a qubit?").len(), 16);
    }

    #[test]
    fn content_hash_utf8() {
        assert_eq!(content_hash("hello"), sha256_hex(b"hello"));
    }
}
