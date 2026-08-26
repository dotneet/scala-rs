# scala-rs

Rust で書いた、Scala 2.13（nsc）サブセットのコンパイラです。JVM classfile を出力します。

scalac のソースを移植したものではありません。オリジナルの再実装です。Scala 3 / TASTy は対象外です。

## これは何か

scala-rs は、Scala 2.13 の構文と意味論のごく一部を、Rust から JVM バイトコードへ落とす実験的コンパイラです。

- フロントエンドは nsc の `Tree` に近い AST を持ちます。
- ターゲットは Java 6 相当の classfile（major version 50）です。StackMapTable は出しません。
- scala-library は同梱しません。Option / List / FunctionN は **scala-rs 独自のランタイム classfile**（`scala/Option` など）です。scalac が出す `scala-library.jar` とはバイナリ互換ではありません。

完成した Scala コンパイラではありません。仕様への完全準拠も主張しません。

## ビルド

Rust の Cargo workspace です。CLI クレートは `scala-rs-cli`、バイナリ名は `scala-rs` です。

```bash
cargo build -p scala-rs-cli --release
```

デバッグ実行:

```bash
cargo run -p scala-rs-cli -- compile file.scala -d out/
```

成果物は `target/release/scala-rs`（または `target/debug/scala-rs`）です。

## 使い方

ソースを classfile にコンパイルしてディレクトリへ書き出します。

```bash
scala-rs compile file.scala -d out/
```

コンパイルしてエントリポイント（`object Main` の `main`）を実行します。

```bash
scala-rs run file.scala
```

出力した classfile は、scalac と同様に `java` から起動できます。object はモジュールクラス `Main$` と、静的 `main` を持つフォワーダ `Main` を出します。ランタイム（`scala/Option` など）も同じ `-d` ディレクトリに出ます。

```bash
java -cp out Main
```

フロントエンドの中間結果を見るデバッグフラグ:

- `--parse` — パーサの AST ダンプ
- `--typer` — namer / typer 後の木のダンプ

フィクスチャはデフォルトパッケージ（`package` 句なし）なので、`-cp out` の `Main` でそのまま動く想定です。

## 実装している言語サブセット

Scala **2.13** 構文です。Scala 3 の `then`、トップレベル定義、TASTy はありません。エントリポイントは `def main(args: Array[String]): Unit` です。

パースできる（またはデシュガーする）構文:

- packages / imports
- objects / classes / traits / case classes
- `val` / `var` / `def`（ネストした `def` はパースする）
- パラメータ、ラムダ（型付き / 期待型から推論）、ブロック
- `if` / `else`、`while`
- `match`（コンストラクタパターン、リテラル、ワイルドカード）
- for-comprehension（`map` / `flatMap` / `foreach` / `withFilter` へデシュガー。ランタイムの `List` / `Option` で実行できる）
- apply / select / infix（`:` 終わりの演算子は右結合で、レシーバは右オペランド。`1 :: Nil` → `Nil.::(1)`）
- リテラル、タプル
- 名前付き型・ジェネリック型（`Array[String]`、`def id[T](x: T): T` など）
- `s"..."` 文字列補間
- `lazy val`
- implicit val / def（ローカル、import、パッケージオブジェクト、コンパニオン）、implicit パラメータ、スコープ内の implicit conversion。第二パラメータ節の明示渡し `foo(x)(y)` を含む
- デフォルト引数、by-name パラメータ（`=> T`）
- 名前付き引数（呼び出し側で並べ替え）

フィクスチャで実際に動く範囲は README 末尾の表を見てください。

### Erasure

パイプラインは次のとおりです。

```
parse → namer → typer → erasure → emit
```

erasure は typer と backend のあいだの独立パスです。型引数を落とし、型パラメータを `Object` にし、プリミティブと `Object` の境に box / unbox を挿入します。by-name は `Function0` に下げます。バックエンドの ad-hoc な推測だけには頼っていません。

### Implicit 解決（第一カット）

nsc に寄せた探索順です。偽の「何でも変換」はありません。

1. 現在のスコープと、囲んでいるクラス / object の `implicit` メンバー（`import Foo._` で入れたメンバーを含む）
2. 囲んでいるパッケージのパッケージオブジェクト（`package object p` の implicit メンバー）
3. 目標型（変換なら元の型も）のコンパニオンオブジェクトの `implicit` メンバー

呼び出し側で implicit パラメータ節を明示できます: `add(5)(3)` / `foo(x)(ev)`。探索で埋めるのは、その節が省略されたときだけです。

数値の widening（`Int` → `Long` / `Double` など）は **implicit 探索の前** に特別扱いします。scalac の implicit ではなく、typer の組み込みです。

失敗はスタブせず、診断を出します。

- `no implicit: could not find implicit value of type …`
- `ambiguous implicit: …`

### lazy val

フィールドに加えて `bitmap$0: Int` と、同期したアクセサを出します。初期化は最初の読み取りまで遅延します。

## 実装していないもの

次は実装していません。スタブで「動いたことにする」こともしていません。

- マクロ
- コンパイラプラグイン
- 完全な Scala 標準ライブラリ（ここにある Option / List は scala-rs ランタイムであり、scala-library ではない）
- Scala 3 構文
- implicit の優先度 / `Predef` の全変換 / `scala.Int` コンパニオンの enrichment
- 具象メンバー付き trait の mixin / 線形化（`$class` 実装クラスとフォワーダ）。抽象メンバーだけの trait は JVM interface として動く
- `try` / `catch` / `finally` の JVM 例外テーブル（パーサと typer はある。Code 属性のテーブル枠は出した。ハンドラ本体の生成は未了）
- 内部クラス / ネストした object の実行（ネストした定義はパース・ネーミングする）
- 匿名クラス
- XML リテラル
- existential types
- view bounds
- TASTy
- StackMapTable（Java 6 ターゲットのまま）

パーサは未対応構文を黙って捨てず、診断と `Unimplemented` ノードを出します。

## アーキテクチャ

Cargo workspace のクレート:

| crate | 役割 |
| --- | --- |
| `scala-rs-span` | ソース位置と診断 |
| `scala-rs-lexer` | 字句解析（セミコロン推論用の改行トークン、`s"..."` のモードスタック） |
| `scala-rs-parser` | 再帰下降パーサ。AST は nsc の `Tree` に近い |
| `scala-rs-typer` | namer + typer + erasure。implicit 探索を含む |
| `scala-rs-backend` | JVM classfile 出力（major 50）と scala-rs ランタイム |
| `scala-rs-driver` | パイプライン駆動 |
| `scala-rs-cli` | コマンドライン。バイナリ名 `scala-rs` |

## scalac 2.13 との比較

正直な差分です。

- **規模**: nsc のごく一部。言語仕様を満たしません。
- **ライブラリ**: scala-library を同梱しません。`Predef.println`、プリミティブ演算、およびコンパイラが出す Option / List / FunctionN ランタイムだけです。`java -cp out:scala-library.jar` で混ぜる想定ではありません。
- **object**: scalac と同様、`Main$`（モジュール）と静的フォワーダ `Main` を出します。`java Main` が動くのはそのためです。
- **プリミティブ**: `Int` の `+` などは `scala.Int` のボックスメソッドではなく、JVM 命令（`iadd` など）として出します。
- **trait**: 抽象メンバーだけの trait は JVM interface として出します。具象メンバー付き trait の `$class` 実装クラスと線形化フォワーダはまだ出していません。
- **名前付き引数**: 呼び出し側で `f(b = 2, a = 1)` を並べ替えます。巨大な rewrite フェーズはありません。
- **ラムダ**: `FunctionN` を実装する合成クラス（`Main$$$anonfun$0` など）です。invokedynamic / LambdaMetaFactory は使いません（Java 6）。
- **フェーズ**: nsc の uncurry / mixin / lambdaLift などの独立パスはありません。erasure とラムダのクロージャ変換はあります。

scalac の代替ではありません。サブセットの再実装です。

## テスト

```bash
cargo test
```

実行時の期待値は `tests/fixtures/` にあります。各 `.scala` に対して `tests/fixtures/expected/` に同名の `.txt`（`println` と同じ末尾改行付きの stdout）を置いています。`java` がある環境では CLI の e2e が stdout を比較します。

| フィクスチャ | 内容 | 期待 stdout |
| --- | --- | --- |
| `hello.scala` | 挨拶を `println` | `hello, scala-rs` |
| `arithmetic.scala` | `1+2*3` など Int 演算（優先順位込み） | `7` `6` `5` `1` |
| `class_methods.scala` | `Counter` を 10 から 2 回 `inc` | `12` |
| `case_match.scala` | `Point(3,4)` を match | `7` |
| `factorial.scala` | `fact(5)` 再帰 | `120` |
| `trait_impl.scala` | trait 実装の `greet` | `Hello, Scala` |
| `while_loop.scala` | `while (i < 3)` | `3` |
| `string_interp.scala` | `s"hello $name"` | `hello world` |
| `list_for.scala` | `1 :: 2 :: 3 :: Nil` の for-yield / guard | `2` `3` `4` `20` `30` |
| `option_for.scala` | `Some` / `None` の for-comprehension | `4` `true` |
| `lazy_val.scala` | `lazy val` の遅延と一度きりの初期化 | `0` `42` `42` `1` |
| `implicits.scala` | implicit パラメータとコンパニオンの conversion | `15` `14` |
| `generic_id.scala` | `def id[T](x: T): T` の erasure | `42` `hi` |
| `defaults.scala` | デフォルト引数 | `hi Scala!` `hi Scala?` |
| `byname.scala` | by-name パラメータが二度評価される | `6` `2` |

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストで見ています。コンパイルを成功扱いにしていません。

## ライセンス

Apache-2.0
