# scala-async / `-Xasync`

## 利用方法

Scala 2.13 用の scala-async 1.0.1 をクラスパスへ追加します。

```sh
target/release/scala-rs compile Main.scala \
  --scala-library /path/to/scala-library-2.13.16.jar \
  -cp /path/to/scala-async_2.13-1.0.1.jar -Xasync -d out
java -cp out:/path/to/scala-library-2.13.16.jar:/path/to/scala-async_2.13-1.0.1.jar Main
```

```scala
import scala.async.Async.{async, await}
import scala.concurrent.{Await, Future}
import scala.concurrent.ExecutionContext.Implicits.global
import scala.concurrent.duration.Duration

object Main {
  def main(args: Array[String]): Unit = {
    val answer = async {
      val first = await(Future.successful(40))
      first + await(Future.successful(2))
    }
    println(Await.result(answer, Duration(10, "seconds")))
  }
}
```

`async` は `Future` を返します。`await` は非同期処理を中断・再開するための
マーカーで、スレッドをブロックする `Await.result` とは異なります。上の例の
`Await.result` は、main が出力を待つために明示的に使用しています。

## 変換と対応範囲

`crates/typer/src/async_lower.rs` が、型付け後のライブラリ呼び出しを識別し、
`Future.flatMap` と `Future.successful` に変換します。元のシンボルを保持して
ローカル変数の参照・キャプチャを維持し、待機をまたぐオペランドを一時変数に
保存します。`while` / `do-while` は Future を返すローカルメソッドを介して
次の反復へ進みます。ガード内の `await` が false なら次の case を試します。

対応する実行例は `tests/fixtures/async_runtime.scala` と `async_values.scala`:

- 待機なし、連続待機、入れ子の `async` / `await`。
- 単一スレッドの実行コンテキストで、Promise を待つ処理と完了させる処理。
- `val` / `var`、代入、配列・フィールド更新、コンストラクタ引数。
- クラス・オブジェクトのフィールド初期化内での async と、可変ローカル変数の共有。
- `if`、`match` と非同期ガード、短絡評価、10,000 回のループ。
- 型引数、異なる分岐結果の共通型、ローカルメソッド・クラスのキャプチャ。
- 待機先の失敗、本体の例外、ExecutionContext の一度だけの評価。
- import の別名と、変換対象ではない同名の通常メソッド。
- 非ローカル `return`、`return await(...)`、ループ・生成クラスをまたぐ戻り先。

### 非ローカル `return`

`async` 内の `return` は、Scala 2.13 と同じく、字句的に外側にあるメソッドを
戻り先とします。`async` の結果を返すための構文ではありません。
戻り先のメソッドを呼ぶたびに新しいキーを生成し、
`scala.runtime.NonLocalReturnControl` とキーが一致する呼び出しだけで捕捉します。
同じメソッドへ再入した場合にも、内側の呼び出しが戻り値を横取りしません。

即時実行する ExecutionContext なら、まだ実行中の外側のメソッドへ戻れます。
待機後、すでに外側のメソッドが終了している場合はこの制御例外が外へ伝播し、
async の Future は未完了のままです。実行コンテキストによる例外報告も含め、
一般の `return` を非同期処理の結果通知として使用しないでください。
`async_return.scala` は即時実行・遅延再開・再入・finally を実 scalac と比較します。

### 独自ライブラリの変換フック

`c.internal.markForAsyncTransform(owner, method, awaitSymbol, config)` を使う
マクロも受け取れます。変換対象は指定された await メソッドであり、名前が
`await` である必要はありません。この経路では scala-async の jar は不要です。
マクロ実装のクラスパスには、通常のマクロと同じく scala-reflect が必要です。

生成クラスが提供する次のプロトコルを使用します。

- `state` / `state_=`: 開始状態と中断後の状態。
- `onComplete(awaitable)`: 再開コールバックの登録。
- `getCompleted(awaitable)`: 省略可能な、完了済み結果の取得。
- `tryGet(completion)`: 値の取得。状態機械自身を返すと継続を打ち切る。
- `completeSuccess(value)` / `completeFailure(error)`: 結果の通知。

内部では Promise と継続を接続します。実行待ちの継続をキューで処理するため、
完了済みの値を繰り返し await してもコールスタックを積み上げません。
`allowExceptionsToPropagate` 設定にも対応し、例外を `completeFailure` へ渡す代わりに
呼び出し側へ伝播します。未知の設定キーは nsc と同様に無視します。
通常設定では制御例外を含む `Throwable` を元の種類のまま `completeFailure` に渡します。
したがって、独自ライブラリでの非ローカル `return` の伝播は、このメソッドの実装にも
依存します。例えば、例外を再スローする Option の実装では外側のメソッドへ戻り、
例外を保存する CompletableFuture の実装では Future が例外終了します。

`async_hook_impl.scala` / `async_hook_runtime.scala` は、独自の Option と
Java CompletableFuture のマクロで、中断・再開・途中終了・例外・10,000 回のループを
比較します。別スレッドからの 1,000 回の完了通知、割り込み状態、完了フックの例外、
同名の通常オーバーロードも差分試験で確認します。
`async_hook_bad.scala` は対象外に残った await と不正な入れ子を拒否します。

`async_bad.scala` では、本体外の `await`、入れ子のメソッド・関数・クラス・
オブジェクト、lazy val、by-name 引数、try 内での `await` を拒否します。
`await` を含まないローカル定義・try は利用できます。

## 検証

Scala 2.13.16 と同じソースをコンパイルし、`java -Xverify:all` で出力を比較します。
単一スレッドでの試験には実行期限を設け、ブロッキング実装のデッドロックも
テストの失敗として検出します。jar を通常の Coursier キャッシュ以外に置く場合:

```sh
SCALA_ASYNC_JAR=/path/to/scala-async_2.13-1.0.1.jar tests/cli_test.sh xflags
```

関連する型検査修正として、手書き prelude の外にある `scala.concurrent` クラスの
親型を、引数の適合性判定前に補完します。これにより `Future[A]` と
`Awaitable[A]`、`FiniteDuration` と `Duration` の関係が、ロード順に依存せず
`Await.result` で利用できます。

初回実装の統合ゲートと変更前比較は、[初回の検証記録](notes/async-validation-2026-09-15.md)、
汎用フックと非ローカル return の追加検証は、[追加検証記録](notes/async-hooks-validation-2026-09-16.md)
を参照してください。

## 制限と参照先

nsc が生成する単一 `FutureStateMachine` とバイトコードの形・割り当て数は
一致しません。汎用フックの `postAnfTransform` / `stateDiagram` 設定コールバックと、
状態機械インスタンスを追加パラメータで渡す形式は未対応で、明示的に診断します。
このため、`-Xasync` の内部 API 全体との完全互換を意味するものではありません。

一次資料:

- [scala-async の利用方法と制限](https://github.com/scala/scala-async)
- scala-async 1.0.1 sources jar の `scala/async/Async.scala`:
  `async` はマクロ、`await` は compileTimeOnly のマーカーであり、`-Xasync` を
  検査してから `markForAsyncTransform` を呼びます。
- [Scala 2.13 の公開変換フック](https://github.com/scala/scala/blob/v2.13.16/src/reflect/scala/reflect/api/Internals.scala)
- [Scala 2.13 の async 変換](https://github.com/scala/scala/blob/v2.13.16/src/compiler/scala/tools/nsc/transform/async/AsyncPhase.scala)
- [コンパイラ側の -Xasync の設計](https://contributors.scala-lang.org/t/design-of-xasync/4419)

差分試験の境界: Scala 2.13.16 / scala-async 1.0.1 では、
`mark("a") + await(Future { mark("b") }) + mark("c")` が `bac` の順で
副作用を実行しました。scala-rs はオペランドを左から右へ保存し、`abc` になります。
共通の実行試験では `val first = mark("a")` として順序を明示しています。
また、待機前に定義したローカルクラスが間接的に捕捉する値だけを待機後に
使う例は、同じ参照コンパイラで `VerifyError` になりました。共通試験では
その値の直接参照も残しています。これらは参照コンパイラとの一致を主張する
対象外です。
