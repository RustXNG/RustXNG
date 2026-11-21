# fuzz — 信頼できないバイト列を食べるパーサーの harness

外部から届くバイト列を解析する関数だけを対象にした libFuzzer の harness 集。
各 target は実関数を 1 行で呼ぶだけにし、解析ロジックをここへ複製しない。

## 対象

| target | 実関数 | 入力の出どころ |
| --- | --- | --- |
| `content_extract` | `src/content.rs` `main_text_limited` | 生ページ本文（`include_raw_content` / MCP `read_page`） |
| `resp_reply` | `src/engines/valkey_server.rs` `Connection::read_reply` | 設定済みキー・バリューストアの応答 |
| `xpath_subset` | `src/engine/xpath.rs` `XPath::parse` | エンジン定義と事業者設定の式 |
| `robots_txt` | `src/robots.rs` `Robots::from_response` | 取得先サイトの robots.txt |
| `sitemap` | `src/crawl.rs` `sitemap_locations` | 取得先サイトの sitemap |
| `proxy_signature` | `src/web/image_proxy.rs` `is_valid` / `is_public_url` | `/image_proxy` の `url` と `h` |

`resp_reply` だけは形が違う。`Connection` は非公開でソケットからしか読めないため、
ループバックに応答サーバーを 1 つ立て、fuzz 入力をそのまま応答として返し、実エンジンの
`search_offline` を通す。解析対象は fuzz 入力そのもので、パーサーのコピーは持たない。

## ビルド

```sh
cargo build --manifest-path fuzz/Cargo.toml
```

確認済み: rustc 1.100.0 / cargo 1.100.0（stable）で 6 target すべてビルドできる。
`fuzz/Cargo.toml` は `[workspace]` を持つ独立クレートなので、リポジトリ直下の
`cargo build` / `cargo test` / `cargo clippy` はこれらを巻き込まない。
`db-engines` feature だけは有効にしてある（RESP のパーサーがその feature 配下にあるため）。

## 実行

```sh
cargo install cargo-fuzz          # 初回だけ
cargo +nightly fuzz run sitemap   # target 名を指定
```

coverage 誘導の fuzzing には cargo-fuzz（nightly の SanitizerCoverage）が要る。
上の `cargo build` だけでは instrument されないので、その実バイナリを直接叩くと
「no interesting inputs were found」と出るのが正常。ビルド確認と、既知の再現には十分。

入力長は 4 KiB 程度で足りる（`-max_len=4096`）。長時間回すときは
`-rss_limit_mb=2048` を明示すると、巨大確保を OOM として検出できる。

## 既知の検出（2026-11-21）

`resp_reply` は、応答が告知したバルク長をそのまま確保する箇所を検出する。

```
$ printf '$999999999999\r\n' > /tmp/oom/seed
$ ./target/debug/resp_reply -runs=1 /tmp/oom
...
memory allocation of 999999999999 bytes failed
SUMMARY: libFuzzer: deadly signal
```

`src/engines/valkey_server.rs:357` の `read_reply` は
`vec![0u8; length as usize + 2]` を読む前に確保する。同じ形が `src/cache.rs:682` の
`read_reply_inner` にもある。相手は事業者が設定したストアなので優先度は高くないが、
ストア側の不具合や侵害でプロセスが落ちる。修正は「告知長を上限（例: 8 MiB）で
clamp し、超えたらエラーにする」の 2 箇所。どちらもこの変更の対象ファイル外なので、
harness はこの検出を隠さずに残してある。

## コーパス

```
fuzz/corpus/<target>/    # 実行時に育つ。コミットしない（.gitignore 済み）
fuzz/artifacts/          # クラッシュの再現入力。再現したら中身を確認して削除する
```

最小の種を 1 つ置いてから回すと、初回から意味のある経路に入る。種は各 target の
入力形式そのもの（例: `sitemap` は `<urlset>...`、`proxy_signature` は
`url\n署名候補`）。
