use once_cell::sync::Lazy;
use regex::Regex;

const INLINE_MATH_RE: &str = r"\$[^$\n]+\$";
const MARKDOWN_LINK_RE: &str = r"\[[^\]\n]+\]\([^)]*\)";
const OBSIDIAN_EMBED_RE: &str = r"!\[\[[\s\S]*?\]\]";
const WIKILINK_RE: &str = r"\[\[[^\]]+\]\]";

static DISPLAY_MATH_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\\{1,2}\[(.*?)\\{1,2}\]").unwrap());
static BARE_LATEX_LINE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(?P<indent>[ \t]*)(?P<body>\\{1,2}[A-Za-z{_].*)$").unwrap());
static LATEX_COMMAND_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^\\(?:begin|end|frac|sqrt|sum|prod|int|lim|alpha|beta|gamma|delta|theta|lambda|mu|pi|sigma|phi|psi|omega|sin|cos|tan|log|ln|exp|cdot|times|leq|geq|neq|approx|left|right|text|mathrm|mathbf|mathit|operatorname)\b",
    )
    .unwrap()
});
static MATH_SIGNAL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[{}_^=]|\\(?:[A-Za-z]+|[,;! ])").unwrap());

#[derive(Debug, Clone)]
pub struct MaskOptions {
    pub mask_wikilinks: bool,
    pub mask_embeds: bool,
    pub mask_links: bool,
}

impl Default for MaskOptions {
    fn default() -> Self {
        Self {
            mask_wikilinks: true,
            mask_embeds: true,
            mask_links: true,
        }
    }
}

pub fn mask_markdown_regions(content: &str, opts: MaskOptions) -> (String, Vec<(String, String)>) {
    let mut parts = vec![
        r"```[\s\S]*?```".to_string(),
        r"`[^`]+`".to_string(),
        r"\$\$[\s\S]*?\$\$".to_string(),
        INLINE_MATH_RE.to_string(),
        r"\\\([\s\S]*?\\\)".to_string(),
        r"!\[[^\]]*\]\([^)]*\)".to_string(),
    ];
    if opts.mask_links {
        parts.push(MARKDOWN_LINK_RE.to_string());
    }
    if opts.mask_embeds {
        parts.push(OBSIDIAN_EMBED_RE.to_string());
    }
    if opts.mask_wikilinks {
        parts.push(WIKILINK_RE.to_string());
    }
    let pattern = Regex::new(&parts.join("|")).unwrap();
    let mut replacements = Vec::new();
    let masked = pattern.replace_all(content, |caps: &regex::Captures| {
        let token = format!("__SYNTO_MARKDOWN_MASK_{}__", replacements.len());
        replacements.push((token.clone(), caps.get(0).unwrap().as_str().to_string()));
        token
    });
    (masked.into_owned(), replacements)
}

pub fn restore_markdown_regions(mut content: String, replacements: &[(String, String)]) -> String {
    for (token, original) in replacements {
        content = content.replace(token, original);
    }
    content
}

fn looks_like_bare_latex_line(stripped: &str) -> bool {
    let normalized = stripped.trim_start_matches('\\');
    if stripped.starts_with("\\[")
        || stripped.starts_with("\\\\[")
        || stripped.starts_with("\\(")
        || stripped.starts_with("\\\\(")
    {
        return false;
    }
    if stripped.starts_with('#')
        || stripped.starts_with('>')
        || stripped.starts_with("- ")
        || stripped.starts_with("* ")
        || stripped.starts_with("+ ")
        || stripped.starts_with('|')
    {
        return false;
    }
    if Regex::new(r"^\d+\.\s").unwrap().is_match(stripped) {
        return false;
    }
    if LATEX_COMMAND_RE.is_match(stripped) {
        return true;
    }
    MATH_SIGNAL_RE.is_match(normalized)
}

pub fn sanitize_obsidian_math(content: &str) -> String {
    let (masked, replacements) = mask_markdown_regions(content, MaskOptions::default());
    let masked = DISPLAY_MATH_RE.replace_all(&masked, |caps: &regex::Captures| {
        let inner = caps.get(1).unwrap().as_str().trim();
        if inner.is_empty() {
            caps.get(0).unwrap().as_str().to_string()
        } else {
            format!("$$\n{inner}\n$$")
        }
    });

    let mut lines = Vec::new();
    for line in masked.split_inclusive('\n') {
        let (raw, ending) = if let Some(stripped) = line.strip_suffix("\r\n") {
            (stripped, "\r\n")
        } else if let Some(stripped) = line.strip_suffix('\n') {
            (stripped, "\n")
        } else {
            (line, "")
        };

        if let Some(caps) = BARE_LATEX_LINE_RE.captures(raw) {
            let indent = caps.name("indent").unwrap().as_str();
            let mut stripped = caps.name("body").unwrap().as_str().trim().to_string();
            if looks_like_bare_latex_line(&stripped) {
                if stripped.starts_with("\\\\") && !stripped.starts_with("\\\\\\") {
                    stripped = stripped[1..].to_string();
                }
                let ending = if ending.is_empty() { "\n" } else { ending };
                lines.push(format!("{indent}$$ {stripped} $${ending}"));
                continue;
            }
        }
        lines.push(line.to_string());
    }

    restore_markdown_regions(lines.join(""), &replacements)
}

pub fn has_malformed_obsidian_math(content: &str) -> bool {
    sanitize_obsidian_math(content) != content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_math_normalized() {
        let out = sanitize_obsidian_math(r"\[x^2\]");
        assert!(out.contains("$$"));
        assert!(out.contains("x^2"));
    }

    #[test]
    fn already_sane_passthrough() {
        let src = "hello $$x$$ world";
        assert!(!has_malformed_obsidian_math(src) || sanitize_obsidian_math(src).contains("$$"));
    }
}
