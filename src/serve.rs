use std::io::{BufRead, Write};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::config::Config;
use crate::llm::ModelRouter;
use crate::pipeline::query::run_query;
use crate::state::StateDb;
use crate::vault::{is_concept_article_path, mapping_get_str, parse_note};
use crate::Result;

fn article_visible(meta: &serde_yaml::Mapping, config: &Config) -> bool {
    let vis = mapping_get_str(meta, "visibility")
        .unwrap_or_else(|| config.mcp.default_visibility.clone());
    if vis != "public" {
        return false;
    }
    let tags = crate::vault::mapping_get_seq_str(meta, "tags");
    !tags
        .iter()
        .any(|t| config.mcp.exclude_tags.iter().any(|e| e == t))
}

pub fn run_server(
    config: Config,
    transport: &str,
    name: Option<&str>,
    host: &str,
    port: u16,
) -> Result<()> {
    let db = Arc::new(
        StateDb::open_readonly(&config.state_db_path())
            .or_else(|_| StateDb::open(&config.state_db_path()))?,
    );
    let server_name = name.unwrap_or("notus");
    match transport {
        "streamable-http" => {
            eprintln!("streamable-http listening on http://{host}:{port}/mcp (JSON-RPC POST)");
            let listener = std::net::TcpListener::bind((host, port))?;
            for stream in listener.incoming().flatten() {
                let _ = handle_http(stream, &config, &db, server_name);
            }
            Ok(())
        }
        _ => serve_stdio(&config, &db, server_name),
    }
}

fn serve_stdio(config: &Config, db: &Arc<StateDb>, server_name: &str) -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let req: Value = serde_json::from_str(&line).unwrap_or(json!({}));
        if let Some(resp) = handle_rpc(&req, config, db, server_name) {
            writeln!(stdout, "{}", resp)?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn handle_http(
    mut stream: std::net::TcpStream,
    config: &Config,
    db: &Arc<StateDb>,
    server_name: &str,
) -> Result<()> {
    use std::io::Read;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let n = stream.read(&mut tmp)?;
    buf.extend_from_slice(&tmp[..n]);
    let text = String::from_utf8_lossy(&buf);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("{}");
    let req: Value = serde_json::from_str(body).unwrap_or(json!({}));
    let resp = handle_rpc(&req, config, db, server_name)
        .unwrap_or(json!({"jsonrpc":"2.0","id":null,"result":{}}));
    let payload = resp.to_string();
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        payload.len(),
        payload
    );
    stream.write_all(header.as_bytes())?;
    Ok(())
}

fn handle_rpc(req: &Value, config: &Config, db: &Arc<StateDb>, server_name: &str) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
    id.as_ref()?;
    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": server_name, "version": crate::paths::VERSION},
        }),
        "tools/list" => json!({"tools": tool_defs()}),
        "tools/call" => {
            let name = req["params"]["name"].as_str().unwrap_or("");
            let args = req["params"]["arguments"].clone();
            match call_tool(name, &args, config, db) {
                Ok(v) => json!({"content": [{"type":"text","text": v.to_string()}]}),
                Err(e) => {
                    json!({"content": [{"type":"text","text": e.to_string()}], "isError": true})
                }
            }
        }
        "ping" => json!({}),
        _ => json!({}),
    };
    Some(json!({"jsonrpc":"2.0","id": id, "result": result}))
}

fn tool_defs() -> Vec<Value> {
    [
        "list_articles",
        "read_article",
        "find_concept",
        "search_articles",
        "get_concept",
        "list_sources",
        "trace_lineage",
        "answer_question",
        "read_source_segment",
        "search_source_segments",
        "get_source_passages",
        "list_segments",
    ]
    .into_iter()
    .map(|n| json!({"name": n, "description": n, "inputSchema": {"type":"object","properties":{}}}))
    .collect()
}

fn call_tool(name: &str, args: &Value, config: &Config, db: &Arc<StateDb>) -> Result<Value> {
    match name {
        "list_articles" => {
            let arts = db.list_articles()?;
            let out: Vec<_> = arts
                .into_iter()
                .filter(|a| a.status == "published")
                .map(|a| json!({"title": a.title, "path": a.path, "kind": a.kind, "id": a.article_id}))
                .collect();
            Ok(json!(out))
        }
        "read_article" => {
            let q = args["name_or_id"].as_str().unwrap_or("");
            let path = config
                .wiki_dir()
                .join(format!("{}.md", crate::vault::sanitize_filename(q, 100)));
            let (meta, body) = parse_note(&path)?;
            if !article_visible(&meta, config) {
                return Err(crate::Error::msg("article not visible"));
            }
            Ok(json!({"name": q, "body": body, "frontmatter": format!("{meta:?}")}))
        }
        "find_concept" | "get_concept" => {
            let q = args
                .get("query")
                .or_else(|| args.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match db.entity_id_for_name(q)? {
                Some(id) => {
                    let label = db.preferred_label_for_entity(&id)?;
                    Ok(json!({"name": label, "entity_id": id, "aliases": db.get_aliases(&id)?}))
                }
                None => Ok(Value::Null),
            }
        }
        "search_articles" => {
            let q = args["query"].as_str().unwrap_or("").to_lowercase();
            let arts = db.list_articles()?;
            let out: Vec<_> = arts
                .into_iter()
                .filter(|a| {
                    a.title.to_lowercase().contains(&q) || a.path.to_lowercase().contains(&q)
                })
                .take(10)
                .map(|a| json!({"title": a.title, "path": a.path}))
                .collect();
            Ok(json!(out))
        }
        "list_sources" => {
            let docs = db.list_source_documents()?;
            Ok(json!(docs
                .into_iter()
                .map(|(id, ty, title)| json!({"id": id, "source_type": ty, "title": title}))
                .collect::<Vec<_>>()))
        }
        "trace_lineage" => {
            let q = args["name_or_id"].as_str().unwrap_or("");
            let arts = db.list_articles()?;
            let found = arts
                .into_iter()
                .find(|a| a.title == q || a.article_id.as_deref() == Some(q));
            Ok(json!({"article": found, "lineage": []}))
        }
        "answer_question" => {
            let q = args["question"].as_str().unwrap_or("");
            let router = ModelRouter::build(config, None)?;
            let res = run_query(config, &router, db, q, false, false)?;
            Ok(
                json!({"answer": res.answer, "title": res.title, "selected_pages": res.selected_pages}),
            )
        }
        "read_source_segment" => {
            let id = args["segment_id"].as_str().unwrap_or("");
            match db.fetch_segment_by_id(id)? {
                Some((id, source_id, text)) => {
                    Ok(json!({"id": id, "source_id": source_id, "text": text}))
                }
                None => Err(crate::Error::msg("segment not found")),
            }
        }
        "list_segments" => {
            let source_id = args["source_id"].as_str().unwrap_or("");
            let segs = db.list_segments_for_source(source_id)?;
            Ok(json!(segs
                .into_iter()
                .map(|(id, text, ord)| json!({"id": id, "ordinal": ord, "chars": text.len()}))
                .collect::<Vec<_>>()))
        }
        "search_source_segments" | "get_source_passages" => Ok(json!({"results": []})),
        _ => Err(crate::Error::msg(format!("unknown tool: {name}"))),
    }
}

#[allow(dead_code)]
fn _is_concept(p: &str) -> bool {
    is_concept_article_path(p)
}
