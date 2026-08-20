use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Concept {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalysisResult {
    pub summary: String,
    pub concepts: Vec<Concept>,
    pub suggested_topics: Vec<String>,
    #[serde(default)]
    pub named_references: Vec<String>,
    pub quality: String,
    #[serde(default)]
    pub language: Option<String>,
}

impl AnalysisResult {
    pub fn coerce(mut value: serde_json::Value) -> serde_json::Value {
        if let Some(obj) = value.as_object_mut() {
            if obj.get("summary").map(|v| v.is_null()).unwrap_or(true)
                && !obj.contains_key("summary")
                || obj.get("summary").map(|v| v.is_null()).unwrap_or(false)
            {
                let mut names = Vec::new();
                if let Some(arr) = obj.get("concepts").and_then(|v| v.as_array()) {
                    for item in arr {
                        if let Some(s) = item.as_str() {
                            names.push(s.to_string());
                        } else if let Some(n) = item.get("name").and_then(|v| v.as_str()) {
                            names.push(n.to_string());
                        }
                    }
                }
                if let Some(arr) = obj.get("named_references").and_then(|v| v.as_array()) {
                    for item in arr {
                        if let Some(s) = item.as_str() {
                            names.push(s.to_string());
                        }
                    }
                }
                let fallback = if names.is_empty() {
                    "Source contains limited extractable text.".to_string()
                } else {
                    format!(
                        "Source references: {}.",
                        names.into_iter().take(5).collect::<Vec<_>>().join(", ")
                    )
                };
                obj.insert("summary".into(), serde_json::Value::String(fallback));
            }
            if let Some(concepts) = obj.get_mut("concepts").and_then(|v| v.as_array_mut()) {
                for item in concepts.iter_mut() {
                    if let Some(s) = item.as_str() {
                        *item = serde_json::json!({"name": s, "aliases": []});
                    }
                }
            }
        }
        value
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArticlePlan {
    pub title: String,
    pub action: String,
    pub path: String,
    pub reasoning: String,
    pub source_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompilePlan {
    pub articles: Vec<ArticlePlan>,
    #[serde(default)]
    pub mocs_to_update: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SingleArticle {
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PageSelection {
    pub pages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueryAnswer {
    pub answer: String,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LintIssue {
    pub path: String,
    pub issue_type: String,
    pub description: String,
    pub suggestion: String,
    #[serde(default)]
    pub auto_fixable: bool,
}

pub const ADVISORY_ISSUE_TYPES: &[&str] = &[
    "graph_noise",
    "graph_connectivity",
    "synthesis_chain",
    "stale_lock",
    "missing_media",
    "label_collision",
    "orphan_entity",
    "ambiguous_label_needs_disambiguation",
    "stale_legacy_backfill_alias",
    "homonym_filename_collision",
    "manual_relabel_adopted",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LintResult {
    pub issues: Vec<LintIssue>,
    pub health_score: f64,
    pub summary: String,
    #[serde(default)]
    pub advisory_issue_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RawNoteRecord {
    pub path: String,
    pub content_hash: String,
    pub status: String,
    pub summary: Option<String>,
    pub quality: Option<String>,
    pub language: Option<String>,
    pub prompt_version: Option<String>,
    pub ingested_at: Option<String>,
    pub compiled_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WikiArticleRecord {
    pub path: String,
    pub title: String,
    pub sources: Vec<String>,
    pub content_hash: String,
    pub created_at: String,
    pub updated_at: String,
    pub status: String,
    pub approved_at: Option<String>,
    pub approval_notes: Option<String>,
    pub kind: String,
    pub question_hash: Option<String>,
    pub synthesis_sources: Vec<String>,
    pub synthesis_source_hashes: Vec<Vec<String>>,
    pub article_id: Option<String>,
    pub last_compile_pipeline: Option<String>,
    pub entity_id: Option<String>,
}

impl WikiArticleRecord {
    pub fn is_draft(&self) -> bool {
        self.status == "draft"
    }
    pub fn is_verified(&self) -> bool {
        self.status == "verified"
    }
    pub fn is_published(&self) -> bool {
        self.status == "published"
    }
    pub fn is_trusted(&self) -> bool {
        self.is_verified() || self.is_published()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeItemRecord {
    pub name: String,
    pub kind: String,
    pub subtype: Option<String>,
    pub status: String,
    pub confidence: f64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ItemMentionRecord {
    pub item_name: String,
    pub source_path: String,
    pub mention_text: String,
    pub context: Option<String>,
    pub evidence_level: String,
    pub confidence: f64,
    pub id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BibliographicMetadata {
    pub title: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    pub year: Option<i32>,
    pub journal: Option<String>,
    pub venue: Option<String>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub bibtex_key: Option<String>,
    #[serde(default)]
    pub affiliations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TermRecord {
    pub name: String,
    pub definition: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub source_segment_id: String,
    pub provenance: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TermExtractionResult {
    pub terms: Vec<TermRecord>,
    pub source_segment_id: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RelationCandidate {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub evidence: String,
    pub source_segment_id: String,
    pub provenance: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RelationExtractionResult {
    pub relations: Vec<RelationCandidate>,
    pub source_segment_id: String,
    pub model: String,
}

pub fn json_schema_template(kind: &str) -> String {
    match kind {
        "AnalysisResult" => serde_json::json!({
            "summary": "2-3 sentence summary in the note's language",
            "concepts": [{"name": "Canonical concept name", "aliases": ["short surface forms"]}],
            "suggested_topics": ["Titles of wiki articles this note should feed into (max 5)"],
            "named_references": ["Exact named references copied from the note"],
            "quality": "high | medium | low",
            "language": "ISO 639-1 language code of the note (e.g. 'en', 'fr', 'de'). Null if uncertain."
        })
        .to_string(),
        "SingleArticle" => serde_json::json!({
            "title": "<title>",
            "content": "Full markdown body with [[wikilinks]] inline (no frontmatter)",
            "tags": ["topic tags, lowercase hyphen-separated, max 6"]
        })
        .to_string(),
        "PageSelection" => serde_json::json!({
            "pages": ["Exact page titles from the wiki index (max 5)"]
        })
        .to_string(),
        "QueryAnswer" => serde_json::json!({
            "answer": "Markdown answer with [[wikilinks]] referencing concepts",
            "title": "Optional short topic title describing the answer subject"
        })
        .to_string(),
        "CompilePlan" => serde_json::json!({
            "articles": [{
                "title": "Article title",
                "action": "create | update",
                "path": "Relative path inside wiki/, e.g. 'physics/quantum.md'",
                "reasoning": "One sentence: why this article",
                "source_paths": ["Raw note paths that feed this article"]
            }],
            "mocs_to_update": ["MOC filenames (e.g. 'MOC-Physics.md') that need updating"]
        })
        .to_string(),
        _ => "{}".into(),
    }
}
