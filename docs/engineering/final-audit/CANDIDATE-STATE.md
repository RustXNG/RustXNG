# CANDIDATE-STATE — 独立監査に渡す候補の状態

記録日: 2026-11-17。この文書は監査対象を一意に指すためのスナップショットである。

## 識別

| 項目 | 値 |
| --- | --- |
| リポジトリ | https://github.com/sahenjp/RustXNG |
| 基点コミット（baseline） | `f7639cede00634a48db3261709bc359752f109eb`（v0.31.2、main） |
| 候補（candidate） | 基点コミット上の**未コミット作業ツリー**（candidate のコミット SHA は存在しない） |
| ブランチ | `main`（`origin/main` と一致） |
| 追跡ファイルの変更 | 41 files changed, 2153 insertions(+), 263 deletions(-) |
| 未追跡の追加 | 21 パス（下記） |

## 追加した未追跡パス

```text
deny.toml
docs/DEPENDENCIES.md
docs/OBSERVABILITY.md
docs/README.md
docs/engineering/
docs/openapi.yaml
src/diagnostics.rs
static/favicon.ico
static/logo-wordmark-light.png
static/logo-wordmark.png
static/logo.png
tests/cli.rs
tests/extraction.rs
tests/fixtures/extraction/
tools/eval_local_index.py
tools/mcp_smoke.py
tools/test_check_bench.py
tools/test_doc_links.py
tools/test_docs_metrics.py
tools/test_eval_local_index.py
tools/test_openapi.py
```

## 検証の要約（2026-11-17 時点）

| 検証 | 結果 |
| --- | --- |
| `cargo fmt --all -- --check` | 成功 |
| `cargo clippy --locked --all-targets -- -D warnings` | 成功 |
| `cargo test --locked --all-targets` | 1178 passed / 4 ignored / 0 failed |
| `cargo check --locked --all-targets --no-default-features` | 成功 |
| `python3 -m unittest discover -s tools` | 86 tests OK |
| `cargo deny --locked check` | licenses / bans / sources / advisories すべて ok |
| `cargo audit` | 0 vulnerabilities / 0 warnings |
| markdownlint（CI と同じ glob） | 0 issues |
| smoke（`tools/smoke_agent_setup.py`） | 3/3 PASS（`/readyz` と MCP stdio を含む） |
| コンテナ（podman） | ビルド 153 MB、非 root、`/readyz`、`--check-config`、SIGTERM で 0 |

詳細と再現手順: [TEST-RESULTS.md](TEST-RESULTS.md)、[REPRODUCE.md](REPRODUCE.md)、
[../PERFORMANCE.md](../PERFORMANCE.md)。

## 未検証（意図的に委譲）

- 外部 MCP クライアント（Codex / Claude Code / OpenCode）の実機接続
- 実エンジン応答・実 Valkey・実 MySQL/MariaDB・実ブラウザー（Firefox レンダラー）・実 LLM 課金
- Windows / macOS 実機
- `rust:1.99.1` から MSRV 1.89 でのビルド（CI の msrv ジョブに委譲）

## 監査担当への注意

- 変更は**未コミット**である。監査は作業ツリーを対象にするか、[RELEASE-PLAN.md](../RELEASE-PLAN.md) の
  単位でコミットしたうえで SHA を記録する。
- 本資料は実装者自身の記録であり、独立監査の代替ではない。