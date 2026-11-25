//! Help corpus shipped with the bridge (UX spec §5, AI-NATIVE.md §1.2).
//!
//! `content/help/*.md` and `index.json` are embedded at build time by `build.rs`. A runtime
//! corpus (`OPENMESH_HELP_DIR`, or `content/help` relative to the working directory for
//! development) takes precedence when present, so docs workstreams can edit content without
//! rebuilding the daemon.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

include!(concat!(env!("OUT_DIR"), "/help_embedded.rs"));
include!(concat!(env!("OUT_DIR"), "/openapi_embedded.rs"));

/// Directory to read live help content from, when available.
pub fn runtime_help_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("OPENMESH_HELP_DIR").filter(|v| !v.is_empty()) {
        let path = PathBuf::from(dir);
        if path.is_dir() {
            return Some(path);
        }
    }
    let default = PathBuf::from("content").join("help");
    if default.is_dir() {
        return Some(default);
    }
    None
}

fn read_dir_file(dir: &Path, slug: &str) -> Option<String> {
    if !valid_slug(slug) {
        return None;
    }
    std::fs::read_to_string(dir.join(format!("{slug}.md"))).ok()
}

fn read_dir_index(dir: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(dir.join("index.json")).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value.get("topics").and_then(|v| v.as_array())?;
    Some(value)
}

/// A slug is a plain lowercase file stem: no separators, dots or traversal.
pub fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 96
        && slug
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The help index: `{version, topics:[{slug,title,summary,audience,minutes,related}]}`.
pub fn index_value() -> Value {
    if let Some(dir) = runtime_help_dir() {
        if let Some(value) = read_dir_index(&dir) {
            return value;
        }
    }
    serde_json::from_str(INDEX_JSON).unwrap_or_else(|_| json!({ "version": 1, "topics": [] }))
}

/// Known topic slugs, in index order (runtime directory first, embedded fallback).
pub fn slugs() -> Vec<String> {
    let index = index_value();
    if let Some(topics) = index.get("topics").and_then(|v| v.as_array()) {
        let list: Vec<String> = topics
            .iter()
            .filter_map(|topic| topic.get("slug").and_then(|v| v.as_str()))
            .map(str::to_string)
            .collect();
        if !list.is_empty() {
            return list;
        }
    }
    if let Some(dir) = runtime_help_dir() {
        let mut list: Vec<String> = std::fs::read_dir(&dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|entry| {
                        let path = entry.path();
                        (path.extension().and_then(|e| e.to_str()) == Some("md"))
                            .then(|| path.file_stem()?.to_str().map(str::to_string))
                            .flatten()
                    })
                    .collect()
            })
            .unwrap_or_default();
        list.sort();
        if !list.is_empty() {
            return list;
        }
    }
    SLUGS.iter().map(|s| s.to_string()).collect()
}

/// Markdown body for one topic (runtime corpus then embedded copy).
pub fn markdown(slug: &str) -> Option<String> {
    let slug = slug.trim().to_ascii_lowercase();
    if !valid_slug(&slug) {
        return None;
    }
    if let Some(dir) = runtime_help_dir() {
        if let Some(text) = read_dir_file(&dir, &slug) {
            return Some(text);
        }
    }
    embedded_markdown(&slug).map(str::to_string)
}

fn humanize_slug(slug: &str) -> String {
    let mut title = String::new();
    for (index, word) in slug.split('-').enumerate() {
        if index > 0 {
            title.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            title.push(first.to_ascii_uppercase());
            title.extend(chars);
        }
    }
    title
}

/// One topic: markdown plus the index metadata (`{slug,title,summary,audience,minutes,related,markdown}`).
pub fn topic_value(slug: &str) -> Option<Value> {
    let slug = slug.trim().to_ascii_lowercase();
    let markdown = markdown(&slug)?;
    let index = index_value();
    let metadata = index
        .get("topics")
        .and_then(|v| v.as_array())
        .and_then(|topics| {
            topics
                .iter()
                .find(|topic| topic.get("slug").and_then(|v| v.as_str()) == Some(slug.as_str()))
        })
        .cloned();
    let mut value = metadata.unwrap_or_else(|| {
        json!({
            "slug": slug,
            "title": humanize_slug(&slug),
            "summary": "",
            "audience": "both",
            "minutes": 0,
            "related": [],
        })
    });
    if let Some(object) = value.as_object_mut() {
        object.insert("markdown".into(), Value::String(markdown));
    }
    Some(value)
}

/// Plain-text help index used by `openmesh_help`/agent prompts: one bullet per topic.
pub fn index_text() -> String {
    let index = index_value();
    let mut out = String::new();
    if let Some(topics) = index.get("topics").and_then(|v| v.as_array()) {
        for topic in topics {
            let slug = topic.get("slug").and_then(|v| v.as_str()).unwrap_or("?");
            let title = topic.get("title").and_then(|v| v.as_str()).unwrap_or(slug);
            let summary = topic.get("summary").and_then(|v| v.as_str()).unwrap_or("");
            let audience = topic
                .get("audience")
                .and_then(|v| v.as_str())
                .unwrap_or("both");
            let minutes = topic.get("minutes").and_then(|v| v.as_u64()).unwrap_or(0);
            if summary.is_empty() {
                out.push_str(&format!("- {slug}: {title} ({audience}, {minutes} min)\n"));
            } else {
                out.push_str(&format!(
                    "- {slug}: {title} ({audience}, {minutes} min) — {summary}\n"
                ));
            }
        }
    }
    out
}

/// Embedded OpenAPI 3.1 document (`docs/api/openapi.json`).
pub fn openapi_json() -> &'static str {
    OPENAPI_JSON
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_embeds_topics_from_content() {
        let index = index_value();
        let topics = index
            .get("topics")
            .and_then(|v| v.as_array())
            .expect("topics");
        assert!(!topics.is_empty(), "help index is embedded at build time");
        let slugs = slugs();
        assert!(slugs.iter().any(|slug| slug == "getting-started"));
    }

    #[test]
    fn topic_has_markdown_and_metadata() {
        let topic = topic_value("getting-started").expect("topic exists");
        assert_eq!(topic["slug"], "getting-started");
        assert!(!topic["title"].as_str().unwrap_or("").is_empty());
        let markdown = topic["markdown"].as_str().unwrap_or("");
        assert!(
            markdown.starts_with("# "),
            "topic is markdown: {markdown:?}"
        );
        assert!(topic["audience"].is_string());
    }

    #[test]
    fn unknown_and_unsafe_slugs_are_rejected() {
        assert!(topic_value("does-not-exist").is_none());
        assert!(markdown("../secret").is_none());
        assert!(markdown("..").is_none());
        assert!(!valid_slug("a/b"));
    }

    #[test]
    fn openapi_document_parses() {
        let value: Value = serde_json::from_str(openapi_json()).expect("embedded openapi parses");
        assert_eq!(value["openapi"], "3.1.0");
        assert!(value["paths"].is_object());
    }
}
