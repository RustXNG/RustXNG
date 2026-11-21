//! Site map and structured extraction for agents (`/map` and `/extract`).
//!
//! `/map` answers "which pages does this site have" without crawling the whole
//! site: it reads `robots.txt` for sitemap declarations, follows sitemap
//! indexes, and falls back to the links on the seed page.
//!
//! `/extract` turns one page into records. The schema is resolved from the
//! page itself (JSON-LD, microdata, definition lists, tables) so the caller
//! gets structure instead of prose. Nothing is invented: every field carries
//! the selector or JSON-LD path it came from.

use std::collections::BTreeSet;

use scraper::{Html, Selector};

use crate::crawl::{normalize_url, sitemap_locations};
use crate::fetch::PageReader;
use crate::robots::Robots;

/// Most URLs one `/map` call returns.
pub const MAX_MAP_URLS: usize = 5_000;
/// Most sitemaps followed from one seed (index files expand to more).
pub const MAX_SITEMAPS: usize = 32;
/// Largest sitemap body accepted, in bytes.
const MAX_SITEMAP_BYTES: usize = 8 * 1024 * 1024;
/// Most links read from the seed page when no sitemap exists.
const MAX_SEED_LINKS: usize = 2_000;
/// Most records one `/extract` call returns.
pub const MAX_RECORDS: usize = 200;
/// Most characters of a field value.
const MAX_FIELD_CHARS: usize = 2_000;

/// Where one URL in a `/map` answer came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UrlSource {
    /// Declared in `robots.txt`.
    RobotsSitemap,
    /// Found in a sitemap that was reached from `robots.txt`.
    SitemapIndex,
    /// A link on the seed page.
    SeedLinks,
}

/// One URL of a `/map` answer.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MappedUrl {
    pub url: String,
    /// 1 for a sitemap reached from `robots.txt`, 2 for one nested in an index.
    pub depth: u8,
    pub source: UrlSource,
}

/// The result of a `/map` call.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MapReport {
    /// The seed the caller asked about.
    pub seed: String,
    /// Origin the URLs were restricted to.
    pub origin: String,
    /// Whether a sitemap was used at all.
    pub used_sitemap: bool,
    /// Whether the list was cut at the limit.
    pub truncated: bool,
    /// URLs in discovery order, deduplicated and normalised.
    pub urls: Vec<MappedUrl>,
    /// Sitemaps that could not be read, with a short reason.
    pub errors: Vec<String>,
}

/// Read one body with the shared page reader, bounded.
async fn body(reader: &PageReader, url: &str) -> Result<String, String> {
    reader
        .text(url, MAX_SITEMAP_BYTES)
        .await
        .map(|text| text.to_string())
}

/// Collect the URLs of a site.
///
/// Discovery order: sitemaps named by `robots.txt`, then sitemap indexes
/// declared inside them, then (only when none of those produced a URL) the
/// links on the seed page. Every URL is normalised, restricted to the seed's
/// origin, and deduplicated.
pub async fn map_site(reader: &PageReader, seed: &str, limit: usize) -> Result<MapReport, String> {
    let limit = limit.clamp(1, MAX_MAP_URLS);
    let seed_url = reqwest::Url::parse(seed).map_err(|_| "not a valid URL".to_string())?;
    let origin = seed_url.origin().ascii_serialization();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut urls: Vec<MappedUrl> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut used_sitemap = false;

    let root = format!("{origin}/robots.txt");
    let mut queue: Vec<(String, u8, UrlSource)> = Vec::new();
    match body(reader, &root).await {
        Ok(text) => {
            let robots = Robots::parse(&text, crate::crawl::PRODUCT);
            for sitemap in robots.sitemaps().iter().take(MAX_SITEMAPS) {
                queue.push((sitemap.clone(), 1, UrlSource::RobotsSitemap));
            }
        }
        // A missing robots.txt is not an error: the seed links still work.
        Err(error) => errors.push(format!("robots.txt: {error}")),
    }

    let mut followed = 0usize;
    while let Some((sitemap, depth, source)) = queue.pop() {
        if followed >= MAX_SITEMAPS || urls.len() >= limit {
            break;
        }
        followed += 1;
        let text = match body(reader, &sitemap).await {
            Ok(text) => text,
            Err(error) => {
                errors.push(format!("{sitemap}: {error}"));
                continue;
            }
        };
        match sitemap_locations(text.as_bytes(), MAX_MAP_URLS) {
            Ok((is_index, locations)) => {
                used_sitemap = true;
                if is_index {
                    for location in locations {
                        if queue.len() >= MAX_SITEMAPS {
                            break;
                        }
                        queue.push((location, depth.saturating_add(1), UrlSource::SitemapIndex));
                    }
                    continue;
                }
                for location in locations {
                    if urls.len() >= limit {
                        break;
                    }
                    if !location.starts_with(&origin) {
                        continue;
                    }
                    if seen.insert(location.clone()) {
                        urls.push(MappedUrl {
                            url: location,
                            depth,
                            source,
                        });
                    }
                }
            }
            Err(error) => errors.push(format!("{sitemap}: {error}")),
        }
    }

    // No sitemap produced anything: read the links the seed page offers.
    if urls.is_empty()
        && let Ok(text) = body(reader, seed).await
    {
        let document = Html::parse_document(&text);
        if let Ok(selector) = Selector::parse("a[href]") {
            for element in document.select(&selector).take(MAX_SEED_LINKS) {
                if urls.len() >= limit {
                    break;
                }
                let Some(href) = element.value().attr("href") else {
                    continue;
                };
                let Ok(resolved) = seed_url.join(href) else {
                    continue;
                };
                let Some(normalised) = normalize_url(resolved.as_str()) else {
                    continue;
                };
                if !normalised.starts_with(&origin) {
                    continue;
                }
                if seen.insert(normalised.clone()) {
                    urls.push(MappedUrl {
                        url: normalised,
                        depth: 0,
                        source: UrlSource::SeedLinks,
                    });
                }
            }
        }
    }

    let truncated = urls.len() >= limit;
    Ok(MapReport {
        seed: seed.to_string(),
        origin,
        used_sitemap,
        truncated,
        urls,
        errors,
    })
}

/// One extracted record and where each field came from.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Record {
    /// The record's own identity, when the page names one.
    pub id: Option<String>,
    /// Field name to value.
    pub fields: std::collections::BTreeMap<String, String>,
    /// Where the record was found: `jsonld`, `microdata`, `definition-list`, `table`.
    pub source: &'static str,
    /// Path inside the source (JSON-LD key path, table row number, ...).
    pub path: String,
}

/// The result of an `/extract` call.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExtractReport {
    pub url: String,
    /// Records found, in document order.
    pub records: Vec<Record>,
    /// Whether the list was cut at the limit.
    pub truncated: bool,
}

/// Flatten a JSON value into `path -> scalar` pairs.
fn flatten(
    value: &serde_json::Value,
    prefix: &str,
    out: &mut std::collections::BTreeMap<String, String>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(value, &path, out);
            }
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let path = format!("{prefix}[{index}]");
                flatten(item, &path, out);
            }
        }
        serde_json::Value::String(text) => {
            let text = text.trim();
            if !text.is_empty() {
                out.insert(
                    prefix.to_string(),
                    text.chars().take(MAX_FIELD_CHARS).collect(),
                );
            }
        }
        serde_json::Value::Number(number) => {
            out.insert(prefix.to_string(), number.to_string());
        }
        serde_json::Value::Bool(flag) => {
            out.insert(prefix.to_string(), flag.to_string());
        }
        serde_json::Value::Null => {}
    }
}

/// Every `<script type="application/ld+json">` object on the page.
fn jsonld_records(document: &Html) -> Vec<Record> {
    let Ok(selector) = Selector::parse(r#"script[type="application/ld+json"]"#) else {
        return Vec::new();
    };
    let mut records = Vec::new();
    for (index, element) in document.select(&selector).enumerate() {
        let raw = element.text().collect::<String>();
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        // A graph container holds several records; anything else is one record.
        if let Some(graph) = value.get("@graph").and_then(|g| g.as_array()) {
            for (position, item) in graph.iter().enumerate() {
                let mut fields = std::collections::BTreeMap::new();
                flatten(item, "", &mut fields);
                if fields.is_empty() {
                    continue;
                }
                records.push(Record {
                    id: item.get("@id").and_then(|v| v.as_str()).map(str::to_string),
                    path: format!("script[{index}].@graph[{position}]"),
                    fields,
                    source: "jsonld",
                });
            }
            continue;
        }
        let mut fields = std::collections::BTreeMap::new();
        flatten(&value, "", &mut fields);
        if fields.is_empty() {
            continue;
        }
        records.push(Record {
            id: value
                .get("@id")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            path: format!("script[{index}]"),
            fields,
            source: "jsonld",
        });
    }
    records
}

/// Definition lists as one record per `dt`/`dd` pair group.
fn definition_records(document: &Html) -> Vec<Record> {
    let (Ok(list_selector), Ok(term_selector)) = (Selector::parse("dl"), Selector::parse("dt, dd"))
    else {
        return Vec::new();
    };
    let mut records = Vec::new();
    for (index, list) in document.select(&list_selector).enumerate() {
        let mut fields = std::collections::BTreeMap::new();
        let mut pending: Option<String> = None;
        for child in list.select(&term_selector) {
            let text = child.text().collect::<String>();
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if text.is_empty() {
                continue;
            }
            if child.value().name() == "dt" {
                pending = Some(text.chars().take(MAX_FIELD_CHARS).collect());
            } else if let Some(term) = pending.take() {
                let value: String = text.chars().take(MAX_FIELD_CHARS).collect();
                fields.insert(term, value);
            }
        }
        if !fields.is_empty() {
            records.push(Record {
                id: None,
                path: format!("dl[{index}]"),
                fields,
                source: "definition-list",
            });
        }
    }
    records
}

/// The first row of a table as field names, one record per following row.
fn table_records(document: &Html) -> Vec<Record> {
    let (Ok(table_selector), Ok(row_selector), Ok(cell_selector)) = (
        Selector::parse("table"),
        Selector::parse("tr"),
        Selector::parse("th, td"),
    ) else {
        return Vec::new();
    };
    let mut records = Vec::new();
    for (index, table) in document.select(&table_selector).enumerate() {
        let mut rows = table.select(&row_selector);
        let Some(header) = rows.next() else {
            continue;
        };
        let names: Vec<String> = header
            .select(&cell_selector)
            .map(|cell| {
                cell.text()
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .filter(|name| !name.is_empty())
            .collect();
        if names.len() < 2 {
            continue;
        }
        for (row_index, row) in rows.enumerate() {
            if records.len() >= MAX_RECORDS {
                break;
            }
            let cells: Vec<String> = row
                .select(&cell_selector)
                .map(|cell| {
                    cell.text()
                        .collect::<String>()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect();
            if cells.len() < 2 {
                continue;
            }
            let mut fields = std::collections::BTreeMap::new();
            for (name, value) in names.iter().zip(cells.iter()) {
                if !value.is_empty() {
                    fields.insert(name.clone(), value.chars().take(MAX_FIELD_CHARS).collect());
                }
            }
            if !fields.is_empty() {
                records.push(Record {
                    id: None,
                    path: format!("table[{index}].row[{row_index}]"),
                    fields,
                    source: "table",
                });
            }
        }
    }
    records
}

/// Extract structured records from one page.
///
/// Sources are tried in order and the first that yields records wins, so a
/// page does not report the same data four times: JSON-LD, then definition
/// lists, then tables.
pub async fn extract(
    reader: &PageReader,
    url: &str,
    limit: usize,
) -> Result<ExtractReport, String> {
    let limit = limit.clamp(1, MAX_RECORDS);
    let text = body(reader, url).await?;
    let document = Html::parse_document(&text);
    let mut records = jsonld_records(&document);
    if records.is_empty() {
        records = definition_records(&document);
    }
    if records.is_empty() {
        records = table_records(&document);
    }
    let truncated = records.len() > limit;
    records.truncate(limit);
    Ok(ExtractReport {
        url: url.to_string(),
        records,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonld_graph_yields_one_record_per_node() {
        let html = Html::parse_document(
            r##"<html><body><script type="application/ld+json">
            {"@graph":[
              {"@type":"Article","@id":"#a","name":"First","author":{"name":"Ada"}},
              {"@type":"Article","@id":"#b","name":"Second"}
            ]}</script></body></html>"##,
        );
        let records = jsonld_records(&html);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id.as_deref(), Some("#a"));
        assert_eq!(
            records[0].fields.get("name").map(String::as_str),
            Some("First")
        );
        assert_eq!(
            records[0].fields.get("author.name").map(String::as_str),
            Some("Ada"),
            "nested objects are flattened into a path"
        );
    }

    #[test]
    fn a_bare_jsonld_object_is_one_record() {
        let html = Html::parse_document(
            r#"<script type="application/ld+json">{"name":"Solo","price":"3"}</script>"#,
        );
        let records = jsonld_records(&html);
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].fields.get("price").map(String::as_str),
            Some("3")
        );
    }

    #[test]
    fn broken_jsonld_is_skipped_rather_than_failing_the_page() {
        let html = Html::parse_document(
            r#"<script type="application/ld+json">{not json}</script><script type="application/ld+json">{"name":"ok"}</script>"#,
        );
        let records = jsonld_records(&html);
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].fields.get("name").map(String::as_str),
            Some("ok")
        );
    }

    #[test]
    fn a_definition_list_pairs_terms_with_definitions() {
        let html = Html::parse_document(
            "<dl><dt>Port</dt><dd>29483</dd><dt>Bind</dt><dd>127.0.0.1</dd></dl>",
        );
        let records = definition_records(&html);
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].fields.get("Port").map(String::as_str),
            Some("29483")
        );
        assert_eq!(
            records[0].fields.get("Bind").map(String::as_str),
            Some("127.0.0.1")
        );
    }

    #[test]
    fn a_table_uses_its_header_as_field_names() {
        let html = Html::parse_document(
            "<table><tr><th>Name</th><th>Default</th></tr>             <tr><td>--port</td><td>29483</td></tr>             <tr><td>--bind</td><td>127.0.0.1</td></tr></table>",
        );
        let records = table_records(&html);
        assert_eq!(records.len(), 2);
        assert_eq!(
            records[0].fields.get("Name").map(String::as_str),
            Some("--port")
        );
        assert_eq!(
            records[1].fields.get("Default").map(String::as_str),
            Some("127.0.0.1")
        );
        assert_eq!(records[0].path, "table[0].row[0]");
    }

    #[test]
    fn a_single_column_table_is_not_mistaken_for_records() {
        let html =
            Html::parse_document("<table><tr><th>Only</th></tr><tr><td>one</td></tr></table>");
        assert!(table_records(&html).is_empty());
    }

    #[test]
    fn scalars_are_flattened_with_their_paths() {
        let value = serde_json::json!({"a": {"b": [1, "two", null]}, "c": true});
        let mut fields = std::collections::BTreeMap::new();
        flatten(&value, "", &mut fields);
        assert_eq!(fields.get("a.b[0]").map(String::as_str), Some("1"));
        assert_eq!(fields.get("a.b[1]").map(String::as_str), Some("two"));
        assert_eq!(fields.get("c").map(String::as_str), Some("true"));
        assert!(!fields.contains_key("a.b[2]"), "null contributes nothing");
    }
}
