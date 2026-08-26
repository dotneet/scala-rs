# scala-rs

Rust で書いた、Scala 2.13（nsc）サブセットのコンパイラです。JVM classfile を出力します。

scalac のソースを移植したものではありません。オリジナルの再実装です。Scala 3 / TASTy は対象外です。

## これは何か

scala-rs は、Scala 2.13 の構文と意味論のごく一部を、Rust から JVM バイトコードへ落とす実験的コンパイラです。

- フロントエンドは nsc の `Tree` に近い AST を持ちます。
- ターゲットは Java 8 相当の classfile（major version 52）です。Code 属性に StackMapTable（full_frame）を出します。
- デフォルトでは scala-library を同梱しません。Option / List / FunctionN は **scala-rs 独自のランタイム classfile**（`scala/Option` など）です。
- `--scala-library [<jar>]`（または `SCALA_LIBRARY_JAR`）を付けると、Option / List / FunctionN / Tuple2 に加え、`Predef$`（`println` / `assert` / `require` / `???` / `identity` / `locally` / `implicitly`）、`any2stringadd`（`1 + "x"`）、`ArrowAssoc` の `->`、`intWrapper` / `RichInt`（`1.abs` / `1.max` / `1.to`）、`longWrapper` / `doubleWrapper` / `charWrapper`（`(-3L).abs` / `1.0.max` / `'9'.isDigit`）、`StringOps`（`augmentString` 経由の `toInt` / `length` / `*` / `take` / `drop` / `isEmpty`）、`WithFilter` / `Iterator`、`Map` / `Vector` / `List` / `Set`（varargs `apply` を含む）は **scala-library 2.13.16 の ABI** にリンクし、衝突する私有 classfile は出しません。jar パスを省略すると `SCALA_LIBRARY_JAR`、`/tmp/scala-rs-lib`、cwd を探します。**`scala-rs compile` と `scala-rs run` は、jar が自動検出できればそれを既定で使い**、見つからなければ私有ランタイムに落ちます。**`--no-scala-library` は私有ランタイムを強制**します。jar リンク時はさらに `Either` と `scala.util.Try` / `Success` / `Failure` も乗ります。

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
scala-rs compile file.scala -d out/ --no-scala-library
scala-rs compile B.scala -d outB -cp outA --no-scala-library
```

コンパイルしてエントリポイント（`object Main` の `main`）を実行します。`--scala-library` 付きの `run` は jar を `java -cp` に足します。**jar が自動検出できるときは `compile` / `run` が既定でそれを使い**、見つからなければ私有ランタイムです。`--no-scala-library` で私有に戻せます。

```bash
scala-rs run file.scala
scala-rs run file.scala --scala-library /path/to/scala-library-2.13.16.jar
```

出力した classfile は、scalac と同様に `java` から起動できます。object はモジュールクラス `Main$` と、静的 `main` を持つフォワーダ `Main` を出します。**私有ランタイム**（`--no-scala-library`、または jar が無いとき）ではランタイム（`scala/Option` など）も同じ `-d` ディレクトリに出ます。jar リンク時は私有の Option/List/FunctionN は出さず、jar 側を使います。

```bash
java -cp out Main
java -cp out:scala-library-2.13.16.jar Main
```

フロントエンドの中間結果を見るデバッグフラグ:

- `--parse` — パーサの AST ダンプ
- `--typer` — namer / typer 後の木のダンプ
- `-Xfatal-warnings` — warning をエラーにする（非網羅 match など）
- `--scala-library <jar>` — scala-library 2.13 にリンク（私有 Option/List を出さない）。環境変数 `SCALA_LIBRARY_JAR` でも可。パス省略時は自動検出。**`compile` / `run` の既定は自動検出できた jar。見つからなければ私有。`--no-scala-library` で私有を強制**
- `-cp` / `--class-path` — 先にコンパイルした classfile を読む（`ScalaSignature` pickle subset と JVM メソッド。vals / パラメータ付き defs / 型パラメータ / `$default$n` ゲッター / case class の ctor フィールドを含む。自前 `-cp` は companion `apply` も読む。nsc は companion apply `Point(...)` / term `Point` / extractor `unapply` / `List[_]` の existentials / `List[_ <: AnyRef]` / `@deprecated("msg", "2.13.0")` の annotation args / `this.type` / `Int @unchecked` も読む）

フィクスチャはデフォルトパッケージ（`package` 句なし）なので、`-cp out` の `Main` でそのまま動く想定です。

## 実装している言語サブセット

Scala **2.13** 構文です。Scala 3 の `then`、トップレベル定義、TASTy はありません。エントリポイントは `def main(args: Array[String]): Unit` です。

パースできる（またはデシュガーする）構文:

- packages / imports
- objects / classes / traits / case classes
- `val` / `var` / `def`（ネストした `def` はパースする）
- パラメータ、ラムダ（型付き / 期待型から推論）、ブロック
- `if` / `else`、`while`、`do { ... } while (cond)`
- `try` / `catch` / `finally`（catch は `{ case ... }`。JVM 例外テーブルを出す）
- `match`（コンストラクタパターン、リテラル、ワイルドカード）
- for-comprehension（`map` / `flatMap` / `foreach` / `withFilter` へデシュガー。私有ランタイムでは `List.withFilter` は eager な `List`。`--scala-library` 時は `scala.collection.WithFilter`。`Option.withFilter` は `Option$WithFilter`）
- apply / select / infix（`:` 終わりの演算子は右結合で、レシーバは右オペランド。`1 :: Nil` → `Nil.::(1)`）
- リテラル、タプル
- 名前付き型・ジェネリック型（`Array[String]`、`def id[T](x: T): T` など）
- 存在型のよくある形: `List[_]`、`T forSome { type X }`、`List[_]` を取るメソッド、境界付き `List[_ <: AnyRef]` と `List[X] forSome { type X <: AnyRef }`（名前付き量化は `BoundedWildcard` に落として既存の pickle/erase 経路を使う）。ワイルドカードは Object 相当に erase する。`forSome { val … }` や入れ子の packed existentials は診断する（黙って捨てない）
- compiled class/object に **ScalaSignature**（クラス属性 `ScalaSig` マーカー + `RuntimeVisibleAnnotations` の pickle subset）。`javap -v` で見える。自前 unpickler が読める範囲で `-cp` による別コンパイルができる。nsc 完全 pickle ではないが、ワイヤ形式は nsc と同じ（nentries、tag/len、ビッグエンディアン Nat、SID-10 は `0x7f→0`）。`val` / パラメータ付き `def` / 型パラメータ `id[T]` / `case class` の `new` と ctor フィールド / **companion apply `Point(3, 4)`（term `Point` / `MODULE$`）** / **extractor `unapply`（`p match { case Point(a, b) => … }`）** / object の `def` / **`List[_]`（EXISTENTIALtpe）** / **`List[_ <: AnyRef]`（量化 TYPEsym の hi bound）** / **`@deprecated("msg", "2.13.0")`（SYMANNOT + LITERALstring）** / **`this.type`（THIStpe をメソッド結果に）** / **`Int @unchecked`（ANNOTATEDtpe）** は scalac 2.13.16 が読める形（object は CLASSsym+MODULE + MODULESYM、クラス pickle にも companion の MODULESYM を載せる、`<empty>` / scala / java.lang の EXTMODCLASSref、POLYtpe は restpe 先行、val は NullaryMethodType ゲッター、case class は CASE / CASEACCESSOR、ユーザー型は `<empty>` 所有の EXTREF、`Option` / `TupleN` / `List` は scala / `scala.collection.immutable` モジュール所有の TypeRef + 型引数、Flags は nsc raw long を `rawToPickledFlags` して出す）。full pickle とは主張しない。残る穴は README Remaining
- `s"..."` / `f"..."` / `raw"..."` 文字列補間。`f"$n%02d"` は `String.format` に落とす。`raw` はエスケープを解釈しない。日付時刻（`%t`/`%T`）、引数インデックス、相対 `% <` は診断する。`--scala-library` 時はカスタム interpolator（`implicit class Q(sc: StringContext) { def q(args: Any*) }` の `q"a$x"`）を `StringContext.apply(parts*).q(args*)` へデシュガーして実行する。私有ランタイムでは `s`/`f`/`raw` 以外は診断する
- コンテキストバウンド `T: ClassTag`（メソッド型パラメータ）。nsc と同様、implicit evidence `ClassTag[T]` へデシュガーする。クラス型パラメータの `: C` は診断する。`--scala-library` 時は `implicitly[ClassTag[Int]]` と `new Array[T]`（ClassTag が必要な生成）が jar の `scala.reflect.ClassTag` にリンクして動く
- `lazy val`
- implicit val / def（ローカル、import、パッケージオブジェクト、コンパニオン）、implicit パラメータ、スコープ内の implicit conversion。第二パラメータ節の明示渡し `foo(x)(y)` を含む。候補が複数あるときは nsc 風の **more-specific**（結果型の subtype、または定義クラスが subclass である origin）。型と origin が食い違うと（親のより specific な implicit と、子に定義した less-specific な local）`ambiguous implicit`。同じ型が二つなら曖昧。目標型が `A => B` で `A <: B` のときは nsc と同様 identity view を合成する（view bound の呼び出し側）
- `@tailrec`（末尾再帰でない `def` は nsc 風にエラー。object の末尾再帰は通して実行する。while 変換はしない）/ `@deprecated`（引数付きアノテーションを pickle の SYMANNOT に載せる。コンパイルは壊さない）/ Java `@Override`（本当に override しているメソッドは受理。そうでなければ `overrides nothing`）/ Java `@Deprecated`（メソッドの `RuntimeVisibleAnnotations` に `Ljava/lang/Deprecated;` を出す。`javap -v` で見える。pickle の JAVA フラグは出さない）。未対応アノテーションは診断する
- 非ローカル `return`（ネストしたラムダ / `foreach` から囲みの名前付きメソッドへ。nsc 風 `scala.runtime.NonLocalReturnControl`）。ネストした `def` の `return` はその def 自身。クラスコンストラクタからの `return` は `return outside method definition`
- `eq` / `ne`（AnyRef の参照等価）と `synchronized`（monitorenter / monitorexit。本体はロック中に評価）
- `--scala-library` 時の `Array(1, 2, 3)` / `arr(0)` / `arr.length` / `arr.update`（jar の `scala.Array$` + `ClassTag`。私有ランタイムでは companion apply は無い）
- オーバーロード: 同じ名前の `def` を引数型と arity で nsc 風に選ぶ（より specific なパラメータ型が勝つ）。曖昧なら `ambiguous overload`、該当なしなら `no matching overload`
- `{ case … }` を `PartialFunction[A,B]` 期待位置で匿名クラスにする。`isDefinedAt` / `apply` / `applyOrElse` が動く。`--scala-library` 時は `List.collect`
- `private[this]` と `protected[C]`（`protected[pkg]` も同じ資格）を typer で enforce。`private[this]` は `this` プレフィックス以外（他インスタンス）を拒否。`protected[C]` は C の内部とサブクラスからの `this` を許可
- ネストした `def` の **lambda-lift**（ローカルを捕獲する合成メソッド。値として使う / ラムダから再帰呼び出しするケースが動く）
- デフォルト引数、by-name パラメータ（`=> T`）。デフォルトは scalac と同じ `{method}$default$n` ゲッター（1 始まり、先行パラメータを取る）として classfile に出る。呼び出し側は AST をインラインせずそのゲッターを呼ぶので、別コンパイルしたコードからも使える
- view bounds `T <% Ordered[T]` / `T <% Ordered[Int]`（メソッド型パラメータ）。nsc と同様、implicit evidence `T => V` へデシュガーする。クラス型パラメータや高階型パラメータの `<%` は診断する
- 名前付き引数（呼び出し側で並べ替え）
- 具象メンバー付き trait の mixin（`T$class` 静的実装 + 線形化順のフォワーダ）
- 内部クラス（`$outer`）とネストした object。匿名クラス `new Trait { def f = ... }` と `new { def x = 1 }`（合成 classfile。型は refinement ではなく `$anon$N`）
- eta-expansion `foo _` と、FunctionN が期待される位置への未適用メソッド（`xs.map(inc)`）。ネストしたパラメータリストは **uncurry** で 1 リスト + クロージャになる
- `super` / 修飾付き `this`（`Outer.this`）。trait の `super` は、具象クラスなら `T$class`、スタック可能な `abstract override` なら `T$$super$m` 経由
- `sealed` 階層の match 網羅検査（不足は **warning**。`-Xfatal-warnings` でエラー）
- extractor の `unapply`（`Option` / `Boolean` / `Tuple2`）と `unapplySeq`（`List` と可変長 `_*`）。名前付き extractor 引数（`Point(y = b, x = a)`）
- `AnyVal` 値クラス（1 引数。生成は underlying へ erase。メソッドは `name$extension`）
- Predef の一部: `assert` / `require` / `???` / ArrowAssoc の `->` / `identity` / `locally` / `implicitly` / `any2stringadd`（`1 + "x"`）/ String の `length`・`toInt`（`toLong` / `toDouble` もある）。**`--scala-library`** 時はこれらを jar の `Predef$` / `StringOps` / `Predef$ArrowAssoc` / `Predef$any2stringadd` にリンクする。さらに `intWrapper` / `RichInt`（`abs` / `max` / `to` / `until`）、`longWrapper` / `RichLong`、`doubleWrapper` / `RichDouble`、`floatWrapper` / `RichFloat`、`charWrapper` / `RichChar`、`StringOps` の `*` / `take` / `drop` / `isEmpty` / `toUpperCase` / `toLowerCase` / `stripPrefix` / `split`、`Map` / `Vector` / `List` / `Set` / `Seq` / `LazyList` の varargs `apply`、`Either`（`Left` / `Right`）、`Try` / `Success` / `Failure`（`Try(1)` / `map` / `getOrElse`）も jar リンク時のみ
- 具象 `val` 付き trait の初期化（`T$class.$init$`）と `abstract override` の super 連鎖
- 抽象型メンバーと型射影: `trait Foo { type A; def x: A }`、`type A = Int`、メソッド署名の `Bar#A`
- パス依存型: 安定パス `c.A`（`c: Foo { type A = Int }` や object / `this` / `val`）。`var` や `def` など不安定パスは nsc と同じ `stable identifier required, but … found`
- singleton / this-types: 安定パスの `x.type` と `this.type` を戻り型として型付け・実行する。不安定な `x.type`（`var` / `def` / `new C()`）は `stable identifier required` で診断する
- compound types: `A with B` を値 / パラメータの型として使い、両側のメンバーを呼ぶ。クラスが二つある違法 compound（`A with B` で両方 class）は `illegal inheritance` で診断する
- 構造的 refinement: `{ def foo: Int }` / `T { def foo: Int }`。実行時は **Java reflection**（`getClass` / `Class.getMethod` / `Method.invoke` + unbox）。2.13 の reflective call と同じ実行意味論のサブセット。`scala.language.reflectiveCalls` は要求しない。構造的代入 / refinement の `var` / 本体付き `def` は診断する
- self type: `trait T { self: Foo => ... }` の typecheck と mixin。実装クラスが self type に適合しないと `illegal inheritance`
- 変性: `class C[+A]` / `class Box[+A](val x: A)` は合法。`class Bad[+A](var x: A)` は nsc と同様 covariant-in-contravariant で拒否

フィクスチャで実際に動く範囲は README 末尾の表を見てください。

### Uncurry / Erasure

パイプラインは次のとおりです。

```
parse → namer → typer → uncurry → lambda-lift → erasure → emit
```

uncurry は nsc と同様、typer と erasure のあいだの独立パスです。ネストしたパラメータリストを 1 リストにまとめ、ネストした `Apply` を 1 回の呼び出しにします。部分適用と eta-expansion（`foo _`、FunctionN 期待位置の未適用メソッド）は `FunctionN` クロージャになります。

lambda-lift は uncurry のあと、erasure の前です。メソッド本体のネストした `def` を囲みクラスの合成メソッドに上げ、捕獲したローカルを先頭パラメータとして渡します。ネスト def を値として eta したときや、ラムダから再帰呼び出しするときも、実際に classfile に出て実行されます。

erasure は型引数を落とし、型パラメータと unbounded ワイルドカードを `Object` にし、プリミティブと `Object` の境に box / unbox を挿入します。by-name は `Function0` に下げます。バックエンドの ad-hoc な推測だけには頼っていません。

### Implicit 解決（第一カット）

nsc に寄せた探索順です。偽の「何でも変換」はありません。

1. 現在のスコープと、囲んでいるクラス / object の `implicit` メンバー（親 class / trait から inherited したメンバーと、`import Foo._` で入れたメンバーを含む）
2. 囲んでいるパッケージのパッケージオブジェクト（`package object p` の implicit メンバー）
3. 目標型の部分（型コンストラクタ・型引数・ネストした prefix）のコンパニオン（`Option[T]` なら `Option`、`Outer.Inner` なら `Inner`）。変換なら元の型の部分も見る

呼び出し側で implicit パラメータ節を明示できます: `add(5)(3)` / `foo(x)(ev)`。探索で埋めるのは、その節が省略されたときだけです。

数値の widening（`Int` → `Long` / `Double` など）は **implicit 探索の前** に特別扱いします。scalac の implicit ではなく、typer の組み込みです。

失敗はスタブせず、診断を出します。

- `no implicit: could not find implicit value of type …`
- `ambiguous implicit: …`

同じ目標型に対して subtype 関係にある implicit が二つあるとき（`A` と `B extends A` を両方 `A` として探す）、より specific な `B` が勝ちます。同じ型が二つなら、これまでどおり曖昧です。定義クラスの origin も nsc と同じで、子クラスに定義した implicit は親の implicit より origin が specific です。親の more-specific な implicit と、子の less-specific な local が両方マッチすると、型と origin が食い違って **ambiguous** です。逆（親が less-specific、子が more-specific）は子が勝ちます。

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

次は実装していません。スタブで「動いたことにする」こともしていません。言語側の残りとライブラリ側の残りを分けます。

言語:

- マクロ
- full nsc pickle（出しているのは TERMname / TYPEname / TYPEsym / CLASSsym / MODULESYM / VALsym / EXTref / EXTMODCLASSref / METHODtpe / POLYtpe / TYPEREFtpe / CLASSINFOtpe / TYPEBOUNDStpe / THIStpe / SINGLEtpe / NOPREFIXtpe / EXISTENTIALtpe / SYMANNOT / LITERALstring / ANNOTATEDtpe / ANNOTINFO のサブセット。ByteCodecs は SID-10。ワイヤ形式は nsc と同じ nentries + ビッグエンディアン Nat。vals は METHOD|STABLE|ACCESSOR ゲッター + NullaryMethodType。case class は CASE + フィールド CASEACCESSOR。Flags は nsc raw long を `rawToPickledFlags`。scalac 2.13.16 が `val` / パラメータ付き `def` / `id[T]` / `new Point` + `p.x` / companion apply `Point(...)` / term `Point` / extractor `unapply` / object の `def` / `def f(xs: List[_]): Int` / `@deprecated("msg", "2.13.0") def g` / `def me: this.type` / `def f(xs: List[_ <: AnyRef])` / `def h(x: Int @unchecked)` を typecheck できる範囲。full pickle ではない。残る穴は Remaining）

対象外（診断する / パースしない）:

- コンパイラプラグイン
- Scala 3 構文 / TASTy / XML リテラル
- `forSome { val x: T }`、入れ子の packed existentials。よくある unbounded `List[_]` / `T forSome { type X }` と境界付き `List[_ <: AnyRef]` / `List[X] forSome { type X <: AnyRef }` は実装済み
- クラス / 高階型パラメータの view bounds（メソッドの `T <% Ordered[T]` は実装済み）

ライブラリ:

- 完全な Scala 標準ライブラリ。`--scala-library` なしでは Option / List / FunctionN / Tuple2 は私有ランタイム。**jar にリンクしても** 完全な StringOps / 全 numeric enrichment（`RichByte` など）/ 任意の `IndexedSeq` / `Queue` ファクトリなどは未対応
- implicit の `scala.Int` コンパニオンの enrichment（jar の `intWrapper` 経由の一部はリンク済み）

パーサは未対応構文を黙って捨てず、診断と `Unimplemented` ノードを出します。

## アーキテクチャ

Cargo workspace のクレート:

| crate | 役割 |
| --- | --- |
| `scala-rs-span` | ソース位置と診断 |
| `scala-rs-lexer` | 字句解析（セミコロン推論用の改行トークン、`s`/`f`/`raw"..."` のモードスタック） |
| `scala-rs-parser` | 再帰下降パーサ。AST は nsc の `Tree` に近い |
| `scala-rs-typer` | namer + typer + uncurry + lambda-lift + erasure。implicit 探索を含む |
| `scala-rs-backend` | JVM classfile 出力（major 52 / StackMapTable）と scala-rs ランタイム |
| `scala-rs-driver` | パイプライン駆動 |
| `scala-rs-cli` | コマンドライン。バイナリ名 `scala-rs` |

## scalac 2.13 との比較

正直な差分です。

- **規模**: nsc のごく一部。言語仕様を満たしません。
- **ライブラリ**: デフォルトの **`compile` / `run`** は jar が自動検出できればリンクし、同名の私有 classfile は出さない。見つからなければ私有ランタイム。`--scala-library`（パス省略時は `SCALA_LIBRARY_JAR` / `/tmp/scala-rs-lib` / cwd を探索）で明示できる。**`--no-scala-library` は私有を強制**する。jar に乗るもの: `Option` / `Some` / `None` / `List` / `Nil` / `::` / `Function0` / `Function1` / `Tuple2` / `NotImplementedError` / `Predef$`（`println` / `assert` / `require` / `???` / `identity` / `locally` / `implicitly`）/ `any2stringadd` / `ArrowAssoc` の `->` / `intWrapper` / `RichInt`（`abs` / `max` / `min` / `to` / `until`）/ `longWrapper` / `RichLong`（`abs` / `max` / `min`）/ `doubleWrapper` / `RichDouble`（`abs` / `max` / `min`）/ `floatWrapper` / `RichFloat`（`abs` / `max` / `min`）/ `charWrapper` / `RichChar`（`isDigit` / `toInt` via `intValue$extension`）/ `StringOps`（`toInt$extension` / `size$extension` / `$times$extension` / `take$extension` / `drop$extension` / `isEmpty` via `augmentString` / `toUpperCase`/`toLowerCase` inlined to `String` / `stripPrefix$extension` / `split$extension`）/ `WithFilter` / `Iterator` / `Map` / `Vector` / `Set` / `Seq` / `LazyList`（`empty` / `foreach` / **varargs `apply`**）/ `Either`（`Left` / `Right` / `isLeft` / `getOrElse` / `map`）/ `Try`（`Try$` / `Success` / `Failure` の `apply` / `map` / `getOrElse`）/ `Array$`（varargs `apply` + `ClassTag`）。dual-run: `hello` / `option_for` / `list_for` / `predef` / `predef_more` / `unapply` / `unapply_seq` / `iterator` / `map` / `vector` / `int_ops` / `string_ops` / `list_apply` / `set` / `long_ops` / `seq` / `either` / `float_ops` / `string_ops2` / `anonymous` / `eta` / `try_util` / `existentials` / `existential_bounds` / `implicit_specific` / `lambda_lift` / `view_bounds` / `implicit_inherit_local` / `partial_function` / `list_collect` / `string_interp` / `overloading` / `classtag` / `custom_interp` / `array_ops`。**まだ intrinsic / 私有、または未リンク**: 完全な StringOps（`stripSuffix` / `lines` 等）、残りの numeric wrapper（`RichByte` 等）、`Queue` / `IndexedSeq` などのファクトリ。`List.unapplySeq` は library では `SeqOps` の identity。`List`/`Seq`/`LazyList`/`Array` の varargs `apply` は **library のみ**。
- **object**: scalac と同様、`Main$`（モジュール）と静的フォワーダ `Main` を出します。`java Main` が動くのはそのためです。
- **プリミティブ**: `Int` の `+` などは `scala.Int` のボックスメソッドではなく、JVM 命令（`iadd` など）として出します。
- **trait**: 抽象メンバーだけの trait は JVM interface です。具象メンバーは `T$class` 静的実装と、C3 線形化順のインスタンスフォワーダです。Java 8 default method は使いません。`val` は getter/setter + `$init$` です。`abstract override` は `T$$super$m` です。
- **名前付き引数**: 呼び出し側で `f(b = 2, a = 1)` を並べ替えます。巨大な rewrite フェーズはありません。extractor パターンでも case class なら並べ替えます。
- **try**: Code 属性に例外テーブルと StackMapTable を出します。
- **ラムダ**: `FunctionN` を実装する合成クラス（`Main$$$anonfun$0` など）です。`PartialFunction` 期待位置の `{ case }` は `scala/PartialFunction` を実装し、`isDefinedAt` / `apply` / `applyOrElse` を出します。invokedynamic / LambdaMetaFactory は使いません。
- **フェーズ**: nsc の mixin などの独立パスはありません。**uncurry**、**lambda-lift**（ネスト def）、erasure、ラムダのクロージャ変換はあります。
- **sealed**: 非網羅 match は scalac と同様 warning です。`-Xfatal-warnings` でエラーになります。
- **AnyVal**: scalac は値クラスのクラスファイルと拡張メソッドの両方を出します。scala-rs もクラスは出しますが、呼び出しは `$extension` 静的メソッドで、`new C(x)` は underlying に消えます。
- **Predef / StringOps**: 私有では `assert` / `require` / `???` / `->`（`Tuple2` 直結）/ `identity` / `locally` / `implicitly` / `any2stringadd` と String の `length`/`toInt`/`isEmpty`。library では `Predef$.println/assert/require/???/identity/locally/implicitly`、`any2stringadd.$plus$extension`、`ArrowAssoc.$minus$greater$extension`、`intWrapper` → `RichInt.abs$extension` / `max$extension` / `to$extension`、`longWrapper` → `RichLong.abs$extension` / `max$extension`、`doubleWrapper` → `RichDouble.abs$extension` / `max$extension`、`floatWrapper` → `RichFloat.abs$extension` / `max$extension`、`charWrapper` → `RichChar.isDigit$extension` / `intValue$extension`（`.toInt`）、`augmentString` → `StringOps.toInt$extension` / `size$extension`（`.length`）/ `$times$extension` / `take$extension` / `drop$extension` / `stripPrefix$extension` / `split$extension`（`.isEmpty` / `.toUpperCase` / `.toLowerCase` は StringOps 経由で `String` にインライン）。**`StringOps` / `RichInt` / `RichLong` / `RichDouble` / `RichFloat` / `RichChar` classfile は出していません。**
- **unapplySeq**: `List` とユーザー定義 extractor、`_*`、名前付き case class パターン。library リンク時の `List.unapplySeq` は `SeqOps` 戻り。

scalac の代替ではありません。サブセットの再実装です。

## テスト

```bash
cargo test
```

実行時の期待値は `tests/fixtures/` にあります。各 `.scala` に対して `tests/fixtures/expected/` に同名の `.txt`（`println` と同じ末尾改行付きの stdout）を置いています。`java` がある環境では CLI の e2e が stdout を比較します。

scala-library 2.13.16 が取れる環境では、次を `--scala-library` でコンパイルし、`java -cp out:scala-library.jar Main` でも同じ stdout になることを見ます（私有の `scala/Option.class` / `scala/Predef$.class` 等が無いこと）: `hello` / `option_for` / `list_for` / `predef` / `predef_more` / `unapply` / `unapply_seq` / `iterator` / `map` / `vector` / `int_ops` / `string_ops` / `list_apply` / `set` / `long_ops` / `seq` / `either` / `float_ops` / `string_ops2` / `anonymous` / `eta` / `try_util` / `existentials` / `existential_bounds` / `implicit_specific` / `lambda_lift` / `view_bounds` / `implicit_inherit_local` / `partial_function` / `list_collect` / `string_interp` / `overloading` / `classtag` / `custom_interp` / `array_ops`。`iterator.scala` / `map.scala` / `vector.scala` / `int_ops.scala` / `string_ops.scala` / `list_apply.scala` / `set.scala` / `long_ops.scala` / `seq.scala` / `either.scala` / `float_ops.scala` / `string_ops2.scala` / `try_util.scala` / `list_collect.scala` / `classtag.scala` / `custom_interp.scala` / `array_ops.scala` は library リンク時のみ。フラグなしの `compile` は jar を自動検出してリンクし、`--no-scala-library` は私有ランタイムを出す。

| フィクスチャ | 内容 | 期待 stdout |
| --- | --- | --- |
| `hello.scala` | 挨拶を `println` | `hello, scala-rs` |
| `arithmetic.scala` | `1+2*3` など Int 演算（優先順位込み） | `7` `6` `5` `1` |
| `class_methods.scala` | `Counter` を 10 から 2 回 `inc` | `12` |
| `case_match.scala` | `Point(3,4)` を match | `7` |
| `factorial.scala` | `fact(5)` 再帰 | `120` |
| `trait_impl.scala` | trait 実装の `greet` | `Hello, Scala` |
| `while_loop.scala` | `while (i < 3)` | `3` |
| `do_while.scala` | `do { ... } while (cond)`（少なくとも一度実行） | `3` `6` |
| `eq_sync.scala` | `eq` / `ne`（参照等価）と `synchronized`（monitor） | `true` `false` `true` `42` |
| `string_interp.scala` | `s"..."` / `f"$n%02d"` / `raw"a\nb"` | `hello world` `07` `a\nb` |
| `overloading.scala` | `f(Int)`/`f(String)` と arity-1/2 | `int` `str` `1` `2` |
| `list_for.scala` | `1 :: 2 :: 3 :: Nil` の for-yield / guard | `2` `3` `4` `20` `30` |
| `option_for.scala` | `Some` / `None` の for-comprehension | `4` `true` |
| `lazy_val.scala` | `lazy val` の遅延と一度きりの初期化 | `0` `42` `42` `1` |
| `implicits.scala` | implicit パラメータとコンパニオンの conversion | `15` `14` |
| `generic_id.scala` | `def id[T](x: T): T` の erasure | `42` `hi` |
| `defaults.scala` | デフォルト引数（`$default$n` ゲッター経由） | `hi Scala!` `hi Scala?` |
| `byname.scala` | by-name パラメータが二度評価される | `6` `2` |
| `trait_concrete.scala` | 具象メソッド付き trait を class が使う | `from trait` |
| `trait_linearize.scala` | `extends A with B` の線形化（B が勝つ） | `B` |
| `try_catch.scala` | throw / catch / finally | `before` `caught` `finally` |
| `nested_class.scala` | `class Outer { class Inner }` | `inner` |
| `anonymous.scala` | `new Trait { ... }` と `new { def msg }` の匿名クラス | `Hello, Scala` `anon` |
| `eta.scala` | カリー化 `add(x)(y)`、`xs.map(inc)` / `inc _` | `3` `11` `12` `2` `3` `2` `3` |
| `existentials.scala` | `List[_]` / `List[X] forSome { type X }` を取るメソッド | `1` `2` `a` `b` |
| `existential_bounds.scala` | `List[_ <: AnyRef]` を取るメソッド | `a` `b` |
| `this_type.scala` | `this.type` と安定 `c.type` の戻り | `1` `1` |
| `compound.scala` | `A with B` の値 / パラメータから両側のメンバーを呼ぶ | `3` `3` |
| `implicit_specific.scala` | より specific な implicit（`B extends A`）が勝つ | `B` |
| `lambda_lift.scala` | ローカル捕獲のネスト `def`、eta、ラムダからの再帰 | `11` `11` `12` `120` `3` |
| `view_bounds.scala` | `T <% Ordered[T]` と `Box.compare` | `true` `false` |
| `implicit_inherited.scala` | 親 class の implicit val が子 object で勝つ | `15` |
| `implicit_inherit_local.scala` | 親の less-specific と子の more-specific。子が勝つ | `B` |
| `partial_function.scala` | `{ case }` の `PartialFunction`（`isDefinedAt` / `apply` / `applyOrElse`） | `true` `false` `2` `3` `0` |
| `private_this.scala` | `private[this]` を同じインスタンスから読む | `41` `42` |
| `protected_qual.scala` | `protected[C]` を C / サブクラスから読む | `40` `40` `40` |
| `implicit_nested.scala` | nested companion と型コンストラクタ companion の implicit | `ok` `ok` |
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
| `list_apply.scala` | `List(1, 2, 3)` の varargs `apply` / `foreach` / `head`（library dual-run のみ） | `1` `2` `3` `1` |
| `set.scala` | `Set(1, 2, 3)` の `contains` / `foreach`（library dual-run のみ） | `true` `false` `1` `2` `3` |
| `long_ops.scala` | `longWrapper` / `doubleWrapper` / `charWrapper`（library dual-run のみ） | `3` `2` `2.5` `2.5` `true` `false` `65` |
| `seq.scala` | `Seq(1,2,3)` / `LazyList(1,2,3)` の varargs `apply` / `foreach`（library dual-run のみ） | `1` `2` `3` `1` `2` `3` |
| `either.scala` | `Right` / `Left` の `isLeft` / `getOrElse`（library dual-run のみ） | `false` `1` `true` `0` |
| `float_ops.scala` | `floatWrapper` / `RichFloat` の `abs` / `max`（library dual-run のみ） | `2.5` `2.5` |
| `string_ops2.scala` | `augmentString` 経由の `toUpperCase` / `toLowerCase` / `stripPrefix` / `split`（library dual-run のみ） | `HELLO` `hello` `bar` `a` `b` |
| `try_util.scala` | `Try(1)` / `Success` / `Failure` の `map` / `getOrElse`（library dual-run のみ） | `2` `2` `0` |
| `list_collect.scala` | `List.collect` に `PartialFunction`（library dual-run のみ） | `10` `20` |
| `trait_val.scala` | trait `val` の初期化 | `from trait` |
| `abstract_override.scala` | `abstract override` の super 連鎖 | `B-A-base` |
| `predef_more.scala` | `any2stringadd` / `implicitly` / `identity` / `locally` | `1x` `41` `42` `here` |
| `sealed_non_exhaustive.scala` | 非網羅 match（warning。実行は覆っている入力だけ） | `3` |
| `type_member.scala` | 抽象型メンバー `type A`、`type A = Int`、`Bar#A` | `41` `42` |
| `self_type.scala` | `self: Foo =>` の mixin と self type メンバー | `15` |
| `variance.scala` | `class Box[+A](val value: A)` | `42` |
| `path_dependent.scala` | `c: Foo { type A = Int }` の `c.A` / `c.x` | `41` `42` |
| `structural.scala` | `{ def foo: Int }` を Java reflection で呼ぶ | `42` |
| `classtag.scala` | `implicitly[ClassTag[Int]]` と `new Array[T]`（library dual-run のみ） | `int` `2` |
| `custom_interp.scala` | `implicit class` + `q"a$x"`（library dual-run のみ） | `q:ok` |
| `tailrec.scala` | `@tailrec` の末尾再帰が実行される | `15` |
| `deprecated.scala` | `@deprecated` を付けた `def` が動く | `42` |
| `array_ops.scala` | `Array(1,2,3)` / apply / length / update（library dual-run のみ） | `1` `3` `9` `8` |
| `nlreturn.scala` | `foreach` ラムダからの非ローカル `return` とネスト def のローカル `return` | `1` `3` `0` `1` |
| `existential_forsome.scala` | `List[X] forSome { type X <: AnyRef }` | `a` `b` |
| `java_override.scala` | 本当に override する `@Override` | `sub` `base` |
| `java_deprecated.scala` | Java `@Deprecated` を付けた `def` が動く | `42` |

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化は `existential_val.scala`（`forSome { val … }`）で診断します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds は `view_bounds_class.scala` で診断します。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、違法 compound は `compound_bad.scala`、構造的代入は `structural_bad.scala`、self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`、高階 / 境界付き型メンバーは `type_member_hk.scala` / `type_member_bounds.scala` で診断します。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` を pickle から読む）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。

### Remaining

- **macros**（def マクロ。skip のまま）
- **leftover pickle holes**（nsc 完全 pickle ではない）: 非リテラルのアノテーション引数（TREE）、Java アノテーションの pickle（JAVA フラグ。メソッドの classfile `RuntimeVisibleAnnotations` で `@Deprecated` は出す）、入れ子の packed existentials、`forSome { val … }`、refinement クラスの pickle（`A with B` は言語側で実行する。classfile には第一親相当で出す）、定数型、残る Flags ビット（JAVA / MACRO / BRIDGE / VARARGS / late・anti flags など）。このスライスで載るもの: 非ローカル `return`（`NonLocalReturnControl`）、`List[X] forSome { type X <: Bound }`（既存の `BoundedWildcard` / EXISTENTIALtpe 経路）、Java `@Override` 検査、メソッド上の Java `@Deprecated` RVA

## ライセンス

Apache-2.0
