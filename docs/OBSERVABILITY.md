# 監視とメトリクス

`GET /metrics` は Prometheus / OpenMetrics 形式で、プロセス内のカウンタとゲージを返します。
`general.enable_metrics` が false のときは 404 を返します（既定は true）。
このエンドポイントは外部プロバイダーへ問い合わせません。

## 主な系列

| 系列 | 型 | 意味 |
| --- | --- | --- |
| `searxng_search_requests_total` | counter | 処理した検索リクエスト数 |
| `searxng_search_results_total` | counter | クライアントへ返した結果数 |
| `searxng_uptime_seconds` | gauge | プロセス稼働秒数 |
| `searxng_search_duration_seconds_sum` / `_count` | counter | 検索の所要時間の合計と件数。平均は sum / count で求める（分位点は未提供） |
| `searxng_ready` | gauge | 検索元（エンジンまたはローカル索引）が構成されていれば 1。`/readyz` と同じ判定 |
| `searxng_engines_configured` | gauge | 構成から構築されたエンジン数 |
| `searxng_local_index` | gauge | 構成されたローカル索引がディスク上に存在すれば 1 |
| `searxng_cache_hit_total` / `searxng_cache_miss_total` | counter | 応答キャッシュの命中・外れ |
| `searxng_cache_error_total` | counter | キャッシュのデコード・通信エラー |
| `searxng_cache_entries` / `searxng_cache_bytes` | gauge | キャッシュの件数と保持バイト数（Valkey では 0） |
| `searxng_limiter_requests_total{verdict="allowed\|challenged\|blocked"}` | counter | レート制限の判定別件数 |
| `searxng_limiter_tracked` | gauge | レート制限が追跡しているクライアント数 |
| `searxng_http_responses_total{status="2xx\|3xx\|4xx\|5xx"}` | counter | 返した HTTP 応答のクラス別件数。エラー率のアラート向け |
| `searxng_engines_*{engine="..."}` | counter / gauge | エンジン別の送信数・結果数・エラー数・応答時間・信頼度・休止状態 |

## 使い方の目安

- 生存確認は `/healthz`、構成の妥当性は `/readyz`（または `rustxng --check-config`）で行い、
  `searxng_ready` はアラート用の複製として使います。3 つは同じ判定を共有しています。
- `searxng_engines_error_count_total` の増加は上流の失敗、`searxng_engines_resting` は空応答が続いた休止を示します。
- 検索の遅さは `searxng_search_duration_seconds_sum / _count` の平均で追います。分位点（p95 など）は提供しないため、必要なら `/stats` のタイミング表示や保存済み測定を使ってください。
- `searxng_http_responses_total{status="5xx"}` の増加は自インスタンスの障害、`4xx` の増加はクライアント側の誤りかレート制限です。応答自体は `/metrics` の描画後に数えるため、最初の取得では自分自身がまだ含まれません。
- `/healthz` も `/readyz` も上流の可用性は保証しません。実際の検索可否は JSON 検索で確認してください。

## 停止

- `SIGTERM`（コンテナや systemd の停止）と Ctrl-C（`SIGINT`）で安全に停止します。
  受け付けたリクエストは完了させてからプロセスを終了するため、`docker stop` や `systemctl stop` で
  強制終了（終了コード 143）になりません。
- `server.idle_timeout`（または `RUSTXNG_IDLE_TIMEOUT`）が 0 でなければ、無通信が続いた時点でも停止します。

## 制約

- メトリクスはプロセス単位です。複数インスタンスを並べる場合は集約側で合算してください。
- `searxng_cache_bytes` はメモリキャッシュのみ意味を持ちます（Valkey バックエンドでは 0）。
- 系列名のうち `searxng_engines_*` は上流 SearXNG の名前を踏襲しています。
