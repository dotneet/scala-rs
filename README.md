# scala-rs

Rust で書いた、Scala 2.13（nsc）サブセットのコンパイラです。JVM classfile を出力します。

scalac のソースを移植したものではありません。オリジナルの再実装です。Scala 3 / TASTy は対象外です。

## これは何か

scala-rs は、Scala 2.13 の構文と意味論のごく一部を、Rust から JVM バイトコードへ落とす実験的コンパイラです。

- フロントエンドは nsc の `Tree` に近い AST を持ちます。
- ターゲットは Java 6 相当の classfile（major version 50）です。StackMapTable は出しません。
- デフォルトでは scala-library を同梱しません。Option / List / FunctionN は **scala-rs 独自のランタイム classfile**（`scala/Option` など）です。
- `--scala-library [<jar>]`（または `SCALA_LIBRARY_JAR`）を付けると、Option / List / FunctionN / Tuple2 に加え、`Predef$`（`println` / `assert` / `require` / `???` / `identity` / `locally` / `implicitly`）、`any2stringadd`（`1 + "x"`）、`ArrowAssoc` の `->`、`intWrapper` / `RichInt`（`1.abs` / `1.max` / `1.to`）、`StringOps`（`augmentString` 経由の `toInt` / `length` / `*` / `take` / `drop` / `isEmpty`）、`WithFilter` / `Iterator`、`Map` / `Vector`（varargs `apply` を含む）は **scala-library 2.13.16 の ABI** にリンクし、衝突する私有 classfile は出しません。jar パスを省略すると `SCALA_LIBRARY_JAR`、`/tmp/scala-rs-lib`、cwd を探します。**`compile` はフラグも環境変数も無いときは私有ランタイムのまま**です。**`scala-rs run` は jar が自動検出できればそれを既定で使い**、見つからなければ私有ランタイムに落ちます。

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
scala-rs compile file.scala -d out/ --scala-library /path/to/scala-library-2.13.16.jar
```

コンパイルしてエントリポイント（`object Main` の `main`）を実行します。`--scala-library` 付きの `run` は jar を `java -cp` に足します。**jar が自動検出できるときは `run` が既定でそれを使い**、`compile` は明示しない限り私有ランタイムのままです。

```bash
scala-rs run file.scala
scala-rs run file.scala --scala-library /path/to/scala-library-2.13.16.jar
```

出力した classfile は、scalac と同様に `java` から起動できます。object はモジュールクラス `Main$` と、静的 `main` を持つフォワーダ `Main` を出します。デフォルトではランタイム（`scala/Option` など）も同じ `-d` ディレクトリに出ます。`--scala-library` 時は私有の Option/List/FunctionN は出さず、jar 側を使います。

```bash
java -cp out Main
java -cp out:scala-library-2.13.16.jar Main
```

フロントエンドの中間結果を見るデバッグフラグ:

- `--parse` — パーサの AST ダンプ
- `--typer` — namer / typer 後の木のダンプ
- `-Xfatal-warnings` — warning をエラーにする（非網羅 match など）
- `--scala-library <jar>` — scala-library 2.13 にリンク（私有 Option/List を出さない）。環境変数 `SCALA_LIBRARY_JAR` でも可。パス省略時は自動検出。**`compile` の既定は私有ランタイム。`run` の既定は自動検出できた jar**

フィクスチャはデフォルトパッケージ（`package` 句なし）なので、`-cp out` の `Main` でそのまま動く想定です。

## 実装している言語サブセット

Scala **2.13** 構文です。Scala 3 の `then`、トップレベル定義、TASTy はありません。エントリポイントは `def main(args: Array[String]): Unit` です。

パースできる（またはデシュガーする）構文:

- packages / imports
- objects / classes / traits / case classes
- `val` / `var` / `def`（ネストした `def` はパースする）
- パラメータ、ラムダ（型付き / 期待型から推論）、ブロック
- `if` / `else`、`while`
- `try` / `catch` / `finally`（catch は `{ case ... }`。JVM 例外テーブルを出す）
- `match`（コンストラクタパターン、リテラル、ワイルドカード）
- for-comprehension（`map` / `flatMap` / `foreach` / `withFilter` へデシュガー。私有ランタイムでは `List.withFilter` は eager な `List`。`--scala-library` 時は `scala.collection.WithFilter`。`Option.withFilter` は `Option$WithFilter`）
- apply / select / infix（`:` 終わりの演算子は右結合で、レシーバは右オペランド。`1 :: Nil` → `Nil.::(1)`）
- リテラル、タプル
- 名前付き型・ジェネリック型（`Array[String]`、`def id[T](x: T): T` など）
- `s"..."` 文字列補間
- `lazy val`
- implicit val / def（ローカル、import、パッケージオブジェクト、コンパニオン）、implicit パラメータ、スコープ内の implicit conversion。第二パラメータ節の明示渡し `foo(x)(y)` を含む
- デフォルト引数、by-name パラメータ（`=> T`）
- 名前付き引数（呼び出し側で並べ替え）
- 具象メンバー付き trait の mixin（`T$class` 静的実装 + 線形化順のフォワーダ）
- 内部クラス（`$outer`）とネストした object
- `super` / 修飾付き `this`（`Outer.this`）。trait の `super` は、具象クラスなら `T$class`、スタック可能な `abstract override` なら `T$$super$m` 経由
- `sealed` 階層の match 網羅検査（不足は **warning**。`-Xfatal-warnings` でエラー）
- extractor の `unapply`（`Option` / `Boolean` / `Tuple2`）と `unapplySeq`（`List` と可変長 `_*`）。名前付き extractor 引数（`Point(y = b, x = a)`）
- `AnyVal` 値クラス（1 引数。生成は underlying へ erase。メソッドは `name$extension`）
- Predef の一部: `assert` / `require` / `???` / ArrowAssoc の `->` / `identity` / `locally` / `implicitly` / `any2stringadd`（`1 + "x"`）/ String の `length`・`toInt`（`toLong` / `toDouble` もある）。**`--scala-library`** 時はこれらを jar の `Predef$` / `StringOps` / `Predef$ArrowAssoc` / `Predef$any2stringadd` にリンクする。さらに `intWrapper` / `RichInt`（`abs` / `max` / `to` / `until`）、`StringOps` の `*` / `take` / `drop` / `isEmpty`、`Map` / `Vector` の varargs `apply`（`Map(1 -> "a")` / `Vector(1,2,3)`）も jar リンク時のみ
- 具象 `val` 付き trait の初期化（`T$class.$init$`）と `abstract override` の super 連鎖

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

### Trait mixin

Java 6 には default method がないので、具象メンバー付き trait は次のように出します。

- trait 自体はすべてのメソッドが abstract な JVM interface
- 具象本体は `T$class` の static メソッド（第一引数が `$this: T`）
- 実装クラスは線形化（右の mixin がより具体的）で勝った定義へフォワーダを出す

`class C extends A with B` で A と B が同じ `msg` を持つとき、実行時は B です。線形化は Scala の C3 です（`C extends Base with A with B` → `C, B, A, Base`）。

trait の `val` は interface 上の getter / `$init$set$` と、`T$class.$init$` で右辺を評価します。実装クラスがフィールドを持ち、コンストラクタが mixin `$init$` を（より一般的な親から）呼びます。

スタック可能な trait の `abstract override` は、`T$class` 内の `super.m` を `T$$super$m`（実装クラスが線形化の次へフォワード）にします。`class C extends Base with A with B` で両方 `abstract override def msg` なら、実行時は `B-A-base` です。

### try / catch / finally

`try` 本体を例外テーブルで覆い、ハンドラで catch のパターン（`case _: RuntimeException` など）を `instanceof` します。マッチしなければ再 throw します。`finally` は成功パスと catch パスの両方で実行します（`jsr` は使いません。コードを複製します）。

### ネストした型

`class Outer { class Inner }` は `Outer$Inner` になり、非 static な内部クラスは `$outer` をコンストラクタで受け取ります。`object Outer { object Inner }` は `Outer$Inner$` と `MODULE$` です。

### lazy val

フィールドに加えて `bitmap$0: Int` と、同期したアクセサを出します。初期化は最初の読み取りまで遅延します。

### super / 修飾付き this

`super.m(...)` はクラス親なら `invokespecial`、具象 trait 親なら `T$class.m($this, ...)` です。線形化の「右端の親」を `super` の対象にします（`super[T]` の mixin 指定もパースして使います）。`Outer.this` は内部クラスの `$outer` を辿ります。

trait 本体の `super`（`abstract override` を含む）は、ミックス先クラスが埋める `T$$super$m` です。trait の `val` 初期化は `$init$` です。

### sealed と exhaustiveness

同じコンパイル単位の `sealed` 子（case class / case object / class）を記録し、`match` が葉を覆っていないと **warning** にします。

```
match may not be exhaustive. It would fail on the following input: …
```

scalac 2.13 と同じく hard error ではありません。`-Xfatal-warnings` を付けるとエラーになります。ガード付き case は網羅に数えません。ワイルドカード / 小文字の変数は catch-all です。

### unapply / unapplySeq

`Even(n)` のような extractor はコンパニオン（または object）の `unapply` を呼びます。戻りが `Option[T]` なら `isEmpty` / `get`、`Boolean` なら真偽、`Option[(A,B)]` なら `Tuple2` の `_1` / `_2` です。`unapply` が無いパターンは `not found: extractor` です。

`unapplySeq` は `List` のコンパニオンと、ユーザー定義の可変長 extractor です。`List(a, b, c)`、`List(h, rest @ _*)`、`PairSeq(a, b)` が動きます。名前付き引数は case class のコンストラクタパターンで並べ替えます（`Point(y = b, x = a)`）。

### AnyVal

`class Meter(val n: Int) extends AnyVal` は、値の表現を underlying（ここでは `Int`）に erase します。`new Meter(x)` は `x` になり、`m.n` は `m` です。メソッドは `Meter.doubled$extension(n)` のような static です。Any として使うときの box（`Integer`）は、他のプリミティブと同じ erasure の box 挿入に従います。値クラスをラップする専用のヒープオブジェクトは、このパスでは出しません。

### Predef（このスライス）

- `assert(cond)` / `require(cond)`（第 2 引数の by-name メッセージあり）。**私有ランタイム**では `AssertionError` / `IllegalArgumentException` を直接 `new`。**`--scala-library`** では `scala.Predef$.assert` / `require`
- `???` は **私有**では `new scala.NotImplementedError`（`RuntimeException` サブクラス）。**library** では `Predef$.???`（jar の `NotImplementedError` は `Error`）。dual-run フィクスチャは `Throwable` で捕捉する
- `any2ArrowAssoc` による `1 -> "a"`。**私有**では `scala.Tuple2` を直接 `new`（`Predef.ArrowAssoc` は呼ばない）。**library** では implicit `any2ArrowAssoc` → `Predef$ArrowAssoc$.$minus$greater$extension`
- `identity` / `locally` / `implicitly`。**私有**では intrinsic。**library** では `Predef$.identity` / `locally` / `implicitly`
- `any2stringadd` の `1 + "x"`。**私有**では StringBuilder 連結（intrinsic）。**library** では implicit `any2stringadd` → `Predef$any2stringadd$.$plus$extension`
- `"x".length`。**私有**では `java.lang.String#length`。**library** では implicit `augmentString` → `StringOps.size$extension`（jar の StringOps は `length` をインライン化しており、同等の `size$extension` が `String#length` を呼ぶ）。`toInt` / `toLong` / `toDouble` は **私有**では `Integer.parseInt` など。**library** では `StringOps.toInt$extension`

## 実装していないもの

次は実装していません。スタブで「動いたことにする」こともしていません。

- マクロ
- コンパイラプラグイン
- 完全な Scala 標準ライブラリ。`--scala-library` なしでは Option / List / FunctionN / Tuple2 は私有ランタイム。**jar にリンクしても** 完全な StringOps / 全 numeric enrichment / 任意の `Seq` ファクトリなどは未対応
- Scala 3 構文
- implicit の優先度 / `scala.Int` コンパニオンの enrichment
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
- **ライブラリ**: デフォルトの **`compile`** は私有ランタイム。`--scala-library`（パス省略時は `SCALA_LIBRARY_JAR` / `/tmp/scala-rs-lib` / cwd を探索）で jar にリンクし、同名の私有 classfile は出さない。**`scala-rs run` は同じ探索で jar が見つかれば既定でリンクする**（見つからなければ私有）。jar に乗るもの: `Option` / `Some` / `None` / `List` / `Nil` / `::` / `Function0` / `Function1` / `Tuple2` / `NotImplementedError` / `Predef$`（`println` / `assert` / `require` / `???` / `identity` / `locally` / `implicitly`）/ `any2stringadd` / `ArrowAssoc` の `->` / `intWrapper` / `RichInt`（`abs` / `max` / `min` / `to` / `until`）/ `StringOps`（`toInt$extension` / `size$extension` / `$times$extension` / `take$extension` / `drop$extension` / `isEmpty` via `augmentString`）/ `WithFilter` / `Iterator` / `Map` / `Vector`（`empty` / `updated` / lookup / `:+` / `foreach` / **varargs `apply`**）。dual-run: `hello` / `option_for` / `list_for` / `predef` / `predef_more` / `unapply` / `unapply_seq` / `iterator` / `map` / `vector` / `int_ops` / `string_ops`。**まだ intrinsic / 私有**: 完全な StringOps、他の numeric wrapper、`List` の varargs `apply`。`List.unapplySeq` は library では `SeqOps` の identity。`compile` のフラグなしは私有ランタイム。
- **object**: scalac と同様、`Main$`（モジュール）と静的フォワーダ `Main` を出します。`java Main` が動くのはそのためです。
- **プリミティブ**: `Int` の `+` などは `scala.Int` のボックスメソッドではなく、JVM 命令（`iadd` など）として出します。
- **trait**: 抽象メンバーだけの trait は JVM interface です。具象メンバーは `T$class` 静的実装と、C3 線形化順のインスタンスフォワーダです。Java 8 default method は使いません（major 50）。`val` は getter/setter + `$init$` です。`abstract override` は `T$$super$m` です。
- **名前付き引数**: 呼び出し側で `f(b = 2, a = 1)` を並べ替えます。巨大な rewrite フェーズはありません。extractor パターンでも case class なら並べ替えます。
- **try**: Code 属性に例外テーブルを出します。StackMapTable はありません。
- **ラムダ**: `FunctionN` を実装する合成クラス（`Main$$$anonfun$0` など）です。invokedynamic / LambdaMetaFactory は使いません（Java 6）。
- **フェーズ**: nsc の uncurry / mixin / lambdaLift などの独立パスはありません。erasure とラムダのクロージャ変換はあります。
- **sealed**: 非網羅 match は scalac と同様 warning です。`-Xfatal-warnings` でエラーになります。
- **AnyVal**: scalac は値クラスのクラスファイルと拡張メソッドの両方を出します。scala-rs もクラスは出しますが、呼び出しは `$extension` 静的メソッドで、`new C(x)` は underlying に消えます。
- **Predef / StringOps**: 私有では `assert` / `require` / `???` / `->`（`Tuple2` 直結）/ `identity` / `locally` / `implicitly` / `any2stringadd` と String の `length`/`toInt`/`isEmpty`。library では `Predef$.println/assert/require/???/identity/locally/implicitly`、`any2stringadd.$plus$extension`、`ArrowAssoc.$minus$greater$extension`、`intWrapper` → `RichInt.abs$extension` / `max$extension` / `to$extension`、`augmentString` → `StringOps.toInt$extension` / `size$extension`（`.length`）/ `$times$extension` / `take$extension` / `drop$extension`（`.isEmpty` は StringOps 経由で `String#isEmpty` にインライン）。**`StringOps` / `RichInt` classfile は出していません。**
- **unapplySeq**: `List` とユーザー定義 extractor、`_*`、名前付き case class パターン。library リンク時の `List.unapplySeq` は `SeqOps` 戻り。

scalac の代替ではありません。サブセットの再実装です。

## テスト

```bash
cargo test
```

実行時の期待値は `tests/fixtures/` にあります。各 `.scala` に対して `tests/fixtures/expected/` に同名の `.txt`（`println` と同じ末尾改行付きの stdout）を置いています。`java` がある環境では CLI の e2e が stdout を比較します。

scala-library 2.13.16 が取れる環境では、次を `--scala-library` でコンパイルし、`java -cp out:scala-library.jar Main` でも同じ stdout になることを見ます（私有の `scala/Option.class` / `scala/Predef$.class` 等が無いこと）: `hello` / `option_for` / `list_for` / `predef` / `predef_more` / `unapply` / `unapply_seq` / `iterator` / `map` / `vector` / `int_ops` / `string_ops`。`iterator.scala` / `map.scala` / `vector.scala` / `int_ops.scala` / `string_ops.scala` は library リンク時のみ（私有ランタイムに `iterator` / `Map` / `Vector` / `RichInt` / 拡張 StringOps は無い）。

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
| `trait_concrete.scala` | 具象メソッド付き trait を class が使う | `from trait` |
| `trait_linearize.scala` | `extends A with B` の線形化（B が勝つ） | `B` |
| `try_catch.scala` | throw / catch / finally | `before` `caught` `finally` |
| `nested_class.scala` | `class Outer { class Inner }` | `inner` |
| `nested_object.scala` | `object Outer { object Inner }` | `nested` |
| `super.scala` | クラス/`trait` の `super` と `Outer.this` | `base!` `T!` `outer` |
| `sealed_match.scala` | sealed + case class/object の網羅 match | `3` `0` |
| `unapply.scala` | `object Even { def unapply }` | `5` `-1` |
| `value_class.scala` | `AnyVal` の `Meter` | `42` `21` |
| `predef.scala` | `assert`/`require`/`toInt`/`->`/`???` | `2` `42` `1` `a` `nyi` |
| `unapply_seq.scala` | `unapplySeq` / `_*` / 名前付き extractor | `6` `21` `1` `7` |
| `iterator.scala` | `List.iterator`（library dual-run のみ） | `true` `1` `2` `false` |
| `map.scala` | `Map(1 -> "a", 2 -> "b")` の apply / get / foreach（library dual-run のみ） | `a` `b` `a` `a` `b` |
| `vector.scala` | `Vector(1, 2, 3)` の apply / length / foreach（library dual-run のみ） | `1` `2` `3` `3` `1` `2` `3` |
| `int_ops.scala` | `intWrapper` / `RichInt` の `abs` / `max` / `to`（library dual-run のみ） | `3` `1` `2` `Range 1 to 3` `1` `2` `3` |
| `string_ops.scala` | `augmentString` 経由の `*` / `take` / `drop` / `isEmpty`（library dual-run のみ） | `ababab` `he` `llo` `true` `false` |
| `trait_val.scala` | trait `val` の初期化 | `from trait` |
| `abstract_override.scala` | `abstract override` の super 連鎖 | `B-A-base` |
| `predef_more.scala` | `any2stringadd` / `implicitly` / `identity` / `locally` | `1x` `41` `42` `here` |
| `sealed_non_exhaustive.scala` | 非網羅 match（warning。実行は覆っている入力だけ） | `3` |

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストで見ています。コンパイルを成功扱いにしていません。

## ライセンス

Apache-2.0
