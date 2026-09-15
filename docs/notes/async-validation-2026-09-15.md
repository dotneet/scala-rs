# scala-async 対応の検証（2026-09-15）

## 対象

変更前 HEAD: `55d7600f`。scala-async 1.0.1、Scala 2.13.16、JVM は Zulu 21.0.5。
実装・使い方・制限は [async/await ガイド](../async.md) を参照。

## 最終差分の検証

- `cargo fmt --all` / `git diff --check`: 合格。
- `cargo clippy --release -p scala-rs-cli`: 終了コード 0。既存警告あり、変更箇所の新規警告なし。
- `SCALA_ASYNC_JAR=/tmp/scala-rs-async/scala-async.jar tests/cli_test.sh xflags`:
  **12 passed / 0 failed**（5.61 秒）。
- `async_runtime.scala` と `async_values.scala` を双方のコンパイラでコンパイルし、
  `-Xfatal-warnings`、`java -Xverify:all` で出力を比較。
- フィールド初期化で mutable local が値として捕捉され、更新前の値を返す不具合を
  追加プローブで発見した。継続に移動した宣言の owner を更新し、各継続に固有の
  関数 owner を割り当てて修正。クラス・オブジェクトのフィールド初期化を
  `async_values.scala` に追加し、上記の最終テストで再確認した。

## 統合ゲートと既存の失敗

`tests/verify_merge.sh` は `VERDICT=FAIL` / `DONE` まで実行した（17 分 15 秒）。
**この全体実行はフィールド初期化の最終修正より前**であり、最終差分の全体合格を
意味しない。最終修正後は上記の関連テストを実行した。

| 検査 | 統合ゲートでの結果 | 変更前 HEAD との比較 |
|---|---|---|
| Slick | 184 files / 71 errors | 件数・診断ログが一致 |
| Cats | 340 files / 149 errors | 件数・診断ログが一致 |
| GitBucket | 354 files / 14 errors | 件数・診断ログが一致 |
| Scala library | 538 files / 1 error | 件数・診断ログが一致 |
| Workspace | 3385 passed / 33 failed | 失敗した CLI 32 件と pickle 1 件を個別再実行し、全件が変更前でも失敗 |
| Corpus | 5324 identities / 保存済み ledger に対して 40 losses | 40 件を変更前 HEAD で再実行し、全件が同じ fail 状態 |

変更前の比較用コンパイラとテストは、`git archive HEAD` から別ディレクトリへ
展開したソースを専用 target ディレクトリでビルドした。コーパスの変更前比較は
保存済み ledger からの損失 40 件に対するもの。HEAD 全体のコーパスを別途一周
したものではなく、最終差分で「全コーパス回帰なし」と主張する検証ではない。
既存の統合基準 `tests/BASELINE.md` は更新していない。

最初に標準ライブラリ全般の親型補完を広げた案では Slick が 73 errors になった。
補完を binary 供給の `scala.concurrent` に限定して、その追加 2 件を解消した。

## このマシンの検証ログ

一時ファイルのため、再起動・清掃後の存在は保証しない。

- `/tmp/scala-rs-async/focused-final.log`
- `/tmp/scala-rs-async/clippy-final.log`
- `/tmp/scala-rs-async/gate-final.log` と `gate-final/`
- `/tmp/scala-rs-async/baseline-test-comparison.json`
- `/tmp/scala-rs-async/baseline-pickle-failure.log`
- `/tmp/scala-rs-async/baseline-lost-corpus.tsv`
- `/tmp/scala-rs-async/{slick,cats,gitbucket,library}-baseline.txt`
