pub fn load_prompt(source_type: &str) -> &'static str {
    match source_type {
        "textbook" => include_str!("../../prompts/textbook.md"),
        "paper" => include_str!("../../prompts/paper.md"),
        "api_docs" => include_str!("../../prompts/api_docs.md"),
        "web_article" => include_str!("../../prompts/web_article.md"),
        "corp_docs" => include_str!("../../prompts/corp_docs.md"),
        _ => include_str!("../../prompts/notes.md"),
    }
    .trim_end_matches('\n')
}
