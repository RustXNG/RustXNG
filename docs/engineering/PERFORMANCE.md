# PERFORMANCE — 測定（2026-11-16, 21:33 更新）

推測ではなく実測のみを書く。測定条件を必ず併記する。

## 環境

| 項目 | 値 |
| --- | --- |
| CPU / RAM | 16 コア / 31 GiB |
| OS | Linux x86_64 |
| Rust | 1.100.0（release: lto=thin, codegen-units=1, strip=debuginfo） |
| コミット | f7639ce + 本セッションの作業ツリー差分（未コミット） |
| プロファイル | clean（外部検索なし、ローカル索引のみ） |
| 索引 | `index-docs` と `index-wikipedia`（同梱の既存索引） |
| キャッシュ | プロセス起動ごとに空（メモリキャッシュ） |

## 起動・メモリ・サイズ（最終ビルド）

| 指標 | 実測 | 方法 |
| --- | ---: | --- |
| 起動→`/healthz` 200 | 0.206 s（最終ビルド）/ 0.054–0.205 s（初回測定） | 50ms 間隔のポーリング |
| アイドル RSS（起動 5 秒後） | 23.8 MiB | `ps -o rss=` |
| ピーク RSS（100 問実行後） | 706,916 kB ≈ 690 MiB | `/proc/PID/status` の VmHWM |
| release バイナリ | 35,492,584 bytes | 変更前ビルド 35,456,640 bytes（+0.10%） |

ピーク RSS は Tantivy 索引の mmap を含む値で、プロセス自身のヒープだけを示さない。起動時間の
ばらつき（0.05–0.21 s）は索引のページキャッシュ状態に依存すると考えられるが、分離測定はしていない。

## オフライン検索（clean・100 問）

`bench/quality-questions-100.json` の 100 問を `/search?format=json&max_results=10` で逐次実行。

| 指標 | 実測 |
| --- | ---: |
| 成功したクエリ | 100 / 100 |
| 返却結果の総数 | 903 |
| Hit@10（gold URL の前方一致） | 36 / 100 |
| レイテンシ p50 | 15.6 ms |
| p95 | 240.3 ms |
| p99 | 358.8 ms |
| 最大 | 379.1 ms |
| 平均 | 59.9 ms |

前回測定（同条件、同日 21:07 のビルド）は p50 15.7 / p95 250.8 / p99 257.8 ms、Hit@10 36/100。
差は同一マシンでの再実行のばらつきの範囲で、変更の効果として解釈しない。

## 再現手順

```bash
cargo build --release --locked --bin rustxng
target/release/rustxng --profile clean --bind 127.0.0.1 --port 29558 &
python3 tools/eval_quality.py --base http://127.0.0.1:29558 \
  --questions bench/quality-questions-100.json --delay 0 --json /tmp/clean100.json
```

起動・RSS の採取は `/tmp/rxng/measure100.py`（本セッションの使い捨てスクリプト、リポジトリ外）で行った。
恒久的な評価は `tools/eval_quality.py` と `bench/results/` を使う。

## 本文抽出のコーパス（2026-11-17）

`tests/fixtures/extraction/` に 5 ページ（英語ドキュメント・数式・日本語・RFC 風・ニュース）と `manifest.json` を
追加し、`cargo test --locked --test extraction` で検証する。ネットワーク不要。

| fixture | 抽出文字数 | 句読点前の余白 |
| --- | ---: | ---: |
| docs-page.html（表・コード・脚注） | 462 | 0 |
| math-page.html（KaTeX / MathML） | 121 | 0 |
| ja-docs.html（日本語・dl・表） | 210 | 0 |
| rfc-page.html（ヘッダー表） | 235 | 0 |
| news-page.html（nav/footer/hidden/aria） | 133 | 0 |

manifest は「残す文字列（表の行・コード行・脚注本文など）」「残してはいけない文字列（nav・footer・hidden・aria-hidden）」
「最小文字数」を宣言し、テストが全 fixture を網羅していることも検査する。この 5 ページは合成 fixture であり、
実サイトの分布を代表しない。実ページ 2 件の A/B は次の節に記録する。

## 本文抽出の変化（Phase 4.5、2026-11-16）

2 つの公開ページを 1 回ずつ取得し、変更前（HEAD の `src/content.rs`）と変更後の抽出結果を比較した。
どちらも `main_text_limited`（上限 1,000,000 文字）で打ち切りなし。取得は 1 URL 1 回、UA を明示。

| ページ | 文字数（前→後） | 単語数（前→後） | 句読点前の余白（前→後） | 表の行（前→後） |
| --- | --- | --- | ---: | ---: |
| RFC 9309 (rfc-editor.org) | 23,187 → 22,449 | 3,263 → 3,042 | 149 → 15 | 45 → 47 |
| The Rust Book 4.1 (doc.rust-lang.org) | 25,300 → 25,251 | 4,430 → 4,381 | 50 → 1 | 0 → 0 |

- **確認できた改善**: `owner .` や `1 . Introduction` のような句読点前の余白が大幅に減った（−90% / −98%）。
  表は Markdown 行として出る（RFC のヘッダー表が `| RFC 9309 | Robots Exclusion Protocol (REP) | ... |` に）。
- **内容の欠落は検出しなかった**: Book の差分は 34 行削除・34 行追加で、全て句読点の空白差。RFC の差は
  表の再整形と見出しの空白で、本文（"Robots Exclusion Protocol"）は保持。
- **ナビゲーション除去の効果はこの 2 ページでは測れなかった**: どちらも本文コンテナ（`<main>` / 本文 div）が
  選ばれ、`<nav>` / `<footer>` が最初から対象外だったため。ナビ除去は `nav` / `footer` / `role` / `hidden` の
  構造シグナルだけを見る防御であり、ユニットテストで固定している（実ページでの削減量は NOT_MEASURED）。

- 限界: 2 ページのみ、キャッシュ・言語・サイト構成の偏りを考慮していない。抽出品質の一般化には
  コーパス（例: 50 ページ以上）と正解テキストの用意が必要で、これは今回の範囲外。

## ビルド構成の差（Phase 3.4）

`--no-default-features`（db-engines を外した slim 構成）と既定構成の release バイナリ。
両方とも同じマシン・同じツールチェーンで `cargo build --release --locked --bin rustxng`。

| 構成 | バイナリ | ビルド時間（実測） |
| --- | ---: | ---: |
| 既定（db-engines 有効） | 35,492,584 bytes | 約 2 分 |
| slim（`--no-default-features`） | 26,961,784 bytes | 1 分 43 秒（依存の再ビルドを含む） |

slim は **8,530,800 bytes（24.0%）小さい**。DB を検索元に使わない配備では slim を選ぶ根拠になる。
ビルド時間はキャッシュ状態に強く依存するため、この 2 値以上の比較はしていない。

## 比較に関する注意

- 本セッションの変更は、キャッシュ・エラー応答・入力上限・research 期限・メディア同時数・readyz が中心で、
  検索経路の性能を狙ったものではない。数値の差を「最適化の効果」と解釈しない。
- ライブ検索の品質（docs/PERFORMANCE-2026-11-05.md の Hit@5 64–69）とは条件が違い、比較できない。

## ローカル索引の評価（2026-11-16 追加）

`examples/local_quality`（index-docs, 41,144 文書）+ `bench/quality-questions-100.json` を
`tools/eval_local_index.py` で採点した基準値。外部プロバイダーへは問い合わせていない。

| 指標 | 基準（TITLE_BOOST=3.0） | 実験（TITLE_BOOST=4.0） | 差分 |
| --- | ---: | ---: | ---: |
| Hit@1 | 19/100 | 19/100 | 0 |
| Hit@5 | 24/100 | 24/100 | 0 |
| Hit@10 | 25/100 | 24/100 | −1 |
| MRR@10 | 0.2136 | 0.2125 | −0.0011 |
| nDCG@10 | 0.2226 | 0.2195 | −0.0031 |
| p50 | 1.08 ms | 0.97 ms | （ばらつき） |

**結論: TITLE_BOOST の変更は採用しない。** 効果が確認できないため既定値（3.0）を維持し、
`src/localindex.rs` は変更していない（実験後に復元済み）。この索引は文書サイト 41k ページのみで、
gold に Wikipedia・論文・公式サイトを含む 100 問のうち到達不能な問いが多く、スコア自体が索引範囲に
縛られる。ランキング定数の調整より、索引の収録範囲を広げる方が効く可能性が高い（外部判断）。

## 依存とビルドの実測（2026-11-17）

| 指標 | 実測 | 条件 |
| --- | --- | --- |
| 依存パッケージ数 | 442 | `cargo metadata --locked` |
| 直接依存（`[dependencies]`） | 34 | Cargo.toml |
| release ビルド（既定 features、依存キャッシュあり） | 1 分 59 秒（wall 119.65s / user 240.42s） | `cargo clean -p rustxng` 後 |
| release ビルド（`--no-default-features`） | 1 分 44 秒（wall 104.25s / user 200.02s） | 同上 |
| dev インクリメンタル（lib を touch） | 9.78 秒 | dev profile、既定 features |
| バイナリ（既定） | 35,805,032 bytes | release |
| バイナリ（slim） | 27,267,848 bytes | `--no-default-features`、−23.8% |
| 起動 → /healthz（standard, 34 engines） | 0.252 秒 | release、RSS 24.7 MiB |
| アイドル RSS | 24.7 MiB | 起動 5 秒後 |
| ピーク RSS（1 検索、外部到達不能） | 82,592 kB | `/proc/PID/status` VmHWM |

クリーンな `cargo build`（依存込み・`target` を空から）は時間とディスクの都合で実行していない（NOT_TESTED）。
上表は依存キャッシュが温まった状態の値で、初回ビルド時間ではない。

## デッドコード削除の前後（2026-11-17、コミット cc34686）

| 指標 | 削除前 | 削除後 |
| --- | ---: | ---: |
| `src/**/*.rs` 行数 | 86,859 | 83,202 |
| 同梱 JSON（wikidata） | 983,030 B | 0 B |
| release ビルド（依存キャッシュあり） | 108.97 s | 112.22 s（誤差） |
| バイナリ | 35,814,192 B | 35,813,744 B |
| 起動 → /healthz | 0.025 s（別回 0.186 s） | 0.186 s |
| アイドル RSS | 24.9 MiB | 24.5 MiB |

実行性能とバイナリサイズは実質不変（リンカが未参照セクションを既に除去）。利得はソースの見通しと保守性。

## feature 構成の比較

詳細は [LIGHTWEIGHT.md](../LIGHTWEIGHT.md)。既定 349 クレート / 35.8 MB、最小 286 / 27.3 MB、起動と RSS の差は誤差。