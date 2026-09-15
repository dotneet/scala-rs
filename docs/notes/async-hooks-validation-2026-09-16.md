# async 変換フックと非ローカル return の検証（2026-09-16）

## 対象

変更前 HEAD: `c3215024`。Scala 2.13.16、scala-async 1.0.1、Zulu JDK 21.0.5。
実装の範囲と残る制限は [async/await ガイド](../async.md) を参照。

## 差分試験

`xflags` の 13 テストが合格。実 scalac と同じソースをコンパイルし、
`java -Xverify:all` で標準出力を比較した。非同期処理の実行期限は 30 秒。
本体のコンパイルには `-Xasync -Xfatal-warnings` を指定した。

- `async_return.scala`: 即時 return、完了済み await、return await、finally、
  ループ、ローカルメソッド、型パラメータ、同一メソッドへの再入、遅延再開。
  遅延再開では制御例外が伝播し、元の Future が未完了であることも比較。
- `async_hook_impl.scala` / `async_hook_runtime.scala`: 実 scalac で作ったマクロを
  scala-rs と scalac の双方から呼び出す。独自 Option と Java CompletableFuture の
  await、成功、途中終了、失敗、型引数、分岐、10,000 回のループを比較。
  実行時クラスパスは生成クラス・独自ライブラリ・scala-library のみ。
- フックの任意の完了パラメータ名、`duplicate` によるマーク保持、
  継承した getCompleted、生成クラス自身の getCompleted、省略時のコールバックを確認。
- 別スレッドから 1,000 回完了させ、結果の合計 42,000 を双方で確認。
- 本体の例外、完了フック自身の例外、AssertionError、InterruptedException、
  制御例外の型・値とスレッドの割り込み状態を比較。
  `allowExceptionsToPropagate` 有無による違いも確認。
- `async_hook_overload.scala`: 同名の通常のオーバーロードは変換も拒否もしない。
- `async_hook_bad.scala`: 外側に残る対象 await と入れ子の関数を双方が拒否。
  scala-rs では `-Xasync` を指定しない独自フックも拒否。

## 互換性修正の要点

非ローカル return のキーは外側のメソッド呼び出しごとに確保する。
生成クラスや継続をまたいでも、同じメソッドの別の呼び出しが捕捉しない。

汎用フックは nsc と同じく Throwable を completeFailure へ渡す。
Scala Future が制御例外を成功値へ変換したり、Error をラップしたりする処理を
独自プロトコルへ漏らさないよう、内部の失敗通知と元の例外を分けた。
例外の伝播は Future の呼び出し境界を出てから行い、割り込み状態を変更しない。

生成されたローカルクラス内の補助メソッドも lambda lifting の対象にした。
また、マクロ呼び出しの結果型を提供する際に、結果型のタグを構築できないだけで
既存の whitebox マクロを拒否しないようにした。ZIO の autoTrace、構造型を返す
マクロ、AnyRef を返すマクロを周辺テストで確認した。

## 検証範囲

最終ソースで **134 passed / 0 failed**。内訳は `xflags` 13 件と周辺 121 件。

```sh
SCALA_ASYNC_JAR=/tmp/scala-rs-async/scala-async.jar tests/cli_test.sh \
  xflags aliaslookup gbmapto slickshape innerclasses lazyref indy \
  macros macrotag macromirror macrotransportbatch tqmacro \
  reify reify2 reifydefs rf_reify
```

`cargo fmt --all -- --check`、`git diff --check` が合格。
`cargo clippy --release -p scala-rs-cli` は終了コード 0（既存警告あり）。

今回の追加変更について、全 workspace / 全 corpus の統合ゲートは再実行していない。
[初回実装の統合ゲート記録](async-validation-2026-09-15.md) は別の変更時点の結果であり、
今回の変更全体が統合ゲートを通過したという意味ではない。
`tests/BASELINE.md` は更新していない。

## このマシンのログ

一時ファイルのため、清掃後の存在は保証しない。

- `/tmp/scala-rs-async-followup/final-tests.log`
- `/tmp/scala-rs-async-followup/clippy-final.log`
