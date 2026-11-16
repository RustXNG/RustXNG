# RISKS — 未解決と追跡項目（2026-11-16）

重大度: P0（即時）/ P1（次リリース前）/ P2（計画対応）/ P3（任意）。

| ID | 重大度 | 内容 | 根拠 | 次の一手 |
| --- | --- | --- | --- | --- |
| R1 | ~~P1~~ 解消 | `/research` の全体期限と同時実行制限（旧指摘） | 修正済み: `research.deadline`（既定120s、超過は 504）と `research.max_concurrency`（既定2、超過は 429）。src/web/handlers.rs, src/settings.rs, tests/integration.rs | limiter 既定 off のままでも増幅は有界 |
| R2 | ~~P2~~ 解消 | 画像/favicon の並列上限と limiter（旧指摘） | 修正済み: `Network.media_slots`（Semaphore=4、期限付き取得）と両ハンドラの limiter。src/network.rs, src/web/image_proxy.rs, src/web/favicons.rs, tests/integration.rs | 残: 実測の負荷試験は NOT_TESTED |
| R3 | P2 | エンジン側の pageno 演算は checked ではない | src/engines/*.rs | 入口上限 1000 を維持。`every_configured_engine_builds_requests_within_the_page_limit` が全エンジン × ページ 1/2/1000 を検証（99 リクエスト構築、open_meteo は設定不足で deferred、ページ固有の失敗 0）。max_page を上げる場合は checked 化が必要 |
| R4 | ~~P3~~ 解消 | `check_bench.py` の CI 未接続と CHANGELOG の乖離 | 修正済み: docs/BENCHMARK.md に現行 CI 範囲を明記し、tools/test_check_bench.py で下限・失敗件数の挙動を固定。CHANGELOG は履歴として変更しない | ライブ nightly の再開は外部判断 |
| R5 | P3 | 外部ネットワーク・実 DB・実ブラウザー依存の検証は CI の一部と ignored テストのみ（4 件） | tests/mysql.rs, tests/rendered_pages.rs, src/mcp/benchmark.rs | 定期実行の維持。結果を docs に残す |
| R6 | P3 | DNS rebinding / 実エンジン応答の再現はコード検証のみで実測していない | src/fetch/direct.rs, src/web/ssrf_tests.rs | ローカル DNS を使った再現手順を追加検討 |
| R7 | P3 | 検索品質（Hit@5 64–69）は旧構成（84）より低い。プロバイダー除外の帰結で、コードでは解決しない | docs/PERFORMANCE-2026-11-05.md, docs/ISSUE-RESOLUTION-2026-11-06.md | 許諾済み広域検索 API の確保、または索引拡充（外部判断） |
| R9 | P1 | `src/i18n.rs`（2,985 行）と `src/wikidata_infobox.rs`（670 行 + 33k 行 JSON）が未参照のデッドコード。削除か配線かの判断が必要 | 計測: `crate::i18n`/`wikidata_infobox` の参照は lib.rs の宣言のみ。バイナリにも翻訳表と JSON が載る | オーナー判断（削除なら約 3,650 行 + データ削減） |
| R10 | P2 | graceful shutdown の drain に期限が無い。`/research`（既定 120s）実行中の SIGTERM は systemd/docker の猶予で SIGKILL され得る | main.rs の serve は drain を無期限に待つ（実測: 遅い POST 保持中 8 秒超生存） | `shutdown_grace`（既定 10s）を追加し、期限後は exit |
| R11 | P2 | engine クライアントの DNS 例外は `base_url` ホストのみで、`search_url` だけを持つ自己ホスト engine は例外にならない | `configured_engine_hosts` が base_url のみ参照（search_url 使用 engine は 18 ファイル） | 例外に search_url のホストも含めるか、仕様として明記 |
| R12 | P2 | 多ソース融合評価の fixture が商用 API ダンプ（gitignore）依存で、クリーンクローンでは n=13 しか再現できない | `.gitignore` に対象 JSON、`bench/local-retrieval/fusion-lab/README.md` に注記 | URL+順位のみの派生 fixture を追跡下に作る（n≥100） |
| R13 | P3 | 既存の融合層は「単一プロバイダ単独」に有意に勝てない（ablation: position-only 0.768 < solo:exa 0.835、p=0.0015） | fusion-lab の実測（ローカル限定 fixture） | 主張を「融合層」と「後段の lexical/authority」に分けて記録 |
| R8 | P3 | プロバイダー条件の確認は「記録」であり法的適合の保証ではない | docs/PROVIDERS.md, docs/provider-policy.json | 利用者向けの注意書きを維持 |
