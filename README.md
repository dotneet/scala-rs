# scala-rs

Rust で書いた、Scala 2.13（nsc）サブセットのコンパイラです。JVM classfile を出力します。

scalac のソースを移植したものではありません。オリジナルの再実装です。Scala 3 / TASTy は対象外です。

## これは何か

scala-rs は、Scala 2.13 の構文と意味論のごく一部を、Rust から JVM バイトコードへ落とす実験的コンパイラです。

- フロントエンドは nsc の `Tree` に近い AST を持ちます。
- ターゲットは Java 6 相当の classfile（major version 50）です。StackMapTable は出しません。
- 標準ライブラリ全体は載せていません。フィクスチャが使う `Int` / `String` / `Unit` / `Boolean` / `Array[String]` / `println` 程度が前提です。

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

出力した classfile は、scalac と同様に `java` から起動できます。object はモジュールクラス `Main$` と、静的 `main` を持つフォワーダ `Main` を出します。

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
- パラメータ、ラムダ（型付き）、ブロック
- `if` / `else`、`while`
- `match`（コンストラクタパターン、リテラル、ワイルドカード）
- for-comprehension（`map` / `foreach` へデシュガー。ランタイムの `List` は未実装）
- apply / select / infix
- リテラル、タプル
- 名前付き型・ジェネリック型（`Array[String]` など）
- `s"..."` 文字列補間

フィクスチャで実際に使う範囲はこれより狭いです。`Int`、`String`、`Unit`、`Boolean`、`Array[String]`、`println`、クラス、object、trait、case class、`match`、`if`/`else`、`while`、再帰、文字列 `+` です。

## 実装していないもの

次は実装していません。スタブで「動いたことにする」こともしていません。

- マクロ
- コンパイラプラグイン
- 完全な Scala 標準ライブラリ（`List` のランタイムなど）
- Scala 3 構文
- implicit 解決（ユーザ定義の implicit conversion / implicit parameter は未実装。プリミティブの numeric widening だけを typer が特別扱いする。偽の完全実装ではない）
- `lazy val` のコード生成
- 内部クラス / 匿名クラス
- XML リテラル
- existential types
- view bounds
- 独立した erasure フェーズ（erasure は JVM emit に折り込む。nsc のような別パスではない）
- TASTy

パーサは未対応構文を黙って捨てず、診断と `Unimplemented` ノードを出します。

## アーキテクチャ

Cargo workspace のクレート:

| crate | 役割 |
| --- | --- |
| `scala-rs-span` | ソース位置と診断 |
| `scala-rs-lexer` | 字句解析（セミコロン推論用の改行トークン、`s"..."` のモードスタック） |
| `scala-rs-parser` | 再帰下降パーサ。AST は nsc の `Tree` に近い |
| `scala-rs-typer` | namer + typer（木を in-place で型付け） |
| `scala-rs-backend` | JVM classfile 出力（major 50） |
| `scala-rs-driver` | パイプライン駆動 |
| `scala-rs-cli` | コマンドライン。バイナリ名 `scala-rs` |

パイプライン:

```
parse → namer → typer → emit
```

後から uncurry / erasure を typer と backend の間に挟めるように、AST は書き換え前提です。現時点ではそのフェーズはありません。

## scalac 2.13 との比較

正直な差分です。

- **規模**: nsc のごく一部。言語仕様を満たしません。
- **ライブラリ**: scala-library を同梱しません。`Predef.println` とプリミティブ演算など、コンパイラが知っているシンボルだけです。
- **object**: scalac と同様、`Main$`（モジュール）と静的フォワーダ `Main` を出します。`java Main` が動くのはそのためです。
- **プリミティブ**: `Int` の `+` などは `scala.Int` のボックスメソッドではなく、JVM 命令（`iadd` など）として出します。
- **trait**: 抽象メンバーだけの trait は JVM interface として出します。具象メンバー付き trait の完全な線形化・実装クラスは載せていません。
- **フェーズ**: nsc の uncurry / erasure / mixin / lambdaLift などの独立パスはありません。

scalac の代替ではありません。サブセットの再実装です。

## テスト

```bash
cargo test
```

実行時の期待値は `tests/fixtures/` にあります。各 `.scala` に対して `tests/fixtures/expected/` に同名の `.txt`（`println` と同じ末尾改行付きの stdout）を置いています。

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

## ライセンス

Apache-2.0
