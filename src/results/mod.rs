//! Result collection, scoring and ordering, ported from `searx/results.py`
//! and `searx/result_types/`.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::engine::EngineItem;

mod rank;

pub use rank::{canonical_host_for_query, page_language, query_terms, relevance_score};

use rank::{
    authority, order_results, page_language_adjustment, relevance, wikipedia_language_penalty,
};

/// Result priority, upstream `MainResult.PriorityType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    /// The engine explicitly marked the result as unimportant.
    Low,
    #[default]
    Medium,
    /// Answers / direct hits, always ranked first.
    High,
}

impl Priority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// Template used to render a result, upstream `result.template`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Template {
    #[default]
    Default,
    Images,
    Videos,
    Map,
    Music,
    Packages,
    /// Key/value rows from a database backed engine (upstream keyvalue.html).
    KeyValue,
}

impl Template {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Images => "images",
            Self::Videos => "videos",
            Self::Map => "map",
            Self::Music => "music",
            Self::Packages => "packages",
            Self::KeyValue => "keyvalue",
        }
    }
}

/// One search result, upstream `MainResult` / `LegacyResult`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchResult {
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub content: String,
    /// The engine that produced the result (first one wins).
    pub engine: String,
    /// Every engine that produced this result.
    #[serde(default)]
    pub engines: Vec<String>,
    /// 1-based positions inside each engine's result list.
    #[serde(default)]
    pub positions: Vec<u32>,
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub category: String,
    #[serde(default, serialize_with = "serialize_template")]
    pub template: Template,
    #[serde(
        default,
        skip_serializing_if = "is_medium",
        serialize_with = "serialize_priority"
    )]
    pub priority: Priority,
    #[serde(
        rename = "publishedDate",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub published_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub img_src: Option<String>,
    /// Engine specific extra fields (length, views, licence, ...).
    ///
    /// Upstream serialises these as one `"key: value | key: value"` string in
    /// the JSON API, so we do the same.
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        serialize_with = "serialize_metadata"
    )]
    pub metadata: BTreeMap<String, String>,
    /// A short URL shown in the UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pretty_url: Option<String>,
    /// Which extra result page this came from (0 = page 1).
    #[serde(skip)]
    pub deep_page: u32,
    /// Readable page text, only when the request asked for raw content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_content: Option<String>,
    /// Whether optional raw content was cut at the configured character limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_content_truncated: Option<bool>,
    /// Why the body is missing or degraded, only when the request asked for
    /// raw content: `ok`, `not_requested`, `too_large`, `fetch_failed`,
    /// `robots_blocked`, `extract_failed`, `render_failed` or `unknown`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_content_status: Option<String>,
    /// Short reader message behind `raw_content_status`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_content_error: Option<String>,
    /// 転載統合（distillation）でこの結果にまとめた転載元のタイトルと URL。
    ///
    /// SearXNG には無い上流超えの蒸留: 同じ原発表の転載（ベンダー公告 → 記事 → ミラー）を
    /// 1 件に統合し、独立した根拠を数え誤らせないようにする（D47）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merged_sources: Vec<String>,
    /// 出典系列（転載チェーンの記録）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_family: Option<String>,
    /// 検索クエリとの関連性（0.0〜1.0）。蒸留の可視化と、転載統合時の代表選択に使う。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relevance_score: Option<f64>,
    /// Whether the result belongs to the group of the previous one (UI only).
    #[serde(skip)]
    pub open_group: bool,
    #[serde(skip)]
    pub close_group: bool,
}

fn is_medium(priority: &Priority) -> bool {
    *priority == Priority::Medium
}

/// Upstream renders `default.html`, `images.html`, ... in its JSON API.
fn serialize_template<S>(template: &Template, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&format!("{}.html", template.as_str()))
}

/// Upstream keeps the engine metadata as one human readable string.
fn serialize_metadata<S>(
    metadata: &BTreeMap<String, String>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let text = metadata
        .iter()
        .map(|(key, value)| format!("{key}: {value}"))
        .collect::<Vec<_>>()
        .join(" | ");
    serializer.serialize_str(&text)
}

/// Upstream uses an empty string for the default priority.
fn serialize_priority<S>(priority: &Priority, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(match priority {
        Priority::Medium => "",
        other => other.as_str(),
    })
}

impl SearchResult {
    pub fn new(url: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            title: title.into(),
            ..Default::default()
        }
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    pub fn with_template(mut self, template: Template) -> Self {
        self.template = template;
        self
    }

    pub fn with_metadata(mut self, key: &str, value: impl Into<String>) -> Self {
        self.metadata.insert(key.to_string(), value.into());
        self
    }
}

/// An infobox, upstream `Infobox`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Infobox {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub img_src: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub urls: Vec<InfoboxUrl>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
    #[serde(default)]
    pub engine: String,
}

/// A link of an infobox (`urls` in the infobox template).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InfoboxUrl {
    pub title: String,
    pub url: String,
}

/// Engine timing, upstream `Timing`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timing {
    pub engine: String,
    pub total: f64,
    pub load: f64,
}

/// An engine that did not answer, upstream `UnresponsiveEngine`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UnresponsiveEngine {
    pub engine: String,
    #[serde(rename = "error_type")]
    pub error_type: String,
    pub suspended: bool,
}

/// Collects, deduplicates and scores the results of all engines.
#[derive(Debug, Default)]
pub struct ResultContainer {
    main: Vec<SearchResult>,
    index: HashMap<String, usize>,
    source_ranks: Vec<Vec<SourceRank>>,
    pub infoboxes: Vec<Infobox>,
    pub suggestions: BTreeSet<String>,
    pub answers: Vec<String>,
    pub corrections: BTreeSet<String>,
    pub engine_data: BTreeMap<String, BTreeMap<String, String>>,
    pub unresponsive_engines: Vec<UnresponsiveEngine>,
    pub timings: Vec<Timing>,
    pub paging: bool,
    pub redirect_url: Option<String>,
    closed: bool,
    ordered: Vec<SearchResult>,
}

#[derive(Debug, Clone, Copy)]
struct SourceRank {
    weight: f64,
    position: u32,
    deep_page: u32,
}

impl SourceRank {
    fn score(self, priority: Priority) -> f64 {
        let score = match priority {
            Priority::Low => 0.0,
            Priority::Medium => self.weight / f64::from(self.position),
            Priority::High => self.weight,
        };
        score * 0.5f64.powf(f64::from(self.deep_page))
    }
}

impl ResultContainer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Merge the results of one engine, weighting them with `weight`.
    pub fn extend(&mut self, engine_name: &str, weight: f64, items: Vec<EngineItem>) {
        self.extend_from_page(engine_name, weight, items, 0);
    }

    /// Merge one engine's results where every item carries its own weight.
    ///
    /// Weighting a whole source uniformly loses information a source itself
    /// already ranked by: the local index blends a semantic score into its
    /// order, and that score has to reach the merged number as well. Weights
    /// must not increase along the source's own order, or a later hit could
    /// outrank an earlier one after the position fusion.
    pub fn extend_weighted(&mut self, engine_name: &str, items: Vec<(f64, EngineItem)>) {
        self.extend_items(engine_name, items, 0);
    }

    /// Merge one engine's results that came from a deeper result page.
    ///
    /// Deeper pages score lower because their positions are shifted by
    /// PAGE_POSITION_STRIDE per extra page, so a page-2 hit never outranks the
    /// same engine's page-1 hits.
    pub fn extend_from_page(
        &mut self,
        engine_name: &str,
        weight: f64,
        items: Vec<EngineItem>,
        page_offset: u32,
    ) {
        let items = items.into_iter().map(|item| (weight, item)).collect();
        self.extend_items(engine_name, items, page_offset);
    }

    fn extend_items(&mut self, engine_name: &str, items: Vec<(f64, EngineItem)>, page_offset: u32) {
        if self.closed {
            return;
        }
        const PAGE_POSITION_STRIDE: u32 = 100;
        let mut position = page_offset.saturating_mul(PAGE_POSITION_STRIDE);
        let deep_page = page_offset;
        for (weight, item) in items {
            match item {
                EngineItem::Suggestion(suggestion) => {
                    if !suggestion.trim().is_empty() {
                        self.suggestions.insert(suggestion);
                    }
                }
                EngineItem::Answer(answer) => self.answers.push(answer),
                EngineItem::Correction(correction) => {
                    if !correction.trim().is_empty() {
                        self.corrections.insert(correction);
                    }
                }
                EngineItem::Infobox(mut infobox) => {
                    infobox.img_src = infobox.img_src.filter(|url| is_web_url(url));
                    infobox.url = infobox.url.filter(|url| is_web_url(url));
                    infobox.urls.retain(|link| is_web_url(&link.url));
                    infobox.engine = engine_name.to_string();
                    if infobox.id.is_empty() {
                        infobox.id = format!("{engine_name}-{}", self.infoboxes.len());
                    }
                    match self
                        .infoboxes
                        .iter_mut()
                        .find(|existing| existing.id == infobox.id)
                    {
                        Some(existing) => merge_infobox(existing, &infobox),
                        None => self.infoboxes.push(infobox),
                    }
                }
                EngineItem::EngineData { key, value } => {
                    self.engine_data
                        .entry(engine_name.to_string())
                        .or_default()
                        .insert(key, value);
                }
                EngineItem::Result(mut result) => {
                    result.deep_page = deep_page;
                    // Unwrap engine redirectors; drop dead wrappers outright so
                    // a redirector URL never reaches the user.
                    match unwrap_redirect(&result.url) {
                        Some(url) => result.url = url,
                        None => continue,
                    }
                    let keyvalue = result.template == Template::KeyValue
                        && result.url.starts_with("keyvalue:");
                    if !keyvalue && !is_web_url(&result.url) {
                        continue;
                    }
                    result.img_src = result.img_src.filter(|url| is_web_url(url));
                    result.thumbnail = result.thumbnail.filter(|url| is_web_url(url));
                    position = position.saturating_add(1);
                    result.positions = vec![position];
                    result.engine = engine_name.to_string();
                    result.engines = vec![engine_name.to_string()];
                    result.score = weight;
                    let rank = SourceRank {
                        weight,
                        position,
                        deep_page,
                    };
                    let key = dedup_key(&result.url);
                    match self.index.get(&key).copied() {
                        Some(existing) => {
                            let merged = &mut self.main[existing];
                            let ranks = &mut self.source_ranks[existing];
                            match merged
                                .engines
                                .iter()
                                .position(|engine| engine == engine_name)
                            {
                                Some(source) => {
                                    let previous = ranks[source];
                                    let score = rank.score(merged.priority);
                                    let previous_score = previous.score(merged.priority);
                                    if score > previous_score
                                        || (score == previous_score && position < previous.position)
                                    {
                                        ranks[source] = rank;
                                        merged.positions[source] = position;
                                    }
                                }
                                None => {
                                    ranks.push(rank);
                                    merged.positions.push(position);
                                }
                            }
                            merged.deep_page =
                                ranks.iter().map(|rank| rank.deep_page).min().unwrap_or(0);
                            merge_result(merged, &result);
                        }
                        None => {
                            self.index.insert(key, self.main.len());
                            self.main.push(result);
                            self.source_ranks.push(vec![rank]);
                        }
                    }
                }
            }
        }
    }

    /// Score and freeze the container (upstream `close`).
    pub fn close(
        &mut self,
        weight_of: impl Fn(&str) -> f64,
        query_terms: &[String],
        relevance_weight: f64,
        query_text: &str,
        authority_weight: f64,
    ) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.infoboxes
            .sort_by_key(|infobox| infobox.img_src.is_none());

        for (result, ranks) in self.main.iter_mut().zip(&self.source_ranks) {
            let mut weight = 1.0;
            for (source, engine) in result.engines.iter().enumerate() {
                weight *= ranks
                    .get(source)
                    .map(|rank| rank.weight)
                    .unwrap_or_else(|| weight_of(engine));
            }
            weight *= result.positions.len() as f64;
            let mut score = 0.0;
            for position in &result.positions {
                match result.priority {
                    Priority::Low => {}
                    Priority::High => score += weight,
                    Priority::Medium => score += weight / (*position as f64),
                }
            }
            // Blend in how well the URL + title cover the query terms. Position
            // fusion alone ranks a homepage above the page that actually answers
            // the query; this lifts the latter the way a neural ranker does.
            let rel = relevance(&result.url, &result.title, &result.content, query_terms);
            result.relevance_score = Some(rel);
            result.score = score + relevance_weight * rel;
            // Naming a site in the query means "that site's own page": lift the
            // canonical host and push mirrors down. This is the part keyword
            // and embedding ranking cannot see (official docs vs its mirror).
            // A page-2 hit is a fallback: it never outranks the same engine's
            // page-1 answer for the query.
            if result.deep_page > 0 {
                result.score *= 0.5f64.powf(f64::from(result.deep_page));
            }
            if authority_weight > 0.0 {
                let signal = authority(query_text, &result.url) - 0.5;
                result.score += authority_weight * signal;
            }
            // A Wikipedia edition or a translated page in the wrong language
            // is noise for this query; one in the query's language is wanted.
            // The lift for the query's language scales with how well the page
            // matches the query, so unrelated translated pages stay down.
            let adjustment = page_language_adjustment(query_text, &result.url);
            let adjustment = if adjustment > 0.0 {
                adjustment * relevance(&result.url, &result.title, &result.content, query_terms)
            } else {
                adjustment
            };
            let language = wikipedia_language_penalty(query_text, &result.url) + adjustment;
            if language != 0.0 {
                result.score += relevance_weight * language;
            }
        }

        // --- 蒸留（上流超え）: 転載の統合 ---
        // 同じ原発表の転載（ベンダー公告 → 記事 → ミラー）が別ドメインで重複する。
        // これを 1 件に統合しないと「独立した根拠の数」を数え誤る（D47）。
        // 統合は保守的: タイトルの正規化一致、数字列の完全一致、Dice >= 0.95 のすべてを満たす場合のみ。
        distill_transfers(&mut self.main, query_terms);

        self.ordered = order_results(&self.main);
    }

    /// Results sorted the way the UI shows them (upstream
    /// `get_ordered_results`).
    pub fn ordered(&self) -> &[SearchResult] {
        &self.ordered
    }

    /// Results without deduplication-group ordering, in score order.
    pub fn main_results(&self) -> &[SearchResult] {
        &self.main
    }

    /// Rough number of results (upstream reports the number of merged results).
    pub fn number_of_results(&self) -> u64 {
        self.main.len() as u64
    }

    pub fn add_unresponsive_engine(&mut self, engine: &str, error_type: &str, suspended: bool) {
        let entry = UnresponsiveEngine {
            engine: engine.to_string(),
            error_type: error_type.to_string(),
            suspended,
        };
        if !self.unresponsive_engines.contains(&entry) {
            self.unresponsive_engines.push(entry);
            self.unresponsive_engines.sort();
        }
    }

    pub fn add_timing(&mut self, engine: &str, total: f64, load: f64) {
        self.timings.push(Timing {
            engine: engine.to_string(),
            total,
            load,
        });
    }
}

/// Allow web links and relative Recoll paths.
fn is_web_url(value: &str) -> bool {
    match url::Url::parse(value) {
        Ok(url) => matches!(url.scheme(), "http" | "https"),
        Err(url::ParseError::RelativeUrlWithoutBase) => !value.trim().is_empty(),
        Err(_) => false,
    }
}

/// Merge `other` into `origin`, keeping the richer field of each pair.
fn merge_result(origin: &mut SearchResult, other: &SearchResult) {
    // Only upgrade to an HTTPS spelling that an engine actually returned.
    // MCP authorization still uses the exact final URL; no caller-provided
    // scheme substitution is accepted.
    if origin.url.starts_with("http://") && other.url.starts_with("https://") {
        origin.url = other.url.clone();
    }
    if !origin.engines.contains(&other.engine) {
        origin.engines.push(other.engine.clone());
    }
    if origin.title.is_empty() && !other.title.is_empty() {
        origin.title = other.title.clone();
    }
    if origin.content.len() < other.content.len() {
        origin.content = other.content.clone();
    }
    if origin.category.is_empty() && !other.category.is_empty() {
        origin.category = other.category.clone();
    }
    if origin.published_date.is_none() {
        origin.published_date = other.published_date.clone();
    }
    if origin.thumbnail.is_none() {
        origin.thumbnail = other.thumbnail.clone();
    }
    if origin.img_src.is_none() {
        origin.img_src = other.img_src.clone();
    }
    if origin.author.is_none() {
        origin.author = other.author.clone();
    }
    for (key, value) in &other.metadata {
        origin
            .metadata
            .entry(key.clone())
            .or_insert_with(|| value.clone());
    }
}

fn merge_infobox(origin: &mut Infobox, other: &Infobox) {
    if origin.title.is_empty() {
        origin.title = other.title.clone();
    }
    if origin.content.is_empty() {
        origin.content = other.content.clone();
    }
    if origin.img_src.is_none() {
        origin.img_src = other.img_src.clone();
    }
    if origin.url.is_none() {
        origin.url = other.url.clone();
    }
    for url in &other.urls {
        if !origin.urls.iter().any(|existing| existing.url == url.url) {
            origin.urls.push(url.clone());
        }
    }
    for (key, value) in &other.attributes {
        origin
            .attributes
            .entry(key.clone())
            .or_insert_with(|| value.clone());
    }
}

/// Group-by-category ordering, ported from `ResultContainer.get_ordered_results`.
/// URL からホストだけを抜く（転載は別ドメインの再掲のみを指す）。
fn url_host(url: &str) -> &str {
    url.split(['/', ':']).nth(3).unwrap_or("")
}

/// タイトルを正規化する（転載判定用）。
///
/// NFKC 相当の大文字小文字・記号・空白の揺れを吸収する。ただし数字列は残す
/// （版番号・CVE 番号の違いは別の記事の目印なので）。
fn normalize_title_for_dedup(title: &str) -> String {
    // 空白・記号を除去して比較する。日本語は空白で区切らないので、
    // 空白を残すと「PHP の脆弱性」と「PHPの脆弱性」が別物になってしまう。
    let mut out = String::with_capacity(title.len());
    for ch in title.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        }
    }
    out
}

/// タイトルから数字列だけを抜く（版番号・CVE 番号の照合用）。
fn title_numbers(title: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in title.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// 語の Dice 係数（正規化済みタイトルの類似度）。
fn title_dice(a: &str, b: &str) -> f64 {
    let words_a: std::collections::BTreeSet<&str> = a.split_whitespace().collect();
    let words_b: std::collections::BTreeSet<&str> = b.split_whitespace().collect();
    if words_a.is_empty() && words_b.is_empty() {
        return 1.0;
    }
    let overlap = words_a.intersection(&words_b).count();
    2.0 * overlap as f64 / (words_a.len() + words_b.len()) as f64
}

/// 転載の統合（distillation）。
///
/// 統合条件はすべてを満たす場合のみ（誤統合は根拠を失わせるので最悪）:
///  1. 正規化タイトルの完全一致
///  2. 数字列の完全一致（版番号・CVE 番号の違いは別の記事）
///  3. Dice >= 0.95（語集合の類似）
///
/// 統合時はスコアの高い方を代表とし、engines / positions / merged_sources を寄せる。
fn distill_transfers(results: &mut Vec<SearchResult>, _query_terms: &[String]) {
    if results.len() < 2 {
        return;
    }
    let mut keep: Vec<SearchResult> = Vec::with_capacity(results.len());
    'outer: for mut candidate in results.drain(..) {
        let cand_norm = normalize_title_for_dedup(&candidate.title);
        // タイトルが空の結果同士は転載判定できない（空文字同士が全件一致してしまう）。
        // 統合せず、そのまま残す。
        if cand_norm.is_empty() {
            keep.push(candidate);
            continue 'outer;
        }
        let cand_nums = title_numbers(&candidate.title);
        for existing in keep.iter_mut() {
            // 転載は「別ドメインでの再掲」。同じホストの別ページ（例: サイト内の一覧と詳細）は
            // タイトルが似ていても別のページなので統合しない。
            if url_host(&existing.url) == url_host(&candidate.url) {
                continue;
            }
            let ex_norm = normalize_title_for_dedup(&existing.title);
            if ex_norm != cand_norm {
                continue;
            }
            if title_numbers(&existing.title) != cand_nums {
                continue; // 数字が違う = 別の記事（版番号・CVE 番号が違う）
            }
            if title_dice(&ex_norm, &cand_norm) < 0.95 {
                continue;
            }
            // 転載として統合する。代表（existing）に寄せる。
            let merged_label = format!("{} | {}", candidate.title, candidate.url);
            existing.merged_sources.push(merged_label);
            for engine in candidate.engines.drain(..) {
                if !existing.engines.contains(&engine) {
                    existing.engines.push(engine);
                }
            }
            existing.positions.append(&mut candidate.positions);
            if existing.score < candidate.score {
                // より強い結果を代表にする
                std::mem::swap(&mut existing.url, &mut candidate.url);
                std::mem::swap(&mut existing.title, &mut candidate.title);
                std::mem::swap(&mut existing.content, &mut candidate.content);
                std::mem::swap(&mut existing.score, &mut candidate.score);
                std::mem::swap(
                    &mut existing.relevance_score,
                    &mut candidate.relevance_score,
                );
            }
            continue 'outer;
        }
        keep.push(candidate);
    }
    *results = keep;
}

/// Search engines wrap outbound links in their own redirector
/// (\`google.com/goto?url=\`, \`duckduckgo.com/l/?uddg=\`, ...). Left alone, those
/// wrappers pollute the result set with links that are not the page at all.
///
/// Returns the real destination, \`None\` when the wrapper carries no usable
/// target (the result must be dropped), and the input unchanged otherwise.
pub fn unwrap_redirect(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let Ok(parsed) = url::Url::parse(trimmed) else {
        return Some(trimmed.to_string());
    };
    let host = parsed.host_str().unwrap_or_default().to_lowercase();
    let path = parsed.path().to_lowercase();

    // Which query parameter carries the destination, per wrapper.
    let wrapper = if (host.ends_with("google.") || host.contains(".google."))
        && (path.starts_with("/goto") || path.starts_with("/url"))
    {
        Some(&["url", "q", "continue"][..])
    } else if host.ends_with("duckduckgo.com") && path.starts_with("/l/") {
        Some(&["uddg"][..])
    } else if host.ends_with("bing.com") && path.starts_with("/ck/a") {
        Some(&["u", "url"][..])
    } else if host.ends_with("l.facebook.com") && path.starts_with("/l.php") {
        Some(&["u"][..])
    } else if (host.ends_with("link.zhihu.com") || host.ends_with("link.juejin.cn"))
        && path.starts_with("/")
    {
        Some(&["target"][..])
    } else {
        None
    };
    let Some(keys) = wrapper else {
        return Some(trimmed.to_string());
    };
    for (key, value) in parsed.query_pairs() {
        if keys.contains(&key.as_ref())
            && (value.starts_with("http://") || value.starts_with("https://"))
        {
            return Some(value.into_owned());
        }
    }
    // A wrapper without a target is a dead end: drop the result rather than
    // showing the redirector itself.
    None
}

/// The host of a URL, lowercased and without a port.
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map(|(_, rest)| rest)?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?.split(':').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_lowercase())
    }
}

/// Whether [host] is [domain] itself or one of its subdomains.
pub fn domain_matches(host: &str, domain: &str) -> bool {
    let domain = normalize_domain(domain);
    !domain.is_empty() && (host == domain || host.ends_with(&format!(".{domain}")))
}

pub(crate) fn normalize_domain(domain: &str) -> String {
    domain
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or(domain)
        .split(':')
        .next()
        .unwrap_or(domain)
        .trim_start_matches("www.")
        .to_lowercase()
}

/// Apply the optional agent-facing result filters.
pub fn filter_results(
    results: &mut Vec<SearchResult>,
    include_domains: &[String],
    exclude_domains: &[String],
    max_results: Option<usize>,
) {
    if !include_domains.is_empty() {
        results.retain(|result| {
            host_of(&result.url).is_some_and(|host| {
                include_domains
                    .iter()
                    .any(|domain| domain_matches(&host, domain))
            })
        });
    }
    if !exclude_domains.is_empty() {
        results.retain(|result| {
            host_of(&result.url).is_none_or(|host| {
                !exclude_domains
                    .iter()
                    .any(|domain| domain_matches(&host, domain))
            })
        });
    }
    if let Some(limit) = max_results {
        results.truncate(limit.max(1));
    }
}

/// The deduplication key of a URL: scheme independent, without fragment and
/// without the usual tracking parameters.
///
/// The key is assembled from the parsed URL structure, not by concatenating
/// decoded text, so values and separators stay distinguishable:
///
/// - the host keeps its port, except the https default (443). The key always
///   assumes https, so `http://e.test/p`, `https://e.test/p` and
///   `https://e.test:443/p` share one key, while `:8443` and `:9443` differ;
/// - query pairs are re-encoded as pairs, so `?a=b%26c%3Dd` (one value holding
///   `&c=d`) never collapses into `?a=b&c=d` (two parameters);
/// - query order is preserved, since servers may treat it as significant.
pub fn dedup_key(url: &str) -> String {
    let trimmed = url.trim();
    let Ok(mut parsed) = url::Url::parse(trimmed) else {
        return trimmed.to_lowercase();
    };
    parsed.set_fragment(None);
    let _ = parsed.set_scheme("https");
    let host = parsed.host_str().unwrap_or_default().to_lowercase();
    // `port()` keeps an explicit port unless it is the default for the current
    // scheme, and `set_scheme` does not renormalise, so compare explicitly.
    let authority = match parsed.port().filter(|port| *port != 443) {
        Some(port) => format!("{host}:{port}"),
        None => host,
    };
    let path = parsed.path().trim_end_matches('/');
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in parsed.query_pairs() {
        if is_tracking_param(&key) {
            continue;
        }
        serializer.append_pair(&key, &value);
    }
    let query = serializer.finish();
    format!(
        "{authority}{path}{}",
        if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        }
    )
}

/// Tracking parameters removed when deduplicating (a subset of the ClearURLs
/// rules used by the tracker remover plugin).
pub fn is_tracking_param(key: &str) -> bool {
    let key = key.to_lowercase();
    key.starts_with("utm_")
        || matches!(
            key.as_str(),
            "fbclid"
                | "gclid"
                | "dclid"
                | "msclkid"
                | "yclid"
                | "_openstat"
                | "igshid"
                | "mc_eid"
                | "mkt_tok"
                | "vero_id"
                | "wickedid"
                | "oly_anon_id"
                | "oly_enc_id"
                | "s_cid"
                | "spm"
                | "ref_src"
                | "ref_url"
                | "cmpid"
                | "campaign_id"
        )
}

/// A short human readable URL for the UI.
pub fn pretty_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) => {
            let host = parsed.host_str().unwrap_or_default();
            let path = parsed.path().trim_end_matches('/');
            if path.is_empty() || path == "/" {
                host.to_string()
            } else {
                format!("{host}{path}")
            }
        }
        Err(_) => url.to_string(),
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::engine::EngineItem;

    fn result(url: &str, title: &str) -> EngineItem {
        EngineItem::Result(SearchResult::new(url, title))
    }

    #[test]
    fn duplicate_urls_are_merged() {
        let mut container = ResultContainer::new();
        container.extend("a", 1.0, vec![result("https://example.com/x", "one")]);
        container.extend("b", 1.0, vec![result("http://example.com/x#anchor", "one")]);
        assert_eq!(container.main_results().len(), 1);
        assert_eq!(container.main_results()[0].engines.len(), 2);
    }

    #[test]
    fn executable_result_urls_are_rejected() {
        for url in [
            "javascript:void(0)",
            "JaVaScRiPt:void(0)",
            "\tjava\nscript:void(0)",
            "data:text/html,example",
            "vbscript:example",
            "https://",
            "",
        ] {
            for template in [Template::Default, Template::KeyValue] {
                let mut container = ResultContainer::new();
                let item = SearchResult::new(url, "unsafe").with_template(template);
                container.extend("a", 1.0, vec![EngineItem::Result(item)]);
                assert!(container.main_results().is_empty(), "{url:?}");
            }
        }
    }

    #[test]
    fn web_relative_and_keyvalue_results_are_preserved() {
        for url in [
            "https://example.com/a",
            "http://example.com/b",
            "/docs/a.pdf",
            "../docs/b.pdf",
            "//example.com/c",
            "?page=2",
            "#section",
        ] {
            let mut container = ResultContainer::new();
            container.extend("a", 1.0, vec![result(url, "safe")]);
            assert_eq!(container.main_results()[0].url, url);
        }

        let mut container = ResultContainer::new();
        let row = SearchResult::new("keyvalue:0", "database row")
            .with_template(Template::KeyValue)
            .with_metadata("magnetlink", "magnet:?xt=urn:btih:example");
        container.extend("db", 1.0, vec![EngineItem::Result(row)]);
        assert_eq!(container.main_results()[0].url, "keyvalue:0");
        assert_eq!(
            container.main_results()[0].metadata["magnetlink"],
            "magnet:?xt=urn:btih:example"
        );
    }

    #[test]
    fn merged_images_and_infobox_links_reject_executable_urls() {
        let mut container = ResultContainer::new();
        container.extend("a", 1.0, vec![result("https://example.com/a", "safe")]);
        let mut duplicate = SearchResult::new("https://example.com/a", "safe");
        duplicate.img_src = Some("JaVaScRiPt:void(0)".into());
        duplicate.thumbnail = Some("data:text/html,example".into());
        let infobox = Infobox {
            id: "box".into(),
            title: "reference".into(),
            ..Default::default()
        };
        container.extend("a", 1.0, vec![EngineItem::Infobox(infobox.clone())]);
        container.extend(
            "b",
            1.0,
            vec![
                EngineItem::Result(duplicate),
                EngineItem::Infobox(Infobox {
                    img_src: Some("java\tscript:void(0)".into()),
                    url: Some("javascript:void(0)".into()),
                    urls: vec![
                        InfoboxUrl {
                            title: "unsafe".into(),
                            url: "JaVaScRiPt:void(0)".into(),
                        },
                        InfoboxUrl {
                            title: "safe".into(),
                            url: "https://example.com/reference".into(),
                        },
                    ],
                    ..infobox
                }),
            ],
        );
        assert!(container.main_results()[0].img_src.is_none());
        assert!(container.main_results()[0].thumbnail.is_none());
        assert!(container.infoboxes[0].img_src.is_none());
        assert!(container.infoboxes[0].url.is_none());
        assert_eq!(container.infoboxes[0].urls.len(), 1);
        assert_eq!(
            container.infoboxes[0].urls[0].url,
            "https://example.com/reference"
        );

        let mut safe = SearchResult::new("https://example.com/a", "safe");
        safe.img_src = Some("https://example.com/image.png".into());
        safe.thumbnail = Some("/thumb.png".into());
        container.extend(
            "c",
            1.0,
            vec![
                EngineItem::Result(safe),
                EngineItem::Infobox(Infobox {
                    id: "box".into(),
                    img_src: Some("https://example.com/image.png".into()),
                    url: Some("/reference".into()),
                    ..Default::default()
                }),
            ],
        );
        assert_eq!(
            container.main_results()[0].img_src.as_deref(),
            Some("https://example.com/image.png")
        );
        assert_eq!(
            container.main_results()[0].thumbnail.as_deref(),
            Some("/thumb.png")
        );
        assert_eq!(
            container.infoboxes[0].img_src.as_deref(),
            Some("https://example.com/image.png")
        );
        assert_eq!(container.infoboxes[0].url.as_deref(), Some("/reference"));
    }

    #[test]
    fn tracking_parameters_do_not_prevent_merging() {
        let mut container = ResultContainer::new();
        container.extend("a", 1.0, vec![result("https://example.com/x?a=1", "one")]);
        container.extend(
            "b",
            1.0,
            vec![result("https://example.com/x?a=1&utm_source=news", "one")],
        );
        assert_eq!(container.main_results().len(), 1);
    }

    #[test]
    fn higher_ranked_positions_score_higher() {
        let mut container = ResultContainer::new();
        container.extend(
            "a",
            1.0,
            vec![
                result("https://example.com/first", "first"),
                result("https://example.com/second", "second"),
            ],
        );
        container.close(|_| 1.0, &[], 0.0, "", 0.0);
        let ordered = container.ordered();
        assert_eq!(ordered[0].url, "https://example.com/first");
        assert!(ordered[0].score > ordered[1].score);
    }

    #[test]
    fn engine_weight_multiplies_the_score() {
        let mut container = ResultContainer::new();
        container.extend("a", 0.5, vec![result("https://example.com/a", "a")]);
        container.extend("b", 2.0, vec![result("https://example.com/b", "b")]);
        container.close(
            |engine| if engine == "a" { 0.5 } else { 2.0 },
            &[],
            0.0,
            "",
            0.0,
        );
        // `ordered` is the ranking the UI shows: engine weight wins.
        assert_eq!(container.ordered()[0].url, "https://example.com/b");
    }

    #[test]
    fn weights_supplied_with_results_override_registry_defaults() {
        let mut container = ResultContainer::new();
        container.extend(
            "routed",
            2.0,
            vec![result("https://example.com/routed", "routed")],
        );
        container.extend(
            "official site",
            0.5,
            vec![result("https://example.com/official", "official")],
        );
        container.close(|_| 1.0, &[], 0.0, "", 0.0);
        assert_eq!(container.ordered()[0].score, 2.0);
        assert_eq!(container.ordered()[1].score, 0.5);
    }

    #[test]
    fn per_item_weights_reach_the_merged_score() {
        let mut container = ResultContainer::new();
        container.extend_weighted(
            "local_index",
            vec![
                (1.5, result("https://example.com/first", "first")),
                (1.2, result("https://example.com/second", "second")),
            ],
        );
        container.close(|_| 1.0, &[], 0.0, "", 0.0);
        let ordered = container.ordered();
        assert_eq!(ordered[0].url, "https://example.com/first");
        assert_eq!(ordered[0].score, 1.5);
        assert_eq!(ordered[1].score, 0.6);
    }

    #[test]
    fn repeated_urls_from_one_source_cast_one_vote() {
        let mut container = ResultContainer::new();
        container.extend(
            "a",
            1.0,
            vec![
                result("https://example.com/page", "page"),
                result("http://example.com/page?utm_source=a", "page again"),
            ],
        );
        container.extend_from_page(
            "a",
            1.0,
            vec![result("https://example.com/page", "page again")],
            1,
        );
        container.close(|_| 1.0, &[], 0.0, "", 0.0);
        let result = &container.ordered()[0];
        assert_eq!(result.engines, ["a"]);
        assert_eq!(result.positions, [1]);
        assert_eq!(result.score, 1.0);
    }

    #[test]
    fn independent_sources_still_contribute_their_best_positions() {
        let mut container = ResultContainer::new();
        container.extend_from_page(
            "a",
            1.0,
            vec![result("https://example.com/page", "page")],
            1,
        );
        container.extend("a", 1.0, vec![result("https://example.com/page", "page")]);
        container.extend("b", 1.0, vec![result("https://example.com/page", "page")]);
        container.close(|_| 1.0, &[], 0.0, "", 0.0);
        let result = &container.ordered()[0];
        assert_eq!(result.engines, ["a", "b"]);
        assert_eq!(result.positions, [1, 1]);
        assert_eq!(result.deep_page, 0);
        assert_eq!(result.score, 4.0);
    }

    #[test]
    fn query_specific_weights_do_not_leak_between_results_of_one_source() {
        for reverse in [false, true] {
            let mut container = ResultContainer::new();
            let mut passes = vec![
                (
                    1.0,
                    vec![
                        result("https://example.com/plain", "plain"),
                        result("https://example.com/shared", "shared"),
                    ],
                ),
                (
                    2.0,
                    vec![
                        result("https://example.com/shared", "shared"),
                        result("https://example.com/routed", "routed"),
                    ],
                ),
            ];
            if reverse {
                passes.reverse();
            }
            for (weight, items) in passes {
                container.extend("source", weight, items);
            }
            container.close(|_| 1.0, &[], 0.0, "", 0.0);
            let scores: std::collections::BTreeMap<_, _> = container
                .ordered()
                .iter()
                .map(|result| (result.url.as_str(), result.score))
                .collect();
            assert_eq!(scores["https://example.com/plain"], 1.0);
            assert_eq!(scores["https://example.com/shared"], 2.0);
            assert_eq!(scores["https://example.com/routed"], 1.0);
        }
    }

    #[test]
    fn suggestions_answers_and_corrections_are_collected() {
        let mut container = ResultContainer::new();
        container.extend(
            "a",
            1.0,
            vec![
                EngineItem::Suggestion("did you mean rust".into()),
                EngineItem::Answer("42".into()),
                EngineItem::Correction("corrected".into()),
                result("https://example.com/x", "x"),
            ],
        );
        assert!(container.suggestions.contains("did you mean rust"));
        assert_eq!(container.answers, vec!["42".to_string()]);
        assert!(container.corrections.contains("corrected"));
    }

    #[test]
    fn results_of_the_same_category_stay_close_together() {
        let mut container = ResultContainer::new();
        let mut items = Vec::new();
        for index in 0..10 {
            let mut image = SearchResult::new(format!("https://img.example.com/{index}"), "img");
            image.category = "images".into();
            image.template = Template::Images;
            items.push(EngineItem::Result(image));
            let mut web = SearchResult::new(format!("https://web.example.com/{index}"), "web");
            web.category = "general".into();
            web.score = 100.0 - index as f64;
            items.push(EngineItem::Result(web));
        }
        container.extend("a", 1.0, items);
        container.close(|_| 1.0, &[], 0.0, "", 0.0);
        let ordered = container.ordered();
        // Within the first 8 entries of the images group every second entry is an
        // image result (upstream groups results of one category).
        assert!(ordered[..8].iter().all(|r| r.url.contains("example.com")));
    }

    #[test]
    fn dedup_key_ignores_scheme_fragment_and_tracking() {
        assert_eq!(
            dedup_key("http://Example.com/a/?utm_source=x#frag"),
            dedup_key("https://example.com/a")
        );
    }

    #[test]
    fn query_delimiters_inside_a_value_do_not_collide_with_parameter_boundaries() {
        // `b%26c%3Dd` is one value that contains "&c=d", not two parameters.
        assert_eq!(
            dedup_key("https://e.test/p?a=b%26c%3Dd"),
            "e.test/p?a=b%26c%3Dd"
        );
        assert_eq!(dedup_key("https://e.test/p?a=b&c=d"), "e.test/p?a=b&c=d");
        assert_ne!(
            dedup_key("https://e.test/p?a=b%26c%3Dd"),
            dedup_key("https://e.test/p?a=b&c=d")
        );
    }

    #[test]
    fn the_port_is_part_of_the_identity() {
        assert_eq!(dedup_key("https://e.test:8443/p"), "e.test:8443/p");
        assert_eq!(dedup_key("https://e.test:9443/p"), "e.test:9443/p");
        assert_ne!(
            dedup_key("https://e.test:8443/p"),
            dedup_key("https://e.test:9443/p")
        );
    }

    #[test]
    fn the_https_default_port_is_elided() {
        // Spec: the key is scheme independent and always assumes https, so the
        // explicit default port and the absent port produce the same key.
        assert_eq!(dedup_key("https://e.test:443/p"), "e.test/p");
        assert_eq!(dedup_key("http://e.test/p"), "e.test/p");
        assert_eq!(dedup_key("https://e.test:8443/p"), "e.test:8443/p");
    }

    #[test]
    fn query_parameter_order_is_significant() {
        assert_eq!(dedup_key("https://e.test/p?a=1&b=2"), "e.test/p?a=1&b=2");
        assert_eq!(dedup_key("https://e.test/p?b=2&a=1"), "e.test/p?b=2&a=1");
        assert_ne!(
            dedup_key("https://e.test/p?a=1&b=2"),
            dedup_key("https://e.test/p?b=2&a=1")
        );
    }

    #[test]
    fn duplicate_results_keep_the_returned_https_url_in_either_order() {
        for urls in [
            ["http://github.com/trending", "https://github.com/trending"],
            ["https://github.com/trending", "http://github.com/trending"],
        ] {
            let mut container = ResultContainer::new();
            container.extend("a", 1.0, vec![result(urls[0], "trending")]);
            container.extend("b", 1.0, vec![result(urls[1], "trending")]);
            assert_eq!(container.main_results().len(), 1);
            assert_eq!(
                container.main_results()[0].url,
                "https://github.com/trending"
            );
        }
    }

    #[test]
    fn redirect_wrappers_are_unwrapped_or_dropped() {
        assert_eq!(
            unwrap_redirect("https://www.google.com/goto?url=https%3A%2F%2Ftokio.rs%2F"),
            Some("https://tokio.rs/".to_string())
        );
        assert_eq!(
            unwrap_redirect("https://duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa&rut=x"),
            Some("https://example.com/a".to_string())
        );
        // a wrapper with no destination is dropped
        assert_eq!(unwrap_redirect("https://www.google.com/goto?url="), None);
        // an ordinary URL is untouched
        assert_eq!(
            unwrap_redirect("https://tokio.rs/tokio/tutorial"),
            Some("https://tokio.rs/tokio/tutorial".to_string())
        );
    }

    #[test]
    fn pretty_url_strips_the_scheme() {
        assert_eq!(pretty_url("https://example.com/a/b/"), "example.com/a/b");
        assert_eq!(pretty_url("https://example.com/"), "example.com");
    }

    fn sample(url: &str) -> SearchResult {
        SearchResult {
            url: url.to_string(),
            title: url.to_string(),
            ..SearchResult::default()
        }
    }

    #[test]
    fn host_of_handles_port_userinfo_and_case() {
        assert_eq!(
            host_of("https://WWW.Example.com:8443/a"),
            Some("www.example.com".into())
        );
        assert_eq!(
            host_of("https://user:pw@example.com/a"),
            Some("example.com".into())
        );
        assert_eq!(host_of("not a url"), None);
    }

    #[test]
    fn domain_matches_subdomains_but_not_lookalikes() {
        assert!(domain_matches("example.com", "example.com"));
        assert!(domain_matches("docs.example.com", "example.com"));
        assert!(domain_matches(
            "docs.example.com",
            "https://www.example.com/path"
        ));
        assert!(!domain_matches("notexample.com", "example.com"));
        assert!(!domain_matches("example.com.evil.test", "example.com"));
    }

    #[test]
    fn filter_results_applies_include_exclude_and_limit() {
        let mut results = vec![
            sample("https://keep.test/a"),
            sample("https://docs.keep.test/b"),
            sample("https://drop.test/c"),
            sample("https://other.test/d"),
        ];
        filter_results(&mut results, &["keep.test".into()], &[], None);
        assert_eq!(results.len(), 2);
        filter_results(&mut results, &[], &["docs.keep.test".into()], None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://keep.test/a");
        filter_results(&mut results, &[], &[], Some(0));
        assert_eq!(results.len(), 1, "a limit below one keeps one result");
    }

    #[test]
    fn raw_content_is_serialised_only_when_present() {
        let mut result = sample("https://example.com/");
        let plain = serde_json::to_value(&result).expect("json");
        assert!(plain.get("raw_content").is_none());
        result.raw_content = Some("page text".into());
        let rich = serde_json::to_value(&result).expect("json");
        assert_eq!(rich["raw_content"], "page text");
    }
    // -------------------------------------------------------------------------
    // 転載統合（distillation）: 上流超えの蒸留
    // -------------------------------------------------------------------------

    fn sr(title: &str, url: &str, engine: &str) -> SearchResult {
        SearchResult {
            url: url.to_string(),
            title: title.to_string(),
            content: String::new(),
            engine: engine.to_string(),
            engines: vec![engine.to_string()],
            positions: vec![1],
            score: 1.0,
            ..SearchResult::default()
        }
    }

    #[test]
    fn transfer_of_the_same_article_is_merged() {
        // 同じ記事の転載（全角半角・空白の揺れ）は 1 件に統合する
        let mut results = vec![
            sr("PHP の脆弱性対策について", "https://jvn.jp/a", "jvn_vu"),
            sr(
                "PHPの脆弱性対策について",
                "https://mirror.example/b",
                "bing_web",
            ),
        ];
        distill_transfers(&mut results, &["php".to_string()]);
        assert_eq!(results.len(), 1, "転載を統合する");
        assert_eq!(results[0].merged_sources.len(), 1, "統合元を記録する");
        assert!(results[0].engines.contains(&"jvn_vu".to_string()));
        assert!(results[0].engines.contains(&"bing_web".to_string()));
    }

    #[test]
    fn different_version_numbers_are_kept_separate() {
        // 数字（版番号）が違う記事は別の記事。統合してはいけない（根拠を失わせる）
        let mut results = vec![
            sr("PHP 8.0.33 の更新", "https://a.example/1", "jvn_vu"),
            sr("PHP 8.0.31 の更新", "https://b.example/2", "bing_web"),
        ];
        distill_transfers(&mut results, &["php".to_string()]);
        assert_eq!(results.len(), 2, "版番号が違えば統合しない");
    }

    #[test]
    fn unrelated_articles_are_kept_separate() {
        let mut results = vec![
            sr("Apache の脆弱性", "https://a.example/1", "jvn_vu"),
            sr("nginx の設定ミス", "https://b.example/2", "bing_web"),
        ];
        distill_transfers(&mut results, &[]);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn normalize_title_for_dedup_folds_case_and_punctuation() {
        assert_eq!(
            normalize_title_for_dedup("PHP の脆弱性対策について！"),
            normalize_title_for_dedup("phpの脆弱性対策について"),
        );
    }

    #[test]
    fn title_numbers_detects_version_differences() {
        assert_eq!(
            title_numbers("PHP 8.0.33"),
            vec!["8".to_string(), "0".to_string(), "33".to_string()]
        );
        assert_ne!(title_numbers("PHP 8.0.33"), title_numbers("PHP 8.0.31"));
    }
}
