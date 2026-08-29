# scala-rs

Rust で書いた、Scala 2.13（nsc）サブセットのコンパイラです。JVM classfile を出力します。

scalac のソースを移植したものではありません。オリジナルの再実装です。Scala 3 / TASTy は対象外です。

## これは何か

scala-rs は、Scala 2.13 の構文と意味論のごく一部を、Rust から JVM バイトコードへ落とす実験的コンパイラです。

- フロントエンドは nsc の `Tree` に近い AST を持ちます。
- ターゲットは Java 8 相当の classfile（major version 52）です。Code 属性に StackMapTable（full_frame）を出します。
- デフォルトでは scala-library を同梱しません。Option / List / FunctionN は **scala-rs 独自のランタイム classfile**（`scala/Option` など）です。
- `--scala-library [<jar>]`（または `SCALA_LIBRARY_JAR`）を付けると、Option / List / FunctionN / Tuple2 に加え、`Predef$`（`println` / `assert` / `require` / `???` / `identity` / `locally` / `implicitly`）、`any2stringadd`（`1 + "x"`）、`ArrowAssoc` の `->`、`intWrapper` / `RichInt`（`1.abs` / `1.max` / `1.to`）、`longWrapper` / `doubleWrapper` / `charWrapper`（`(-3L).abs` / `1.0.max` / `'9'.isDigit`）、`StringOps`（`augmentString` 経由の `toInt` / `length` / `*` / `take` / `drop` / `isEmpty`）、`WithFilter` / `Iterator`、`Map` / `Vector` / `List` / `Set`（varargs `apply` を含む）、**`scala.jdk.CollectionConverters` の `asScala` / `asJava`** は **scala-library 2.13.16 の ABI** にリンクし、衝突する私有 classfile は出しません。jar パスを省略すると `SCALA_LIBRARY_JAR`、`/tmp/scala-rs-lib`、cwd を探します。**`scala-rs compile` と `scala-rs run` は、jar が自動検出できればそれを既定で使い**、見つからなければ私有ランタイムに落ちます。**`--no-scala-library` は私有ランタイムを強制**します。jar リンク時はさらに **right-biased な `Either`**（`map` / `flatMap` / `fold` / `swap` / `toOption` / `filterOrElse` / `left` の `LeftProjection`）と **`scala.util.Try`**（`recover` / `recoverWith` / `transform` / `toEither` / `withFilter`）も乗り、どちらも `for` 内包表記で使えます。

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
scala-rs compile file.scala -d out/ -Xsource:3
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
- `-Xsource:<version>` — ソースレベル。`2.13`（既定）/ `3` / `3-cross`。`3` 系は **Scala 3 の綴り**（このサブセットでは `A & B` の交差型）を受け付ける。nsc と同じく現行メジャー未満（`-Xsource:2.12` など）はエラー
- `--scala-library <jar>` — scala-library 2.13 にリンク（私有 Option/List を出さない）。環境変数 `SCALA_LIBRARY_JAR` でも可。パス省略時は自動検出。**`compile` / `run` の既定は自動検出できた jar。見つからなければ私有。`--no-scala-library` で私有を強制**
- `-cp` / `--class-path` — 先にコンパイルした classfile を読む（`ScalaSignature` pickle subset と JVM メソッド。vals / パラメータ付き defs / 型パラメータ / `$default$n` ゲッター / case class の ctor フィールドを含む。自前 `-cp` は companion `apply` も読む。nsc は companion apply `Point(...)` / term `Point` / extractor `unapply` / `List[_]` の existentials / `List[_ <: AnyRef]` / `List[_ <: List[_]]` / `@deprecated("msg", "2.13.0")` の annotation args / Java `@Deprecated`（SYMANNOT + `java.lang.Deprecated`） / `this.type` / `Int @unchecked` / refinement `A with B { def f: Int }` も読む）。**Java の `.class`** も同じ `-cp` / jar / jmod / JDK（`java.base.jmod` や `rt.jar`）からオンデマンドで読む（ScalaSignature の無い pickle-less Java は pickle インストーラに載せない。`JAVA` / `protected` / `static` を落とさないため）。prelude に無い JDK クラスのメソッド・フィールド（`java.lang.Math.abs` / `java.util.ArrayList#add`）を解決する。**Signature 属性**があればジェネリックを raw にしない（`ArrayList[String]#get` は `E`＝`String`。無ければ `Object` のまま `String` へは通さない）。**ワイルドカード／型パラメータ境界**（`Class[*]` → `Class[_]`、`Collection<+TT>` → `Collection[_ <: T]`、`<T:Number>` の hi bound）は存在型として残し raw `Object` にしない。`ArrayList[Byte] <: List[_ <: T]` は親ウォークより先にワイルドカードを照合し、継承した `add` は `drop_overridden` する。**静的 inner**（`java.util.Map.Entry` / `AbstractMap.SimpleEntry`）と **Java varargs**（`ACC_VARARGS` の `String.format` / `Arrays.asList`。Scala `Seq` wrap ではなく `Object[]`）も classfile から読む。Java の `throws` 検査例外は Scala と同様チェックしない。**Java `protected`** は同じパッケージかサブクラス（nsc / JLS）から見え、それ以外は診断する。Scala の `Base.secretStatic()` は Java クラスの `MODULE$` を出さず `invokestatic` する。ScalaSignature pickle だけに頼らない。**Java enum**（`ACC_ENUM` のクラスと定数。`values` / `valueOf` は classfile の static。非 enum に `values` を合成しない）。未対応の classfile 機能（未知 CP tag、`ACC_MODULE`、壊れた magic）は診断する（黙って成功にしない）

フィクスチャはデフォルトパッケージ（`package` 句なし）なので、`-cp out` の `Main` でそのまま動く想定です。

## 実装している言語サブセット

Scala **2.13** 構文です。Scala 3 の `then`、トップレベル定義、TASTy はありません。エントリポイントは `def main(args: Array[String]): Unit` です。

パースできる（またはデシュガーする）構文:

- packages / imports
- objects / classes / traits / case classes。**補助コンストラクタ** `def this(...) = this(...)`（連鎖の先頭は `this(...)`。`super(...)` や文のあとの `this` は診断）。サブクラスの `extends C(1)` は primary が親 ctor を呼ぶ。内部クラスの `new Inner` は ctor overload 選択後も `$outer` を `<init>` の第一引数に残す。**case class の `copy(...)`**（positional / 一部省略時は自分自身の対応フィールドを default / 名前付き引数。`copy` は namer 時点ではまだ ctor フィールドの型が確定していないため、フィールド型解決後の typer フェーズで `copy` 自身の引数シンボルと `copy$default$N` を作り直す。private ランタイムでも動く）。**コンストラクタの省略可能引数**（`class C(x: Int, y: Int = 5)` の `new C(1)` / `new C(y = 2, x = 1)`）: 末尾を省略した呼び出しへのデフォルト値の充填は、通常の `def` の default getter 経由ではなく（`this` が無い呼び出し元では使えないため）呼び出し側でその場を型付けする素朴なフォールバックのみ実装（先行引数を参照するデフォルトは非対応）。**名前付き引数での並べ替えは `new C(...)` でも動く**（コンストラクタのオーバーロードはパラメータ名で絞ってから型で決める）
- `val` / `var` / `def`（ネストした `def` はパースする）
- パラメータ、ラムダ（型付き / 期待型から推論）、ブロック。**placeholder `_`**（nsc `withPlaceholders`）: `_ + 1` / `_.abs` / `f(_)` / `xs.map(_ + 1)` / Function2 `_ + _` / 入れ子 `_.map(_ + 1)` に加え **typed `_ : T`**（`(_: Int) + 1` / `(_: Int) + (_: Int)` / `(_: Int).abs` / `xs.map((_: Int) + 1)`）。レキサが `_:` を `Ident("_")` にするので、式位置では Underscore と同じ placeholder にする。bare `(_: Int)` は `unbound placeholder parameter`。`xs.map(_ : Int)` は nsc どおり wrap せず map に Int が渡り mismatch。unary / Function2 の既存 wrap は触らない。**メソッド適用のセクション** `f(_, x)` / `f(_, _)` は期待型が無くても呼び先のシグネチャからパラメータ型を取る（nsc と同じ条件で、呼び先が単一の非ジェネリックメソッドのときだけ。`poly(_, 3)` や overload された `"abc".substring(_)` は `missing parameter type for expanded function` のまま）。合成パラメータはソース順で並べる（`two(_, _)` は `(a, b) => two(a, b)`）
- `if` / `else`、`while`、`do { ... } while (cond)`
- `try` / `catch` / `finally`（catch は `{ case ... }`。`try/finally` と `try/catch/finally`。finally は正常終了と例外（catch からの throw 含む）の両方で走る。JVM 例外テーブルを出す。パーサは `finally` を落とさない）
- `match`（コンストラクタパターン、リテラル、ワイルドカード、Java enum 定数の安定識別子 `Thread.State.NEW`）
- for-comprehension（`map` / `flatMap` / `foreach` / `withFilter` へデシュガー。私有ランタイムでは `List.withFilter` は eager な `List`。`--scala-library` 時は `scala.collection.WithFilter`。`Option.withFilter` は `Option$WithFilter`）
- apply / select / infix（`:` 終わりの演算子は右結合で、レシーバは右オペランド。`1 :: Nil` → `Nil.::(1)`）。代入 `xs(i) = v` は nsc どおり `xs.update(i, v)`。代入でない `c(1)` で `apply` が無ければ診断する（黙って `update` にしない）
- リテラル、タプル
- 名前付き型・ジェネリック型（`Array[String]`、`def id[T](x: T): T` など）。infix 型 `A Either B` は `Either[A, B]`。`Map[K, V]` の applied 構文はそのまま。**高階型** `trait Functor[F[_]]` / `class Box[F[_], A](val fa: F[A])`。具象は `Id[_]` など。kind 不一致（`F[_]` を proper 位置で使う、proper 型を型コンストラクタとして使う）は診断する（黙って捨てない）。**高階型メンバー** `trait M { type F[_] }` とパス依存適用 `m.F[Int]`。具象は subclass で `type F[X] = Id[X]`（または `List[X]`）。メンバーの kind 不一致（`type F[_]` を `type F = Int` で束縛、逆も）は診断する。**refinement の高階型メンバー** `M { type F[X] = Id[X] }` と適用。**HK 境界** `type F[_] <: Bound`（proper な境界。`type F[_] <: List` は nsc どおり `takes type parameters`）。**refinement の境界** `{ type A <: Int }`。クラス / トレイトの nullary `type A <: T` は未実装のまま診断する。**入れ子型射影** `Outer#Inner#X` / `Holder#Inner#T`。違法な `Int#X` と抽象 `B#U#T`（メンバー無し）は nsc どおり `is not a member`
- 2.13 の early field defs: `class C extends { val x = 1 } with T`。`x` は親 ctor / trait `$init$` の前にフィールドへ書く（nsc と同じ）。具象フィールド以外（`def` / 文 / 抽象 val）は nsc どおり `only concrete field definitions allowed in early object initialization section`。early 内の `this` は `this can be used only in a class, object, or template`
- SIP-23 定数型のサブセット: `val x: 1 = 1`、`def f(x: 1): Int`。式のリテラルは定数型（`1 <: Int`）。不一致 `val y: 1 = 2` は type mismatch。classfile の pickle は nsc `CONSTANTtpe` + `LITERALint`（scalac 2.13.16 が `-cp` で `def f(x: 1)` / `val one: 1` を typecheck できる）
- `scala.Dynamic`: `d.foo` → `selectDynamic("foo")`、`d.foo(args)` → `applyDynamic("foo")(args)`、`d.foo = v` → `updateDynamic("foo")(v)`、`d.foo(a = x)` → `applyDynamicNamed("foo")(("a", x))`。`import scala.language.dynamics`（または `-language:dynamics`）が必要。`--scala-library` 時は jar の `scala/Dynamic` に対して実行する
- XML リテラルのサブセット（2.13）: `<a>t{e}</a>` / `<a/>` / `<a b={e} c="t"/>` / `<a xmlns:p="u" p:b={e} c="t"/>` / `<p:a xmlns:p="u"/>` / `<p:b xmlns:p="u">t</p:b>` / `<a><!--c--></a>` / `<a><![CDATA[x]]></a>` / `<a><?pi t?></a>` / `<a>&amp;</a>` / `<a>&#65;</a>`（elem / text / splice / 非プレフィックス属性 / `xmlns:p` とプレフィックス属性 `p:b` / プレフィックス付き要素名 / コメント / CDATA / PI / 定義済みエンティティ `&amp;` `&lt;` `&gt;` `&quot;` `&apos;` / 数値 `&#N;` `&#xN;`）。属性は nsc と同じ `UnprefixedAttribute` / `PrefixedAttribute` チェーンと `NamespaceBinding`。プレフィックス付き `Elem` は `prefix` に文字列、`label` にローカル名。コメント / CDATA / PI は `scala.xml.Comment` / `PCData` / `ProcInstr`。定義済みエンティティは `EntityRef`、数値参照は `Text`。レキサは `><!--` を `>` と `<` に分ける。未知のエンティティは診断する。`scala-rs run` は検出できた scala-xml jar を `java -cp` に足す
- `scala.Enumeration`: `object Color extends Enumeration { val Red, Blue = Value }`（複数 `val`）。`--scala-library` 時は jar の `Enumeration.Value()` / `Value.id` / `toString` に対して実行する
- 適合（conformance）まわり: **コレクションの継承関係**（`Vector[A] <: IndexedSeq[A] <: Seq[A] <: collection.Seq[A] <: Iterable[A] <: IterableOnce[A]`、`List` / `LazyList` / `Queue` / `Range` / `ArraySeq`、`Set[A] <: Iterable[A]`、`Map[K, V] <: Iterable[(K, V)]`、mutable 側も同様）を `crates/typer/src/prelude_hier.rs` の 1 枚の表で型引数つきに張る。**アノテーション付き型**は下の型と同じに適合する（`Node` は `Node @uncheckedVariance`）。**モジュールの `.type`** はそのモジュール自身の型（`Some(Nil): Some[Nil.type]`）。反変パラメータを持つクラスの lub はそのパラメータだけ glb を取る（`Act[+R, -E]` の lub は `Act[R lub R2, E glb E2]`）。型パラメータの lub はその上限境界まで辿る。`extends Base[T](y)` の親コンストラクタ引数は`extends` 節が書いた型引数で読む。`type Self >: this.type <: Nd` に対して `this` は適合し（`class Leafy extends Nd { type Self = Leafy }` のように下限の `this.type` をサブクラス側で読み直す）、任意の `Nd` は適合しない
- 言語フラグ `implicitConversions` と `postfixOps` は nsc 2.13 どおり。ユーザー定義の `implicit def` / `implicit class` は import / `-language:implicitConversions` なしだと **warning**。postfix `42 bang` / `42 abs` は `import scala.language.postfixOps`（または `-language:postfixOps`）なしだと **warning**（`-Xfatal-warnings` でエラー）
- 存在型のよくある形: `List[_]`、`T forSome { type X }`、`List[_]` を取るメソッド、境界付き `List[_ <: AnyRef]` と `List[X] forSome { type X <: AnyRef }`（名前付き量化は `BoundedWildcard` に落として既存の pickle/erase 経路を使う）。ワイルドカードは Object 相当に erase する。入れ子の `List[_ <: List[_]]` は hi bound 側の EXISTENTIALtpe として pickle する。`p.Inner forSome { val p: Outer }` は `Outer#Inner` にパックして実行する。その他の `forSome { val … }` は診断する（黙って捨てない）
- compiled class/object に **ScalaSignature**（クラス属性 `ScalaSig` マーカー + `RuntimeVisibleAnnotations` の pickle subset）。`javap -v` で見える。自前 unpickler が読める範囲で `-cp` による別コンパイルができる。nsc 完全 pickle ではないが、ワイヤ形式は nsc と同じ（nentries、tag/len、ビッグエンディアン Nat、SID-10 は `0x7f→0`）。`val` / パラメータ付き `def` / 型パラメータ `id[T]` / `case class` の `new` と ctor フィールド / **companion apply `Point(3, 4)`（term `Point` / `MODULE$`）** / **extractor `unapply`（`p match { case Point(a, b) => … }`）** / object の `def` / **`List[_]`（EXISTENTIALtpe）** / **`List[_ <: AnyRef]`（量化 TYPEsym の hi bound）** / **`@deprecated("msg", "2.13.0")`（SYMANNOT + LITERALstring）** / **Java `@Deprecated`（SYMANNOT + TypeRef(java.lang, Deprecated)。scalac `-deprecation` がメソッド上のアノテーションを見る）** / **`this.type`（THIStpe をメソッド結果に）** / **`Int @unchecked`（ANNOTATEDtpe）** / **`val one: 1` と `def lit(x: 1)`（CONSTANTtpe + LITERALint）** / **`List[_ <: List[_]]`（入れ子 EXISTENTIALtpe）** / **`A with B { def f: Int }`（REFINEDtpe）** / **`@Ann(foo)` / `@Ann(c.x)` / `@Ann(3)` / `@Ann(this)` / `@Ann(classOf[Int])` / `@Ann(ident(1))` / `@Ann(this.x)` / `@Ann(super.foo)` / `@Ann(ident(ident(1)))` / `@Ann(foo = 1)`（TREE Ident/Select/This/Super/Apply + リテラル / LITERALclass Constant。ネストした Apply と Ident 以外の Select 修飾子を含む。named `@Ann(foo = 1)` は nsc と同じ位置 Constant）** / **`def join(xs: String*)`（VARARGS + `<repeated>`）** / **`Ordered` erasure bridge（BRIDGE）** / **`type T = Int`（ALIASsym。2.13 に ALIAStpe は無い）** は scalac 2.13.16 が読める形（object は CLASSsym+MODULE + MODULESYM、クラス pickle にも companion の MODULESYM を載せる、`<empty>` / scala / java.lang の EXTMODCLASSref、POLYtpe は restpe 先行、val は NullaryMethodType ゲッター、case class は CASE / CASEACCESSOR、ユーザー型は `<empty>` 所有の EXTREF、`Option` / `TupleN` / `List` は scala / `scala.collection.immutable` モジュール所有の TypeRef + 型引数、Flags は nsc raw long を `rawToPickledFlags` して出す）。full pickle とは主張しない。残る穴は README Remaining
- `s"..."` / `f"..."` / `raw"..."` 文字列補間。`f"$n%02d"` は `String.format` に落とす。`raw` はエスケープを解釈しない。日付時刻（`%t`/`%T`）、引数インデックス、相対 `% <` は診断する。`--scala-library` 時はカスタム interpolator（`implicit class Q(sc: StringContext) { def q(args: Any*) }` の `q"a$x"`）を `StringContext.apply(parts*).q(args*)` へデシュガーして実行する。私有ランタイムでは `s`/`f`/`raw` 以外は診断する
- コンテキストバウンド `T: ClassTag` / `T: Ordering` / `T: scala.reflect.ClassTag`（メソッド型パラメータ）と **クラス型パラメータ** `class C[T: Ordering](x: T)`。nsc と同様、implicit evidence `C[T]` へデシュガーする（クラスは primary ctor の extra implicit 節）。トレイトの `: C` / `<%` は nsc どおり `traits cannot have type parameters with context bounds ': ...' nor view bounds '<% ...'`。evidence が無ければ `no implicit`。`--scala-library` 時は jar の `scala.math.Ordering` を classfile から読み、companion の `implicit object Int`（`Ordering$Int$.MODULE$` / InnerClasses）と `ClassTag` にリンクして動く。ジェネリック `Array[T].length` は jar の `ScalaRunTime.array_length` に落とす
- `lazy val`
- implicit val / def（ローカル、import、パッケージオブジェクト、コンパニオン）、implicit パラメータ、スコープ内の implicit conversion。第二パラメータ節の明示渡し `foo(x)(y)` を含む。候補が複数あるときは nsc 風の **more-specific**（結果型の subtype、または定義クラスが subclass である origin）。型と origin が食い違うと（親のより specific な implicit と、子に定義した less-specific な local）`ambiguous implicit`。同じ型が二つなら曖昧。目標型が `A => B` で `A <: B` のときは nsc と同様 identity view を合成する（view bound の呼び出し側）。**implicit class**（object / class 本体。`implicit class Rich(n: Int) { def twice: Int }` の `2.twice`）。**package object の `implicit class`**（同じパッケージの他 compilation unit、または `import pkg._`。pickle の IMPLICIT。トップレベル `implicit class` は nsc どおり `` `implicit` modifier cannot be used for top-level objects ``。import 無しでは enrichment が見えない）
- `@tailrec`（末尾再帰でない `def` は nsc 風にエラー。object の末尾再帰は通して実行する。while 変換はしない）/ `@deprecated`（引数付きアノテーションを pickle の SYMANNOT に載せる。コンパイルは壊さない）/ Java `@Override`（本当に override しているメソッドは受理。そうでなければ `overrides nothing`）/ Java `@Deprecated`（メソッドの `RuntimeVisibleAnnotations` に `Ljava/lang/Deprecated;` を出す。pickle は `SYMANNOT` + `java.lang.Deprecated` の TypeRef。`javap -v` と scalac `-deprecation` の両方で見える）/ ユーザー定義の `StaticAnnotation`（`@Ann(foo)` / `@Ann(this)` / `@Ann(classOf[Int])` / `@Ann(ident(1))` / `@Ann(this.x)` / `@Ann(super.foo)` / `@Ann(ident(ident(1)))` / `@Ann(foo = 1)` / `@Ann(foo = this.x)` / `@Ann(foo = bar)` の Ident/Select/This/Super/Apply / リテラル / classOf / named Constant / named TREE 引数を TREE / Constant として pickle。named は nsc と同じく位置引数に直して pickle）/ `@implicitNotFound("…")`（欠ける implicit は nsc と同じくその文面。`${A}` は型引数）/ `@switch`（`(n: @switch) match`。密な Int は `tableswitch`、疎なら `lookupswitch`。switch にできない match は nsc と同じ warning `could not emit switch for @switch annotated match`）。`@inline` / `@noinline` はアノテーションとして格納するだけで、インライン化はしない。実 scalac 2.13.16 は配置を一切検証しない（val / var / class / type などどれに付けても、両方同時に付けても、警告すら出さない — `-opt:inline:...` のバイトコード最適化器だけが読む情報で、typer は無関係）ので、こちらも同様に検証しない。`@volatile` / `@transient` は classfile の `ACC_VOLATILE` / `ACC_TRANSIENT`（javap で見える）。`@native` はメソッドに付けて `ACC_NATIVE` を出し、本文は付けない（`.so` のリンクはしない。本文付きや val への付与は診断する）
- 非ローカル `return`（ネストしたラムダ / `foreach` から囲みの名前付きメソッドへ。nsc 風 `scala.runtime.NonLocalReturnControl`）。ネストした `def` の `return` はその def 自身。クラスコンストラクタからの `return` は `return outside method definition`
- `eq` / `ne`（AnyRef の参照等価）と `synchronized`（monitorenter / monitorexit。本体はロック中に評価）
- `asInstanceOf[T]` / `isInstanceOf[T]`（`Any` の真にジェネリックなメソッドとして型パラメータ `T0` を持つ。プリミティブは box の `checkcast` + unbox 呼び出し、`String` / クラス型は `checkcast`、erase された/境界なしの対象（`Any` / `AnyRef` / 型パラメータ）はキャスト不要。`x.asInstanceOf[T]` は `TypeApply` の外側ノードでのみ代入された具体的な `T` を持つため、erasure フェーズと backend の両方でこの外側ノードから読む必要がある）。`null` は `AnyRef`／`Any` としてメンバー解決する（`null.asInstanceOf[String]` が動く）。境界なしの型パラメータ `T` も `Any` としてメンバー解決する（`x: T` の `x.asInstanceOf[AnyRef]` が動く）
- `scala.Int` / `Long` / `Short` / `Byte` / `Char` / `Double` / `Float` の companion 定数（`Int.MaxValue` / `MinValue`、`Double.NaN` / `PositiveInfinity` / `NegativeInfinity` / `MinPositiveValue` など）。実体は companion object の nullary メソッド（`scala/Int$.MODULE$.MaxValue()` 等）で、`--scala-library` 時のみ（本物の jar が要る。私有ランタイムでは診断する）
- `java.lang.Throwable` / `Exception` / `RuntimeException` のコンストラクタ（`()` / `(String)` / `(String, Throwable)` / `(Throwable)`）と `getMessage` / `getLocalizedMessage` / `getCause` / `initCause` / `printStackTrace`。以前は 0 引数 ctor だけが（引数無しだったので偶然）"動いていた"。本物の `java.lang.*` なので `--no-scala-library` でも使える
- `--scala-library` 時の `Array(1, 2, 3)` / `arr(0)` / `arr.length` / `arr.update`（jar の `scala.Array$` + `ClassTag`。私有ランタイムでは companion apply は無い）
- オーバーロード: 同じ名前の `def` を引数型と arity で nsc 風に選ぶ（より specific なパラメータ型が勝つ）。曖昧なら `ambiguous overload`、該当なしなら `no matching overload`。**値の位置では引数を取らない候補だけを残す**（SLS 6.26.3）。`val` はそもそもメソッド型ではないので、`object Library { val == = new SqlOperator("=") }` は継承した `Any.==(x: Any)` と曖昧にならず値として読める（`case Library.==(a, b)` の抽出子もこれで見つかる）。同型の候補が継承経路の重複で二重に現れたものは 1 つのメンバとして扱う
- コンパニオンの `apply` / `unapply`: case class の合成メンバだけがコンストラクタのシグネチャで埋められる。**普通のクラスのコンパニオンが自分で書いた `apply`（デフォルト引数つき、可変長引数のあとに implicit 節が続くもの、を含む）はそのまま残る**
- 可変長引数の**あとに続く節**（`def f(ch: Node*)(implicit t: TypedType[T])`）: `f()` のように可変長引数を 0 個で呼んでも、後続の implicit 節はきちんと埋まる
- `{ case … }` を `PartialFunction[A,B]` 期待位置で匿名クラスにする。`isDefinedAt` / `apply` / `applyOrElse` が動く。`--scala-library` 時は `List.collect`
- **パターンマッチ無名関数**（nsc の "pattern-matching anonymous function"）: `xs.map { case (s, t) => … }` / `xs.collect { case … }` / `catch { case … }` は期待型 `A => B` / `PartialFunction[A, B]` からスクルチニ型 `A` をパターンへ渡し、結果型は各 case 本体の lub にする。呼び先の結果型パラメータ（`map` の `B`）がまだ決まっていないときに body を `Any` で型付けして `List[Any]` に潰していたのを直した。`if` / `match` も、期待型が `Any` や未確定の型パラメータのときは枝の lub を採る。コンストラクタパターンはスクルチニの型引数を伝播する（`Box[Int]` に対する `case Box(v)` は `v: Int`。型パラメータへ erase されたフィールドは unbox / checkcast する）
- `private[this]` と `protected[C]`（`protected[pkg]` も同じ資格）を typer で enforce。`private[this]` は `this` プレフィックス以外（他インスタンス）を拒否。`protected[C]` は C の内部とサブクラスからの `this` を許可
- ネストした `def` の **lambda-lift**（ローカルを捕獲する合成メソッド。値として使う / ラムダから再帰呼び出しするケースが動く）
- デフォルト引数、by-name パラメータ（`=> T`）。デフォルトは scalac と同じ `{method}$default$n` ゲッター（1 始まり、先行パラメータを取る）として classfile に出る。呼び出し側は AST をインラインせずそのゲッターを呼ぶので、別コンパイルしたコードからも使える
- **メソッド型パラメータの境界**。**下限境界 `[B >: A]`**: `def ::[B >: A](elem: B): List[B]` のように、引数から推論した `B` と、レシーバから見た `A` の実際の型との **lub** を取る。`Circle(1) :: Rect(2, 3) :: Nil` は `List[Circle]` ではなく `List[Shape]`（`SymbolTable::lub` が親を辿って共通の基底型を求める）。ユーザー定義の `class Box[A] { def widen[B >: A](other: B): Box[B] }` も同じ経路で推論する。可変長引数も同様に全引数の lub を取るので、`--scala-library` 時の `List(Circle(1), Rect(2, 3))` は `List[Shape]`。**上限境界 `[A <: Named]`**: 推論した型引数と明示した型引数の両方を検査し、nsc と同じ文面 `inferred type arguments [Int] do not conform to method f's type parameter bounds [A <: Named]`（明示時は `type arguments [Int] do not conform to …`）で診断する。`[A <: Named]` の値は `Named` を期待する位置で使える
- view bounds `T <% Ordered[T]` / `T <% Ordered[Int]`（メソッド型パラメータ）と **クラス型パラメータ** `class C[A <% Ordered[A]](x: A)`。nsc と同様、implicit evidence `T => V`（クラスは primary ctor の extra implicit 節）へデシュガーする。evidence が無ければ `no implicit`。高階型パラメータの `F[_] <% V` は scalac 2.13.16 が全スペルを拒否する（`type F takes type parameters`）。同じ診断。Scala 3 的なエンコーディングはしない
- `extends App` / `DelayedInit`。`object Main extends App { println(...) }` は nsc どおりコンストラクタ本体を `delayedInit` に移し、`App.main` から起動する。App なしで `DelayedInit` を継承する class も `delayedInit` フックを呼ぶ
- **名前付き引数**（呼び出し側で並べ替え）。メソッド / コンパニオンの `apply` / case class の `copy` / **コンストラクタ `new C(b = 2, a = 1)`** / **オーバーロードのある呼び出し**、および可変長引数 （`def f(a: Int, rest: Int*)` の `f(a = 1)` / `f(a = 1, 2, 3)`）で動く。並べ替えは nsc の `NamesDefaults.removeNames` と同じ規則で、**自分の位置にある名前付き引数はそのあとの位置引数を許す**（`f(a = 1, 2)` は通り、`f(b = 1, 2)` は `positional after named argument.`）。オーバーロードは nsc と同じくまずパラメータ**名**で候補を絞り、名前だけで決まらないときに引数の型で決める。診断は実 scalac と同じ文面（`unknown parameter name: q` / `parameter 'c' is already specified at parameter position 2` / `positional after named argument.`）で、nsc と同様に 1 呼び出しにつき 1 件だけ出す（後続の「引数が足りない」等はカスケードなので出さない）
- 具象メンバー付き trait の mixin（`T$class` 静的実装 + 線形化順のフォワーダ）。フォワーダは `class` と `object` の両方に出す。trait の `val` / `override val` / `var` の実行時表現は「Trait mixin」節
- **case class / case object の合成メンバー**: case class は `toString` / `equals` / `hashCode` / `canEqual` / `productPrefix` / `productArity`。**case object** は module class 側に nsc と同じ定数畳み込みの `toString`（`Foo`。`Foo$@1a2b3c` ではない）/ `productPrefix` / `hashCode`（`"Foo".hashCode`）/ `productArity`（0）/ `canEqual` を出す。`equals` は nsc と同じくシングルトンの参照等価（`Object` 由来）のまま。手書きの定義があればそちらが勝つ。`scala.Product` を親に付けるところまでは実装していないので、`productElement` / `productIterator` はまだ無い
- **`val` への再代入の診断**（`val x = 1; x = 2` も `d.v = 5`（trait の `val`）も nsc と同じ `reassignment to val`）。Java のフィールドとコンパイラ生成の synthetic な項は対象外
- 内部クラス（`$outer`）とネストした object。匿名クラス `new Trait { def f = ... }` と `new { def x = 1 }`（合成 classfile。型は refinement ではなく `$anon$N`）
- メソッド本体の中で定義したクラス（匿名クラス `new T { … }` と**ローカル `class` / `object`**）が、**囲みメソッドのパラメータ / ローカルをキャプチャ**する。nsc と同じ形で、自由変数ごとに `x$1` という public final フィールドと、末尾に付く追加のコンストラクタ引数を出す。各インスタンスメソッドの先頭でそのフィールドをローカルスロットに読み戻すので、キャプチャした `var` の `scala.runtime.*Ref` 経由の読み書きも、匿名クラス内のラムダによる二重キャプチャ（`$captured$N`）も、既存の経路のまま動く。メソッドの中のクラスにも `$outer` が付き、囲みクラスのメンバは `$outer` チェーンで読む
- eta-expansion `foo _` と、FunctionN が期待される位置への未適用メソッド（`xs.map(inc)`）。ネストしたパラメータリストは **uncurry** で 1 リスト + クロージャになる。SIP-21 の SAM: ラムダ / 未適用メソッドを `Runnable` / `java.util.Comparator[Int]` / `java.util.function.Function[A,B]`（単一抽象メソッド）に適合。SAM でない型へは type mismatch（黙ってラップしない）。`def go(): Unit` を `_` なしで `Runnable` に渡すのは nsc と同じく auto-apply して mismatch。合成クラスは既存の anonfun と同じく invokedynamic は使わない
- `super` / 修飾付き `this`（`Outer.this`）。trait の `super` は、具象クラスなら `T$class`、スタック可能な `abstract override` なら `T$$super$m` 経由
- `sealed` 階層の match 網羅検査（不足は **warning**。`-Xfatal-warnings` でエラー）
- extractor の `unapply`（`Option` / `Boolean` / `Tuple2`）と `unapplySeq`（`List` と可変長 `_*`）。名前付き extractor 引数（`Point(y = b, x = a)`）
- `AnyVal` 値クラス（1 引数。生成は underlying へ erase。メソッドは `name$extension`）。`extends Any` した universal trait を mix-in でき、参照が要る位置（`Any` / その trait / 型引数 / 配列要素）では `new C(u)` で box する。パターンマッチ（`case x: C`）と `classOf[C]` / `asInstanceOf[C]` は box したクラスを見る。`equals` / `hashCode` は underlying から合成する（nsc の `equals$extension` / `hashCode$extension` 相当）
- Predef の一部: `assert` / `require` / `???` / ArrowAssoc の `->` / `identity` / `locally` / `implicitly` / `any2stringadd`（`1 + "x"`）/ String の `length`・`toInt`（`toLong` / `toDouble` もある）。**`--scala-library`** 時はこれらを jar の `Predef$` / `StringOps` / `Predef$ArrowAssoc` / `Predef$any2stringadd` にリンクする。さらに `intWrapper` / `RichInt`（`abs` / `max` / `to` / `until`）、`longWrapper` / `RichLong`、`doubleWrapper` / `RichDouble`、`floatWrapper` / `RichFloat`、`charWrapper` / `RichChar`、`StringOps` の `*` / `take` / `drop` / `isEmpty` / `toUpperCase` / `toLowerCase` / `stripPrefix` / `split`、`Map` / `Vector` / `List` / `Set` / `Seq` / `LazyList` の varargs `apply`、**`Either`**（`Left` / `Right` / `isLeft` / `isRight` / `map` / `flatMap` / `fold` / `getOrElse` / `orElse` / `swap` / `toOption` / `toSeq` / `contains` / `exists` / `forall` / `foreach` / `filterOrElse`、および `left` が返す `LeftProjection` の `e` / `get` / `getOrElse` / `map` / `flatMap` / `foreach` / `exists` / `forall` / `toOption` / `toSeq` / `filterToOption`）、**`Try` / `Success` / `Failure`**（`Try(1)` / `isSuccess` / `isFailure` / `get` / `getOrElse` / `map` / `flatMap` / `filter` / `withFilter` / `foreach` / `orElse` / `recover` / `recoverWith` / `collect` / `toOption` / `toEither` / `failed` / `transform` / `fold`）も jar リンク時のみ。`Option` の `toList` / `toRight` / `toLeft` / `zip` / `collect` / `flatten` も jar リンク時のみ（`getOrElse` / `isDefined` / `nonEmpty` / `contains` / `exists` / `forall` / `filter` / `filterNot` / `orElse` / `fold` は私有ランタイムでも動く）。このスライスでは **ArrayOps の残り**（`lengthIs` / `sizeIs` / `indexOf` / `copyToArray` / `iterator`。`zipWithIndex`/`knownSize`/`sizeCompare`/`filterNot`/`headOption`/`lastOption`/`partition`/`splitAt`/`span`/`find`/`contains`/`distinct` とそれ以前は触らない）、**StringOps の残り**（`++` / `lengthIs` / `sizeIs` / `flatMap`。`iterator`/`sizeCompare`/`knownSize`/`appendedAll`/`prependedAll`/`>`/`>=`/`<=`/`compare`/`patch` とそれ以前は触らない）、**`scala.collection.View`**（`List.view.map.toList`、`View.fill` / `View.iterate`。私有 View classfile は出さない。LazyList/Iterator は View 呼び出しに必要な範囲以外は触らない）を同じ jar にリンクする
- Predef の一部: `assert` / `require` / `???` / ArrowAssoc の `->` / `identity` / `locally` / `implicitly` / `any2stringadd`（`1 + "x"`）/ String の `length`・`toInt`（`toLong` / `toDouble` もある）。**`--scala-library`** 時はこれらを jar の `Predef$` / `StringOps` / `Predef$ArrowAssoc` / `Predef$any2stringadd` にリンクする。さらに `intWrapper` / `RichInt`（`abs` / `max` / `to` / `until`）、`longWrapper` / `RichLong`、`doubleWrapper` / `RichDouble`、`floatWrapper` / `RichFloat`、`charWrapper` / `RichChar`、`StringOps` の `*` / `take` / `drop` / `isEmpty` / `toUpperCase` / `toLowerCase` / `stripPrefix` / `split`、`Map` / `Vector` / `List` / `Set` / `Seq` / `LazyList` の varargs `apply`、`Either`（`Left` / `Right`）、`Try` / `Success` / `Failure`（`Try(1)` / `map` / `getOrElse`）も jar リンク時のみ。このスライスでは **ArrayOps の残り**（`lengthIs` / `sizeIs` / `indexOf` / `copyToArray` / `iterator`。`zipWithIndex`/`knownSize`/`sizeCompare`/`filterNot`/`headOption`/`lastOption`/`partition`/`splitAt`/`span`/`find`/`contains`/`distinct` とそれ以前は触らない）、**StringOps の残り**（`++` / `lengthIs` / `sizeIs` / `flatMap`。`iterator`/`sizeCompare`/`knownSize`/`appendedAll`/`prependedAll`/`>`/`>=`/`<=`/`compare`/`patch` とそれ以前は触らない）、**`scala.collection.View`**（`List.view.map.toList`、`View.fill` / `View.iterate`。私有 View classfile は出さない。LazyList/Iterator は View 呼び出しに必要な範囲以外は触らない）を同じ jar にリンクする
- **`scala.collection.immutable.List` のコアメンバ**。`--scala-library` 時は scala-library 2.13.16 の実シグネチャ（`javap -s` で確認した descriptor）にリンクする。`map` / `flatMap` / `collect` / `zip` / `groupBy` / `sortBy` / `minBy` / `maxBy` / `foldLeft` / `foldRight` / `scanLeft` / `::` / `:::` / `+:` / `:+` / `++` / `:++` / `++:` / `updated` / `distinctBy` / `startsWith` / `endsWith` は**真に多相**（メソッド型パラメータ `B` を持つ）で、`xs.map(x => "n" + x): List[String]` のように要素型が追える。ほかに `filter` / `filterNot` / `take` / `drop` / `takeRight` / `dropRight` / `takeWhile` / `dropWhile` / `slice` / `splitAt` / `span` / `partition` / `reverse` / `distinct` / `init` / `last` / `headOption` / `lastOption` / `size` / `length` / `nonEmpty` / `contains` / `exists` / `forall` / `count` / `find` / `indexOf` / `mkString`（0/1/3 引数）/ `sum` / `product` / `min` / `max` / `reduce` / `reduceLeft` / `reduceRight` / `sorted` / `sortWith` / `zipWithIndex` / `grouped` / `sliding` / `toList` / `toArray` / `toSet` / `toVector` / `toSeq` / `Iterator.toList`。`List` 自身に無いものは `IterableOnceOps` / `IterableOps` / `SeqOps` の default メソッドなので invokeinterface で呼び、`Object` / `LinearSeq` に erase される戻り値は checkcast / unbox する。`sum` / `product` 用に `scala.math.Numeric`（`IntIsIntegral` / `LongIsIntegral` / `DoubleIsFractional`）、`sorted` / `max` / `sortBy` 用に `Ordering` の `String` / `Long` / `Boolean` インスタンスを implicit スコープに足した。**私有ランタイム（`--no-scala-library`）**では `crates/backend/src/runtime.rs` が classfile に実装している分（`length` / `size` / `nonEmpty` / `last` / `reverse` / `filter` / `filterNot` / `contains` / `exists` / `forall` / `count` / `take` / `drop` / `mkString` 0/1/3 引数）だけを宣言し、それ以外は**黙って通さず診断する**（`value sorted is not a member of List[Int]`）
- Predef の一部: `assert` / `require` / `???` / ArrowAssoc の `->` / `identity` / `locally` / `implicitly` / `any2stringadd`（`1 + "x"`）/ String の `length`・`toInt`（`toLong` / `toDouble` もある）。**`--scala-library`** 時はこれらを jar の `Predef$` / `StringOps` / `Predef$ArrowAssoc` / `Predef$any2stringadd` にリンクする。さらに `intWrapper` / `RichInt`（`abs` / `max` / `to` / `until`）、`longWrapper` / `RichLong`、`doubleWrapper` / `RichDouble`、`floatWrapper` / `RichFloat`、`charWrapper` / `RichChar`、`StringOps` の `*` / `take` / `drop` / `isEmpty` / `toUpperCase` / `toLowerCase` / `stripPrefix` / `split`、`Map` / `Vector` / `List` / `Set` / `Seq` / `LazyList` の varargs `apply`、`Either`（`Left` / `Right`）、`Try` / `Success` / `Failure`（`Try(1)` / `map` / `getOrElse`）も jar リンク時のみ。このスライスでは **ArrayOps の変換・集約系**（`toList` / `toSeq` / `toIndexedSeq` / `toSet` / `toVector` / `toBuffer` / `groupBy` / `sortBy` / `sorted` / `sortWith` / `sum` / `product` / `min` / `max` / `minBy` / `maxBy` / `mkString`（0/1/3 引数）/ `reduce` / `reduceLeft` / `indexWhere`（1/2 引数）/ `lastIndexOf` / `patch` / `updated` / `appended` / `prepended` / `concat` / `++`。`toList`/`toSet`/`toVector`/`toBuffer`/`sum`/`product`/`min`/`max`/`minBy`/`maxBy`/`mkString`/`reduce`/`reduceLeft` は `javap -s scala.collection.ArrayOps` で確認したとおり `ArrayOps` 自身には `$extension` も直接メソッドも無く、実行時は `scala.Predef$.MODULE$.genericWrapArray` で `scala.collection.mutable.ArraySeq` に包んでから `scala.collection.IterableOnceOps` のデフォルトメソッドを呼ぶ。`sum`/`product`/`min`/`max`/`minBy`/`maxBy` 用に `scala.math.Numeric`（`Int`/`Long`/`Double` の `implicit object`）を新設。他メソッドは既存の `Ordering`/`ClassTag` implicit をそのまま使う）、**`scala.collection.MapView`**（`Map.view` / `keys` / `values` / `filterKeys` / `mapValues`（型引数は明示無しで推論できる）/ `toMap`（`A <:< (K, V)` witness は `scala.$less$colon$less$.MODULE$.refl()` を codegen 側で合成）/ `toList` / `toSeq` / `size` / `isEmpty` / `foreach`。私有 MapView classfile は出さない）を同じ jar にリンクする
- 具象 `val` 付き trait の初期化（`T$class.$init$`）と `abstract override` の super 連鎖
- 抽象型メンバーと型射影: `trait Foo { type A; def x: A }`、`type A = Int`、メソッド署名の `Bar#A`。object / class の **type alias** `type T = List[Int]` とトレイトの `type A = String` は vals/defs で underlying 型として使う。循環 `type A = B; type B = A` は `illegal cyclic reference`。pickle は nsc `ALIASsym`（2.13 に `ALIAStpe` タグは無い）
- パス依存型: 安定パス `c.A`（`c: Foo { type A = Int }` や object / `this` / `val`）。`var` や `def` など不安定パスは nsc と同じ `stable identifier required, but … found`
- singleton / this-types: 安定パスの `x.type` と `this.type` を戻り型として型付け・実行する。不安定な `x.type`（`var` / `def` / `new C()`）は `stable identifier required` で診断する
- compound types: `A with B` を値 / パラメータの型として使い、両側のメンバーを呼ぶ。クラスが二つある違法 compound（`A with B` で両方 class）は `illegal inheritance` で診断する
- 構造的 refinement: `{ def foo: Int }` / `T { def foo: Int }`。実行時は **Java reflection**（`getClass` / `Class.getMethod` / `Method.invoke` + unbox）。2.13 の reflective call と同じ実行意味論のサブセット。`scala.language.reflectiveCalls` は要求しない。**構造的代入** `x.foo = v`（`{ var foo: T }` または getter + `foo_=`）と構造的 `x(i) = v`（`update`）。nsc どおり reflective `foo_=` / `update`。違法な `{ def foo: Int }; x.foo = 1` は `foo_= is not a member`。本体付き `def` は診断する
- self type: `trait T { self: Foo => ... }` の typecheck と mixin。実装クラスが self type に適合しないと `illegal inheritance`
- 変性: `class C[+A]` / `class Box[+A](val x: A)` は合法。`class Bad[+A](var x: A)` は nsc と同様 covariant-in-contravariant で拒否。`A @uncheckedVariance`（メソッド引数や型引数位置）は nsc と同じくその出現の変性検査を外す

- **def マクロの定義**: `def f: T = macro Impl.method[A]`。パースし、実装参照を解決して
  `Impl$` / `method` のバインディングをシンボルに記録し、マクロ def のバイトコードは
  nsc と同じく**出さない**（だから Java から呼べない）。戻り値型の省略 / object のメソッド
  でない実装 / `Context` を第 1 引数に取らない実装 / 解決できない参照 / whitebox は診断する。
  **展開は未実装**なので、呼び出し地点は診断して落とす。設計は [`docs/macros.md`](docs/macros.md)

フィクスチャで実際に動く範囲は README 末尾の表を見てください。

### Uncurry / Erasure

パイプラインは次のとおりです。

```
parse → namer → typer → uncurry → lambda-lift → erasure → emit
```

uncurry は nsc と同様、typer と erasure のあいだの独立パスです。ネストしたパラメータリストを 1 リストにまとめ、ネストした `Apply` を 1 回の呼び出しにします。部分適用と eta-expansion（`foo _`、FunctionN 期待位置の未適用メソッド）は `FunctionN` クロージャになります。

lambda-lift は uncurry のあと、erasure の前です。メソッド本体のネストした `def` を囲みクラスの合成メソッドに上げ、捕獲したローカルを先頭パラメータとして渡します。ネスト def を値として eta したときや、ラムダから再帰呼び出しするときも、実際に classfile に出て実行されます。

anon-capture は lambda-lift の直後、erasure の前です。メソッドの中で定義したクラスごとに、囲みメソッドの自由変数を最初に参照した順で集め、クラスシンボルに記録します（`crates/typer/src/anon_capture.rs`）。バックエンドはその並びをそのままフィールド・コンストラクタ引数・`new` の引数に使うので、両者の順序は必ず一致します。内側のクラスがキャプチャしたものは外側のクラスのキャプチャにも入るため、入れ子でも `new` の位置で値が揃います。捕獲した `var` は `scala.runtime.*Ref` に箱詰めされ、ボックス自体が引数として渡ります。

erasure は型引数を落とし、型パラメータと unbounded ワイルドカードを `Object` にし、プリミティブと `Object` の境に box / unbox を挿入します。by-name は `Function0` に下げます。バックエンドの ad-hoc な推測だけには頼っていません。配列は nsc と同じく**要素が抽象型のときだけ** `Object` に潰します（`def d[T](x: Array[T])` は `(Ljava/lang/Object;)`、`Array[AnyRef]` / `Array[Any]` / `Array[AnyVal]` は `[Ljava/lang/Object;`）。

### メソッド型パラメータの推論（引数＋期待型）

nsc の `instantiateExpecting` と同じく、メソッドの型パラメータは**引数と期待型の両方**を制約として解きます（`crates/typer/src/check.rs` の `add_expected_constraints`）。

- 結果型の**不変位置**では期待型が引数の解より優先します。`Array` は非変なので `val a: Array[AnyRef] = Array("x", "y")` は `T = AnyRef`（`[Ljava.lang.Object;`）、`val b: Array[Any] = Array(1, 2)` は `T = Any` でボックスされます。
- **共変位置**の期待型は上界にすぎないので引数の解が勝ちます（`cov("q"): List[Any]` は `T = String`）。
- 解いた型引数は**implicit 引数リストの解決より前**に確定します。`def column[T](n: String)(implicit tt: TypedType[T]): Rep[T]` を `Rep[Int]` の位置で呼ぶと `TypedType[Int]` を探しに行きます。
- どちらでも決まらない型パラメータは `Nothing` で埋めず、nsc と同じ診断（`could not find implicit value …`）を出します。

これに伴い `Array` は **非変** になりました（`Array[Int]` は `Array[Any]` に渡せません。scalac と同じ）。また、継承したメンバの型は**適用済みの親**を通して見るようになりました（`OptionMapper2[…, Boolean, …].column` の implicit は `TypedType[BR]` ではなく `TypedType[Boolean]` を探します）。

**明示的な型適用**も同じ経路を通します。オーバーロードされた呼び先は、まず SLS 6.26.3 どおり
**型パラメータの個数**で候補を絞り、残りが一つならそれに確定してから型引数を代入します。
`fs.typed[Boolean](ch)`（`def typed(tpe: Type, ch: Node*)` と `def typed[T : ScalaBaseType](ch: Node*)` の
オーバーロード）が、絞らないままだと `fun.ty` にオーバーロード型が残り、後続の implicit 節が
未代入の `ScalaBaseType[T]` を探しに行っていました。

まだ決まっていない型パラメータを `Any` に緩めるのは（`xs.collect { case … }` の `B` のように）
**呼び先自身の**型パラメータだけです。スコープにあるクラスの型パラメータは確定した型なので
緩めません。`def take[T](r: Rep[T])` を `trait Base[P1]` の中で `take(c)`（`c: Rep[P1]`）と
呼ぶと `T = P1` であって、`Rep[Any]` を要求してはいけません。

親コンストラクタの引数は**親の型引数を代入してから**照合します。
`class ReWrap[T : TT] extends Wrap[T](implicitly[TT[T]])` の `Wrap[A](val tt: TT[A])` は
`TT[A]` ではなく `TT[T]` を要求します。

### Implicit 解決

nsc に寄せた探索順です。偽の「何でも変換」はありません。

1. 現在のスコープと、囲んでいるクラス / object の `implicit` メンバー（親 class / trait から inherited したメンバーと、`import Foo._` で入れたメンバーを含む）
2. 囲んでいるパッケージのパッケージオブジェクト（`package object p` の implicit メンバー）
3. 目標型の部分（型コンストラクタ・型引数・ネストした prefix）と、その **基底クラス** のコンパニオン（`Option[T]` なら `Option`、`Outer.Inner` なら `Inner`、`A =:= B` なら `=:=` が継承している `<:<` のコンパニオン）。変換なら元の型の部分も見る

呼び出し側で implicit パラメータ節を明示できます: `add(5)(3)` / `foo(x)(ev)`。探索で埋めるのは、その節が省略されたときだけです。

数値の widening（`Int` → `Long` / `Double` など）は **implicit 探索の前** に特別扱いします。scalac の implicit ではなく、typer の組み込みです。

継承した implicit メンバーは**親の型引数を通して**見ます（as-seen-from）。
`trait Base[P1] { protected[this] implicit def p1Type: TT[P1] }` を
`trait Mid[P1] extends Base[P1]` から使うとき、候補の型は `Base` の `P1` ではなく
`Mid` の `P1` です（`Typer::implicit_candidate_ty`）。ここを素の宣言型のままにすると
`implicitly[TT[P1]]` が自分の親の実装を見つけられません（slick の
`Library.Abs.column[P1](n)`）。

同じ型の候補が複数あるときは nsc `Infer#isStrictlyMoreSpecific` と同じ**足し算**で決めます:
（型の特定度の差）＋（定義クラスの subclass 関係の差）> 0。型が同じでも、より派生した
クラスで定義されたほうが勝ちます（`ConstColumn[T : TypedType]` 自身の evidence が、
`Rep.TypedRep` から継承した `tpe` に勝つ）。型と origin が食い違うときは相殺して
ambiguous になり、これも nsc と同じです。

失敗はスタブせず、診断を出します。

- `no implicit: could not find implicit value of type …`
- `ambiguous implicit: …`
- `diverging implicit expansion for type … starting with method …`

#### 多相な implicit def / implicit val

候補が自分の型パラメータを持つとき（`implicit def showList[A](implicit s: Show[A]): Show[List[A]]`）、
候補の結果型と期待型を**両側ユニフィケーション**して型引数を決めます（`crates/typer/src/implicits.rs` の
`Unify` / `implicit_solve`）。片側だけの `unify_one` と違い、

- 候補の型パラメータ（`A`）と、
- 呼び出し側の**未決定**型パラメータ（nsc の undetermined tparam。`toMap[K, V](implicit ev: A <:< (K, V))`
  の `K` / `V` は呼び出しのどこにも現れないので、witness を見つける探索そのものが決めるしかない）

を同時に解きます。候補側は必要なら基底型へ広げてから合わせるので、`<:<.refl[A]: A =:= A` を
`From <:< To` に当てて `A = From`、そこから `From <: To` を要求する、という nsc と同じ導出になります
（`scala.<:<` / `scala.=:=` の witness は専用のフォールバックではなく、`refl` を `implicit` として
普通に探索して見つけます）。タプル糖衣 `(A, B)` と `Tuple2[A, B]` は同じ型として単一化します。

決まらない型パラメータが残った候補は**落とします**（黙って `Any` を入れません）。
決まった後は、候補の implicit 引数を**再帰的に**解決します（`Show[List[List[Int]]]` は
`showList[List[Int]](showList[Int](showInt))`）。

再帰には二重の打ち切りがあります。

- 深さ上限（`MAX_IMPLICIT_DEPTH = 8`）
- nsc の diverging implicit expansion 相当: 同じ implicit を、同じ head シンボルで複雑さの減らない
  目標型に対して再入したら打ち切る（`implicit def loop[A](implicit a: A): A`）。
  診断は `diverging implicit expansion for type Show[Int] starting with method loop`

specificity は nsc の `isAsSpecific` に寄せて、候補の型パラメータをワイルドカードに潰してから比較します。
`implicit val tagInt: Tag[Int]` は `implicit def tagAny[A]: Tag[A]` より specific なので、`Tag[Int]` では
`tagInt` が勝ちます。同じ形の多相 implicit が二つあれば **ambiguous** です。

同じ目標型に対して subtype 関係にある implicit が二つあるとき（`A` と `B extends A` を両方 `A` として探す）、より specific な `B` が勝ちます。同じ型が二つなら、これまでどおり曖昧です。定義クラスの origin も nsc と同じで、子クラスに定義した implicit は親の implicit より origin が specific です。親の more-specific な implicit と、子の less-specific な local が両方マッチすると、型と origin が食い違って **ambiguous** です。逆（親が less-specific、子が more-specific）は子が勝ちます。

### Trait mixin

Java 6 には default method がないので、具象メンバー付き trait は次のように出します。

- trait 自体はすべてのメソッドが abstract な JVM interface
- 具象本体は `T$class` の static メソッド（第一引数が `$this: T`）
- 実装クラスは線形化（右の mixin がより具体的）で勝った定義へフォワーダを出す

`class C extends A with B` で A と B が同じ `msg` を持つとき、実行時は B です。線形化は Scala の C3 です（`C extends Base with A with B` → `C, B, A, Base`）。

trait の `val` は interface 上の getter と **nsc と同じ名前の mixin setter `T$_setter_$v_$eq`**（パッケージ付きなら `p$q$T$_setter_$v_$eq`）で表し、`T$class.$init$` が右辺を評価してその setter を呼びます。実装クラスがフィールドを持ち、コンストラクタが mixin `$init$` を（より一般的な親から）呼びます。`object O extends T` も同じで、フィールド・アクセサ・`$init$` 呼び出しをクラスと同じだけ出します。

`class D extends T { override val v = "d" }` は、nsc と同じく **mixin setter を空実装（`return` のみ）**にします。`D` は自分のフィールド・getter を持ち、`$init$` のあとにコンストラクタで自分の右辺を書くので、trait 側の初期化が override を上書きしません。

trait の `var` は nsc どおり getter と**普通の setter `v_$eq`**（mixin setter ではない）です。抽象 `var n: Int` の場合も interface に `n()` と `n_$eq(I)` を出し、実装クラス側の `var n` がその両方を埋めます。trait 本体・実装クラス・外部（`d.n = 5`）のどこからの代入も、フィールドへの `putfield` ではなく `n_$eq` の `invokeinterface` にします（trait にフィールドは無いので `putfield` は `NoSuchFieldError` になる）。trait の `val` への代入は nsc どおり `reassignment to val` として診断します。

スタック可能な trait の `abstract override` は、`T$class` 内の `super.m` を `T$$super$m`（実装クラスが線形化の次へフォワード）にします。`class C extends Base with A with B` で両方 `abstract override def msg` なら、実行時は `B-A-base` です。

### 複数コンパイル単位のケーキパターン（ヘッダパス）

`typecheck_units` は run 全体を 1 つのシンボル表で型検査します。パスは
**namer（全ユニット）→ ヘッダパス（全ユニット）→ シグネチャパス（全ユニット）→ 本体パス**の
4 段です。

ヘッダパスは `crates/typer/src/check.rs` の `parents_pass` です。namer は親を**名前のまま**
（`rough_parents`）記録し、その名前を解くのは `class_sym_of` で、**そのとき現在のスコープ**を
引きます。シグネチャパスはユニットをコマンドライン順に歩くので、親鎖が後ろのファイルにある
クラスは祖父母を別ファイルのスコープで引くことになり、そこで鎖が切れて継承した型がまるごと
見えなくなっていました。slick の `DB2Profile`（`slick/jdbc/`）が 4 段上の
`RelationalTableComponent`（`slick/relational/`）の内部クラス `Table` を参照する形が
`not found: type Table` になっていたのはこれです。

```scala
// a.scala（コマンドラインで先）
trait Child extends P1 { def f(t: Table[?]): String = t.n }
// z.scala（後）
trait TC { self: P1 => abstract class Table[T](val n: String) }
trait P1 extends TC
```

ヘッダパスは全ユニットの親リストを**自分の定義位置のスコープ**（そのファイルの import 込み）で
シンボルに固定します。内部クラスが外側の継承した名前を親に書くことがあるので、変化が無くなる
まで（最大 3 周）回します。最後にもう 1 周して**プライマリコンストラクタのパラメータ型**を
付けます。`extends Table[Int](n)` は親の `<init>` に対して検査されるので、親が後ろのファイルだと
引数型が付いておらず `no matching overload for constructor` になっていました。

ヘッダパスは解決のためだけに走るので、そこで出た診断は**すべて捨てます**（本物の診断は後続の
シグネチャパス・本体パスが出します）。`import scala.language.*` によるフラグも前後で保存・
復元します。

自己型のエイリアス（`trait T { self: P => }` の `self`）は**継承されません**。親や自己型の
メンバーをスコープに入れる箇所は `Symbol::self_alias` を見てこれを外します。そうしないと、
複数のコンポーネントが揃って `self` と名乗る slick のようなケーキで `self` がオーバーロード
集合になってしまいます。

型選択の接頭辞は**項**なので、companion 対では object の側が選ばれなければいけません。
`trait Rep[T]` と `object Rep { abstract class TypedRep[T] }` が並んでいるとき、
`Rep.TypedRep` の `Rep` はオブジェクトです（`qualified_type_owners` が候補を全部返し、
そのメンバーを実際に持つものを採る）。

### 親コンストラクタの implicit / デフォルト引数

`extends P` は引数を書いていなくても、`P` のコンストラクタが implicit 節や
デフォルト引数だけの節を持つなら、JVM 上ではその引数を渡さなければいけません。

```scala
trait TT[T]
class TypedRep[T](implicit val tpe: TT[T])
class ConstColumn[T : TT] extends TypedRep[T]   // TypedRep.<init>(TT) を呼ぶ
```

`type_parent` は、親のコンストラクタが**そのクラス自身のものひとつだけ**で、書かれていない
パラメータが全部 implicit かデフォルト付きのとき、親の木を `extends P()` の形（`Apply`）に
書き換えてから、呼び出し側と同じ `fill_defaults_and_implicits` で埋めます。埋められなければ
黙って通さず診断を出します（scalac は
`could not find implicit value for parameter tpe: TT[String]`、こちらは同じ位置で
`no implicit: could not find implicit value of type TT[String]`）。引数無しの `new P` も
同じ書き換えを通ります。

親位置は**ヘッダパス・シグネチャパス・本体パスの 3 回**歩かれるので、次の 3 点で二度目に
壊れないようにしています。

- 埋めるのは**本体パスだけ**（`sigs_only == false`）。シグネチャパスの時点では、後ろの
  ファイルにある親の context bound の evidence パラメータがまだ生えていないことがある。
- 埋めた木は `parent_fill_done`（file / NodeId / span / クラス）に記録して二度は埋めない。
  `sig_done` / `lazy_done` と同じ考え方。
- 合成した引数（`NodeId(0)` かつ型が付いている）は次のパスで**再型付けしない**。名前で
  引き直すと、その時スコープに入っていない evidence パラメータを見失う。

オーバーロード解決は**ソースに書かれた引数だけ**で行い、埋めるのはその後です。埋めた引数で
解決をやり直すと、implicit が見つからなかった 1 件の診断が
`no matching overload for constructor` に化けて増えてしまいます。

implicit 探索のスコープも nsc に合わせます。親コンストラクタの引数はコンストラクタ自身の
コンテキストで型付けされ、そこに `this` はまだ無いので、**自分のクラスと継承したメンバーは
候補になりません**（`crates/typer/src/implicits.rs` の `implicits_in_scope` を
`parent_ctor_scope` で切ります）。これが無いと
`class NullJdbcType extends DriverJdbcType[Null]` が、親から継承しようとしている
`implicit val classTag` 自身を親の `ClassTag[Null]` の答えに使ってしまい ambiguous になります。

パラメータ付き型エイリアスの適用（`type BaseColumnType[T] = JdbcType[T] & BaseTypedType[T]`
に対する `BaseColumnType[U]`）は `is_sub_type` で展開します。context bound
`[U : BaseColumnType]` が作る evidence の型がこの形なので、展開しないと `JdbcType[U]` に
適合せず implicit が見つかりません。

### try / catch / finally

`try` 本体を例外テーブルで覆い、ハンドラで catch のパターン（`case _: RuntimeException` など）を `instanceof` します。マッチしなければ再 throw します。`finally` は成功パスと catch パスの両方で実行します（`jsr` は使いません。コードを複製します）。

`catch` の後ろは case 節のブロックだけでなく、**`PartialFunction[Throwable, U]` の値**でも構いません（`try close() catch ignoreFollowOnError`）。nsc の `makeCatchFromExpr` と同じ木に落とします。

```scala
try close() catch ignoreFollowOnError
// ↓
try close() catch {
  case catchArg$1: Throwable =>
    val catchExpr$1 = ignoreFollowOnError
    if (catchExpr$1.isDefinedAt(catchArg$1)) catchExpr$1.apply(catchArg$1)
    else throw catchArg$1
}
```

ハンドラ式は **case 節の中** で評価されます。つまり本体が実際に投げたときだけ、高々 1 回です。ハンドラが受け付けない例外はそのまま再送出します。`case` で始まらない `catch { expr }` も同じ扱いで、`catch {}` は「節が無い」のままです。

`try` 本体が必ず投げる（`Nothing`）ときの型は、nsc どおりハンドラ側との lub です。`val n = try throw e catch toLen` は `Nothing` ではなく `Int` になります。

本体や catch 節からの **`return`** は finalizer を飛ばしません。nsc と同じく、値をローカルに退避してから finalizer のコピー（例外テーブルの範囲**外**に置くので、finalizer 自身が投げても二重には走りません）へ跳び、そこで本当に return します。入れ子の `try ... finally` は内側から順に繋がります。`synchronized { ... return x ... }` も同じ仕組みで `monitorexit` を通ります。

### 到達不能コード

`throw` / `return` / `goto` のあとにコード生成が出した命令は、次のラベル（またはメソッド末尾）で**捨てます**。`def boom(): Int = throw e` は `athrow` で終わり、その後ろに `ireturn` は出ません（出すと `VerifyError: Operand stack underflow`）。到達不能な区間ではスタックマップフレームもジャンプ先の記録も取りません — 捨てるバイト列を指すフレームや、空スタックを合流させたラベルは、どちらも検証を壊します。到達不能なままメソッドが終わっても終端命令は残るので `Control flow falls through code end` にはなりません。到達不能でも**型検査はします**（`tests/fixtures/dead_bad.scala`）。

例外ハンドラのフレームは、覆っている区間の**入口**のローカルと、区間中に書かれたローカルの共通の上位型だけを名乗ります（ハンドラは区間のどこからでも入りうるため）。

### ネストした型

`class Outer { class Inner }` は `Outer$Inner` になり、非 static な内部クラスは `$outer` をコンストラクタで受け取ります。primary / 補助コンストラクタの overload 選択はソース引数だけを見ますが、呼び出す `<init>` 記述子には `$outer` を前置します。`object Outer { object Inner }` は `Outer$Inner$` と `MODULE$` です。

**trait のメンバークラス**も同じです。nsc と同じく `$outer` の JVM 型は外側 trait の
interface 型（自分型 `self: P =>` があり、それが外側 trait の派生なら `P`）で、
`<init>` の第一引数に置きます。内側から外側の `def` / `val` / `lazy val` / 型メンバを
読むと `$outer` を辿って `invokeinterface` になります。多段ネスト
（`trait T { class Inner { class Deep } }`）は `$outer` を 2 段辿ります。

`new` に外側インスタンスを渡す先は次の順で決めます。

- `new p.Inner`（`p` は val / `this` / object）のように**前置詞が書かれていれば**それ
- `this` とその `$outer` チェーンで届くならそこ
- 届かなければ、外側の `object`（`object O extends T { class R extends Inner }` は
  `O$.MODULE$` を親コンストラクタへ渡す。nsc と同じ）

trait のメンバークラスを継承したクラス／オブジェクトは、親の `<init>` にも `$outer` を
渡します。

### lazy val

フィールドに加えて `bitmap$0: Int` と、同期したアクセサを出します。初期化は最初の読み取りまで遅延します。

trait の `lazy val` は（nsc の mixin フェーズと同じく）実装クラス／オブジェクトごとに
フィールド・`bitmap$0` のビット・アクセサを複製します。ビットはクラス自身の `lazy val`
と継承したものを 1 本のリストにして採番するので衝突しません。interface 側は abstract
宣言だけなので、呼び出しは `invokeinterface` です。

### 型注釈のないメンバのシグネチャ（lazy completer）

`val p = 1` / `def p = 1` は右辺を型付けするまで型が決まりません。typer はテンプレートをソース順に歩くので、定義より前の位置からの参照は本来 `<notype>` になります。nsc と同じように、シンボルごとに「未完成の定義」を持たせ、**型が必要になった瞬間に**その定義を完成させます（`crates/typer/src/lazysig.rs`）。

```scala
class C { def f: Int = D.p }   // D.p は Int になる
object D { val p = 1 }
```

namer が定義木を控え、typer のシグネチャパスがテンプレートのスコープ（import / 継承メンバー / 型エイリアス）付きで控え直します。完成した木は元の木に差し戻すので、evidence パラメータや default getter の合成が二重に走ることはありません。

完成中の定義に再入したら nsc の `CyclicReference` と同じ診断を出します（スタックオーバーフローにはしません）。

```
recursive value y needs type          // object A { val x = y; val y = x }
recursive method f needs result type  // object A { def f = g; def g = f }
```

`val` は自分の右辺を型付けする間ロックしません（`def` はロックします）。これは scalac 2.13.16 の実際の出力に合わせたもので、上の 2 例はメッセージ・行・列まで一致します。

`type T = rhs` も同じ仕組みに乗ります。ユニットはコマンドラインの順に型付けするので、先に来たファイルのシグネチャが後のファイルの `B.T` を名指すと、右辺が未解決のまま `<notype>` を見てしまいます。型エイリアスへの参照はその場でエイリアスを完成させます（循環時は `illegal cyclic reference involving type T`）。抽出子の `unapply` も同じで、`def unapply(n: Nd) = Some((n.v, n.tag))` のように結果型を書いていないものは、パターンが `<notype>` を見て部分パターンを 1 個と数えないよう、パターンの型付け前に完成させます。

### 型エイリアス（alias type member）

`type Scope = Map[K, V]` は右辺と**同じ型**です（nsc の dealias）。`<:` の左右、`x.m` のレシーバ、消去（`Scope` は `Map` に消去される）のいずれでも右辺に展開します。抽象型メンバ（`type T <: Bound`）は展開せず、従来どおり上限境界で扱います。

型の位置での `p.T` の `p` は**項**です。同名の trait と companion object があるときは module class を先に見ます（クラス射影は `C#T` と書く）。Java の static nested class のためにクラスもフォールバックとして残します。

`new A(...)` の `A` がエイリアスなら、nsc と同じく**右辺を構築**します（`type Alias = Base` の `new Alias("hi")`）。エイリアスのシンボル自身はコンストラクタを持たないので、これが無いと `no matching overload for constructor Alias` になります。修飾付き（`new p.A(...)`）は従来から dealias 済みでした。抽象型メンバ（`type A <: Bound`）は対象外です（`new A` はプログラムではなく、上限を構築するのは別のプログラムです）。

#### jar の package object にある型エイリアス

scalac は package object の `type` を classfile に一切書きません。`ScalaSignature` の
pickle にしか無いので、`<pkg>/package$.class` を読んでメンバーを畳み込むだけでは
`scala.NoSuchElementException`（= `java.util.NoSuchElementException`）や
`cats.effect.Ref` / `Async` / `Resource` が解決できませんでした。

package object を**最初に必要としたとき**にその pickle から `ALIASsym` を読み、
パッケージの型メンバー（`SymKind::TypeMember`）として登録します。先読みはしません。

- 型パラメータ付きのエイリアス（`type Ref[F[_], A] = cats.effect.kernel.Ref[F, A]`）は、
  各型パラメータの**カインド（arity）**まで復元します。`F[_]` の arity を 1 にしないと
  使用側が `does not take type parameters` になります。
- 右辺が名指すクラスは classpath からオンデマンドで読みます。pickle リーダ単独では
  `scala.*` しか辿れないので、解決できなかった名前を報告させ、typer 側で classfile を
  読み、もう一度変換する、というのを**何も新しく解決できなくなるまで**繰り返します。
- prelude と実在のクラスが常に勝ちます。エイリアスは穴を埋めるだけです。
- **右辺を復元できないエイリアスは登録しません**。代わりに理由を覚えておき、その名前が
  使われたときに `not found: type ParallelF -- package object cats.effect declares it as
  an alias for cats.effect.kernel.Par.ParallelF[F, A], which this compiler cannot express`
  のように出します。黙って `Any` になるより、何が起きたか言う方が良いからです。
- 併せて暗黙の `import scala._` を（`import java.lang._` より上の優先度で）入れました。
  何も見つからなかったときだけ引く経路なので、実際に届くのは `scala` package object の
  型エイリアスです。`--no-scala-library` では pickle が無いので供給せず、
  `not found: value NoSuchElementException` と診断します。

### super / 修飾付き this

`super.m(...)` はクラス親なら `invokespecial`、具象 trait 親なら `T$class.m($this, ...)` です。線形化の「右端の親」を `super` の対象にします（`super[T]` の mixin 指定もパースして使います）。`Outer.this` は内部クラスの `$outer` を辿ります。

**型位置の `super.T`** も通ります。親の型メンバーへのパスで、slick が多用する綴りです。

```scala
override def createUpsertBuilder(node: Insert): super.InsertBuilder = new SQLiteUpsertBuilder(node)
trait SimpleInsertActionComposer[U] extends super.InsertActionExtensionMethodsImpl[U]
```

戻り値型・パラメータ型・ローカル `val` の型・`extends` の親、そして `C.super.T` / `super[Mix].T` の綴りに対応します。テンプレートの親リストは nsc どおり**外側のコンテキスト**で型付けするので、`trait Mid` の中の `class MidBuilder(m: Int) extends super.Builder(m)` の `super` は MidBuilder のものではなく **Mid のもの**です。

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

### AnyVal（値クラスと universal trait）

`class Meter(val n: Int) extends AnyVal` は、値の表現を underlying（ここでは `Int`）に erase
します。`new Meter(x)` は `x` になり、`m.n` は `m` です。メソッドは `Meter.doubled$extension(n)`
のような static です。

**参照が要る位置では nsc と同じく本物の `Meter` インスタンスに box します。** `Integer` では
ありません。box が要るのは

- `Any` / `AnyRef` への代入と、`println` のような `Any` を取るパラメータ
- `extends Any` した universal trait（`final class Meters(val n: Int) extends AnyVal with Univ`
  の `Univ` 位置）。ここを `Integer` に box すると実行時 `IncompatibleClassChangeError` になる
- 型引数（`List[Meters]` / `Option[Meters]` / ジェネリックメソッドの引数）と配列要素
  （`Array[Meters]` は `[LMeters;`）。ラムダのパラメータも `FunctionN.apply` が `Object` を
  取るので box された側を受け取る
- 値クラス自身が宣言していないメンバの受け手（`==` / `toString` / `hashCode`）

逆方向（unbox）は `((Meters) x).n()` です。パターン `case x: Meters` は `instanceof Meters` +
`getfield`、`classOf[Meters]` は `Meters.class`（`Integer.TYPE` ではない）、
`x.asInstanceOf[Meters]` は `checkcast Meters` です。

`equals` / `hashCode` は nsc の `SyntheticMethods` と同じく underlying から合成します
（`equals$extension(u, that)` / `hashCode$extension(u)` の static も出します）。これがないと
box された `Meters(5)` 同士が参照比較になり、`Object.toString` も `Meters@5` ではなく identity
hash を出します。case class のフィールドが値クラスのときの `toString` も
`Leg(Meters@3,b)` と表示します（値は unbox のまま持ちますが、印字だけ box します）。

box するのは**このコンパイル単位が出す値クラス**だけです。prelude が持つライブラリ側の値クラス
（`StringOps` / `ArrayOps` など）は `augmentString` を identity 変換としてモデル化していて、
underlying をそのまま表現に使っているので box しません（`erasure::note_source_value_classes`）。

nsc との差: `$extension` static は nsc がコンパニオン `Meters$` に本体を置いてクラス側に
フォワーダを出すのに対し、scala-rs はクラス側に直接出します。同一プログラム内では等価ですが、
scalac が出した classfile との相互リンクはできません。

### ボックス型（`java.lang.Integer` と `scala.Int`）

scalac と同じく**別の型**です。`scala.Int` は値クラス、`java.lang.Integer` はその箱で、
両者を行き来するのは `Predef` の 16 本の暗黙変換（`int2Integer` / `Integer2int` /
`char2Character` / `Character2char` / …）です。

- `val i: java.lang.Integer = 3`（`int2Integer`）と `val n: Int = i`（`Integer2int`）
- `java.lang.Integer.valueOf` / `parseInt` / `MAX_VALUE`、`java.lang.Character.isDigit`、
  `java.lang.Double.parseDouble` などの static メンバ
- 箱は普通の参照型なので型引数に書けます: `new java.util.ArrayList[java.lang.Long]` への
  `add(7L)`、`List[java.lang.Integer](1, 2, 3)`
- nsc の weak conformance に合わせて数値の widening を通します: `xs.add(7)`（`Int` →
  `Long` → `long2Long`）、`val i: java.lang.Integer = 'c'`（`Char` → `Int`）
- 変換は intrinsic（`Integer.valueOf` / `Integer.intValue`）として出すので、
  **私有ランタイム**でも `scala/Predef$.int2Integer` を要求せずに動きます

分離のための修正点は 3 つです。(1) `scala.Int` の `jvm_name` は erasure（`java/lang/Integer`）
であって identity ではないので、`classpath::find_by_jvm` はプリミティブ値クラスを飛ばします
（`SymbolTable::is_primitive_value_class`）。飛ばさないと `java.lang.Integer` の classfile が
`scala.Int` に流し込まれ、`java.lang` に `Integer` が入りませんでした。(2) 同じ理由で
`add_package_paths` も値クラスを `java.lang` に登録しません（`java.lang.Long` が
`scala.Long` になっていました）。(3) Java の static はインスタンス経由では選べません
（nsc: "Static Java members belong to companion objects in Scala"）。これが無いと
`java.lang.Integer.max(int,int)` が `RichInt.max` と競合して `1.max(2)` が壊れます。
なお `0.5.isNaN` は nsc と同じく `Predef.double2Double(0.5).isNaN()` に解決します
（`doubleWrapper` は `LowPriorityImplicits` 側なので優先度が下）。

### 数値変換の塔と `Byte` / `Short`

nsc は `Byte` / `Short` / `Char` / `Int` / `Long` / `Float` / `Double` の**それぞれに
7 本すべての `toByte` / `toShort` / `toChar` / `toInt` / `toLong` / `toFloat` / `toDouble`**
を宣言します。合計 49 本を `crates/typer/src/prelude_numconv.rs` で入れ、コード生成は
`Intrinsic::NumConv("<from><to>")`（JVM ディスクリプタ文字のペア）から
`i2b` / `i2c` / `i2s` / `i2l` / `i2f` / `i2d` / `l2i` / `l2f` / `l2d` / `f2i` / `f2l` /
`f2d` / `d2i` / `d2l` / `d2f` を出します（`gen::emit_num_conv`）。`Byte` / `Short` /
`Char` はスタック上では `int` なので、そこへの変換は「まず `int` 幅にしてから
`i2b` / `i2s` / `i2c`」です。7×7 すべてを real scalac 2.13.16 と dual-run で突き合わせて
あります（NaN / ±Inf / 各型の MIN・MAX 込み）。

**`Byte` と `Short` は本物の JVM プリミティブ**になりました。以前は prelude が
`scala/Byte` / `scala/Short` という**実在しない**クラスを JVM 名にしていたため、
`def take(x: Byte): Int = x.toInt` が `invokevirtual scala/Byte.toInt` として出て
`VerifyError: Type integer is not assignable to 'scala/Byte'` になっていました。
`Int` / `Long` と同じく、値クラスの JVM 名はその**箱**（`java/lang/Byte` /
`java/lang/Short`）です。合わせて次を直しました。

- Java の `byte` / `short` ディスクリプタを `Int` ではなく `Byte` / `Short` として読む
  （`java.lang.Byte.valueOf(byte)`、`String#getBytes` の `Array[Byte]`）
- 演算子の表に `Byte` / `Short` / `Char` を受け手としても被演算子としても入れる
  （`crates/typer/src/prelude_bsops.rs`）。`b * 3` / `b < s` / `-b` / `~b` / `b << 2` は
  nsc と同じく `Int` へ昇格する
- SLS 3.5.3 の弱適合 `Byte <= Short <= Int <= Long <= Float <= Double` と `Char <= Int`
  （`val l: Long = b`）。`Long -> Float` も足した
- `Ordering[Byte]` / `Ordering[Short]` / `Numeric[Byte]` / `Numeric[Short]`
  （jar の `Ordering$Byte$` / `Numeric$ShortIsIntegral$`。library ABI のみ）
- `Byte` / `Short` / `Char` のスクルティニーに `Int` 定数パターンを許す
  （`case DatabaseMetaData.functionNoTable` は `==` 比較なので nsc も通す）

**プリミティブ配列の要素アクセス**もこのスライスで直しました。`Array[Long]` /
`Array[Double]` / `Array[Char]` / `Array[Float]` / `Array[Byte]` / `Array[Short]` /
`Array[Boolean]` は `aaload` / `aastore` ではなく専用命令
（`laload` / `dastore` / `caload` / `baload` …）が要ります。以前は `Array[Int]` と
`Array[Boolean]` 以外すべてが `VerifyError` でした（`Array[Boolean]` は `iaload` に
なっていて、これも誤り。JVM では `boolean[]` は `byte` の命令を使う）。

ついでに `Long.toInt` が `invokevirtual java/lang/Long.toInt`（存在しない）として出て
`NoSuchMethodError` になっていたのと、`1 + 2.5f` が `int` を `float` の位置に積んで
`VerifyError` になっていたのも直しました。

### Predef（このスライス）

- `assert(cond)` / `require(cond)`（第 2 引数の by-name メッセージあり）。**私有ランタイム**では `AssertionError` / `IllegalArgumentException` を直接 `new`。**`--scala-library`** では `scala.Predef$.assert` / `require`
- `???` は **私有**では `new scala.NotImplementedError`（`RuntimeException` サブクラス）。**library** では `Predef$.???`（jar の `NotImplementedError` は `Error`）。dual-run フィクスチャは `Throwable` で捕捉する
- `any2ArrowAssoc` による `1 -> "a"`。**私有**では `scala.Tuple2` を直接 `new`（`Predef.ArrowAssoc` は呼ばない）。**library** では implicit `any2ArrowAssoc` → `Predef$ArrowAssoc$.$minus$greater$extension`
- `identity` / `locally` / `implicitly`。**私有**では intrinsic。**library** では `Predef$.identity` / `locally` / `implicitly`
- `any2stringadd` の `1 + "x"`。**私有**では StringBuilder 連結（intrinsic）。**library** では implicit `any2stringadd` → `Predef$any2stringadd$.$plus$extension`
- `"x".length`。**私有**では `java.lang.String#length`。**library** では implicit `augmentString` → `StringOps.size$extension`（jar の StringOps は `length` をインライン化しており、同等の `size$extension` が `String#length` を呼ぶ）。`toInt` / `toLong` / `toDouble` は **私有**では `Integer.parseInt` など。**library** では `StringOps.toInt$extension`

### import の解決

`import` の**接頭辞**（`a.b.c` の部分）は、式として型付けするのではなく**シンボルとして 1 区切りずつ**解決します。jar の中にしか存在しないパッケージは式にならない（型を持たない）ので、以前は `import cats.syntax.all._` が `value all is not a member of <notype>` で落ちていました。

解決できる接頭辞は次のとおりです。

- **同一実行のパッケージ**（1 階層 / 2 階層 / 3 階層以上）
- **jar 由来のパッケージ**。`p/n.class` / `p/n$.class` をオンデマンドで読み、無ければ `p/` プレフィックスがあるかで**パッケージ自体**を作る
- **オブジェクト**と**package object**。package object は `p/package$` にコンパイルされ、そのメンバーは `p` 自身のメンバーです。同一実行のものは namer が畳み込み、jar のものはここで読み込んで畳み込みます（`import cats.syntax.all._` の `all` は `cats/syntax/package$all$`）。**型エイリアス**は classfile に無いので、あわせて pickle からも読みます（「jar の package object にある型エイリアス」節）
- **項（term）接頭辞**（`import someObject.field._`）は従来どおり typer に落とします

選択子は 4 形すべて動きます。

| 形 | 例 |
| --- | --- |
| 単一 | `import p.C` |
| ワイルドカード | `import p._` / `import p.*` |
| 名前付き | `import p.{A, B}` |
| リネーム / 隠蔽 | `import p.{A => B}` / `import p.a as b` / `import p.{A => _, _}` |

ワイルドカードは、**その時点で分かっているメンバーを先に入れ**、あわせて**スコープに owner を記録**します。jar のパッケージはエントリを列挙せずに 1 クラスずつ読むので、`import cats.data._` の直後に `NonEmptyList` の classfile が読まれているとは限らないからです。未解決の名前が出たときに記録した owner を順に引き直します（`Checker::expose_unqualified`）。`{X => _, _}` で隠された名前はこの遅延経路にも渡すので、隠蔽が後から破られることはありません。

名前を 1 つも解決できなかった選択子は nsc と同じく**その場で診断**します（黙って何もしない import にはしません）。

```
value Nope is not a member of package p1
```

`case class C` は、同じファイルの後ろにある `object C` が名前付けされる前に**合成コンパニオン**を持つので、`C` に Module が 2 つ答えることがあります。接頭辞解決は**同じ種類の候補を全部**（書かれた object を先に）返し、選択子をその全部から引きます。種類は最良のものだけに絞るので、`import scala.util.control.Breaks._` は同名のトレイトではなく object を指したままです。

`scala.language` の機能名（`existentials` / `higherKinds` / `reflectiveCalls` / `experimental.macros`）も import できる名前として置きました（`crates/typer/src/prelude_lang.rs`）。これらは何もゲートしません。scala-rs が実際にコンパイルできない構文は、使用地点で従来どおり診断されます。

### 単一型 `X.type` と名前空間

Scala は**項の名前空間と型の名前空間が別**です。`X.type` の `X` は項なので、内側のスコープが
`X` を型としてだけ束縛していても、外側の項 `X` に届かなければいけません。slick の
`HList.scala` がこの形です。

```scala
object syntax {
  type HNil = hl.HNil.type      // 型としての HNil
}
object HList {
  import syntax._               // 型名 HNil を内側に持ち込む
  def empty: HNil.type = HNil   // ここの HNil は外側の object HNil
}
object HNil extends HList { … }
```

`SymbolTable::lookup_type` の対になる `lookup_term` を足して、名前を型としてしか束縛していない
スコープは飛ばして外へ探しに行くようにしました（`is_stable_path` / `term_path_sym` /
`term_path_type` / 項位置の `type_ident`）。

同じスライスで、`X.type` の接頭辞まわりの穴を 2 つ直しました。

- **パッケージ接頭辞**（`p.HNil.type`）。パッケージは値ではないので型を持たず、
  `term_path_type` が答えられませんでした。パッケージ / モジュールを直接たどる
  `path_owner_sym` を通します。あわせて `singleton_to_type` が先頭の識別子を
  `expose_unqualified` するようにしました（`p` がまだスコープに入っていないことがある）。
- **object のネスト**（`ColumnOption.AutoInc.type`）。`object O { object I }` の `I` は
  モジュールクラス `O$` のメンバなので、接頭辞の型からメンバを引くときに Module →
  モジュールクラスへ正規化します（`path_member_owner`）。

### 改行が文を切る条件（nsc `inLastOfStat` / `inFirstOfStat`）

`crates/lexer/src/lib.rs` の `drop_non_separating_newlines` は nsc の Scanners と同じ規則で、
改行の**前**のトークンが文を終われて、**後**のトークンが文を始められて、いま `{ … }` 領域
（または最上位）にいるときだけ NEWLINE を残します。

パーサ側にも対応する規則があります。nsc の `postfixExpr` のループは「今のトークンが識別子
でなければ止まる」だけで、NEWLINE はそれ自体が 1 つのトークンなので**そこで中置式は終わり**
です。つまり

```scala
val x = { 1 }
-1          // ← 2 文。`{1} - 1` ではない
```

`}` は `inLastOfStat`、`-` は `inFirstOfStat` なので改行は残り、`-1` は別の文になります。
`if (c) { 1 }` の直後、`(1)` の直後、識別子だけの行の直後も同じです。

演算子が**行末**にある場合は続きます。これは nsc の `newLineOptWhenFollowing`（演算子を積んだ
あとで、次が式を始められるなら NEWLINE を 1 つ読み飛ばす）に当たります。

```scala
val a = 1 +
  -2        // ← 1 つの式。-1 になる
```

括弧・角括弧の中では改行がそもそも文を切らないので、`(c` で始まって次の行が `- 1)` の
形は今までどおり引き算です。

以前は「改行のあとの演算子は式の続き」と読んでいたため、上の 1 つ目が `{1} - 1` になり
`value - is not a member of Nothing` のような診断になっていました。

なお、値を持つ式が**文の位置**に来たとき（`if (c) { buf += x }` や `x match { case … => 1 }`）は
nsc の `genLoadIf` / `genLoadMatch` と同じく `expectedType = UNIT` で生成します。片側だけが値を
残す分岐を `Any` に lub した型で生成すると、合流点でスタックの高さが揃わず
`VerifyError: Inconsistent stackmap frames` になっていました。

### ブロック位置の関数リテラル（nsc `expr(InBlock)`）

nsc の `expr(location)` は、**ブロックの文**として現れた関数リテラルの本体を `expr()` ではなく `block()` で読みます。つまり `{ x => val n = 1; n }` は「`val` を式の位置に書いた」ものではなく、**ブロックを本体に持つラムダ**です。ここも同じにしました。`parse_block_stat` が `in_block` を立て、`parse_expr1` がそれを消費して（入れ子の部分式は nsc の `Local` に戻る）、`=>` の本体を `parse_case_body`（`case` / `}` / EOF まで）で読みます。

あわせて nsc の `typeOrInfixType(location)` も入れました。ブロック位置の型注釈は `InfixType` で止まるので、`{ x: Int => body }` の `=>` は**関数型ではなくラムダのもの**になります（`Local` 位置、たとえば括弧の中の `(f: Int => Int)` は今までどおり関数型）。ついでに nsc と同じ順（型注釈 → `=>` → `match`）に並べ替えました。

これは `-Xsource:3` ではなく素の 2.13 の挙動です。slick では `state.map { tree => val replace = …; … }` の形が多く、この 1 点で 17 ファイルが先頭でパースに失敗していました。

### `?` ワイルドカード型と `-Xsource:3` の `&` 交差型

**`?` ワイルドカード型**（`List[?]` / `Shape[? <: Level, T, ?]` / `? >: Lo <: Hi`）は `_` の別名として、`_` とまったく同じ匿名 `TypeDef` に落とします。scalac 2.13.16 は **`-Xsource:3` なしでも `?` を受け付ける**（`?` は予約されていて、型名として使うには backtick が要る）ので、こちらもフラグ無しで受け付けます。実 scalac に合わせて、backtick 無しの `type ?[A, B]` や `Int ? String` は診断します。

```
using `?` as a type name requires backticks
```

backtick 付きの `` `?` `` は普通の名前のままです。ついでに `_ >: Lo <: Hi`（下限→上限の順、nsc の綴り）も通るようにしました。以前は上限しか見ておらず `expected ], found subtype` になっていました。

**`&` 交差型**（`R <: Product & Serializable`）は `-Xsource:3` / `-Xsource:3-cross` のときだけ受け付け、2.13 の `with` による compound type と**同じ木**（`CompoundTypeTree`）に落とします。`A & B with C { def f: Int }` のように `with` と混ぜても、refinement を付けても同じです。フラグ無しでは 2.13 素のまま、`&` は普通の中置型コンストラクタとして扱われて診断されます（scalac は `not found: type &`）。

**可変長パターン `case Cast(ch*)`** も `-Xsource:3` / `-Xsource:3-cross` のときだけ受け付け、2.13 の `case Cast(ch @ _*)` と**同じ木**（`Bind` + `Star`）に落とします。nsc と同じく名前の大文字小文字は問わず（`Foo(One*)` は stable id `One` との照合ではなく `One @ _*` の束縛）、`)` の直前の `*` だけが対象です。`case p * q` はどの source level でも中置 extractor のままだからです。フラグ無しでは scalac 2.13.16 と同じ文面・同じ桁で診断します。

```
bad simple pattern: use _* to match a sequence
```

これは slick（`build.sbt` が `-Xsource:3` / `-Xsource:3-cross`）を通すためのスライスです。`?` は slick 176 ファイル中 59 ファイル、`&` は 41 箇所、`ch*` は 2 箇所で使われています。

**`*` ワイルドカード import と `as` リネーム**も `-Xsource:3` / `-Xsource:3-cross` のときだけ受け付け、2.13 の `_` / `=>` と**同じ木**に落とします（`import p.*` は `import p._`、`import p.{a as b}` と `import p.a as b` は `import p.{a => b}`）。フラグ無しでは `*` は普通の名前のままなので、scalac 2.13.16 と同じく import 選択子の位置で診断します（scalac は `object * is not a member of package p1`、こちらは `value * is not a member of package p1`）。slick では `import slick.ast.*` などで 60 箇所以上使われています。`given` / `using` は 2.13 の構文ではないので**対象外**です。

### 型パラメータを取る型メンバーと高階 context bound

slick の profile ケーキが多用する形を実 scalac 2.13.16 と突き合わせて直したスライスです。最小再現を `tests/fixtures/tmember{1,2,3}.scala` に置き、`crates/cli/tests/tmember.rs` が **scalac と scala-rs の両方でコンパイルして `Main` の出力を比較**します。

**1. 型パラメータ付き型メンバーのオーバーライド判定**。`trait A { type C[T] <: TypedType[T] }` を `trait B extends A { type C[T] = JdbcType[T] }` で実装するとき、親の `T` と子の `T` は**別のシンボル**です。以前は `JdbcType[T_子]` を `TypedType[T_親]` と比べて必ず失敗し、`incompatible type in overriding` を出していました。nsc と同じく、比較の前に**親の型パラメータを子のものへ置換**するようにしました。さらに境界が兄弟メンバーを指す（`type B[T] <: C[T]`）場合は、子がその `C` をどう実装したかで読み直します（`expand_type_members`）。境界違反そのもの（`type C[T] = Int` に対する `<: Bound[T]`）は今までどおり診断します。

**2. 高階型パラメータの context bound `F[_]: Async`**。実 scalac 2.13.16 で `def f[F[_]: Async]()` も `class C[F[_]: Async]` も**通る**ことを確認したうえで、`(implicit ev: Async[F])` へデシュガーするようにしました。README には「HK の context bound は nsc どおり `takes type parameters`」とありましたが、**これは誤り**で、拒否されるのは *view* bound `F[_] <% V` の方だけです（そちらは今までどおり診断します）。context bound の境界が**型パラメータを取る型メンバー**（`def base[U: BaseColumnType]`）でも `BaseColumnType[U]` へ適用します。

**3. 型と項の名前空間の分離**。`trait D[F[_]] { def g = { val F = asyncF; val u: F[Unit] = … } }` のように、型パラメータ `F` と同名の `val F` があると、型位置の `F` まで項に食われて `not found: type F` になっていました。型名の解決は**型名前空間だけを見て外側のスコープへ抜ける** `lookup_type` を使うようにしました。

**4. 型メンバーの解決を外側のインスタンスまで辿る**。`Main.factory: Main.Factory` のように、抽象メンバーを宣言したトレイトの**内側のクラス**を経由するとき、nsc は outer instance の prefix でその実装に到達します。`expand_type_members` が `from` の**レキシカルな外側クラス**まで見るようにしました。あわせて、型メンバーの別名本体がまだ抽象メンバーを名指している（`type C[T] = self.C[T]`）場合は、適用の結果を `this_class` から読み直します。

**5. 未解決の型名の診断**。`Missing[Int]` は kind エラーではなく **`not found: type Missing`**（nsc と同じ文面）にしました。以前は `Missing does not take type parameters` という誤解を招く文面でした。

**6. 型メンバーの上限経由のメンバー参照**。`type C[T] <: TypedType[T]` の `c: C[U]` に対して `c.name` が引けるよう、型パラメータ付き抽象メンバーの `class_sym_of` を**上限を辿って**解決します。相互再帰する境界（`type Self >: this.type <: Self`）で回らないよう visited 集合で止めます。ついでに、`subst_as_seen_from` が `Applied { ctor: TypeMember }` で無限再帰していた（`apply_type_ctor` が同じ型を返すため）のを直しました。

`Rep[?]` のような**境界内のワイルドカード**、`Query[?, U, C]` のような**高階パラメータを型引数に渡す形**、`Profile#AbstractTable[?]` のような**型引数付き `#` 射影**、package object の**型パラメータ付き別名**（`type DBIO[+R] = DBIOAction[R, NoStream, Effect.All]`）は、いずれもこのスライスの前から通っていることを最小再現で確認しました。

計測（`tests/slick_measure.sh`、slick 177 ファイル、`-Xsource:3`）では、型検査エラーは **13,245 → 13,164**、そのうち **kind 関連のエラーは 605 → 34** になりました。「`X does not take type parameters`」の多くは実は**未解決の型名**で、今は `not found: type X` と正しく出ます。

**残っているもの**:

- 高階型パラメータの**推論**（`def take[U, C[_]](q: Query[?, U, C])` に匿名サブクラスを渡すと `C` を合わせられない。明示の型引数なら通る）
- 残り 34 件の kind エラーはほぼ**別の担当**の下流です。`ColumnOption` / `::` / `Ordering` は `import slick.ast.*` の**スター形ワイルドカード import** が効いていないため（`import slick.ast._` は通る）、prelude の同名（`scala.math.Ordering` など）を拾ってしまっているものです。`IO` / `F` / `StreamIO` の kind 不一致は上の高階推論の穴です

## 実装していないもの

次は実装していません。スタブで「動いたことにする」こともしていません。言語側の残りとライブラリ側の残りを分けます。

言語:

- **def マクロの展開**。定義側（`def f: T = macro Impl.method`）はパースし、実装への
  バインディングをシンボルに記録しますが、**展開は未実装**です。呼び出し地点は
  `macro expansion is not implemented: cannot expand f (implementation Impl$.method)` と
  診断します（黙って通しません）。マクロ def は nsc と同じくバイトコードを持ちません。
  実行モデルの設計・実証・段階的な計画は **[`docs/macros.md`](docs/macros.md)** にあります。
  当面のゲート: whitebox マクロ / macro bundle / マクロバインディングの pickle /
  `scala.reflect` API の prelude / quasiquote / `reify`
- full nsc pickle（出しているのは TERMname / TYPEname / TYPEsym / CLASSsym / MODULESYM / VALsym / EXTref / EXTMODCLASSref / METHODtpe / POLYtpe / TYPEREFtpe / CLASSINFOtpe / TYPEBOUNDStpe / THIStpe / SINGLEtpe / NOPREFIXtpe / CONSTANTtpe / LITERALint / LITERALboolean / LITERALstring ほかリテラル / EXISTENTIALtpe / REFINEDtpe / SYMANNOT / ANNOTATEDtpe / ANNOTINFO / TREE（IDENTtree / SELECTtree / THIStree / SUPERtree / APPLYtree）のサブセット。ByteCodecs は SID-10。ワイヤ形式は nsc と同じ nentries + ビッグエンディアン Nat。vals は METHOD|STABLE|ACCESSOR ゲッター + NullaryMethodType。case class は CASE + フィールド CASEACCESSOR。Flags は nsc raw long を `rawToPickledFlags`（VARARGS / BRIDGE / JAVA を適用箇所で出す）。scalac 2.13.16 が `val` / パラメータ付き `def` / `id[T]` / `new Point` + `p.x` / companion apply `Point(...)` / term `Point` / extractor `unapply` / object の `def` / `def f(xs: List[_]): Int` / `@deprecated("msg", "2.13.0") def g` / `def me: this.type` / `def f(xs: List[_ <: AnyRef])` / `def h(x: Int @unchecked)` / `val one: 1` / `def lit(x: 1)` / `def nest(xs: List[_ <: List[_]])` / `def idRef(x: MixA with MixB { def f: Int })` / `@Ann(foo)` / `@Ann(c.x)` / `@Ann(this)` / `@Ann(classOf[Int])` / `@Ann(ident(1))` / `@Ann(this.x)` / `@Ann(super.foo)` / `@Ann(ident(ident(1)))` / `@Ann(foo = 1)` / `@Ann(foo = this.x)` / `@Ann(foo = bar)` / `Lib.join("a","b")` / `new OrdBox(1).compare(...)` を typecheck できる範囲。full pickle ではない。残る穴は Remaining）

対象外（診断する / パースしない）:

- コンパイラプラグイン
- Scala 3 構文 / TASTy。XML リテラルの未知エンティティ参照は診断する（elem / text / splice / 非プレフィックス属性 / `xmlns:p` / プレフィックス属性 / プレフィックス付き要素名 / コメント / CDATA / PI / `&amp;` `&lt;` `&gt;` `&quot;` `&apos;` / `&#N;` は実装済み）
- その他の `forSome { val x: T }`（`p.Inner forSome { val p: Outer }` は実装済み）。よくある unbounded `List[_]` / `T forSome { type X }` と境界付き `List[_ <: AnyRef]` / `List[X] forSome { type X <: AnyRef }` / 入れ子 `List[_ <: List[_]]` は実装済み
- 高階型パラメータの view bounds: scalac 2.13.16 は `F[_] <% Ordered[_]` / `F[_] <% Ordered[F[A]]` など全スペルを `type F takes type parameters` で拒否する。同じ診断（メソッド / クラスの proper `T <% V` は実装済み）。**context bound `F[_]: C` は別扱い**で、scalac 2.13.16 は受理する（実測済み）ので実装済み — 下の「型メンバーと高階 context bound」を参照

対象外から外した（このスライスで実装）:

- **package object の `implicit class`**（同じパッケージの他 compilation unit / `import pkg._`。pickle の IMPLICIT。ネスト classfile `package$Rich` は outer のメンバー `Rich` として `-cp` に載せる。トップレベル `implicit class` は nsc どおり `` `implicit` modifier cannot be used for top-level objects ``。import 無しでは enrichment が見えない。ローカル implicit class の合成は触っていない）
- **構造的代入** `x.foo = v`（`{ var foo: T }` または getter + `foo_=`）と構造的 `x(i) = v`（`update`）。nsc 2.13 どおり reflective `foo_=` / `update`。違法な `{ def foo: Int }; x.foo = 1` は `foo_= is not a member`
- scala-library 2.13.16 の **`IndexedSeq` / `immutable.Queue`**（本物の jar。`IndexedSeq(1,2)(1)` と `enqueue` / `dequeue`。無いメンバーは診断。偽 classfile は出さない）

ライブラリ:

- 完全な Scala 標準ライブラリ。`--scala-library` なしでは Option / List / FunctionN / Tuple2 は私有ランタイム。**jar にリンクしても** 完全な StringOps / 全 numeric enrichment（`RichByte` など）などは未対応
- implicit の `scala.Int` コンパニオンの enrichment（jar の `intWrapper` 経由の一部はリンク済み。`Int.MaxValue` 等の companion 定数そのものはこのスライスで実装済み。`RichInt` 側の追加メソッドは別）

パーサは未対応構文を黙って捨てず、診断と `Unimplemented` ノードを出します。

## アーキテクチャ

Cargo workspace のクレート:

| crate | 役割 |
| --- | --- |
| `scala-rs-span` | ソース位置と診断 |
| `scala-rs-lexer` | 字句解析（セミコロン推論用の改行トークン、`s`/`f`/`raw"..."` のモードスタック） |
| `scala-rs-parser` | 再帰下降パーサ。AST は nsc の `Tree` に近い |
| `scala-rs-pickle` | nsc `ScalaSignature` pickle のリーダ。`typer` と `backend` の両方が使う |
| `scala-rs-typer` | namer + typer + uncurry + lambda-lift + erasure。implicit 探索を含む |
| `scala-rs-backend` | JVM classfile 出力（major 52 / StackMapTable）と scala-rs ランタイム |
| `scala-rs-driver` | パイプライン駆動 |
| `scala-rs-cli` | コマンドライン。バイナリ名 `scala-rs` |

### ScalaSignature からのシンボル自動供給

標準ライブラリのメンバは長らく `crates/typer/src/prelude*.rs` に手書きしてきました。
2.13 互換に到達するにはこの方式では足りないので、
**scala-library の classfile に埋まっている `ScalaSignature`（nsc PickleFormat）を読んで
シンボルを自動供給する**経路を入れました。手書き prelude と併存し、
**prelude に無いメンバだけをオンデマンドで補完**します。

| モジュール | 役割 |
| --- | --- |
| `crates/pickle/src/codec.rs` | SID-10 ByteCodecs（ライタと共用） |
| `crates/pickle/src/classfile.rs` | `ScalaSignature` に届くだけの classfile 解析。`ScalaLongSignature`（配列値）も扱う |
| `crates/pickle/src/names.rs` | Scala `NameTransformer`（`++` ↔ `$plus$plus`）。backend と共用 |
| `crates/pickle/src/read.rs` | pickle **リーダ**。バイト列 → エントリ表 |
| `crates/pickle/src/sym.rs` | エントリ表 → クラスシグネチャ。親を辿り、型引数を代入して解決 |
| `crates/typer/src/pickle_supply.rs` | `SigType` → `scala_rs_parser::Type`、`SymbolTable` への投入 |
| `crates/backend/src/pickle.rs` | pickle **ライタ**（既存。nsc PickleFormat のサブセット） |

`crates/pickle` を独立クレートにしたのは、`crates/typer` が `crates/backend` に
依存できない（依存は逆向き）ためです。

#### リーダ

`read.rs` は nsc 2.13 `PickleFormat.scala` のタグを**全て**扱います
（シンボル / 型 / リテラル / `SYMANNOT` / `ANNOTINFO` / `CHILDREN` / `TREE` 各種 / `MODIFIERS`）。
方針として**未知タグや長さの合わない本体は握り潰さず `ReadError` にします**。
各エントリは宣言された長さをぴったり消費したことを検証するので、
形式の取り違えはそのままテストの失敗になります。

`sym.rs` は親クラスの classfile を `ClassSource` 越しにオンデマンドで開き、
**各ホップで親の型引数を代入**して、問い合わせたクラスの語彙で返します。

```
List#filter (from scala.collection.IterableOps)
    (pred: scala.Function1[A, scala.Boolean])scala.collection.immutable.List[A]
```

`IterableOps` の宣言は不透明な `C` を返しますが、代入により `List[A]` になります。
これが無いと typer は `C` を束縛できません。

#### 型検査への接続（`pickle_supply.rs`）

`check.rs` のメンバ解決が**完全に失敗したときだけ**呼ばれます。3 つの規則で嘘を防ぎます。

1. **手書き prelude が必ず勝つ。** 何も見つからなかった後にしか動かないので、
   既存の宣言を上書きも隠蔽もしません（`the_prelude_wins_over_the_pickle` で固定）。
2. **忠実に表せないメンバは供給しない。** 型が `scala_rs_parser::Type` に落ちない、
   erase 済み descriptor が一意に決まらない、といった場合は供給せず、
   従来どおり `is not a member` を出します。誤った型より無い方がましです。
3. **先読みしない。** 解決に失敗した `(受け手, 名前)` の組ごとに classfile 1 個、以降キャッシュ。
4. **クラス側とコンパニオン側を両方見て合併する。** 受け手がクラスのときは
   `PickleSupply::complete` がクラスとそのコンパニオンの**両方**に問い合わせ、結果を
   合併します。以前は「クラス側で 1 つでも供給できたらコンパニオンは見ない」だったため、
   答えが**関係のない大域状態に依存**していました。`scala.math.BigDecimal` は
   インスタンスメソッド `apply(MathContext)` を宣言していますが、その引数型は
   `java.math.MathContext` がシンボル表に入るまで表現できません。つまり
   `java.math.BigDecimal` に触れた後だけクラス側の供給が成功し、コンパニオンの 7 個の
   `apply` が丸ごと供給されなくなって、`BigDecimal(2)` が**文の順序次第で**通ったり
   通らなかったりしていました。合併は順序に依存しません。

オーバーロード集合が**複数の owner にまたがる**場合（クラスとそのコンパニオン）、
`check.rs` の `resolve_overload` は `Type::Overload` が型しか持たないため候補の
シンボルを `fun.sym` の owner から引き直します。これだと片方の owner の候補が
**丸ごと落ちる**ので、引き直しで失われる集合だけを `Check::overload_groups` に
覚えて使うようにしました。加えて、引数が 1 つも適合しなかったときに限り、
**クラス名を term 位置で使った選択**（nsc ではコンパニオンオブジェクトを指す）を
コンパニオンのメンバで広げてから 1 度だけ解決し直します
（`Check::widen_with_companion`）。エラーを出す直前の経路にしかいないので、
拒否を解決に変えることしかできません。

erase 済み descriptor は scalac の erasure を再実装するのではなく、
**classfile のメソッド表そのもの**から取ります（super とインタフェースを辿る。
`List#mkString` は `IterableOnceOps` の default method）。
同じ arity の候補が同じ階層に 2 つあるときは、選ばずに供給を諦めます。

`SCALA_RS_PICKLE_DEBUG=1` で、どのメンバをなぜ供給した / しなかったかを追えます。

#### 手書き prelude はどれだけ置き換えられるか（調査のみ・削除はしていない）

補完は「解決が失敗したときだけ」動くので、prelude にあるメンバを pickle から
作れるかどうかは通常わかりません。そこで一時的なフックで
`PickleSupply::complete` を prelude 済みのメンバに直接当て、
**pickle だけからどんなシグネチャが作れるか**を出させました。
`List` / `Option` / `Vector` の手書きメンバ 39 個のうち **38 個**は作れます。

| 受け手 | pickle から作れたもの |
| --- | --- |
| `List` | `map` `foreach` `head` `tail` `isEmpty` `length` `size` `nonEmpty` `reverse` `apply` `contains` `exists` `forall` `toList` `toString` `collect` `zip` `sum` `min` `max` `indexOf` `drop` `take` |
| `Option` | `get` `isEmpty` `isDefined` `getOrElse` `map` `flatMap` `foreach` `filter` `toList` `orElse` |
| `Vector` | `map` `apply` `length` `foreach` `head` |

作れなかったのは `List#withFilter` だけです（戻り値の `WithFilter` は
prelude が独自の形で持っているクラスで、pickle の形と噛み合いません）。

**ただしこれは「シグネチャの形が取れた」だけで、そのまま消せる根拠ではありません。**
実際 `List#zip` は `List[(tparam#289, tparam#2739)]`、`Option#orElse` は
`(=> #29[tparam#2719])Option[B]` のように、型パラメータの束縛が表示上崩れています。
置き換えるなら 1 つずつ、fixture の実行結果で確かめながら進める必要があります。
今回は**一覧と根拠を残すだけで、prelude からは何も削っていません**。

再現するには `PickleSupply::complete` を prelude 済みシンボルに直接呼ぶ
一時テストを書きます（メンバごとにシンボル表を作り直すので、39 個で約 100 秒かかります）。

#### codegen 側

**`gen.rs` の変更は不要でした。** 既存の仕組みがそのまま噛み合っています。

- メソッドシンボルの `jvm_name` が `(` で始まるとき、`method_desc_from_sym` は
  それを descriptor としてそのまま使う。供給したメンバはここに erase 済み
  descriptor を入れる。
- 呼び出しの owner はシンボルの owner、つまり**受け手クラス自身**なので、
  `invokevirtual scala/collection/immutable/List.mkString(...)` が
  継承メソッド・interface default method のどちらにも正しく解決される。
- 戻り値が `Object` の場合の checkcast / unbox は `maybe_unbox_erased_result` が既に行う。

#### 探索順は線形化（SLS 5.1.2）

継承したメンバがどの型引数束縛で返るかは、**親をどの順で探すか**で決まります。
`immutable.Set[A]` は `Iterable[A]` を混ぜたあとに `SetOps[A, Set, Set[A]]` を混ぜるので、
SLS 5.1.2 の「後の親が勝つ」規則により `IterableOps` の不透明な `C` は
`Set[A]` に解決されます。幅優先だと `Iterable` 経由で先に `IterableOps` に着き、
弱い `Iterable[A]` を返してしまい、その型はシンボル表に無いのでメンバごと供給を諦めていました。

`L(C) = C, L(Cn) +: … +: L(C1)` を `acc = L(Ci) ++ (acc − L(Ci))` として左から畳み込みます。
コレクション階層は広いので、深さと総ステップ数に上限を置いています。

#### 名前・オーバーロード・既定引数

- **演算子名**: nsc は演算子名を**エンコードしたまま**持ちます。`SetOps` は `&` を
  `$amp` として pickle し、classfile も `$amp` を宣言します。つまり pickle 検索も
  descriptor 検索も**エンコード名**で行い、登録するシンボルはソース名のままにします。
  `NameTransformer` は `crates/pickle/src/names.rs` に移して backend と共用しました
  （アセンブラは元から出力名をエンコードしているので codegen 側の変更は不要）。
- **オーバーロードの重複排除**は erase 後の引数リストで行います。同じに erase される宣言は
  別の親から見た同一 JVM メソッドで、結果型だけが違う場合
  （`IterableOps.map[B]: Iterable[B]` と `MapOps.map[K2,V2]: Map[K2,V2]`）は
  scalac が期待型で選ぶところをこちらは選べないので、線形化順で先に来る派生側を採ります。
  引数が違うもの（`Iterator.from(Int)` と `from(IterableOnce)`）は別物として両方残します。
- ただし**関数を取るオーバーロードは 1 つだけ**にします。ラムダの引数型は
  単一の期待型からしか推論できないので、2 つ目を足すと
  `xs.segmentLength(_ < 3)` が解けないオーバーロード集合になります。
- **既定引数**: パラメータに印を付け、クラスの `<method>$default$<n>` ゲッタも一緒に供給します
  （ゲッタは synthetic なので、目的を持って取りに行くときだけフィルタを緩めます）。
  ゲッタが供給できないメンバは**まるごと供給しません**。これが無いと
  `xs.lastIndexOf(2)` が型検査を通り、2 引数の descriptor に 1 引数で呼び出す
  バイトコードを出して VerifyError になります。

#### 今できること

`--scala-library <jar>` のとき、**prelude に 1 行も書かずに**次が型検査を通り、
`java -Xverify:all -cp out:jar Main` で scalac 2.13.16 と**バイト単位で同じ**出力を出します。

- `List`: `filter` `filterNot` `count` `exists` `forall` `take` `drop` `takeWhile`
  `dropWhile` `reverse` `mkString`(0/1/3 引数) `contains` `indexOf` `init` `last`
  `distinct` `startsWith` `splitAt` `partition` `span` `slice` `headOption`
  `lastOption` `find` `sorted` `sortBy` `sortWith` `max` `min` `maxBy` `toVector`
  `toSet` `toSeq` `toArray` `scanLeft` `zip` `padTo` `updated` `patch` `indexWhere`
  `tails` `combinations` `permutations` `zipWithIndex` `grouped` `sliding`(1/2 引数)
  `view` `iterator` `flatMap` `foldRight` `reduce` `reduceLeft` `copyToArray`
  `sum` `product`
- 演算子: `:+` `+:` `++` `++:`、`Set` の `&` `|` `&~` `++`、`Map` の `+` `-`
- `Map`: `map` `filter` `keySet`。`Set`: `map` `filter`。`Vector`: `map` `filter` `mkString`
- `Range` / `IndexedSeq`: `filter` `map`
- `Option`: `exists` `forall` `contains` `filter` `toList`
- companion: `Iterator.from` `.continually` `.single`、`List.fill` `.tabulate`、
  `Vector.fill` `.tabulate`、`Set.empty`

型パラメータの扱いで nsc と同じ判断を 2 つ入れています。

- `scala.package.List` / `scala.package.Ordering` は package object の**型エイリアス**で、
  pickle はエイリアス名で参照します。表を持たず、`scala/package.class` の pickle から
  `ALIASsym` を引いて展開します。ソースが同じ別名を**名前で**使う経路（`new
  NoSuchElementException("x")` / `Ref[F, A]`）は「jar の package object にある型エイリアス」
  節を参照。同じ `ALIASsym` を、展開ではなくパッケージの型メンバーとして登録します。
- `def max[B >: A](implicit ord: Ordering[B]): A` は呼び出し側に `B` を決める材料が
  ありません。scalac は下限 `A` に解決するので同じことをします。これが無いと typer は
  `Ordering[B]` を解けず、**エラーにせず `xs.max` を関数値へ eta 展開**して
  `Main$$$anonfun$4@...` を印字していました。この処理後も未決定の型パラメータが残る
  メンバは供給しません。

#### まだできないこと

- **シンボル表にあるクラスの作り直し**。`scala/collection/Seq` は
  `find_or_stub_java_class` が**型パラメータ無し**で入れているので `Seq[B]` が当たらず、
  `diff` / `intersect` / `union` / `indexOfSlice` / `containsSlice` は供給できません。
  一度は pickle の型パラメータを後付けして通しましたが、prelude が組んだシンボルを
  作り替えると影響が広く、`Seq` を変えた途端に**手書きの** `segmentLength` /
  `scanRight` が解決しなくなりました。動いているものを壊す方が悪いので表には触れません。
  表に無いクラスを新規にスタブするのはそのまま有効です。
- **スタブに親を付けない**（表に無いクラスを新規に作る場合）。親鎖を与えると
  部分型関係が全体的に変わるので、`Type::AnyRef` だけにしています。
  スタブ型は基本的にそれ自身としてしか使えません。
  なお、補完したクラスには pickle が宣言する親を**足します**（`attach_parents`）。
  これが無いと `Set#&`（引数が `collection.Set[A]`）は供給できても呼べません。
- **既定引数のゲッタ規約の食い違い**。`default_getter_apply` は既定引数より前の実引数を
  ゲッタに渡しますが、scalac は `SeqOps.lastIndexOf$default$2()` を引数無しで生成します。
  食い違う形は供給しません（`lastIndexOf` は現状これで落ちます）。
  直すには `check.rs` の既定引数経路に手を入れる必要があります。
- **`String.format`**: `augmentString` → `StringOps` の**拡張メソッド**経路で、
  補完フックはメンバ解決の失敗後にあります。受け手は `java/lang/String` なので
  `scala/` スコープにも入りません。
- **`scala.io.Source`**: pickle 経路ではなく Java classfile ローダ側で解決されています。
- **`reduceOption`**: `[B >: A](op: (B, B) => B): Option[B]`。ラムダから `B` を解けず、
  `bound_lo` を入れても届きません（推論側の話）。
- **`collect { case … }`**: インラインの部分関数リテラルからの推論は
  pickle 供給以前の typer の制約です（`list_collect.scala` は名前付き `PartialFunction` を渡しています）。

`SCALA_RS_PICKLE_DEBUG=1` で、どのメンバをなぜ供給した / しなかったかを追えます。

### 2.13.16 の pickle で分かったこと

- `List$.class` には `ScalaSignature` が**無い**。companion pair の pickle は
  クラス側の classfile にしか置かれないので、module class は companion にフォールバックする。
- 純 Java 由来のクラス（`BoxesRunTime` / `*Ref` / `ScalaNumber` / `scala.collection.concurrent` の
  ノード類）は `ScalaSignature` を持たない。
- `pflags`（pickle 上の flag ビット）は nsc が下位 12bit を `rawToPickledFlags` で
  並べ替えるため raw の Flags 位置とは違う。bit 12 以上は raw と同じで、
  **term と type で意味を共有するビットがある**（`COVARIANT`/`BYNAMEPARAM` が同じ bit、
  `TRAIT`/`DEFAULTPARAM` も同じ bit）。
  最初この表は bit 16 以上が 1 つずれていて、`is_public_api` が SYNTHETIC のつもりで
  STABLE を、LOCAL のつもりで JAVA を見ていた（過剰に弾く方向だったので結果は合っていた）。
  今は実シンボルに対する `flag_bits_match_the_library` で全位置を固定している。
  bit 30 以上は必要が無いので名前を付けていない。

## scalac 2.13 との比較

正直な差分です。

- **規模**: nsc のごく一部。言語仕様を満たしません。
- **ライブラリ**: デフォルトの **`compile` / `run`** は jar が自動検出できればリンクし、同名の私有 classfile は出さない。見つからなければ私有ランタイム。`--scala-library`（パス省略時は `SCALA_LIBRARY_JAR` / `/tmp/scala-rs-lib` / cwd を探索）で明示できる。**`--no-scala-library` は私有を強制**する。jar に乗るもの: `Option` / `Some` / `None` / `List` / `Nil` / `::` / `Function0` / `Function1` / `Tuple2` / `NotImplementedError` / `Predef$`（`println` / `assert` / `require` / `???` / `identity` / `locally` / `implicitly`）/ `any2stringadd` / `ArrowAssoc` の `->` / `intWrapper` / `RichInt`（`abs` / `max` / `min` / `to` / `until`）/ `longWrapper` / `RichLong`（`abs` / `max` / `min` / `to` / `until` → 本物の `NumericRange[Long]`）/ `doubleWrapper` / `RichDouble`（`abs` / `max` / `min`）/ `floatWrapper` / `RichFloat`（`abs` / `max` / `min`）/ `charWrapper` / `RichChar`（`isDigit` / `toInt` via `intValue$extension` / `to` / `until` → 本物の `NumericRange[Char]`）/ `byteWrapper` / `RichByte`（`abs` / `max` / `min` / `to` / `until` → 本物の `NumericRange[Byte]`）/ `shortWrapper` / `RichShort`（`abs` / `max` / `min` / `to` / `until` → 本物の `NumericRange[Short]`）/ `booleanWrapper` / `RichBoolean.compare`（インスタンス `compare(Object)`）/ `StringOps`（`toInt$extension` / `size$extension` / `$times$extension` / `take$extension` / `drop$extension` / `isEmpty` via `augmentString` / `toUpperCase`/`toLowerCase` inlined to `String` / `stripPrefix$extension` / `split$extension` / `stripSuffix$extension` / `padTo$extension(Int,Char)` / `linesIterator$extension` / `toIntOption$extension` / `stripMargin$extension` / `lines$extension` / `capitalize$extension` / `reverse$extension` / `slice$extension` / `takeRight$extension` / `dropRight$extension` / `contains$extension(Char)` / `head$extension` / `last$extension` / `stripLineEnd$extension` / `replaceAllLiterally$extension` / `tail$extension` / `init$extension` / `distinct$extension` / `mkString$extension`）/ `WithFilter` / `Iterator` / `Map` / `Vector` / `IndexedSeq`（unqualified `IndexedSeq(1, 2)(1)`）/ `Queue`（`scala.collection.immutable.Queue` の `enqueue` / `dequeue`）/ `ArrayBuffer`（`scala.collection.mutable.ArrayBuffer` の varargs `apply` / `+=` / `apply` / `update`）/ `ListBuffer`（`scala.collection.mutable.ListBuffer` の varargs `apply` / `+=` / `apply`）/ `StringBuilder`（`scala.collection.mutable.StringBuilder` の `new` / `+=` / `append` / `toString`）/ `HashMap`（`scala.collection.mutable.HashMap` の companion `empty` / varargs `apply` / `update` / `+=` / `apply` / `get`）/ `HashSet`（`scala.collection.mutable.HashSet` の companion `empty` / varargs `apply` / `+=` / `contains`）/ `LinkedHashMap`（`scala.collection.mutable.LinkedHashMap` の companion `empty` / varargs `apply` / `update` / `+=` / `apply` / 挿入順 `foreach`。`HashMap` は順を保証しない）/ `LinkedHashSet`（`scala.collection.mutable.LinkedHashSet` の companion `empty` / varargs `apply` / `+=` / `contains` / 挿入順 `foreach`）/ `ArrayDeque`（`scala.collection.mutable.ArrayDeque` の companion `empty` / varargs `apply` / `+=` / `prepend` / `apply`）/ `ArrayOps`（`intArrayOps` 経由の `head` / `tail` / `foreach` / `map[B: ClassTag]`。`longArrayOps` 経由の `head` / `foreach`。`refArrayOps` 経由の参照配列 `map`。私有 `ArrayOps` classfile は出さない）/ `Set` / `Seq` / `LazyList`（`empty` / `foreach` / **varargs `apply`**）/ `Either`（`Left` / `Right` と right-biased な `isLeft` / `isRight` / `map` / `flatMap` / `fold` / `getOrElse` / `orElse` / `swap` / `toOption` / `toSeq` / `contains` / `exists` / `forall` / `foreach` / `filterOrElse` / `left`。`Either$LeftProjection` classfile は出さない）/ `Try`（`Try$` / `Success` / `Failure` の `apply` と `isSuccess` / `isFailure` / `get` / `getOrElse` / `map` / `flatMap` / `filter` / `withFilter`（`Try$WithFilter`）/ `foreach` / `orElse` / `recover` / `recoverWith` / `collect` / `toOption` / `toEither` / `failed` / `transform` / `fold`）/ `Array$`（varargs `apply` + `ClassTag`）。dual-run: `hello` / `option_for` / `list_for` / `predef` / `predef_more` / `unapply` / `unapply_seq` / `iterator` / `map` / `vector` / `int_ops` / `string_ops` / `list_apply` / `set` / `long_ops` / `seq` / `either` / `float_ops` / `string_ops2` / `anonymous` / `eta` / `try_util` / `existentials` / `existential_bounds` / `implicit_specific` / `lambda_lift` / `view_bounds` / `view_bounds_class` / `hk_types` / `app` / `delayed_init` / `implicit_inherit_local` / `partial_function` / `list_collect` / `string_interp` / `overloading` / `classtag` / `context_bounds` / `context_bounds_class` / `type_member_hk` / `refine_hk` / `refine_bound` / `nested_proj` / `type_member_bounds` / `assign_op` / `collection_converters` / `pkg_implicit_class` / `structural_update` / `indexedseq_queue` / `string_ops3` / `byte_ops` / `arraybuffer` / `string_ops4` / `numeric_range` / `listbuffer` / `string_ops5` / `short_range` / `stringbuilder` / `string_ops6` / `long_range` / `hashmap` / `string_ops7` / `char_range` / `hashset` / `string_ops8` / `array_ops2` / `linkedhashmap` / `string_ops9` / `array_ops3` / `linkedhashset` / `string_ops10` / `array_ops4` / `arraydeque` / `custom_interp` / `array_ops` / `either_ops` / `either_left` / `either_for` / `option_x1` / `option_x2` / `try_ops` / `try_recover` / `try_for`。**まだ intrinsic / 私有、または未リンク**: 残りの StringOps、残りの numeric、他の mutable コレクション。`List.unapplySeq` は library では `SeqOps` の identity。`List`/`Seq`/`LazyList`/`Array` の varargs `apply` は **library のみ**。
- **ライブラリ**: デフォルトの **`compile` / `run`** は jar が自動検出できればリンクし、同名の私有 classfile は出さない。見つからなければ私有ランタイム。`--scala-library`（パス省略時は `SCALA_LIBRARY_JAR` / `/tmp/scala-rs-lib` / cwd を探索）で明示できる。**`--no-scala-library` は私有を強制**する。jar に乗るもの: `Option` / `Some` / `None` / `List` / `Nil` / `::` / `Function0` / `Function1` / `Tuple2` / `NotImplementedError` / `Predef$`（`println` / `assert` / `require` / `???` / `identity` / `locally` / `implicitly`）/ `any2stringadd` / `ArrowAssoc` の `->` / `intWrapper` / `RichInt`（`abs` / `max` / `min` / `to` / `until` / `toBinaryString` / `toHexString` / `toOctalString` / `sign`。`Range`（`withFilter` / `filter` / `map` / `flatMap` / `foldLeft` / `foldRight` / `sum` / `product` / `min` / `max` / `toList` / `toVector` / `zipWithIndex` / `take` / `drop` など）と `scala.math`（`abs` / `max` / `min` / `signum` / `pow` / `sqrt` / `floor` / `ceil` / `round` / `random`）も乗った）/ `longWrapper` / `RichLong`（`abs` / `max` / `min` / `to` / `until` → 本物の `NumericRange[Long]`）/ `doubleWrapper` / `RichDouble`（`abs` / `max` / `min`）/ `floatWrapper` / `RichFloat`（`abs` / `max` / `min`）/ `charWrapper` / `RichChar`（`isDigit` / `toInt` via `intValue$extension` / `to` / `until` → 本物の `NumericRange[Char]`）/ `byteWrapper` / `RichByte`（`abs` / `max` / `min` / `to` / `until` → 本物の `NumericRange[Byte]`）/ `shortWrapper` / `RichShort`（`abs` / `max` / `min` / `to` / `until` → 本物の `NumericRange[Short]`）/ `booleanWrapper` / `RichBoolean.compare`（インスタンス `compare(Object)`）/ `StringOps`（`toInt$extension` / `size$extension` / `$times$extension` / `take$extension` / `drop$extension` / `isEmpty` via `augmentString` / `toUpperCase`/`toLowerCase` inlined to `String` / `stripPrefix$extension` / `split$extension` / `stripSuffix$extension` / `padTo$extension(Int,Char)` / `linesIterator$extension` / `toIntOption$extension` / `stripMargin$extension` / `lines$extension` / `capitalize$extension` / `reverse$extension` / `slice$extension` / `takeRight$extension` / `dropRight$extension` / `contains$extension(Char)` / `head$extension` / `last$extension` / `stripLineEnd$extension` / `replaceAllLiterally$extension` / `tail$extension` / `init$extension` / `distinct$extension` / `mkString$extension`）/ `WithFilter` / `Iterator` / `Map` / `Vector` / `IndexedSeq`（unqualified `IndexedSeq(1, 2)(1)`）/ `Queue`（`scala.collection.immutable.Queue` の `enqueue` / `dequeue`）/ `ArrayBuffer`（`scala.collection.mutable.ArrayBuffer` の varargs `apply` / `+=` / `apply` / `update`）/ `ListBuffer`（`scala.collection.mutable.ListBuffer` の varargs `apply` / `+=` / `apply`）/ `StringBuilder`（`scala.collection.mutable.StringBuilder` の bare 名 / `new` / `append` 全オーバーロード / `+=` / `++=` / `insert` / `deleteCharAt` / `setLength` / `reverse` / `clear` / `toString` / `result`）/ `HashMap`（`scala.collection.mutable.HashMap` の companion `empty` / varargs `apply` / `update` / `+=` / `apply` / `get`）/ `HashSet`（`scala.collection.mutable.HashSet` の companion `empty` / varargs `apply` / `+=` / `contains`）/ `LinkedHashMap`（`scala.collection.mutable.LinkedHashMap` の companion `empty` / varargs `apply` / `update` / `+=` / `apply` / 挿入順 `foreach`。`HashMap` は順を保証しない）/ `LinkedHashSet`（`scala.collection.mutable.LinkedHashSet` の companion `empty` / varargs `apply` / `+=` / `contains` / 挿入順 `foreach`）/ `ArrayDeque`（`scala.collection.mutable.ArrayDeque` の companion `empty` / varargs `apply` / `+=` / `prepend` / `apply`）/ `ArrayOps`（`intArrayOps` 経由の `head` / `tail` / `foreach` / `map[B: ClassTag]`。`longArrayOps` 経由の `head` / `foreach`。`refArrayOps` 経由の参照配列 `map`。私有 `ArrayOps` classfile は出さない）/ `Set` / `Seq` / `LazyList`（`empty` / `foreach` / **varargs `apply`**）/ `Either`（`Left` / `Right` / `isLeft` / `getOrElse` / `map`）/ `Try`（`Try$` / `Success` / `Failure` の `apply` / `map` / `getOrElse`）/ `Array$`（varargs `apply` + `ClassTag`）。dual-run: `hello` / `option_for` / `list_for` / `predef` / `predef_more` / `unapply` / `unapply_seq` / `iterator` / `map` / `vector` / `int_ops` / `string_ops` / `list_apply` / `set` / `long_ops` / `seq` / `either` / `float_ops` / `string_ops2` / `anonymous` / `eta` / `try_util` / `existentials` / `existential_bounds` / `implicit_specific` / `lambda_lift` / `view_bounds` / `view_bounds_class` / `hk_types` / `app` / `delayed_init` / `implicit_inherit_local` / `partial_function` / `list_collect` / `string_interp` / `overloading` / `classtag` / `context_bounds` / `context_bounds_class` / `type_member_hk` / `refine_hk` / `refine_bound` / `nested_proj` / `type_member_bounds` / `assign_op` / `collection_converters` / `pkg_implicit_class` / `structural_update` / `indexedseq_queue` / `string_ops3` / `byte_ops` / `arraybuffer` / `string_ops4` / `numeric_range` / `listbuffer` / `string_ops5` / `short_range` / `stringbuilder` / `string_ops6` / `long_range` / `hashmap` / `string_ops7` / `char_range` / `hashset` / `string_ops8` / `array_ops2` / `linkedhashmap` / `string_ops9` / `array_ops3` / `linkedhashset` / `string_ops10` / `array_ops4` / `arraydeque` / `custom_interp` / `array_ops`。**まだ intrinsic / 私有、または未リンク**: 残りの StringOps、残りの numeric、他の mutable コレクション。`List.unapplySeq` は library では `SeqOps` の identity。`List`/`Seq`/`LazyList`/`Array` の varargs `apply` は **library のみ**。
- **ライブラリ**: デフォルトの **`compile` / `run`** は jar が自動検出できればリンクし、同名の私有 classfile は出さない。見つからなければ私有ランタイム。`--scala-library`（パス省略時は `SCALA_LIBRARY_JAR` / `/tmp/scala-rs-lib` / cwd を探索）で明示できる。**`--no-scala-library` は私有を強制**する。jar に乗るもの: `Option` / `Some` / `None` / `List` / `Nil` / `::` / `Function0` / `Function1` / `Tuple2`（`_1` / `_2` に加え `swap` / `toString`）/ `NotImplementedError` / `Predef$`（`println` / `assert` / `require` / `???` / `identity` / `locally` / `implicitly`）/ `any2stringadd` / `ArrowAssoc` の `->` / `intWrapper` / `RichInt`（`abs` / `max` / `min` / `to` / `until`）/ `longWrapper` / `RichLong`（`abs` / `max` / `min` / `to` / `until` → 本物の `NumericRange[Long]`）/ `doubleWrapper` / `RichDouble`（`abs` / `max` / `min`）/ `floatWrapper` / `RichFloat`（`abs` / `max` / `min`）/ `charWrapper` / `RichChar`（`isDigit` / `toInt` via `intValue$extension` / `to` / `until` → 本物の `NumericRange[Char]`）/ `byteWrapper` / `RichByte`（`abs` / `max` / `min` / `to` / `until` → 本物の `NumericRange[Byte]`）/ `shortWrapper` / `RichShort`（`abs` / `max` / `min` / `to` / `until` → 本物の `NumericRange[Short]`）/ `booleanWrapper` / `RichBoolean.compare`（インスタンス `compare(Object)`）/ `StringOps`（`toInt$extension` / `size$extension` / `$times$extension` / `take$extension` / `drop$extension` / `isEmpty` via `augmentString` / `toUpperCase`/`toLowerCase` inlined to `String` / `stripPrefix$extension` / `split$extension` / `stripSuffix$extension` / `padTo$extension(Int,Char)` / `linesIterator$extension` / `toIntOption$extension` / `stripMargin$extension` / `lines$extension` / `capitalize$extension` / `reverse$extension` / `slice$extension` / `takeRight$extension` / `dropRight$extension` / `contains$extension(Char)` / `head$extension` / `last$extension` / `stripLineEnd$extension` / `replaceAllLiterally$extension` / `tail$extension` / `init$extension` / `distinct$extension` / `mkString$extension`）/ `WithFilter` / `Iterator` / `Map`（`apply` / `get` / `updated` / `+` / `foreach` に加え `getOrElse` / `contains` / `keys` / `values` / `keySet` / `-` / `size` / `isEmpty` / `nonEmpty` / `filter` / `toList` / `toSeq` / `iterator` / `mkString` / `head` / `foldLeft` / `withDefaultValue` / `view` / `MapView.mapValues`）/ `Vector`（`apply` / `length` / `updated` / `:+` / `foreach` に加え `size` / `isEmpty` / `nonEmpty` / `head` / `map` / `filter` / `toList` / `toSeq` / `iterator` / `mkString` / `foldLeft`）/ `IndexedSeq`（unqualified `IndexedSeq(1, 2)(1)`）/ `Queue`（`scala.collection.immutable.Queue` の `enqueue` / `dequeue`）/ `ArrayBuffer`（`scala.collection.mutable.ArrayBuffer` の varargs `apply` / `+=` / `apply` / `update` / `length` / `size` / `isEmpty` / `nonEmpty` / `head` / `last` / `mkString`(0/1/3) / `foreach` / `map` / `filter` / `toList` / `iterator` / `clear` / `remove` / `insert` / `contains` / `indexOf` / `reverse` / `foldLeft` / `append` / `++=` / `-=` / `sortBy` / `sorted`）/ `ListBuffer`（`scala.collection.mutable.ListBuffer` の同じメンバー一式）/ 新規 `mutable.Map[K, V]` と `mutable.Set[A]`（従来は `HashMap` / `HashSet` のみ乗っていた。`Map$` / `Set$` companion は `MapFactory$Delegate` / `IterableFactory$Delegate` 経由で `HashMap` / `HashSet` に実行時委譲するが静的型はトレイトのまま。`mutable.Map` は `apply` / `get` / `update` / `getOrElse` / `getOrElseUpdate` / `contains` / `keys` / `values` / `+=` / `-=` / `remove` / `size` / `isEmpty` / `nonEmpty` / `clear` / `foreach` / `filter` / `toList` / `toSeq` / `iterator` / `mkString`、`mutable.Set` は `contains` / `+=` / `-=` / `remove` / `size` / `isEmpty` / `nonEmpty` / `clear` / `foreach` / `map` / `filter` / `toList` / `toSeq` / `iterator` / `mkString`）/ `StringBuilder`（`scala.collection.mutable.StringBuilder` の `new` / `+=` / `append` / `toString`）/ `HashMap`（`scala.collection.mutable.HashMap` の companion `empty` / varargs `apply` / `update` / `+=` / `apply` / `get`）/ `HashSet`（`scala.collection.mutable.HashSet` の companion `empty` / varargs `apply` / `+=` / `contains`）/ `LinkedHashMap`（`scala.collection.mutable.LinkedHashMap` の companion `empty` / varargs `apply` / `update` / `+=` / `apply` / 挿入順 `foreach`。`HashMap` は順を保証しない）/ `LinkedHashSet`（`scala.collection.mutable.LinkedHashSet` の companion `empty` / varargs `apply` / `+=` / `contains` / 挿入順 `foreach`）/ `ArrayDeque`（`scala.collection.mutable.ArrayDeque` の companion `empty` / varargs `apply` / `+=` / `prepend` / `apply`）/ `ArrayOps`（`intArrayOps` 経由の `head` / `tail` / `foreach` / `map[B: ClassTag]`。`longArrayOps` 経由の `head` / `foreach`。`refArrayOps` 経由の参照配列 `map`。私有 `ArrayOps` classfile は出さない）/ `Set`（`contains` / `foreach` に加え `+` / `-` / `++` / `size` / `isEmpty` / `nonEmpty` / `filter` / `map` / `toList` / `toSeq` / `iterator` / `mkString` / `head`）/ `Seq` / `LazyList`（`empty` / `foreach` / **varargs `apply`**）/ `Either`（`Left` / `Right` / `isLeft` / `getOrElse` / `map`）/ `Try`（`Try$` / `Success` / `Failure` の `apply` / `map` / `getOrElse`）/ `Array$`（varargs `apply` + `ClassTag`）。dual-run: `hello` / `option_for` / `list_for` / `predef` / `predef_more` / `unapply` / `unapply_seq` / `iterator` / `map` / `vector` / `int_ops` / `string_ops` / `list_apply` / `set` / `long_ops` / `seq` / `either` / `float_ops` / `string_ops2` / `anonymous` / `eta` / `try_util` / `existentials` / `existential_bounds` / `implicit_specific` / `lambda_lift` / `view_bounds` / `view_bounds_class` / `hk_types` / `app` / `delayed_init` / `implicit_inherit_local` / `partial_function` / `list_collect` / `string_interp` / `overloading` / `classtag` / `context_bounds` / `context_bounds_class` / `type_member_hk` / `refine_hk` / `refine_bound` / `nested_proj` / `type_member_bounds` / `assign_op` / `collection_converters` / `pkg_implicit_class` / `structural_update` / `indexedseq_queue` / `string_ops3` / `byte_ops` / `arraybuffer` / `string_ops4` / `numeric_range` / `listbuffer` / `string_ops5` / `short_range` / `stringbuilder` / `string_ops6` / `long_range` / `hashmap` / `string_ops7` / `char_range` / `hashset` / `string_ops8` / `array_ops2` / `linkedhashmap` / `string_ops9` / `array_ops3` / `linkedhashset` / `string_ops10` / `array_ops4` / `arraydeque` / `custom_interp` / `array_ops`。**まだ intrinsic / 私有、または未リンク**: 残りの StringOps、残りの numeric、他の mutable コレクション。`List.unapplySeq` は library では `SeqOps` の identity。`List`/`Seq`/`LazyList`/`Array` の varargs `apply` は **library のみ**。
- **object**: scalac と同様、`Main$`（モジュール）と静的フォワーダ `Main` を出します。`java Main` が動くのはそのためです。
- **プリミティブ**: `Int` の `+` などは `scala.Int` のボックスメソッドではなく、JVM 命令（`iadd` など）として出します。
- **trait**: 抽象メンバーだけの trait は JVM interface です。具象メンバーは `T$class` 静的実装と、C3 線形化順のインスタンスフォワーダです。Java 8 default method は使いません。`val` は getter/setter + `$init$` です。`abstract override` は `T$$super$m` です。
- **名前付き引数**: 呼び出し側で `f(b = 2, a = 1)` を並べ替えます。巨大な rewrite フェーズはありません。メソッド・`apply`・`copy`・コンストラクタ・オーバーロードのある呼び出しのすべてで並べ替え、省略されたデフォルト引数はその場で埋めます（通常のメソッドは `{method}$default$n` ゲッター経由、コンストラクタは呼び出し側でデフォルト式を型付けします）。extractor パターンでも case class なら並べ替えます。パーサは `x = e` を一律に代入としてパースし、**引数位置のそれを名前付き引数として扱うのは typer** です（nsc と同じ作り）。
- **try**: Code 属性に例外テーブルと StackMapTable を出します。
- **ラムダ**: `FunctionN` を実装する合成クラス（`Main$$$anonfun$0` など）です。SAM 期待位置ではその Java インタフェース（`Runnable` / `Comparator` / `java.util.function.Function`）を実装します。`PartialFunction` 期待位置の `{ case }` は `scala/PartialFunction` を実装し、`isDefinedAt` / `apply` / `applyOrElse` を出します。invokedynamic / LambdaMetaFactory は使いません。
- **フェーズ**: nsc の mixin などの独立パスはありません。**uncurry**、**lambda-lift**（ネスト def）、erasure、ラムダのクロージャ変換はあります。
- **sealed**: 非網羅 match は scalac と同様 warning です。`-Xfatal-warnings` でエラーになります。
- **AnyVal**: scalac は値クラスのクラスファイルと拡張メソッドの両方を出します。scala-rs も同じで、`new C(x)` は underlying に消え、呼び出しは `$extension` 静的メソッドです。参照が要る位置（`Any` / universal trait / 型引数 / 配列要素）では nsc と同じく `new C(u)` で box し、`equals` / `hashCode` も underlying から合成します。違いは `$extension` の本体の置き場所で、nsc はコンパニオン `C$` に置いてクラス側をフォワーダにしますが、scala-rs はクラス側に直接出します。
- **Predef / StringOps**: 私有では `assert` / `require` / `???` / `->`（`Tuple2` 直結）/ `identity` / `locally` / `implicitly` / `any2stringadd` と String の `length`/`toInt`/`isEmpty`。library では `Predef$.println/assert/require/???/identity/locally/implicitly`、`any2stringadd.$plus$extension`、`ArrowAssoc.$minus$greater$extension`、`intWrapper` → `RichInt.abs$extension` / `max$extension` / `to$extension` / `until$extension`、`longWrapper` → `RichLong.abs$extension` / `max$extension` / `to` / `until`（`NumericRange$.inclusive` / `apply` + `Numeric$LongIsIntegral$`）、`doubleWrapper` → `RichDouble.abs$extension` / `max$extension`、`floatWrapper` → `RichFloat.abs$extension` / `max$extension`、`charWrapper` → `RichChar.isDigit$extension` / `intValue$extension`（`.toInt`）/ `to` / `until`（`NumericRange$.inclusive` / `apply` + `Numeric$CharIsIntegral$`）、`byteWrapper` → `RichByte.abs$extension` / `max$extension` / `to` / `until`（`NumericRange$.inclusive` / `apply` + `Numeric$ByteIsIntegral$`）、`shortWrapper` → `RichShort.max$extension` / `to` / `until`（`NumericRange$.inclusive` / `apply` + `Numeric$ShortIsIntegral$`）、`booleanWrapper` → `RichBoolean.compare(Object)`、`augmentString` → `StringOps.toInt$extension` / `size$extension`（`.length`）/ `$times$extension` / `take$extension` / `drop$extension` / `stripPrefix$extension` / `split$extension` / `stripSuffix$extension` / `padTo$extension`（`Int, Char`）/ `linesIterator$extension` / `toIntOption$extension` / `stripMargin$extension` / `lines$extension` / `capitalize$extension` / `reverse$extension` / `slice$extension` / `takeRight$extension` / `dropRight$extension` / `contains$extension`（`.isEmpty` / `.toUpperCase` / `.toLowerCase` は StringOps 経由で `String` にインライン。`startsWith` / `endsWith` / `indexOf` は nsc どおり `java.lang.String`。`head$extension` / `last$extension` / `stripLineEnd$extension` / `replaceAllLiterally$extension` / `tail$extension` / `init$extension` / `distinct$extension` / `mkString$extension` / `filter$extension` / `reverseIterator$extension`）。`intArrayOps` → `ArrayOps.head$extension` / `tail$extension` / `foreach$extension(Object,Function1)V` / `map$extension(Object,Function1,ClassTag)Object`。`longArrayOps` → 同じ `head` / `foreach`（`[J]`）。`refArrayOps` → 参照配列の `map`。**`StringOps` / `ArrayOps` / `RichInt` / `RichLong` / `RichDouble` / `RichFloat` / `RichChar` / `RichByte` / `RichShort` / `RichBoolean` / `ArrayBuffer` / `ListBuffer` / `StringBuilder` / `HashMap` / `HashSet` / `LinkedHashMap` / `LinkedHashSet` / `ArrayDeque` / `NumericRange` classfile は出していません。**
- **unapplySeq**: `List` とユーザー定義 extractor、`_*`、名前付き case class パターン。library リンク時の `List.unapplySeq` は `SeqOps` 戻り。

scalac の代替ではありません。サブセットの再実装です。

## テスト

```bash
cargo test
```

pickle リーダの回帰テストは `crates/pickle/tests/lib_jar.rs` です。
`/tmp/scala-rs-lib/scala-library-2.13.16.jar`（または `SCALA_LIBRARY_JAR`）があるとき、
jar の**全 classfile**を走査して次を見ます。jar が無ければスキップします。

- `reads_every_pickle_in_scala_library`: `@ScalaSignature` / `@ScalaLongSignature` を
  宣言している classfile（2.13.16 では 2891 個中 799 個）の pickle が**全て**読めること。
  「宣言しているか」は定数プールのディスクリプタのバイト検索という独立した判定で見るので、
  抽出漏れも失敗になる。合計 169275 エントリ。主要タグが実際に登場していることも確認する。
  さらに全 pickle からクラスシグネチャを組み立て（2209 クラス）、
  **未解決参照がゼロ**であること（`ClassSig::unresolved` が空）も見る。
- `list_pickle_has_the_collection_members`: `List.class` の pickle から `List` と `map`。
- `resolves_inherited_list_members_through_parents`: `List#filter` / `sum` / `mkString` /
  `map` / `flatMap` / `head` / `foldLeft` を**親クラスを辿って**解決できること。
- `resolves_module_class_members`: module class（`object List`）の解決。
- `set_filter_binds_c_through_setops_not_iterable` / `linearization_puts_later_parents_first`:
  探索順が SLS 5.1.2 の線形化であること（`Set#filter` が `Iterable[A]` でなく `Set[A]` を返す）。
- `flag_bits_match_the_library`: pickle 上の flag ビット位置を実シンボルで固定する
  （trait / accessor / stable / synthetic / private+local / 既定引数）。

自前ライタが書いた pickle を自前リーダで読み直すテストは
`crates/backend/tests/pickle_roundtrip.rs` です。

型検査への接続は `crates/cli/tests/pickle_lib.rs`（fixture 接頭辞 `pickle_lib`）です。
`e2e.rs` とは別ファイルにしています。

- `pickle_lib1`（継承したメンバ）/ `pickle_lib2`（Ordering・型エイリアス・カリー化）/
  `pickle_lib3`（線形化とスタブしたクラス）/ `pickle_lib4`（演算子・companion・`sum`）:
  jar にリンクしてコンパイルし、`java -Xverify:all` で期待 stdout と比較する。
  **4 つとも期待値は本物の scalac 2.13.16 の出力とバイト単位で一致することを確認済み**
  （自前コンパイラ同士の比較ではない）。
- `a_member_in_no_pickle_is_still_an_error`（`pickle_lib1_bad`）:
  どの pickle にも無い名前は補完せず `is not a member` になる。
- `private_runtime_still_diagnoses_library_only_members`:
  `--no-scala-library` では読む pickle が無いので、黙って通さずきちんと診断する。

補完の不変条件は `crates/typer/src/pickle_supply.rs` のユニットテストで固定しています。
`the_prelude_wins_over_the_pickle`（手書き `List#map` は置き換えも複製もされない。
一方 prelude に無い `filter` は descriptor 付きで供給される）と
`nothing_is_supplied_when_nothing_is_missing`（先読みしない）です。

実行時の期待値は `tests/fixtures/` にあります。各 `.scala` に対して `tests/fixtures/expected/` に同名の `.txt`（`println` と同じ末尾改行付きの stdout）を置いています。`java` がある環境では CLI の e2e が stdout を比較します。

scala-library 2.13.16 が取れる環境では、`--scala-library` でコンパイルして `java -cp out:scala-library.jar Main` を走らせ、私有ランタイム版と同じ stdout になることを確認します（私有の `scala/Option.class` / `scala/Predef$.class` 等が出ないこと）。対象のフィクスチャ一覧は `crates/cli/tests/e2e.rs` の `scala_library_dual_run_*` テストが正本です。フラグなしの `compile` は jar を自動検出してリンクし、`--no-scala-library` は私有ランタイムを出します。
複数ファイルを 1 回の `compile` に渡す回帰テストは `crates/cli/tests/multifile.rs`、
ソースは `tests/multi/` です。ケーキパターンのフィクスチャは接頭辞 `cake`（`cake_profile.scala` /
`cake_relational.scala` / `cake_component.scala` の正常系と、`cake_bad_leaf.scala` /
`cake_bad_base.scala` の異常系）で、正常系は**ファイル順を入れ替えても**同じ結果になることまで
見ます。異常系は線形化に無い名前（どこにも無い `Missing`、ミックスインされていない
コンポーネントの `Detached`）が黙って通らないことを固定します。どちらも real scalac 2.13.16 と
出力・診断が一致することを確認済みです。

prelude の穴・小さな型検査の穴を潰したフィクスチャは接頭辞 `gap_`（`gap_numeric` / `gap_asinstanceof` / `gap_copy` / `gap_exception`、それぞれ `_bad` の異常系あり）で、`crates/cli/tests/e2e.rs` ではなく別ファイル `crates/cli/tests/gaps.rs` に置いています（他エージェントが同時に `e2e.rs` を編集していてもコンフリクトしないように）。`--scala-library` dual-run に加えて、`scalac` が取れる環境では実行結果を毎回その場で real scalac の出力と直接 diff します（`expected/*.txt` は real scalac の出力から作成済み）。`gap_copy` は private ランタイムでも動きます。

ボックス型（`java.lang.Integer` と `scala.Int` の分離）のフィクスチャは接頭辞 `boxed` で、同じ理由から `crates/cli/tests/boxed.rs` に置いています。`boxed.scala` は `--scala-library` dual-run と real scalac との実行結果 diff の両方、`boxed_rt.scala` は私有ランタイムでも動く部分（変換 intrinsic と JDK のラッパークラス）を dual-run と private ランタイムの両方で見ます。`boxed_bad.scala` は real scalac が拒否する 5 つの変換（`java.lang.Integer = 3L` / `Long` の箱 → `Integer` / `Long` の箱 → `Int` / 箱 → `String` / インスタンス経由の static `parseInt`）を、jar モードと私有ランタイムモードの両方で診断します。`scala.Int` と `java.lang.Integer` が別シンボルであることは typer 側の不変条件テスト `prelude_has_no_duplicate_jvm_classes` でも見ています（同じ JVM 名を共有してよいのは値クラスとその箱のペアだけで、箱の側は `java.lang` が持つ、という形に言い直しました）。

数値変換の塔と `Byte` / `Short` のプリミティブ化のフィクスチャは接頭辞 `numt`（`numt.scala` / `numt_bad.scala`）で、同じ理由から `crates/cli/tests/numtower.rs` に置いています。`numt.scala` は 7×7 の変換すべて（NaN / ±Inf / MIN・MAX 込み）、`Byte` / `Short` のパラメータ・戻り値・フィールド・配列・オーバーフロー、演算子の昇格、弱適合、`Short` スクルティニーの `Int` 定数パターンを 1 本にまとめてあり、**私有ランタイムと `--scala-library` の両方**で `java -Xverify:all` の下に走らせて `expected/numt.txt`（real scalac 2.13.16 の stdout）と比較します。`no_scala_byte_or_short_class_reference` は、出した classfile の定数プールに `scala/Byte` / `scala/Short` という実在しないクラス名が現れないことを直接見ます。`numt_bad.scala` は real scalac も拒否するもの（暗黙の縮小変換、範囲外の定数、`Boolean` / `Unit` の `toX`、`Double` → `Int`）を jar モードと私有ランタイムモードの両方で診断します。プリミティブ配列の要素命令（`laload` / `dastore` / `baload` …）と `1 + 2.5f` の `i2f` は個別のテストで固定しています。

`agent/smallgaps` スライス（`@inline` / `@noinline` の配置、curried case class companion、companion への後方参照、`Option.flatMap` の多相性、`None`/`Some` の `lub`、`Iterable.apply`）のフィクスチャは接頭辞 `sgap`（`sgap` / `sgap_lib`）で、同じ理由から `crates/cli/tests/smallgaps.rs` に置いています。`sgap.scala` は `--no-scala-library` で `check` 済み、`sgap_lib.scala` は `Iterable.apply` が library ABI（`IterableFactory$Delegate.apply` の継承）にしか無いため library dual-run 専用（`fixtures_sgap_lib_without_library_is_error` で `--no-scala-library` が診断のまま残ることも見ています）。

オーバーロード集合が別のクラスの読み込みで消える回帰のフィクスチャは接頭辞 `oshadow`（`oshadow` / `oshadow_java_first` / `oshadow_java_last` / `oshadow_bad`）で、同じ理由から `crates/cli/tests/overloadshadow.rs` に置いています。`oshadow.scala` は `--scala-library` dual-run に加えて real scalac 2.13.16 の実行結果とも直接比較します（`oshadow_matches_scalac`）。`oshadow_java_first.scala` と `oshadow_java_last.scala` は `java.math.BigDecimal` の位置だけを入れ替えた同じプログラムで、`oshadow_order_independent` が両方通ることと stdout が一致することを固定します。`oshadow_bad.scala` は `BigDecimal(Some(1))`（real scalac も拒否）が `no matching overload` になり、しかも**候補一覧が丸ごと**出る（`(String)BigDecimal` を含む）ことを見ます。`oshadow_without_library_is_error` は `--no-scala-library` で `not found: value BigDecimal` の診断が残ることを見ます。
`agent/parentimpl` スライス（親コンストラクタの implicit 節・デフォルト引数の補完）のフィクスチャは接頭辞 `pimpl`（`pimpl` / `pimpl_bad`）で、同じ理由から `crates/cli/tests/parentimpl.rs` に置いています。`pimpl.scala` は slick の `ConstColumn` 形（`class ConstColumn[T : TT] extends TypedRep[T]`）、明示節＋2 引数の implicit 節、全部デフォルト／末尾だけデフォルト、デフォルト節＋implicit 節、匿名クラスの親、引数無しの `new` を 1 本にまとめ、**私有ランタイムと `--scala-library` の両方**で `java -Xverify:all` の下に走らせます。`real_scalac_dual_run_pimpl` は real scalac 2.13.16 でも同じソースを走らせて stdout が一致することを見ます（`expected/pimpl.txt` は scalac の出力そのもの）。`pimpl_late_a.scala` / `pimpl_late_z.scala` は**子を親より先にコンパイル**して、親の context bound の evidence がシグネチャパス時点で未生成でも埋まる（＝ファイル順に依存しない）ことを見ます。`pimpl_bad.scala` は witness の無い親 implicit 節が**黙って通らない**ことを固定し、`pimpl_bad_reports_the_extends_clause_once` で診断が `extends` の行に 1 件だけ出る（3 パス分に増えない）ことも見ています。

trait の `val` / `override val` / `var` の実行時表現と `case object` の合成メンバーのフィクスチャは接頭辞 `tval`（`tval` / `tval_bad`）で、同じ理由から `crates/cli/tests/traitval.rs` に置いています。`tval.scala` は私有ランタイム（`--no-scala-library`）と library dual-run の両方で走らせ、`expected/tval.txt` は **real scalac 2.13.16 の出力そのもの**です（3 モードがバイト単位で一致することを確認済み）。バイトコード側の不変条件も 2 本のテストで固定しています。`trait_val_setters_follow_nsc_names` は mixin setter が nsc と同じ `Named$_setter_$label_$eq` であること、`override val` したクラスのその setter が空実装（`putfield` なし）であること、trait の `var` への代入が `putfield` ではなく `count_$eq` 呼び出しであることを `javap -p -c` で見ます。`case_object_members_are_on_the_module_class` は `Asc$` に `toString` / `productPrefix` / `hashCode` / `productArity` が出ていることを見ます。`tval_bad.scala` は trait の `val` への代入が `reassignment to val` になることを固定します。

値クラス + universal trait、`}` の次の行の単項マイナス、`X.type` の名前解決のフィクスチャは
接頭辞 `vcls`（`vcls` / `vcls_nl` / `vcls_arr` / `vcls_hnil` / `vcls_bad`）で、同じ理由から
`crates/cli/tests/valclass.rs` に置いています。`vcls.scala`（値クラス + universal trait、`Any`
への代入、パターンマッチ、`==`、`asInstanceOf`、`}` の次行の `-1`）と `vcls_nl.scala`
（改行規則だけを集めたもの: `}` / `if` / `)` / 識別子の直後の `-`、行末演算子の継続、括弧内、
文の位置の `if` / `match`）は**私有ランタイムと library dual-run の両方**で走ります。
`vcls_arr.scala`（`Array[Meters]` / `List[Meters]` / `Option[Meters]` / ジェネリックメソッド /
case class フィールド / `Set`）と `vcls_hnil.scala`（`import syntax._` で型名が隠れた
`HNil.type`、パッケージ修飾の `p.HNil.type`、ネストした `object` の
`ColumnOption.AutoInc.type`）は `List.apply` / `Array.apply` が要るので library dual-run 専用
です。`expected/*.txt` はすべて real scalac 2.13.16 の stdout そのものです。
`vcls_bad.scala` は universal trait 越しに underlying を触る（`u.n`）、存在しないメンバ、
`def` を `X.type` に使うの 3 つが黙って通らないことを固定します。バイトコード側は
`vcls_classfile_matches_nsc_shape`（`Meters implements Univ`、`describe$extension(int)` /
`plus$extension(int, int)` / `equals$extension` / `hashCode$extension` の static、`equals` /
`hashCode`）と `vcls_boxes_into_the_value_class_not_its_underlying_box`（universal trait を
取る呼び出しの直前が `new Meters` であって `Integer.valueOf` ではないこと）で見ています。

jar の package object にある**型エイリアス**のフィクスチャは接頭辞 `pkgalias`（`pkgalias` / `pkgalias_bad`）で、同じ理由から `crates/cli/tests/pkgalias.rs` に置いています。`pkgalias.scala` は `scala` package object の pickle にしか無い別名（`NoSuchElementException` / `Throwable` / `UnsupportedOperationException` / `IllegalArgumentException` / `Exception` / `IterableOnce[A]` / `Seq[A]`）だけを使い、library dual-run 専用です（`pkgalias_without_library_is_diagnosed` で `--no-scala-library` では `not found: value NoSuchElementException` と診断されることも見ています）。`pkgalias_bad.scala` は package object が宣言していない名前が黙って通らないことを固定します。`expected/pkgalias.txt` は real scalac 2.13.16 の出力です。


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
| `namedargs.scala` | 名前付き引数とデフォルト引数（メソッド / `apply` / `copy` / コンストラクタ / オーバーロード / 後続の引数リスト / 可変長引数）。実 scalac 2.13.16 と dual-run | `12` `12` `123` `129` `129` `153` `14` `1927` `1122` `45` `100` `105` … |
| `byname.scala` | by-name パラメータが二度評価される | `6` `2` |
| `trait_concrete.scala` | 具象メソッド付き trait を class が使う | `from trait` |
| `trait_linearize.scala` | `extends A with B` の線形化（B が勝つ） | `B` |
| `try_catch.scala` | throw / catch / finally | `before` `caught` `finally` |
| `try_finally.scala` | `try/finally` の成功と throw、`try/catch/finally` で catch が再 throw | `ok` `fin-ok` `before-throw` `fin-throw` `outer` `caught` `fin-catch` `outer2` |
| `type_alias.scala` | `type T = List[Int]` とトレイト `type A = String` を vals/defs で使う | `1` `ok` |
| `update_assign.scala` | `arr(i) = v` とユーザー定義 `def update` | `1` `2` `9` `11` `13` |
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
| `view_bounds_class.scala` | クラス型パラメータ `A <% Ordered[A]`（ctor の implicit evidence。library dual-run） | `1` `2` |
| `hk_types.scala` | `Functor[F[_]]` / `Box[F[_], A]` と `Id` インスタンス | `41` `2` |
| `type_member_hk.scala` | 高階型メンバー `type F[_]`、subclass の `type F[X] = Id[X]`、パス依存 `c.F[Int]` | `41` `2` |
| `refine_hk.scala` | refinement `M { type F[X] = Id[X] }` と `m.wrap(41).value` | `41` `2` |
| `refine_bound.scala` | `{ type A <: Int }` と HK 境界 `type F[_] <: Bound` | `41` `2` |
| `type_member_bounds.scala` | クラス / トレイトの nullary `type A <: Bound` / `type A >: Null` と subclass の `type A = …` | `41` `41` `41` `41` `ok` |
| `assign_op.scala` | `var` Int の `+=`（`x = x.+(1)`）と `def +=` を持つクラス | `41` `2` |
| `collection_converters.scala` | `scala.jdk.CollectionConverters` の `ArrayList.asScala` / `List.asJava`（library dual-run のみ） | `41` `2` |
| `nested_proj.scala` | 入れ子射影 `Outer#Inner#X` と `Holder#Inner#T` | `41` `2` |
| `app.scala` | `object Main extends App` の delayed init | `hello-app` |
| `delayed_init.scala` | `class C extends DelayedInit` の ctor 本体 | `from-ctor` |
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
| `unchecked_variance.scala` | `A @uncheckedVariance`（contravariant 引数と invariant 型引数） | `1` `41` |
| `switch.scala` | `@switch` の密な Int `tableswitch` と疎な `lookupswitch` | `10` `14` `12` `2` |
| `early_defs.scala` | `extends { val x = 1 } with T` の early init（親より先に `x`） | `11` `10` |
| `infix_either.scala` | infix 型 `Int Either String`（library dual-run のみ。`Left` / `Right`） | `1` `2` |
| `sam.scala` | SIP-21 SAM: ラムダ / `go _` / 未適用 `cmp` を `Runnable` / `Comparator[Int]` / `Function[Int,Int]` に | `2` `1` `2` `-2` `41` |
| `volatile.scala` | `@volatile` / `@transient` の読み書き | `3` `7` `9` |
| `inline.scala` | `@inline` / `@noinline` を付けたメソッドが動く（インライン化はしない） | `3` |
| `sgap.scala`（`crates/cli/tests/smallgaps.rs`） | `agent/smallgaps` スライスの複合 fixture: `@inline val` / `@inline @noinline def` の受理、curried 主コンストラクタ（`case class Pair(a: Int)(val b: Int, val c: Int)`）の companion `apply` が正しく curry される、case class のフィールド型が**自分の companion に後方参照する**入れ子型（`Ordering.Direction`）を指すときの解決順序、`case object` が引数付きの `sealed abstract class` を extends するときの module `<init>` codegen、`Option.flatMap` の多相性、`if`/`else` の `None`/`Some` 分岐で（型注釈なしでも）`lub` が `Option[X]` になり `.getOrElse` が解決すること | `42` `6` `true` `n=5` `3` `-1` |
| `sgap_lib.scala`（`crates/cli/tests/smallgaps.rs`、library dual-run のみ） | `Iterable(...)` companion `apply`（実ライブラリの `IterableFactory$Delegate.apply` 継承。私有ランタイムに裏付けが無いので `--no-scala-library` では診断のまま） | `List(a, b, c)` `3` |
| `oshadow.scala`（`crates/cli/tests/overloadshadow.rs`、library dual-run のみ） | 別のクラスを読んでも既存のオーバーロード集合が消えないこと: `java.math.BigDecimal` を**前にも後にも**置いた上での `BigDecimal(Int)` / `(Long)` / `(String)` / `(BigInt)` / `(java.math.BigDecimal)`、`Option[BigDecimal].getOrElse` | `2` `3` `4.25` `6` `12.5` `12.5` `-1` `7` `8.75` `9` |
| `oshadow_java_first.scala` / `oshadow_java_last.scala`（`crates/cli/tests/overloadshadow.rs`、library dual-run のみ） | 同じプログラムを `java.math.BigDecimal` の位置だけ入れ替えた 2 本。両方通り、stdout が一致すること（順序依存の回帰テスト） | `1` `2` `3.5` |
| `pimpl.scala`（`crates/cli/tests/parentimpl.rs`） | `agent/parentimpl` スライス: 親コンストラクタの implicit 節・デフォルト引数の補完（`class ConstColumn[T : TT] extends TypedRep[T]`、明示節＋2 引数の implicit 節、context bound の親への受け渡し、全部／末尾だけデフォルト、デフォルト節＋implicit 節、匿名クラスの親、引数無しの `new`）。私有ランタイム・library dual-run・real scalac dual-run の 3 通り | `rep[Int]` `rep[String]` … `anon:Int` `Int` |
| `vcls.scala`（`crates/cli/tests/valclass.rs`） | 値クラス + universal trait（`Meters` / `Name` が `Univ`）、trait 位置と `Any` への代入で `new Meters` に box、`toString` / `isInstanceOf` / `case x: Meters` / `==` / `asInstanceOf`、`}` の次行の `-1`、行末 `+` の継続 | `5m` `5m5m` `<ada><ada>` `5m` `Meters@5` `true` `meters 5` `true` `false` `8m` `5` `-1` `-1` |
| `vcls_nl.scala`（`crates/cli/tests/valclass.rs`） | 改行が文を切る条件: `}` / `if` / `)` / 識別子の直後の `-`、行末演算子の継続、括弧内は継続、文の位置の `if` / `match` | `-1` `-2` `-3` `-4` `-1` `4` `y` `` |
| `vcls_arr.scala`（`crates/cli/tests/valclass.rs`、library dual-run のみ） | `Array[Meters]`（`[LMeters;`、`mkString`、`new Array` + 代入）、`List[Meters]` / `map(_.n)`、`Option[Meters]`、ジェネリックメソッド、case class のフィールド、`Set` | `2` `1` `Meters@1,Meters@2` `7m` `7` … `1` |
| `vcls_hnil.scala`（`crates/cli/tests/valclass.rs`、library dual-run のみ） | `import syntax._` が型名 `HNil` を隠したうえでの `HNil.type`、前方参照、パッケージ修飾 `hl.HNil.type`、型引数位置、ネストした object の `ColumnOption.AutoInc.type` | `0` `2` `0` `0` `1` `1` `PrimaryKey` `AutoInc` `1` |
| `pkgalias.scala`（`crates/cli/tests/pkgalias.rs`、library dual-run のみ） | jar の package object にしかない**型エイリアス**（`scala/package$` の pickle）: `new NoSuchElementException(...)` と `catch`、`Throwable` / `UnsupportedOperationException` / `IllegalArgumentException` / `Exception`、型パラメータ付きの `IterableOnce[Int]` / `Seq[Int]` | `gone` `java.lang.UnsupportedOperationException` `java.lang.IllegalArgumentException` `3` `r` `9` |
| `java_cp.scala` | JDK の Java `.class` から `Math.abs` / `Byte.MAX_VALUE` / `ArrayList.add` を解決して実行 | `3` `127` `true` `1` |
| `java_sig.scala` | Java Signature（`ArrayList[String]#get` は `String`）、inner `Map.Entry` / `SimpleEntry`、Java varargs `String.format` / `Arrays.asList` を実行 | `hi` `2` `k` `v` `k` `x-3` `2` |
| `java_wild.scala` | Signature の `Class[_]` / `Collection[_ <: Number]` / `Collections.max`（tparam bound）を存在型として実行 | `java.lang.String` `2` `9` |
| `java_throws.scala` | Java `throws` 検査例外（`Thread.sleep`）を Scala はチェックせず実行 | `ok` |
| `java_prot.scala` | Java `protected` を同じパッケージとサブクラスから呼んで実行（`-cp` 上の小さな Java クラス） | `7` `7` `11` |
| `java_enum.scala` | JDK の Java enum（`Thread.State`）の定数 / `values` / `valueOf` / `match` | `NEW` `RUNNABLE` `6` `1` `2` `true` |
| `context_bounds.scala` | `T: Ordering` と `T: scala.reflect.ClassTag`（library dual-run のみ） | `1` `0` `3` |
| `context_bounds_class.scala` | クラス型パラメータ `T: Ordering`（ctor の implicit evidence。library dual-run のみ） | `2` `1` |
| `aux_ctor.scala` | 補助コンストラクタ連鎖と `extends C(1)` / `extends C(z)` | `7` `5` `1` `9` |
| `native.scala` | `@native def` を `ACC_NATIVE`（本文なし）で出し、呼ばずに `main` だけ実行 | `42` |
| `path_dependent.scala` | `c: Foo { type A = Int }` の `c.A` / `c.x` | `41` `42` |
| `structural.scala` | `{ def foo: Int }` を Java reflection で呼ぶ | `42` |
| `structural_update.scala` | 構造的 `var` 代入 / getter+`foo_=` / `x(i) = v` | `41` `7` `9` |
| `pkg_implicit_class.scala` | package object の `implicit class` + `import enrich._` | `4` |
| `indexedseq_queue.scala` | `IndexedSeq(1,2)(1)` と `Queue.enqueue` / `dequeue`（library dual-run のみ） | `2` `1` `2` |
| `string_ops3.scala` | `augmentString` 経由の `stripSuffix` / `padTo(Int,Char)` / `linesIterator` / `toIntOption`（library dual-run のみ） | `foo` `abxxx` `a` `Some(12)` `None` |
| `byte_ops.scala` | `byteWrapper` / `shortWrapper` / `booleanWrapper`（`max` / `abs` / `compare`。library dual-run のみ） | `2` `3` `3` `1` |
| `arraybuffer.scala` | `mutable.ArrayBuffer(1,2) += 3` と `apply` / `update`（library dual-run のみ） | `1` `9` `3` |
| `string_ops4.scala` | `augmentString` 経由の `stripMargin` / `stripMargin(Char)` / `lines`（library dual-run のみ） | `hello` `world` `hello` `world` `a` |
| `numeric_range.scala` | `RichInt` の `1 to 3` / `1 until 3`（foreach / `mkString`）と `RichByte` の `NumericRange`（library dual-run のみ） | `Range 1 to 3` `Range 1 until 3` `1` `2` `3` `1,2,3` `1,2` `NumericRange 1 to 3` `NumericRange 1 until 3` `1,2,3` `1,2` |
| `listbuffer.scala` | `mutable.ListBuffer(1,2) += 3` と `apply`（library dual-run のみ） | `1` `2` `3` |
| `string_ops5.scala` | `augmentString` 経由の `capitalize` / `reverse` / `slice`（library dual-run のみ） | `Hello` `cba` `bcd` |
| `short_range.scala` | `RichShort` の `to` / `until` → `NumericRange`（library dual-run のみ） | `NumericRange 1 to 3` `NumericRange 1 until 3` `1,2,3` `1,2` |
| `stringbuilder.scala` | `new mutable.StringBuilder` の `+=` / `append` / `toString`（library dual-run のみ） | `abc` |
| `string_ops6.scala` | `augmentString` 経由の `takeRight` / `dropRight` / `contains(Char)`（library dual-run のみ） | `def` `abcd` `true` `false` |
| `long_range.scala` | `RichLong` の `to` / `until` → `NumericRange[Long]`（library dual-run のみ） | `NumericRange 1 to 3` `NumericRange 1 until 3` `1,2,3` `1,2` |
| `hashmap.scala` | `mutable.HashMap.empty` / varargs `apply` の `update` / `+=` / `apply` / `get`（library dual-run のみ） | `a` `b` `c` `x` `y` |
| `string_ops7.scala` | `java.lang.String` の `startsWith` / `endsWith` / `indexOf`（nsc と同じ。library dual-run のみ） | `true` `true` `2` |
| `char_range.scala` | `RichChar` の `to` / `until` → `NumericRange[Char]`（library dual-run のみ） | `NumericRange a to c` `NumericRange a until c` `a,b,c` `a,b` |
| `hashset.scala` | `mutable.HashSet.empty` / varargs `apply` の `+=` / `contains`（library dual-run のみ） | `true` `false` `true` `false` |
| `string_ops8.scala` | `augmentString` 経由の `head` / `last` / `stripLineEnd` / `replaceAllLiterally`（2.13.16 に残存。library dual-run のみ） | `h` `o` `hello` `a_b_a` |
| `array_ops2.scala` | `intArrayOps` 経由の `Array(1,2,3).head` / `tail`（library dual-run のみ。既存 `array_ops` の load/store は維持） | `1` `2` `2` |
| `linkedhashmap.scala` | `mutable.LinkedHashMap.empty` / varargs `apply` の `update` / `+=` / `apply` / 挿入順 `foreach`（library dual-run のみ） | `a` `b` `1` `2` `3` `4` `5` |
| `string_ops9.scala` | `augmentString` 経由の `tail` / `init` / `distinct` / `mkString` / `mkString(sep)`（library dual-run のみ） | `ello` `hell` `abc` `abc` `a,b,c` |
| `array_ops3.scala` | `intArrayOps` の `foreach` と `longArrayOps` の `head` / `foreach`（library dual-run のみ） | `1` `2` `3` `10` `10` `20` `30` |
| `linkedhashset.scala` | `mutable.LinkedHashSet.empty` / varargs `apply` の `+=` / `contains` / 挿入順 `foreach`（library dual-run のみ） | `true` `false` `1` `2` `3` `4` |
| `string_ops10.scala` | `augmentString` 経由の `filter` / `reverseIterator`（library dual-run のみ。既存 `tail` / `init` / `distinct` / `mkString` は触らない） | `heo` `o` `l` `l` `e` `h` |
| `array_ops4.scala` | `intArrayOps` の `map[B: ClassTag]`（名前付きラムダ）と `refArrayOps` の参照配列 `map`（library dual-run のみ。`map$extension(Object, Function1, ClassTag)Object`） | `2` `3` `4` `ax` `bx` |
| `arraydeque.scala` | `mutable.ArrayDeque.empty` / varargs `apply` の `+=` / `prepend` / `apply`（2.13 Stack 置換。library dual-run のみ。`ArrayBuffer` / `ListBuffer` は触らない） | `0` `1` `2` `3` `4` `5` |
| `placeholder.scala` | nsc placeholder `_`：`Array(1,2,3).map(_ + 1)`（既存 `ArrayOps.map` + ClassTag）、`_ + 1`、`_.abs`、`add1(_)`（library dual-run のみ） | `2` `3` `4` `11` `3` `5` |
| `array_ops5.scala` | `byteArrayOps` / `shortArrayOps` の `head` と `map(_ + 1)`（library dual-run のみ。int/long/ref ArrayOps は触らない） | `1` `2` `3` `1` `2` `3` |
| `string_ops11.scala` | `java.lang.String.split(String)` と `StringOps.diff` / `intersect`（`wrapString`。library dual-run のみ。既存 `filter` / `reverseIterator` / `tail` 等は触らない） | `a` `b` `c` `ace` `cde` |
| `placeholder2.scala` | Function2 `_ + _` と入れ子 `_.map(_ + 1)`（library dual-run のみ。unary `_ + 1` / `_.abs` / `f(_)` は触らない。`(1,2,3).zipped.map(_ + _)` は scalac 2.13.16 が拒否） | `3` `2` `3` `4` |
| `array_ops6.scala` | `charArrayOps` / `floatArrayOps` の `head` と `map`（Char は `_ + 1` → `Array[Int]`、Float は `_.abs`。library dual-run のみ。byte/short/int/long/ref は触らない） | `a` `98` `99` `1.0` `1.0` `2.0` |
| `string_ops12.scala` | `StringOps.updated` / `count` / `span`（library dual-run のみ。`split` / `diff` / `intersect` / `filter` / `reverseIterator` は触らない） | `hallo` `2` `(he,llo)` |
| `placeholder3.scala` | typed `_ : T`：`(_: Int) + 1` / `(_: Int) + (_: Int)` / `(_: Int).abs` / `map((_: Int) + 1)` / 入れ子 `_.map((_: Int) + 1)`（library dual-run のみ。unary / Function2 は触らない。`xs.map(_ : Int)` は nsc が拒否） | `11` `3` `3` `2` `3` `4` `2` `3` `4` |
| `array_ops7.scala` | `doubleArrayOps` / `booleanArrayOps` の `head` / `map`（library dual-run のみ。char/float/byte/short/int/long/ref は触らない） | `1.0` `2.0` `3.0` `true` `false` `true` |
| `string_ops13.scala` | `StringOps.partition` / `exists` / `forall` / `splitAt`（library dual-run のみ。`updated` / `count` / `span` / `split` / `diff` / `intersect` / `filter` は触らない） | `(heo,ll)` `true` `false` `true` `false` `(he,llo)` |
| `array_ops8.scala` | `genericArrayOps`：`def first[T](a: Array[T]) = a.head` / `Array[AnyRef].head` / `ClassTag` 付き `map`（library dual-run のみ。primitive wrappers は触らない） | `1` `a` `x` `10` `20` |
| `array_ops9.scala` | `unitArrayOps`：`Array((), ()).head` は nsc どおり `BoxedUnit.UNIT`（stdout `()`。偽の文字列 `"()"` ではない）。`map(_ => 1)`（library dual-run のみ） | `()` `1` `1` |
| `sortedset.scala` | `immutable.SortedSet(3,1,2)` の順序 `foreach` / `contains` と `TreeSet`（library dual-run のみ。HashSet/LinkedHashSet は触らない） | `1` `2` `3` `true` `false` `4` `5` `6` `true` |
| `array_ops10.scala` | ArrayOps `filter` / `slice` / `flatMap`（`List` を返す。library dual-run のみ。wrappers と `head`/`map`/`foreach`/`tail` は触らない） | `2` `3` `2` `3` `1` `11` `2` `12` |
| `string_ops14.scala` | `StringOps.sorted` / `toArray` / `copyToArray`（library dual-run のみ。`partition`/`exists`/`forall`/`splitAt`/`updated`/`count`/`span`/`diff`/`intersect`/`split`/`filter` は触らない） | `abc` `a` `b` `2` `x` `y` |
| `sortedmap.scala` | `immutable.SortedMap(3 -> "c", 1 -> "a")` のキー順 `foreach` / `apply` / `get` と `TreeMap`（library dual-run のみ。HashMap / SortedSet は触らない） | `1` `2` `3` `a` `b` `4` `5` `d` |
| `array_ops11.scala` | ArrayOps 3 引数 `flatMap`（`List`）と 4 引数 Array→Iterable `flatMap`（library dual-run のみ。wrappers は触らない） | `1` `1` `2` `2` `1` `1` `2` `2` |
| `string_ops15.scala` | `StringOps.indices` / `r`（`findFirstIn` / `matches`。library dual-run のみ。`sorted`/`toArray`/`copyToArray`/`partition`/`exists`/`forall`/`splitAt`/`updated`/`count`/`span`/`diff`/`intersect`/`split`/`filter` は触らない） | `0` `1` `2` `aa` `true` |
| `bitset.scala` | `immutable.BitSet(3, 1, 2)` の `contains` とキー順 `foreach`（library dual-run のみ。HashSet / SortedSet は触らない） | `true` `false` `1` `2` `3` |
| `array_ops12.scala` | ArrayOps `take` / `collect` / `zip`（`List`。library dual-run のみ。`head`/`map`/`foreach`/`tail`/`filter`/`slice`/`flatMap` は触らない） | `1` `2` `one` `three` `(1,10)` `(2,20)` |
| `string_ops16.scala` | `StringOps.dropWhile` / `takeWhile` / `nonEmpty` / `headOption` / `lastOption` / `filterNot`（library dual-run のみ。`indices`/`r`/`sorted`/`toArray`/`copyToArray`/`partition`/`exists`/`forall`/`splitAt`/`updated`/`count`/`span`/`diff`/`intersect`/`split`/`filter`/`stripPrefix` は触らない） | `llo` `he` `true` `false` `Some(a)` `None` `Some(b)` `heo` |
| `breaks.scala` | `scala.util.control.Breaks`：`import Breaks._` の途中 `break()` と完走、`new Breaks`、外の `break()` は `BreakControl`（library dual-run のみ。私有 Breaks は出さない） | `0` `1` `2` `done` `0` `1` `2` `full` `0` `1` `new` `scala.util.control.BreakControl` |
| `array_ops13.scala` | ArrayOps `drop` / `dropWhile` / `exists`（library dual-run のみ。`take`/`collect`/`zip`/`head`/`map`/`foreach`/`tail`/`filter`/`slice`/`flatMap` は触らない） | `2` `3` `3` `4` `true` `false` |
| `string_ops17.scala` | `StringOps.find` / `foreach` / `toBoolean` / `toBooleanOption`（library dual-run のみ。`dropWhile`/`takeWhile`/`nonEmpty`/`headOption`/`lastOption`/`filterNot`/`indices`/`r`/`sorted`/`toArray` とそれ以前は触らない） | `Some(l)` `None` `h` `i` `true` `false` `Some(true)` `None` |
| `breaks2.scala` | `Breaks.tryBreakable { … } catchBreak { … }`：途中 `break()` は catchBreak、完走時は catchBreak なし、戻り値、`new Breaks`、非 break 例外は伝播（library dual-run のみ。私有 TryBlock は出さない。`breakable`/`break` は触らない） | `0` `1` `2` `caught` `after-break` `0` `1` `2` `after-full` `1` `2` `new-caught` `java.lang.RuntimeException: boom` |
| `array_ops14.scala` | ArrayOps `foldLeft` / `fold` / `foldRight`（library dual-run のみ。`reduce` は ArrayOps に無い。`drop`/`dropWhile`/`exists`/`take`/`collect`/`zip`/`head`/`map`/`foreach`/`tail`/`filter`/`slice`/`flatMap` は触らない） | `6` `6` `6` |
| `string_ops18.scala` | `StringOps.foldLeft` / `toByte` / `toShort` / `toFloat` / `toLongOption` / `toDoubleOption`（library dual-run のみ。`find`/`foreach`/`toBoolean`/`toBooleanOption`/`dropWhile`/`takeWhile`/`nonEmpty`/`headOption`/`lastOption`/`filterNot`/`indices`/`r`/`sorted`/`toArray` とそれ以前は触らない） | `abc` `12` `12` `1.5` `Some(9)` `None` `Some(1.5)` |
| `bigint.scala` | `scala.math.BigInt` / `BigDecimal`：`apply(Int)` / `apply(String)`、`+` / `*`、`int2bigInt`（library dual-run のみ。私有 classfile は出さない） | `12` `20` `5` `3.5` `3.0` |
| `array_ops15.scala` | ArrayOps `scanLeft` / `count` / `forall`（library dual-run のみ。`foldLeft`/`fold`/`foldRight`/`drop`/`dropWhile`/`exists`/`take`/`collect`/`zip`/`head`/`map`/`foreach`/`tail`/`filter`/`slice`/`flatMap` は触らない） | `0` `1` `3` `6` `2` `true` `false` |
| `string_ops19.scala` | `StringOps.foldRight` / `toByteOption` / `toShortOption` / `toFloatOption` / `grouped`（library dual-run のみ。`foldLeft`/`toByte`/`toShort`/`toFloat`/`toLongOption`/`toDoubleOption`/`find`/`foreach`/`toBoolean` とそれ以前は触らない） | `cba` `Some(12)` `None` `Some(12)` `Some(1.5)` `ab` `cd` `ef` |
| `chaining.scala` | `scala.util.chaining`：`1.pipe(_ + 1)` と `tap` の副作用（library dual-run のみ。私有 classfile は出さない） | `2` `7` `7` |
| `capture_var.scala` | キャプチャした `var` の書き戻し：ラムダ / ネスト `def` / by-name。`--scala-library` では jar の `IntRef` / `ObjectRef`、`--no-scala-library` では私有ランタイムが出す同名クラス（library dual-run のみ） | `1` `1` `ab` `ab` `1` `1` `1` `1` |
| `array_ops16.scala` | ArrayOps `last` / `init` / `reverse` / `size` / `isEmpty` / `nonEmpty`（library dual-run のみ。`scanLeft`/`count`/`forall`/`foldLeft`/`fold`/`foldRight`/`drop`/`exists`/`take`/`collect`/`zip`/`head`/`map`/`foreach`/`tail`/`filter`/`slice`/`flatMap` は触らない） | `3` `1` `2` `3` `2` `1` `3` `false` `true` |
| `string_ops20.scala` | `StringOps.map`（`Char => Char`）/ `:+` / `+:`（library dual-run のみ。`grouped`/`foldRight`/`toByteOption` とそれ以前は触らない） | `Ab` `abc` `xyz` |
| `array_ops17.scala` | ArrayOps `find` / `contains` / `distinct` / `takeRight` / `dropRight` / `takeWhile` / `indices` / `lengthCompare`（library dual-run のみ。`last`/`init`/`reverse`/`size`/`isEmpty`/`nonEmpty`/`scanLeft`/`count`/`forall`/`foldLeft`/`fold`/`foldRight`/`drop`/`exists`/`take`/`collect`/`zip`/`head`/`map`/`foreach`/`tail`/`filter`/`slice`/`flatMap` は触らない） | `Some(2)` `None` `true` `false` `1` `2` `3` `3` `2` `1` `2` `1` `2` `0` `1` `2` `3` `0` `1` |
| `string_ops21.scala` | `StringOps.compare` / `lengthCompare` / `patch(Int, String, Int)` / `<`（library dual-run のみ。`map`/`:+`/`+:`/`grouped`/`foldRight`/`toByteOption` とそれ以前は触らない） | `1` `-1` `-1` `0` `abXYef` `true` `false` |
| `using.scala` | `scala.util.Using.resource`：`AutoCloseable` を成功時と throw 時に close（library dual-run のみ。私有 Using classfile は出さない） | `10` `1` `caught` `1` |
| `array_ops18.scala` | ArrayOps `filterNot` / `headOption` / `lastOption` / `partition` / `splitAt` / `span`（library dual-run のみ。`find`/`contains`/`distinct`/`takeRight`/`dropRight`/`takeWhile`/`indices`/`lengthCompare`/`last`/`init`/`reverse`/`size`/`isEmpty`/`nonEmpty`/`scanLeft`/`count`/`forall`/`foldLeft`/`fold`/`foldRight`/`drop`/`exists`/`take`/`collect`/`zip`/`head`/`map`/`foreach`/`tail`/`filter`/`slice`/`flatMap` は触らない） | `1` `3` `Some(1)` `None` `Some(2)` `None` `1` `2` `2` `3` `1` `2` `3` `2` `1` `2` `3` `2` |
| `string_ops22.scala` | `StringOps.>` / `>=` / `<=`（library dual-run のみ。`compare`/`lengthCompare`/`patch`/`<`/`map`/`:+`/`+:`/`grouped` とそれ以前は触らない） | `true` `false` `true` `false` `true` `false` |
| `using2.scala` | `scala.util.Using.apply`（`Try`）と `Using.Manager`：2 つの `AutoCloseable` を成功・throw・第 2 acquire 失敗で close（library dual-run のみ。`Using.resource` は触らない。私有 classfile は出さない） | `10` `1` `-1` `1` `10` `1` `1` `-1` `1` `1` `-1` `1` |
| `array_ops19.scala` | ArrayOps `zipWithIndex` / `knownSize` / `sizeCompare`（library dual-run のみ。`filterNot`/`headOption`/`lastOption`/`partition`/`splitAt`/`span`/`find`/`contains`/`distinct`/`takeRight`/`dropRight`/`takeWhile`/`indices`/`lengthCompare` とそれ以前は触らない） | `1` `0` `2` `1` `3` `2` `3` `-1` `0` `1` |
| `string_ops23.scala` | `StringOps.iterator` / `sizeCompare` / `knownSize` / `appendedAll` / `prependedAll`（library dual-run のみ。`>`/`>=`/`<=`/`compare`/`lengthCompare`/`patch`/`<`/`map`/`:+`/`+:`/`grouped` とそれ以前は触らない） | `a` `b` `-1` `0` `2` `abcd` `xyab` |
| `using3.scala` | `scala.util.Using.resources`（2 つの `AutoCloseable`、成功時と throw 時に close。library dual-run のみ。`Using.apply` / `Using.Manager` / `Using.resource` は触らない。私有 classfile は出さない） | `10` `1` `1` `caught` `1` `1` |
| `array_ops20.scala` | ArrayOps `lengthIs` / `sizeIs` / `indexOf` / `copyToArray` / `iterator`（library dual-run のみ。`zipWithIndex`/`knownSize`/`sizeCompare`/`filterNot`/`headOption`/`lastOption`/`partition`/`splitAt`/`span`/`find`/`contains`/`distinct` とそれ以前は触らない） | `3` `3` `1` `-1` `3` `1` `3` `0` `1` `2` |
| `string_ops24.scala` | `StringOps.++` / `lengthIs` / `sizeIs` / `flatMap`（library dual-run のみ。`iterator`/`sizeCompare`/`knownSize`/`appendedAll`/`prependedAll`/`>`/`>=`/`<=`/`compare`/`patch` とそれ以前は触らない） | `abcd` `3` `3` `xyxy` |
| `view.scala` | `List.view.map.toList` と `View.fill` / `View.iterate`（library dual-run のみ。私有 View classfile は出さない） | `List(2, 3, 4)` `List(7, 7, 7)` `List(1, 2, 3, 4)` |
| `coll_arraybuffer1.scala` | `mutable.ArrayBuffer[Int]()` の `mkString(0/1/3)` / `length` / `size` / `isEmpty` / `nonEmpty` / `head` / `last` / `foreach` / `map` / `filter` / `toList` / `iterator` / `contains` / `indexOf` / `reverse` / `foldLeft` / `append` / `++=` / `-=` / `insert` / `remove` / `sortBy` / `sorted` / `clear`（library dual-run のみ。既存 `apply` / `update` / `+=` は触らない） | `1 4 9 16 25` … `true` |
| `coll_listbuffer1.scala` | `mutable.ListBuffer[Int]()` の同じメンバー一式（library dual-run のみ。`ArrayBuffer` と同じ実装パターン） | `coll_arraybuffer1.scala` と同一出力 |
| `coll_mutablemap1.scala` | 新規 `scala.collection.mutable.Map[K, V]`（従来は `HashMap` のみ）の `apply` / `update` / `get` / `getOrElse` / `getOrElseUpdate` / `contains` / `keys` / `values` / `+=` / `-=` / `remove` / `size` / `isEmpty` / `nonEmpty` / `clear` / `foreach` / `filter` / `toList` / `toSeq` / `mkString`（library dual-run のみ。companion `Map$` は `MapFactory$Delegate` 経由で実行時は `HashMap` に委譲するが静的型は `mutable.Map` のまま） | `3` `false` `true` `1` `None` … `true` |
| `coll_map_view1.scala` | `immutable.Map.view.mapValues[W](f)` と `MapView` の `toList` / `mkString`（library dual-run のみ。`W` は現状メソッド型パラメータの明示指定が必要 — Remaining 参照） | `3` `a -> 2,b -> 4,c -> 6` |
| `coll_mutableset1.scala` | 新規 `scala.collection.mutable.Set[A]` の `+=` / `-=` / `remove` / `contains` / `size` / `isEmpty` / `nonEmpty` / `clear` / `foreach` / `map` / `filter` / `toList` / `toSeq` / `mkString`（library dual-run のみ） | `3` `false` `true` `true` `false` … `true` |
| `coll_immutablemap1.scala` | `immutable.Map` の `getOrElse` / `contains` / `keys` / `values` / `keySet` / `-` / `filter` / `toList` / `toSeq` / `mkString` / `head` / `foldLeft` / `withDefaultValue`（library dual-run のみ。既存 `apply` / `get` / `updated` / `+` / `foreach` は触らない。`++`/`concat` は Remaining 参照） | `1` `None` `-1` `true` `false` … `4` |
| `coll_immutableset1.scala` | `immutable.Set` の `+` / `-` / `++` / `size` / `isEmpty` / `nonEmpty` / `filter` / `map` / `toList` / `toSeq` / `mkString` / `head`（library dual-run のみ。既存 `contains` / `foreach` は触らない） | `true` `false` `4` `2` `5` … `true` |
| `coll_vector1.scala` | `Vector` の `size` / `isEmpty` / `nonEmpty` / `head` / `map` / `filter` / `toList` / `toSeq` / `iterator` / `mkString` / `foldLeft`（library dual-run のみ。既存 `apply` / `length` / `updated` / `:+` / `foreach` は触らない） | `1` `5` `5` `false` `true` … `15` |
| `coll_tuple2_extra1.scala` | `Tuple2.swap` / `toString`（library dual-run のみ。`_1` / `_2` は既存） | `a` `1` `1` `a` `(a,1)` `(a,1)` |
| `arrconv1.scala` | ArrayOps `toList` / `toSeq` / `toIndexedSeq` / `toSet` / `toVector` / `toBuffer` / `groupBy` / `sorted` / `sortBy` / `sortWith` / `mkString`（0/1/3 引数）（library dual-run のみ。`toList`等は `ArrayOps` に無いので `Predef.genericWrapArray` 経由） | `List(3, 1, 2, 1)` `ArraySeq(3, 1, 2, 1)` `ArraySeq(3, 1, 2, 1)` `Set(3, 1, 2)` `Vector(3, 1, 2, 1)` `ArrayBuffer(3, 1, 2, 1)` `Map(0 -> 1, 1 -> 3)` `List(1, 1, 2, 3)` `List(3, 2, 1, 1)` `List(3, 2, 1, 1)` `3121` `3,1,2,1` `[3,1,2,1]` |
| `arrconv2.scala` | ArrayOps `sum` / `product` / `min` / `max` / `minBy` / `maxBy` / `reduce` / `reduceLeft` / `indexWhere`（1/2 引数）/ `lastIndexOf` / `updated` / `appended` / `prepended` / `concat` / `++` / `patch` / `zipAll`（library dual-run のみ。`sum`等は `Numeric`/`Ordering` implicit 付きで `IterableOnceOps` へ） | `11` `24` `1` `3` `3` `1` `11` `11` `2` `3` `4` `List(9, 1, 2, 2, 2, 3)` `List(1, 1, 2, 2, 2, 3, 9)` `List(9, 1, 1, 2, 2, 2, 3)` `List(1, 1, 2, 2, 2, 3, 9, 8)` `List(1, 1, 2, 2, 2, 3, 9, 8)` `List(1, 9, 8, 2, 2, 3)` `List((1,1), (1,2), (2,0), (2,0), (2,0), (3,0))` |
| `mapview1.scala` | `Map.view` / `MapView.mapValues`（型引数推論）/ `filterKeys` / `keys` / `values` / `toMap` / `toList` / `toSeq` / `size` / `isEmpty` / `foreach`（library dual-run のみ。私有 MapView classfile は出さない） | `Map(a -> 10, b -> 20, c -> 30)` `Map(a -> 1, c -> 3)` `List(a, b, c)` `List(1, 2, 3)` `3` `false` `List((a,1), (b,2), (c,3))` `List((a,1), (b,2), (c,3))` `6` `Map(1 -> 2, 2 -> 3, 3 -> 1)` |
| `classtag.scala` | `implicitly[ClassTag[Int]]` と `new Array[T]`（library dual-run のみ） | `int` `2` |
| `custom_interp.scala` | `implicit class` + `q"a$x"`（library dual-run のみ） | `q:ok` |
| `tailrec.scala` | `@tailrec` の末尾再帰が実行される | `15` |
| `deprecated.scala` | `@deprecated` を付けた `def` が動く | `42` |
| `array_ops.scala` | `Array(1,2,3)` / apply / length / update（library dual-run のみ） | `1` `3` `9` `8` |
| `nlreturn.scala` | `foreach` ラムダからの非ローカル `return` とネスト def のローカル `return` | `1` `3` `0` `1` |
| `existential_forsome.scala` | `List[X] forSome { type X <: AnyRef }` | `a` `b` |
| `java_override.scala` | 本当に override する `@Override` | `sub` `base` |
| `java_deprecated.scala` | Java `@Deprecated` を付けた `def` が動く | `42` |
| `const_types.scala` | SIP-23 定数型 `val x: 1 = 1` / `def f(n: 1)` を実行 | `1` `1` |
| `implicit_class.scala` | `implicit class Rich(n: Int) { def twice }` の `2.twice` | `4` |
| `dynamic.scala` | `scala.Dynamic` の `selectDynamic` / `applyDynamic` / `updateDynamic` / `applyDynamicNamed`（library dual-run のみ。`language.dynamics`） | `foo` `barx` `ok` `bazay` |
| `postfix_ops.scala` | postfix `42 bang`（`language.postfixOps` + `implicitConversions`） | `43` |
| `postfix_abs.scala` | postfix `42 abs`（library dual-run のみ。`language.postfixOps`） | `42` |
| `xml_lit.scala` | XML `<a>t{n}</a>`（library + scala-xml dual-run のみ） | `<a>t1</a>` |
| `xml_attr.scala` | XML `<a b={e} c="t"/>`（library + scala-xml dual-run のみ） | `<a b="1" c="t"/>` |
| `xml_ns.scala` | XML `<a xmlns:p="u" p:b={e} c="t"/>`（library + scala-xml dual-run のみ） | `<a p:b="1" c="t" xmlns:p="u"/>` |
| `xml_prefix.scala` | XML `<p:a xmlns:p="u"/>` / `<p:b xmlns:p="u">t</p:b>`（library + scala-xml dual-run のみ） | `<p:a xmlns:p="u"/>` `<p:b xmlns:p="u">t</p:b>` |
| `xml_comment.scala` | XML コメント / CDATA / PI（library + scala-xml dual-run のみ） | `<a><!--c--></a>` `<a><![CDATA[x]]></a>` `<a><?pi t?></a>` |
| `xml_entity.scala` | XML `&amp;` / `&lt;` / `&#65;`（library + scala-xml dual-run のみ） | `<a>&amp;</a>` … `<a>A</a>` |
| `enumeration.scala` | `object Color extends Enumeration { val Red, Blue = Value }`（library dual-run のみ） | `Red` `0` `Blue` |
| `existential_val_ok.scala` | `p.Inner forSome { val p: Outer }` | `1` |
| `either_ops.scala` | right-biased `Either` の `isRight` / `isLeft` / `getOrElse` / `map` / `flatMap` / `fold` / `swap` / `toOption` / `toSeq` / `contains` / `exists` / `forall` / `foreach` / `filterOrElse` / `orElse`（library dual-run のみ。私有 Either classfile は出さない） | `true` `false` `false` `5` `-1` `Right(10)` `Left(div0)` `Right(1)` `5` `4` `Left(5)` `Some(5)` `None` `List(5)` `true` `true` `false` `5` `Left(small)` `Right(5)` `Right(0)` |
| `either_left.scala` | `Either.left` が返す `LeftProjection` の `e` / `get` / `getOrElse` / `map` / `flatMap` / `foreach` / `exists` / `forall` / `toOption` / `toSeq` / `filterToOption`（library dual-run のみ） | `boom` `boom` `none` `Some(boom)` `None` `List(boom)` `Left(4)` `Right(7)` `Left(boom!)` `true` `false` `boom` `Left(boom)` `Some(Left(boom))` |
| `either_for.scala` | `Either` の `for` 内包表記（`flatMap` + `map`。1〜3 generator。途中が `Left` なら `Left` のまま。2.13 の `Either` に `withFilter` は無いのでガードは書けない） | `Right(10)` `Left(div0)` `Right(9)` `Right(32)` |
| `option_x1.scala` | `Option` の `getOrElse` / `isDefined` / `nonEmpty` / `contains` / `exists` / `forall` / `filter` / `filterNot` / `orElse` / `fold`（私有ランタイムと library dual-run の両方で同じ出力） | `3` `0` `true` `false` `true` `false` `true` `false` `false` `true` `false` `true` `true` `false` `true` `true` `true` `false` `3` `9` `4` `0` |
| `option_x2.scala` | `Option` の `toList` / `toRight` / `toLeft` / `zip` / `collect`（`{ case … }` リテラル）/ `flatten`（`<:<.refl` を渡す。library dual-run のみ） | `List(3)` `List()` `Right(3)` `Left(empty)` `Left(3)` `Right(empty)` `Some((3,4))` `None` `Some(three)` `None` `Some(5)` |
| `try_ops.scala` | `scala.util.Try` の `isSuccess` / `isFailure` / `get` / `getOrElse` / `map` / `flatMap` / `filter` / `toOption` / `toEither` / `orElse` / `fold` / `foreach` / `failed` / `transform`（library dual-run のみ） | `true` `false` `false` `true` `5` `5` `-1` `Success(6)` `Failure(java.lang.ArithmeticException: / by zero)` `Success(10)` `Success(5)` `Some(5)` `None` `Right(5)` `Success(5)` `Success(7)` `5` `-1` `5` `true` `Success(15)` `2` `0` |
| `try_recover.scala` | `Try.recover` / `recoverWith` / `collect` に `{ case _: ArithmeticException => … }` を直接渡す（`PartialFunction` リテラル。`val pf: PartialFunction[Throwable, Int]` 経由も。library dual-run のみ） | `Success(-1)` `Success(5)` `Failure(java.lang.ArithmeticException: / by zero)` `Success(42)` `Success(5)` `Success(five)` `true` `Success(99)` |
| `try_for.scala` | `Try` の `for` 内包表記。ガード付き（`Try$WithFilter`）と失敗の伝播（library dual-run のみ） | `Success(11)` `Failure(java.lang.ArithmeticException: / by zero)` `Success(10)` `Failure(java.util.NoSuchElementException: Predicate does not hold for 5)` `Success(30)` `5` |
| `try_exceptions.scala` | `java.lang` の `IllegalArgumentException` / `ArithmeticException` / `RuntimeException` を `new X("msg")` して catch し、`getMessage` を読む（私有ランタイムと library dual-run の両方で同じ出力） | `bad` `arith: / by zero` `boom` |
| `boxed.scala` | `java.lang.Integer`/`Long`/`Character`/`Boolean`/`Double` と `scala.Int` などの相互変換（`int2Integer` / `Integer2int` ほか）、ラッパーの static（`valueOf` / `parseInt` / `MAX_VALUE` / `isDigit` / `parseDouble` / `toBinaryString`）、`java.util.ArrayList[java.lang.Long]` への `add`、`Any` への自動 boxing、値クラス側の回帰（`1.max(2)` / `(-3).abs` / `'9'.isDigit` / `toString` / `Array`）（library dual-run と real scalac diff の両方） | `3` `4` `4` `3` `-1` … `4` |
| `boxed_rt.scala` | `boxed.scala` のうち私有ランタイムでも動く部分（変換 intrinsic と JDK ラッパー。`RichInt`/`Array.apply` は使わない）（private ランタイムと library dual-run の両方で同じ出力） | `4` `4` `3` `42` `2147483647` `true` `x` `true` `0.5` `99` `9` |

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、違法 compound は `compound_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。`Either` に無いメンバーは `either_ops_bad.scala`、`Option` に無いメンバーは `option_x1_bad.scala`、`Option.toRight` の結果の `Either` に無いメンバーは `option_x2_bad.scala`、`Try` に無いメンバーは `try_ops_bad.scala`、`Throwable` に無いメンバーは `try_exceptions_bad.scala`。`Try.recover` に `PartialFunction` でない全域関数リテラルを渡すのは `try_recover_bad.scala`（nsc どおり `required: PartialFunction`）。`either_ops.scala` / `option_x2.scala` / `try_ops.scala` は `--no-scala-library` では診断になることも見ています（私有ランタイムに `Either` / `Try` は無い）。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` / `@noinline` は実 scalac 2.13.16 と同じくどの定義（val / var / class / type ...）にも、両方同時にも、警告なしで付けられる（`crates/cli/tests/smallgaps.rs` の `fixtures_sgap`。`inline_bad.scala` は削除）。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。
| `list_core1.scala` | `List` の `filter` / `take` / `drop` / `slice` / `reverse` / `distinct` / `init`（library dual-run のみ） | `3,4,5` … `3,1,4,1,5` |
| `list_core2.scala` | `List` の `size` / `length` / `head` / `last` / `contains` / `exists` / `find` / `indexOf`（library dual-run のみ） | `3` … `None` |
| `list_core3.scala` | `List` の `mkString` 0/1/3 引数 / `sum` / `product` / `min` / `max` / `minBy` / `maxBy`（library dual-run のみ） | `314` … `apple` |
| `list_core4.scala` | 真に多相な `map` / `flatMap` / `collect` / `zip` / `zipWithIndex`（library dual-run のみ） | `2,4,6` … `(1,0),(2,1),(3,2)` |
| `list_core5.scala` | `foldLeft` / `foldRight` / `reduce` / `reduceLeft` / `reduceRight` / `scanLeft`（library dual-run のみ） | `10` … `abbccc` |
| `list_core6.scala` | `sorted` / `sortBy` / `sortWith` / `distinctBy` / `groupBy` / `grouped` / `sliding`（library dual-run のみ） | `1,1,3,4,5` … |
| `list_core7.scala` | `::` / `:::` / `+:` / `:+` / `++` / `:++` / `++:` / `updated` / `splitAt` / `span` / `partition` / `startsWith` / `endsWith`（library dual-run のみ） | `1,2,3` … `a,b` |
| `list_core8.scala` | `toArray` / `toSet` / `toVector` / `toSeq` / `toList` / `Iterator.toList`（library dual-run のみ） | `3` … `List(3, 1);List(3)` |
| `list_core9.scala` | case class のリストに対するパイプライン、for-comprehension、空リスト、`List(a, b, c)` パターン（library dual-run のみ） | `apple,fig,pear` … `6` |
| `list_core10.scala` | 私有ランタイムでも動く `List` のコアメンバ（同じ出力を library dual-run でも確認） | `4` … `0` |

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、違法 compound は `compound_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。`List` に無いメンバーは `list_core1_bad.scala`。私有ランタイムに裏付けの無い `List.sorted` を `--no-scala-library` で使うのは `list_core2_bad.scala`（`value sorted is not a member of List[Int]`）。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` / `@noinline` は実 scalac 2.13.16 と同じくどの定義（val / var / class / type ...）にも、両方同時にも、警告なしで付けられる（`crates/cli/tests/smallgaps.rs` の `fixtures_sgap`。`inline_bad.scala` は削除）。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。
| `text_string1.scala` | `java.lang.String` の素のメソッド `trim` / `substring` / `lastIndexOf` / `replace` / `contains` / `equalsIgnoreCase` / `matches` / `concat` / `strip` / `repeat` / `compareTo`（library dual-run のみ。`--no-scala-library` でも動く） | `Hello World` `cdef` `bc` `1` `4` `zbc` `hello there` `true` `false` `true` `true` `abcdef` `x` `ababab` `-1` |
| `text_stringbuilder1.scala` | bare `StringBuilder`（`scala.StringBuilder` エイリアス）の `append` 各オーバーロード / `+=` / `++=` / `insert` / `deleteCharAt` / `setLength` / `reverse` / `clear` / `isEmpty` / `nonEmpty` / `result` / `charAt`（library dual-run のみ） | `hello 42!` `9` `false` `>>hello 42!` `>hello 42!` `>he` `eh>` `true` `abc` `true` `b` |
| `text_range1.scala` | `Range` の `withFilter`（for 内包表記の guard）/ `foldLeft` / `foldRight` / `sum` / `product` / `min` / `max` / `toList` / `toVector` / `filter` / `filterNot` / `map` / `flatMap` / `reverse` / `contains` / `exists` / `forall` / `count` / `take` / `drop` / `takeWhile` / `dropWhile` / `zipWithIndex` / `by`（library dual-run のみ） | `3` `4` `5` `Vector(20, 40)` `15` `15` `15` `120` `5` `1` `List(1, 2, 3, 4, 5)` … |
| `text_math1.scala` | `RichInt`/`RichLong`/`RichDouble`/`RichChar` の `toBinaryString` / `toHexString` / `toOctalString` / `sign` / `isNaN` / `round` / `floor` / `ceil` と `scala.math.{abs,max,min,pow,sqrt,floor,ceil,round,signum}`（library dual-run のみ） | `101` `ff` `377` `-1` `-1` `-1.0` `false` `true` `3` `2.0` `3.0` … |

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、違法 compound は `compound_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` / `@noinline` は実 scalac 2.13.16 と同じくどの定義（val / var / class / type ...）にも、両方同時にも、警告なしで付けられる（`crates/cli/tests/smallgaps.rs` の `fixtures_sgap`。`inline_bad.scala` は削除）。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。`java.lang.String` の未対応メソッドは `text_string1_bad.scala`。`StringBuilder` の未対応メソッドは `text_stringbuilder1_bad.scala`。`Range` の未対応メソッドは `text_range1_bad.scala`。`scala.math` の未対応関数は `text_math1_bad.scala`。
implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、違法 compound は `compound_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。`mutable.ArrayBuffer` の新規メンバーに無いものは `coll_arraybuffer1_bad.scala`。`mutable.ListBuffer` は `coll_listbuffer1_bad.scala`。新規 `mutable.Map` は `coll_mutablemap1_bad.scala`。新規 `mutable.Set` は `coll_mutableset1_bad.scala`。`immutable.Map` の新規メンバーは `coll_immutablemap1_bad.scala`。`immutable.Set` は `coll_immutableset1_bad.scala`。`Vector` の新規メンバーは `coll_vector1_bad.scala`。`Tuple2` の新規メンバーは `coll_tuple2_extra1_bad.scala`。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` / `@noinline` は実 scalac 2.13.16 と同じくどの定義（val / var / class / type ...）にも、両方同時にも、警告なしで付けられる（`crates/cli/tests/smallgaps.rs` の `fixtures_sgap`。`inline_bad.scala` は削除）。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。
| `anoncap1.scala` | 匿名クラスの基本キャプチャ：パラメータ 1 つ、パラメータ 2 つ + ブロックローカル `val`、親コンストラクタ引数と `super` オーバーライドでの使用、匿名クラス自身の `val` 初期化子からの参照（両 ABI で実行） | `mk 7` `13` `b:t9/9` `13` |
| `anoncap2.scala` | キャプチャ + `$outer`（囲みクラスのメンバと同時参照）、匿名クラス内のラムダによる二重キャプチャ、入れ子匿名クラス、ラムダの中の `new`、lambda-lift されるネスト `def` の中の `new`、trait のメソッドの中の匿名クラス（`$outer` はインタフェース型。レシーバは class と object の両方）（両 ABI で実行） | `holder 15` `14` `inner 42` `16` `12` `106` `206` |
| `anoncap3.scala` | キャプチャした `var` への書き込み、コンストラクタ引数を持つローカル `class` のキャプチャ、`var` と `val` の同時キャプチャ、ループをまたいだ `var` の書き戻し、by-name パラメータのキャプチャ（両 ABI で実行） | `3` `7` `acc=20` `6` `byName 6` `6` |

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、違法 compound は `compound_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` / `@noinline` は実 scalac 2.13.16 と同じくどの定義（val / var / class / type ...）にも、両方同時にも、警告なしで付けられる（`crates/cli/tests/smallgaps.rs` の `fixtures_sgap`。`inline_bad.scala` は削除）。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。匿名クラスが囲みメソッドに無い名前を参照するのは `anoncap1_bad.scala`（`not found: value missingLocal`）、匿名クラスより後ろで定義した `val` を参照するのは `anoncap2_bad.scala`（`not found: value later`）。匿名クラス / ローカルクラスのキャプチャは `crates/cli/tests/anoncap.rs` にあり、各 fixture を `--no-scala-library` と `--scala-library` の両方で `java -Xverify:all` 実行して同じ出力になることを見ています。
型パラメータの境界は `lowbound.scala`（`::` の `[B >: A]`、ユーザー定義 `Box.widen`、`[A <: Shape]`。私有ランタイムと `--scala-library` の両方で dual-run。`java -Xverify:all`）と `lowbound_lib.scala`（`List(...)` 可変長の lub。library リンク時のみ）で見ています。境界違反は `lowbound_bad.scala`（推論した上限境界違反）/ `lowbound_bad2.scala`（明示した上限境界違反）/ `lowbound_bad3.scala`（明示した下限境界違反）でコンパイルエラーになることを見ています。これらは `crates/cli/tests/lowbound.rs` から回します。

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、違法 compound は `compound_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` / `@noinline` は実 scalac 2.13.16 と同じくどの定義（val / var / class / type ...）にも、両方同時にも、警告なしで付けられる（`crates/cli/tests/smallgaps.rs` の `fixtures_sgap`。`inline_bad.scala` は削除）。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。ArrayOps の変換系（`toList` 等）に無いメンバーは `arrconv1_bad.scala`、ArrayOps の集約系（`sum` 等）に無いメンバーは `arrconv2_bad.scala`、`MapView` に無いメンバーは `mapview1_bad.scala` です。
| `xsource3_wildcard.scala` | `?` ワイルドカード型（`? <: T` / `? >: Lo <: Hi` / backtick 付き `` `?` ``） | `shape` ×4 `7` |
| `xsource3_intersection.scala` | `-Xsource:3` の `&` 交差型（`with` 混在・型メンバー・上限境界） | `bounded` `ada` `36` `36` `72` |
| `xsource3_block_lambda.scala` | ブロック位置の関数リテラル（`{ x => val n = 1; n }` / `{ x: Int => … }` / `case` 本体・入れ子） | `8` `12` `11` `9` `21` `100` `8` `11` |

型パラメータを取る型メンバー / 型エイリアスと高階 context bound は `crates/cli/tests/tmember.rs` の専用スイート（9 本）で回します。`tmember1.scala`（別トレイトで宣言した `type C[T] <: TypedType[T]` を `type C[T] = JdbcType[T]` で実装、self type 経由の `type C[T] = self.C[T]`、型メンバーを境界に取る context bound `def base[U: BaseColumnType]`）、`tmember2.scala`（`def f[F[_]: Async]()` / `class C[F[_]: Async]`、型パラメータ `F` と同名の `val F` による名前空間の分離）、`tmember3.scala`（境界内のワイルドカード `R <: Rep[?]`、高階パラメータを型引数に渡す `Query[?, U, C]`、型引数付き `#` 射影 `Profile#AbstractTable[?]`）を、**scala-rs で実行した出力**と**実 scalac 2.13.16 で実行した出力**の両方に対して突き合わせます（`same_as_scalac`）。負例は `tmember_bad.scala`（高階 view bound → `type F takes type parameters`）、`tmember_bad2.scala`（未解決の適用型 → `not found: type Missing`）、`tmember_bad3.scala`（`type C[T] = Int` が `<: Bound[T]` に反する → `incompatible type in overriding type C`）で、いずれも実 scalac と同じ診断です。

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、違法 compound は `compound_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` / `@noinline` は実 scalac 2.13.16 と同じくどの定義（val / var / class / type ...）にも、両方同時にも、警告なしで付けられる（`crates/cli/tests/smallgaps.rs` の `fixtures_sgap`。`inline_bad.scala` は削除）。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。`?` ワイルドカードと `&` 交差型は `crates/cli/tests/xsource3.rs` の専用スイート（12 本）で、`xsource3_wildcard.scala`（フラグ無し / `-Xsource:3` / `-Xsource:3-cross` の 3 通りで実行）と `xsource3_intersection.scala`（`-Xsource:3` 系のみ）を回します。負例は `xsource3_intersection_bad.scala`（フラグ無し・`-Xsource:2.13` では `&` を診断し、`-Xsource:3` を付けると同じソースが通ることも見る）と `xsource3_question_bad.scala`（`` 型名 `?` には backtick が要る ``）。`-Xsource:2.12` は nsc と同じくオプションエラーです。パーサ側の単体テストは `crates/parser/src/lib.rs` にあり、`?` の木が `_` と一致すること、`&` の木が `with` と一致すること、フラグ無しでは `&` が中置型のままであることを見ています。ブロック位置の関数リテラルは `xsource3_block_lambda.scala`（フィクスチャ名の衝突を避けるため同じ接頭辞にしています）で、`{ x => val n = 1; n }` / 複数行ブロック本体 / 本体中の `def` / 括弧なし `{ x: Int => … }` / `{ () => … }` / `case` 本体の中のラムダ / 入れ子ラムダを実行します。パーサ単体テストでは、本体が `Local` 位置では従来どおり関数型注釈（`(f: Int => Int)`）になること、本体ブロックが `case` で止まることも見ています。

import の解決は `crates/cli/tests/imports.rs`（fixture 接頭辞 `imports`）の専用スイート（7 本）です。同一実行側は `imports_pkgs.scala` / `imports_pkgs2.scala` / `imports_pkgs3.scala` / `imports_pkgobj.scala`（1・2・3 階層のパッケージと package object、`case class` + `object` のコンパニオン対）を `imports_main.scala` と一緒にコンパイルし、単一 / `{A, B}` / `{A => B}` / `a as b` / `.*` / package object のメンバー / package object の中の入れ子 object を全部使って実行します（`-Xsource:3` と `-Xsource:3-cross` の 2 通り）。jar 側は `imports_jar.scala` で、`scala.collection.mutable.*` / `scala.math.*`（package object）/ `scala.collection.immutable.{ListMap => LM}` / `scala.util.control.NonFatal` を使います。`scala.language` の機能名は `imports_lang.scala`。負例は `imports_star_bad.scala`（`-Xsource:3` なしの `import p1.*`）、`imports_unknown_bad.scala`（`import p1.Nope`）、`imports_hide_bad.scala`（`import p1.p2.{B => _, _}` のあとに `B` を使う）で、いずれも scalac 2.13.16 と同じ診断・同じ成否になることを確認しています。正常系の期待出力は実 scalac 2.13.16 の出力そのままです。

slick 由来の構文は `crates/cli/tests/slickparse.rs`（fixture 接頭辞 `slickparse`）の専用スイート（6 本）です。正常系はすべて **scalac 2.13.16 と scala-rs の両方でコンパイルして実行し、stdout を突き合わせる**差分テストで、`scalac` か jar が無ければスキップします。`slickparse_catch_expr.scala` は `try b catch <PartialFunction 値>`（ハンドラの遅延評価・1 回だけ・受け付けない例外の再送出・`finally` 併用・値位置の `try`・`catch { 値 }`）、`slickparse_pattern_star.scala` は `-Xsource:3` / `-Xsource:3-cross` での `case List(h, t*)`（`t @ _*` / `_*` / ユーザー extractor / 大文字名の束縛と併記）、`slickparse_super_type.scala` は型位置の `super.T`（戻り値型 / パラメータ型 / ローカル `val` / 型エイリアス / `extends` の親 / `C.super`）を回します。負例は `slickparse_pattern_star_bad.scala` で、フラグ無しと `-Xsource:2.13` の両方で nsc と同じ `bad simple pattern: use _* to match a sequence` になることを見ています。

到達不能コードの除去は `crates/cli/tests/deadcode.rs`（fixture 接頭辞 `dead`）の専用スイート（7 本）です。`dead.scala` を私有ランタイムと `--scala-library` の両方でコンパイルし、`java -Xverify:all` で実行して**実 scalac 2.13.16 の stdout そのまま**の期待値と突き合わせます。加えて classfile を `javap -p -c` で読み、`boom()` / `both()` が `athrow` で終わって到達不能な `ireturn` を持たないこと、`Main` / `Main$` の**全メソッドが終端命令で終わる**ことを固定します（前者を落とすと `VerifyError: Operand stack underflow`、後者を落とすと `Control flow falls through code end`）。到達不能でも型検査はすることは `dead_bad.scala` で見ています。

明示的型適用と implicit 節の結び付きは同じファイルの `dead_targs.scala`（library dual-run のみ。
可変長引数の実行に jar の `Seq` が要るため）です。オーバーロードあり / なしの型適用、
implicit 変換で届く拡張メソッド、親から継承した implicit（as-seen-from）、
クラス型パラメータを含む引数からのメソッド型パラメータ推論、
同型候補の specificity（自分の evidence が継承した `tpe` に勝つ）、
親コンストラクタ引数の型引数代入を回します。期待出力は**実 scalac 2.13.16 の stdout そのまま**。
負例は `dead_targs_bad.scala` で、型適用でオーバーロードを絞っても witness の無い implicit は
黙って埋めずに診断します。

trait のメンバークラスの `$outer` と、共変な戻り値型のオーバーライドで要る bridge は
`crates/cli/tests/outer.rs`（fixture 接頭辞 `outer`）の専用スイート（5 本）です。
`outer.scala` は trait のメンバークラス、`class` / `trait` / `object` からのインスタンス化、
2 段ネスト（`Inner` の中の `Deep`）、`new p.Inner`（前置詞つき）、内側からの
`def` / `val` / `lazy val` / 型メンバ参照、そして trait でない従来のネストクラスを回します。
`outer_bridge.scala` は `case object` の `override def reverse: Desc.type = Desc` と
`override def self: Dog` と `object` の共変オーバーライドです。どちらも**私有ランタイムと
`--scala-library` の両方**でコンパイルして `java -Xverify:all` で実行し、期待出力は
**実 scalac 2.13.16 の stdout そのまま**です。`outer_self.scala` は slick のケーキそのもの
（`trait Comp { self: Prof => abstract class Table }`）で、trait メソッド内のローカル
クラスと trait のメンバークラス内の匿名クラスも回します。
`outer_field_is_the_trait_interface` は `T$Inner` の `<init>` が
`(LT;Ljava/lang/String;)V`、`T$Inner$Deep` が `(LT$Inner;)V` であることを、
`outer_field_is_the_self_type` は `Comp$Table` が `(LProf;Ljava/lang/String;)V` である
ことを（どちらも nsc と同じ `$outer` の型と位置）classfile のバイト列で固定します。

def マクロは `crates/cli/tests/macros.rs` にまとめています。呼ばれない macro def のコンパイルと、`Sugar$.class` にメソッドが出ていないことは `macro_def.scala`。マクロ呼び出しの診断は `macro_call_bad.scala`（`macro expansion is not implemented`）。戻り値型の無いマクロ def は `macro_no_result_type_bad.scala`。`Context` を第 1 引数に取らない実装は `macro_impl_shape_bad.scala`。解決できない実装参照は `macro_impl_missing_bad.scala`。whitebox は `macro_whitebox_bad.scala`。設計は [`docs/macros.md`](docs/macros.md)。

名前付き引数とデフォルト引数は `tests/fixtures/namedargs.scala` にまとめ、`crates/cli/tests/e2e.rs` から 2 通りで回します: `scala_library_dual_run_namedargs`（jar リンクで実行し `expected/namedargs.txt` と一致）と `real_scalac_dual_run_namedargs`（**実 scalac 2.13.16 でコンパイル・実行した stdout** と、期待値および scala-rs の出力の三者が一致することを見る）。中身は並べ替え（`Api.area(height = 3, width = 4)`）、自分の位置にある名前付き引数のあとの位置引数（`Api.area(width = 4, 3)`）、デフォルトとの組み合わせ、コンパニオン `apply`、後続の引数リストのデフォルト（`Api.curried(1)(2)` / `Api.dep(4)()`）、可変長引数（`Api.tagged(first = 1)` / `Api.tagged(first = 1, 2, 3)`）、case class の `apply` / `copy` / `super.info.copy(port = 2)`、コンストラクタの名前付き引数とデフォルト（`new Server(threads = 8)` / `new Server()`）、パラメータ名で絞るオーバーロードです。負例は `namedargs_unknown_bad.scala`（`unknown parameter name: q`。メソッドとコンストラクタの両方）、`namedargs_dup_bad.scala`（`parameter 'c' is already specified at parameter position 2`）、`namedargs_order_bad.scala`（`positional after named argument.`）で、いずれも文面を実 scalac 2.13.16 に合わせています。
| `lazysig.scala` | 型注釈のないメンバを前方参照（`Store.base` / `prefix` / `lazy val`） | `60` `log:store` `[store]log:store` `40` `5` `c7:7` |
| `impl2.scala` | 多相 implicit の再帰導出（`Show[List[List[Int]]]` / `Show[(A, B)]` / `Ord[List[List[Int]]]`）、specificity（`Tag[Int]` は `tagInt`）、`<:<`（`upcast[Int, Any]`）、`List`/`Iterator` の `toMap`（library dual-run のみ） | `1` `hi` `[1,2,3]` `[[1],[2,3]]` `(1,x)` `[(1,a),(2,b)]` `Some(7)` … `ab` `cd` |
| `impl2_poly.scala` | 同じ導出をユーザー定義型だけで（私有ランタイムでも走る） | `1` `Box(2)` `Box(Box(3))` `<4,four>` `Box(<5,five>)` `<Box(6),Box(six)>` `int` `any` |

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、違法 compound は `compound_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` を val に付けたのは `inline_bad.scala`。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。型注釈のないメンバどうしの相互再帰は `lazysig_cyclic_bad.scala`（scalac 2.13.16 と同じ `recursive value y needs type`）。多相 implicit の導出が底を打たないのは `impl2_missing_bad.scala`（`no implicit`。型パラメータを黙って `Any` で埋めない）、同じ形の多相 implicit が二つあるのは `impl2_ambiguous_bad.scala`（`ambiguous implicit: boxA, boxB`）、発散する導出は `impl2_diverging_bad.scala`（`implicit def loop[A](implicit a: A): A` を必ず打ち切り、scalac 2.13.16 と同じ `diverging implicit expansion for type Show[Int] starting with method loop`）。
implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、違法 compound は `compound_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` / `@noinline` は実 scalac 2.13.16 と同じくどの定義（val / var / class / type ...）にも、両方同時にも、警告なしで付けられる（`crates/cli/tests/smallgaps.rs` の `fixtures_sgap`。`inline_bad.scala` は削除）。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。型注釈のないメンバどうしの相互再帰は `lazysig_cyclic_bad.scala`（scalac 2.13.16 と同じ `recursive value y needs type`）。
| `exptype.scala` | 期待型からのメソッド型パラメータ推論（nsc `instantiateExpecting`）：`val a: Array[AnyRef] = Array("x", "y")` / `val b: Array[Any] = Array(1, 2)` / 期待型だけが `T` を決める implicit 付き `column[T]` / 不変位置は期待型が勝ち共変位置は引数が勝つ（library dual-run のみ） | `2` `x` `[Ljava.lang.Object;` `2/x` `2` `2` `[Ljava.lang.Object;` `[I` `4` `id:int` `nm:str` `any` `int` `cov str` `List()` |
| `dead.scala` | 到達不能コード（`def f(): Int = throw e` / 片側だけ `throw` の `if` / 両側 `throw` / `throw` する `match` の case / 非局所 return / 常に投げる `try/finally` / catch が投げる `try/catch/finally`）と、finalizer / `monitorexit` を飛ばさない `return`。期待出力は実 scalac 2.13.16 の stdout そのまま | `eboom` `7` `ehalf` `et` `zero` `one` `bad pick 2` `1` `3` `0` `6` `-1` `fin3` `40` `fin3` `1` `105` `2` `fin` `outer inner` `fin2` `caught b` |
| `dead_targs.scala` | 明示的型適用が implicit 節に届くこと（オーバーロードあり／なし、implicit 変換経由の拡張メソッド、可変長引数）、継承 implicit の as-seen-from、クラス型パラメータを含む引数からの推論、同型候補の specificity、親コンストラクタ引数の型引数代入（library dual-run のみ。期待出力は実 scalac 2.13.16 の stdout そのまま） | `abs/int` `abs/bool` `== raw t` `== typed bool` `bool:3` `int` `abs/int` `int\|r` `c!/bool` `int` |
| `ovl.scala` | エイリアス型メンバ / 普通のクラスのコンパニオン `apply`（デフォルト引数・可変長引数＋implicit 節）/ 値の位置で勝つ `val ==` と抽出子 / 型注釈のない `unapply` の前方参照 | `7` `1` `cfg` `t/2/true` `t/2/false` `int/5/false` `string/s/false` `int:3` `string:0` `=` `eq 42` `not 7` `not@7` |
| `numt.scala` | 7×7 の数値変換（NaN / ±Inf / MIN・MAX 込み）、`Byte`/`Short` のパラメータ・戻り値・フィールド・配列・オーバーフロー、演算子の昇格、弱適合、`Short` スクルティニーの `Int` 定数パターン（両 ABI で実行し real scalac の stdout と一致） | `B 0 0 0 0 0 0.0 0.0` … `100\|30000\|a` |

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、違法 compound は `compound_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` を val に付けたのは `inline_bad.scala`。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。型注釈のないメンバどうしの相互再帰は `lazysig_cyclic_bad.scala`（scalac 2.13.16 と同じ `recursive value y needs type`）。オーバーロードで甲乙つけがたい候補は `ovl_ambiguous_bad.scala`（scalac は `ambiguous reference to overloaded definition`）、コンパニオン `apply` のパラメータ型に合わない呼び出しは `ovl_none_bad.scala`（末尾のデフォルト引数は先行パラメータを省略可能にしない）。期待型からの型パラメータ推論の負例は `exptype_unsolved_bad.scala`（引数でも期待型でも `T` が決まらず、nsc と同じ `could not find implicit value …`）と`exptype_arrayvar_bad.scala`（`Array` は非変なので `Array[Int]` は `Array[Any]` に渡せない）です。

| `mism.scala` | 型引数が途中で落ちて `Any` が発明される一群の修正（library dual-run のみ）：呼び出し側の型パラメータを含む引数からのメソッド型パラメータ推論、ジェネリッククラス内の `this` / `super`、反変パラメータを含む lub、`Vector`/`Set`/`Map` → `Seq`/`Iterable` の継承、`@uncheckedVariance` などのアノテーション付き型の適合、モジュールの `.type`、多相メソッドの eta 展開、`extends Base[T](y)` の親コンストラクタ引数、`type Self >: this.type` への `this` の適合 | `1` `2` `1` `2` `1,2,3` … `sub(act:s)` `w` `leaf` |

`mism.scala` は `crates/cli/tests/mismatch.rs` から回します。同ファイルには最小形の受理テスト
（`callers_type_parameter_survives_inference` / `this_carries_the_classes_type_arguments` /
`super_is_seen_from_the_subclass` / `collections_conform_to_their_supertypes` /
`varargs_join_respects_contravariance` / `annotations_and_module_singletons` /
`eta_expansion_solves_the_methods_type_parameters` /
`parent_constructor_arguments_use_the_extends_clauses_arguments` /
`this_conforms_to_a_member_bounded_below_by_this_type`）も置いてあります。逆に、
不変クラスの型引数が違えばちゃんと落ちることは `mism_bad.scala`
（`Inv[Int]` を `Inv[Any]` に渡す）で固定しています。

### Remaining

- **slick の計測は `.fm` テンプレートを展開してから行う**。slick は `GetResult` /
  `SetParameter` / `TupleSupport` など 7 本を FreeMarker テンプレートとして持ち、
  ビルド時に生成します。生成せずに計測すると、その 7 本に依存する 7 ファイルが
  「scalac でも落ちる」エラーを出すため、`tests/expand_fm.py` で展開して一緒に
  コンパイルします（`tests/slick_measure.sh` が自動で実行）。この 7 本を含めた
  時点で計測対象は 177 → 184 ファイルになり、エラー数も一段増えます（1371 → 2064）。
  数字が増えたのは退行ではなく、計測が実際のコンパイル対象に追いついたためです。

- **override 検査が無い**。`override` 修飾子の要否も、override 時の型適合も検査していない。
  scalac が拒否する次の 2 つを黙って通す:
  `trait T { def f: Int = 1 }; class D extends T { def f: Int = 2 }`（`override` 無し。
  scalac: ``` `override` modifier required to override concrete member ```）、
  `class D extends T { override def f: String = "x" }`（親は `Int`。scalac:
  `incompatible type in overriding`）。`val` も同様。受け入れすぎる側の穴。
- **`obj[T1, T2]`（引数リスト無しの暗黙 `apply` 挿入）で型引数が落ちる**。
  `object R { def apply[L, M, U]: Shape[L, M, U, M] = … }` に対し
  `R.apply[L, Rep[T], T]` は通るが `R[L, Rep[T], T]` は
  `found: Shape[L, Rep[T], T, Rep[T]]  required: Shape[L, Rep[T], T, Rep[T]]`
  になる（表示は同じで `L` が呼び先の型パラメータのまま）。`apply` を挿し込む際に
  TypeApply の型引数が結果型へ substitute されていない。slick の
  `RepShape[Level, Rep[T], T]`（`Shape.scala` / `Query.scala`）がこれ。
  明示的型適用まわりなので `agent/deadcode` の担当範囲。
- **`Array[T]` から `Seq[T]` への暗黙変換**。`def k(x: Array[Int]): Seq[Int] = x` は scalac
  では（deprecation 警告つきで）通るが、こちらは type mismatch になる。`Predef` の
  `copyArrayToImmutableIndexedSeq` / `wrapIntArray` 相当の暗黙変換が prelude に無い。
- **可変長引数の要素型を期待型から取れない**。`Vector(a, b)` の `A` は引数の lub からしか
  決まらない。lub 側は反変パラメータと型パラメータ境界を見るようにしたので slick の
  `AndThenAction[R2, S2, E with E2](Vector(this, a))` は通るようになったが、
  期待型 `Iterable[(String, Dumpable)]` から `A` を決める経路（nsc の
  `instantiateExpecting` を親型経由で辿る形）はまだ無い。slick の `DumpInfo(children = …)`
  が `found: Vector[Any]  required: Iterable[(String, X)]` で落ちるのはこれ。

- **明示的な型適用が implicit 引数リストに伝わらない場合がある**。
  `Library.Abs.column[P1](n)`（`def column[T : TypedType]`）や
  `Library.==.typed[Boolean](ch)`（オーバーロードのある `def typed[T : ScalaBaseType]`）
  で、明示指定した型引数が後続の implicit 節に届かず `TypedType[P1]` /
  `ScalaBaseType[T]` を探しに行ってしまう。期待型からの推論（実装済み）とは別の穴で、
  TypeApply とオーバーロード解決の側にある。
- **自己型を通した型メンバーの解決**。`trait JdbcTypesComponent { self: JdbcProfile => }`
  の中で `BaseColumnType` を書くと、自己型の `type BaseColumnType[T] = JdbcType[T] &
  BaseTypedType[T]`（具象別名）ではなく線形化側の抽象宣言
  `type BaseColumnType[T] <: ColumnType[T] & BaseTypedType[T]` が選ばれる。
  そのため `def base[U : BaseColumnType]` の evidence が `JdbcType[U]` に適合せず、
  `new MappedJdbcType[T, U] with BaseTypedType[T]` の親 implicit 節が
  `could not find implicit value of type JdbcType[U]` になる（scalac は通す）。
- **`Ordering[Null]` が探索で見つからない**。nsc は
  `Ordering.ordered[Null](Predef.$conforms[Null])` を組み立てるが、こちらは
  `implicit_tree` の**入れ子**の implicit 引数に対して identity view
  （`A <: B` を `A => B` として使う）のフォールバックを回していないため、`ordered` を
  候補として採れない。`implicitly[Ordering[Null]]` 単独でも落ちる既存の穴で、
  親コンストラクタ／引数無し `new` を埋めるようにしたことで
  slick の `new ScalaBaseType[Null]` からも見えるようになった。
- **値クラス（`extends AnyVal`）が universal trait を mix-in したとき**、
  `final class C(val x: Rep[Int]) extends AnyVal with Numeric[Int, Int]` の
  インスタンスがインタフェースを実装していない classfile になる
  （実行時 `IncompatibleClassChangeError`）。値クラスは box を出していないため。
- **`}` の次の行が `-1` で始まると式が続いていると読む**。
  `if (c) { return n }` の直後の行の `-1` が `(return n) - 1` にパースされ、
  `value - is not a member of Nothing` になる（scalac は改行で切る）。
- **親コンストラクタの implicit 節を埋めていない**。
  `abstract class TypedRep[T](implicit val tpe: TT[T])` を
  `class ConstColumn[T : TT] extends TypedRep[T]` が継承すると、`extends` に引数リストが
  無いので witness を渡さず、コード生成が存在しない `TypedRep.<init>()` を呼ぶ
  （実行時 `NoSuchMethodError`）。**黙って通っている**ので直すべき穴。
  親位置の木は 2 回型付けされるため、埋めた implicit 引数（`Ident` として合成される）が
  2 回目に名前で解決し直されて壊れる、というのが難しいところ。
  ClassTag の `ClassTag.apply(classOf[T])` フォールバックも親位置では型付けできない。
- **値クラスの `$extension` 静的メソッドの置き場所**が nsc と違う。nsc は本体をコンパニオン
  `C$` に置いてクラス側をフォワーダにしますが、scala-rs はクラス側に直接出します。
  同一プログラム内では等価ですが、scalac が出した classfile との相互リンクはできません。
  （universal trait の実装・box / unbox・パターンマッチ・配列要素・`equals` / `hashCode` は
  `agent/valclass` で nsc と一致させました。）
- **ライブラリ側の値クラスは box しない**。prelude が `StringOps` / `ArrayOps` を
  `augmentString` などの identity 変換としてモデル化していて、`map` の戻りのような
  「本当は `String`」の位置を値クラス型で持っているため、そこを box すると
  `println` に `StringOps` を渡してしまいます。box の対象はこのコンパイル単位が出す値クラス
  だけに限ってあります（`erasure::note_source_value_classes`）。prelude の
  `StringOps` シグネチャを実体に合わせれば外せる制限です。
- **ボックス型と値クラスの同一視**。prelude が `scala.Int` に JVM 名 `java/lang/Integer`
  を与えているため、`java.lang.Integer` / `java.lang.Long` が `scala.Int` / `scala.Long`
  に解決されてしまう。`java.lang.Integer.valueOf(3)` は `value Integer is not a member of
  <notype>`、`new java.util.ArrayList[java.lang.Long]` への `add(7L)` は型不一致になる。
  scalac では別の型なので、別シンボルとして分ける必要がある。
- **`Array` の非変性**。`Array[Int]` を `Array[Any]` に渡せてしまう（scalac は拒否）。
  クラスの型引数は不変位置で等価性を要求するようにしたが、`Array` だけは covariant の
  ままにしてある。`val a: Array[AnyRef] = Array("x", "y")` を通すには**期待型からの
  メソッド型パラメータ推論**が要り、それが未実装のため。両方まとめて直すべき穴。
- **Java static のスコープ**は「インスタンス経由では見えない」だけを実装した（scalac と
  同じ `value parseInt is not a member of Integer`）。nsc のように static をコンパニオン
  オブジェクトの本物のメンバとして持ち直してはいないので、`java.lang.Integer.valueOf` は
  クラスシンボル経由の選択として通している。
- **Java の `static final` 定数の ConstantValue を読んでいない**。`public static final
  int functionNoTable = 1;` はディスクリプタどおり `Int` になるので、scalac が
  `Int(1)` の定数型として `Byte` / `Short` へ narrow するところ
  （`val q: Short = java.sql.DatabaseMetaData.functionNoTable`）が
  `type mismatch; found: Int  required: Short` になる。classfile の `ConstantValue`
  属性を読んで `Type::Constant` を付ければ直る。パターン位置
  （`case DatabaseMetaData.functionNoTable`）は `Byte` / `Short` / `Char` の
  スクルティニーに `Int` 定数を許すようにしたので通る。
- **`Long.MinValue` / `Int.MinValue` のリテラル表記**。`-9223372036854775808L` は
  `integer literal out of range`、`-2147483648` は `type mismatch; found: Long
  required: Int` になる（scalac は単項 `-` をリテラルに畳み込む）。回避は
  `-9223372036854775807L - 1L`。
- **`unary_+`**。`+x` はどの数値型にも宣言していない。
- **私有ランタイムの `Array(...)` varargs**。`Array(1, 2)` / `Array(1L, 2L)` /
  `Array(1.toByte)` はいずれも `--no-scala-library` では
  `no matching overload for (Int)Any` になる（`new Array[T](n)` は動く）。
  `Byte` / `Short` に固有の穴ではなく、私有ランタイムに `ClassTag` が無いため。

- **テストハーネスの一時ディレクトリ衝突**（`cargo test --workspace` が不定期に落ちる）。
  `crates/cli/tests/{xsource3,imports,e2e,lang,...}.rs` の `tmp_dir(tag)` は
  `{tag}-{pid}-{nanos}` で名前を作るが、同じ fixture 名を tag に使うテストが
  複数あり、macOS の `SystemTime` はマイクロ秒粒度なので、並列実行で同じ瞬間に
  入った 2 本が**同じディレクトリを共有**する。片方の `remove_dir_all` が
  もう片方の classfile を消し、`NoClassDefFoundError` や
  `ClassFormatError: Truncated class file` になる。main でも再現する。
  各スイートを単独で回せば必ず通る。`tmp_dir` にプロセス内カウンタを足せば直る
  （`crates/cli/tests/outer.rs` はそうしてある）。
- **trait のメンバークラスの分割コンパイル**。同一 run（複数ファイルを 1 回の
  `compile` に渡す）は通るが、先に emit した classfile を `-cp` 経由で読む別 run では
  `value describe is not a member of People` になる。pickle からメンバークラスの
  メンバを復元できていない（`$outer` 対応の前からある穴で、今回変えていない）。
- **`x.foo = v` の setter メソッド呼び出しへの書き換え**。`class C { def foo: Int = …;
  def foo_=(x: Int): Unit = … }` に対する `c.foo = 4` は nsc なら `c.foo_=(4)` だが、
  こちらは型検査を通したうえでフィールドへ `putfield` するので `NoSuchFieldError: foo`
  で落ちる。refinement 型（`structural_select_lhs`）だけは書き換えている。
- **`override` 修飾子と override 適合性の検査**。`class D extends T { val v = "d" }`
  （`override` 無し）も `override val v: Int = 5`（親は `String`）も、nsc は
  それぞれ `override modifier required` / `type mismatch` を出すが、こちらは黙って通す。
  `val` に対する override 検査そのものが無い（`def` の `@Override` 検査だけがある）。
- **implicit 探索の残り**: 多相 implicit のユニフィケーションと再帰導出、発散の打ち切り、nsc 相当の specificity は入った（「Implicit 解決」節）。残るのは (a) `xs.toMap` を `scala.collection.Iterable` にも載せること — pickle 供給が具象コレクション（`HashMap` / `ConstArray` …）に自前の `toMap` を付けるので、継承した 2 本目がオーバーロード衝突になる。いまは `List` / `Iterator` だけに宣言している、(b) 期待型からのメソッド型パラメータ推論が要る implicit（slick の `TypedType[T]` / `TypedType[P1]` はこちらで、implicit 探索ではなく `T` の推論が先に必要）、(c) 診断文面は nsc の複数行（`both … and … match expected type …`）ではなく 1 行のまま
- **def マクロの展開**。`def f: T = macro Impl.method` は**パースして**バインディングを
  シンボル（`Symbol.macro_impl`）に記録し、マクロ def のバイトコードは nsc と同じく出さない。
  展開器はまだ無いので、呼び出し地点は `macro expansion is not implemented` で診断する。
  残件は、実行モデルの実装（`docs/macros.md` §2 の JVM ブリッジ。設計は動く prototype で
  実証済み）、マクロバインディングの pickle（nsc の `MACRO` フラグ + `@macroImpl`。§5）、
  `scala.reflect` API の prelude と ABI コード生成（§6 フェーズ 3）、
  quasiquote と `reify`（fast track マクロなので自前実装が要る。§6.2）、
  whitebox と macro bundle。テストは `crates/cli/tests/macros.rs`
- **leftover pickle holes**（nsc 完全 pickle ではない）: MACRO / late・anti flags は **scalac 2.13.16 が既存 emit（`separate_lib` pickle）を typecheck するのに不要**だったので実装しない。`type T = Int` は nsc **ALIASsym**（tag 5）として載せた。2.13 PickleFormat に **ALIAStpe タグは無い**。named annot args の ctor 順並べ替えは **不要**: scalac 2.13.16 は `#29`/`#30` と同じ位置 pickle（ソース上の RHS 順）で `@Ann2(b = 2, a = "ok")` を typecheck する。nsc 自身は named annot をブロックに変換すると warning を出す。`@Ann(foo = 1)` の Constant と `@Ann(foo = this.x)` / `@Ann(foo = bar)` の TREE は nsc と同じ位置引数として載せた。**JAVA を EXTREF に載せない理由**: PickleFormat の `EXTref` / `EXTMODCLASSref` は `name_Ref [owner_Ref]` だけで flags フィールドが無い。余分な Nat を足すと scalac が owner と取り違える。`java.lang.Object` / `String` などは classpath の Java classfile から complete され、そこで JAVA が付く。local CLASSsym（prelude で `mark_java` したクラスを自前 pickle する場合）には既に JAVA を出している。full pickle とは主張しない
- 残りの **StringOps**（`++` / `lengthIs` / `sizeIs` / `flatMap` / `iterator` / `sizeCompare` / `knownSize` / `appendedAll` / `prependedAll` / `>` / `>=` / `<=` / `compare` / `lengthCompare` / `patch(Int, String, Int)` / `<` / `map`（`Char => Char`）/ `:+` / `+:` / `foldRight` / `toByteOption` / `toShortOption` / `toFloatOption` / `grouped` / `foldLeft` / `toByte` / `toShort` / `toFloat` / `toLongOption` / `toDoubleOption` / `find` / `foreach` / `toBoolean` / `toBooleanOption` / `dropWhile` / `takeWhile` / `nonEmpty` / `headOption` / `lastOption` / `filterNot` / `indices` / `r` / `sorted` / `toArray` / `copyToArray` / `partition` / `exists` / `forall` / `splitAt` / `updated` / `count` / `span` / `diff` / `intersect` / `split(String)` / `filter` / `reverseIterator` 以外）
- 残りの **ArrayOps**（`lengthIs` / `sizeIs` / `indexOf` / `copyToArray` / `iterator` / `zipWithIndex` / `knownSize` / `sizeCompare` / `filterNot` / `headOption` / `lastOption` / `partition` / `splitAt` / `span` / `find` / `contains` / `distinct` / `takeRight` / `dropRight` / `takeWhile` / `indices` / `lengthCompare` / `last` / `init` / `reverse` / `size` / `isEmpty` / `nonEmpty` / `scanLeft` / `count` / `forall` / `foldLeft` / `fold` / `foldRight` / `drop` / `dropWhile` / `exists` / `take` / `collect` / `zip` / `filter` / `slice` / 3 引数 `flatMap` / 4 引数 Array→Iterable `flatMap` と primitive wrappers / `genericArrayOps` の `head`/`map`/`foreach`/`tail` は揃った。他メソッド。`reduce` は 2.13.16 ArrayOps に無い）
- 他の mutable（`ArrayDeque` / `LinkedHashMap` / `LinkedHashSet` / `HashMap` / `HashSet` / `ArrayBuffer` / `ListBuffer` 以外）と他の immutable（`BitSet` / `SortedMap` / `TreeMap` / `SortedSet` / `TreeSet` / `Set` / `Map` / `Vector` 以外）。`scala.collection.View` の `List.view` / `map` / `toList` と `View.fill` / `View.iterate` は乗った（他の View は未）。`scala.util.control.Breaks` の `breakable` / `break` / `tryBreakable`+`catchBreak` は乗った（他の control は未）。`scala.math.BigInt` / `BigDecimal` の `apply(Int)` / `apply(String)` / `+` / `*` / `int2bigInt` は乗った（他の math は未）。`scala.util.chaining` の `pipe` / `tap` は乗った。`scala.util.Using.resource` / `Using.apply` / `Using.Manager` / `Using.resources`（2–4 引数）は乗った（他の Using は未）
- **pickle からのシンボル自動供給の残り**: リーダ・シグネチャ復元・線形化・
  型検査への接続は動いていて、`List` / `Option` / `Map` / `Set` / `Vector` / `Range` /
  `Iterator` の 60 以上のメンバ（演算子と companion メンバを含む）が prelude 手書きなしで通り、
  実行結果は scalac 2.13.16 と一致する。残りは
  (a) **シンボル表に既にあるクラスの作り直し**（`scala/collection/Seq` に型パラメータが無いため
  `diff` / `intersect` / `union` / `indexOfSlice` / `containsSlice` が供給できない。
  後付けは手書きメンバを壊したので採らない）、
  (b) スタブに親鎖を与えないことによる部分型の弱さ、
  (c) **既定引数のゲッタ規約**の食い違い（`check.rs` 側の修正が要る）、
  (d) `String.format` のような拡張メソッド経路と `scala.io.Source` の Java ローダ経路、
  (e) ラムダ由来の型推論（`reduceOption`、インライン `collect { case … }`）。
- **jar の package object の型エイリアスの残り**: `scala` / `cats.effect` の別名は解決するが、
  右辺が **object にネストしたクラス**（`type ParallelF[F[_], A] =
  cats.effect.kernel.Par.ParallelF[F, A]`）のものはまだ復元できない。
  `install_classpath` が companion のある trait の単純名を module class の JVM 名で
  先に取ってしまう（`Outcome` → `cats/effect/kernel/Outcome$`、型パラメータ 0 個）ため、
  `resolve_dotted_class` は「パスが名指す classfile を読み直す」で直しているが、
  `Par.ParallelF` のようにパスの途中が object の形はまだ通っていない。
  復元できない別名は登録せず、使用時に理由付きで診断する。
- **slick の `Ref[F, ExecState]` の推論**: 別名自体は解決するようになったが、
  `Ref[F, ExecState]` を `Ref[Any, ExecState]` と照合してしまう（HK クラス型パラメータ `F`
  が `Any` に落ちる）。これは別名ではなく型引数推論側の穴。
  詳細は「ScalaSignature からのシンボル自動供給」節
- **`Either` / `Try` / `Option` の残り**: `Either` の `joinLeft` / `joinRight` / `flatten` / `toTry` / `cond`（`<:<` を要求するもの、および companion）、`LeftProjection` の `filter`、`Try` の `flatten`、`Option` の `orNull` / `unzip` / `unzip3` / `iterator` / `when` / `unless` / `empty` / `apply`（companion）。**2.13 の `Either` に `withFilter` は無い**ので `for` のガードは nsc どおりコンパイルエラーのまま（`filterOrElse` を使う）。私有ランタイムは `Either` / `Try` を持たないので、そのまま診断する
- **`java.lang` の例外**は `ArithmeticException` / `ClassCastException` / `IllegalArgumentException` / `IllegalStateException` / `IndexOutOfBoundsException` / `NullPointerException` / `NumberFormatException` / `UnsupportedOperationException` と `Throwable` / `Exception` / `RuntimeException` の `()` / `(String)` コンストラクタ、`getMessage` まで。他の JDK 例外・メソッドは未
- 残りの **`List`**: `flatten`（`A => IterableOnce[B]` の暗黙 `Predef.$conforms` 解決が要る）、`toBuffer` / `toIndexedSeq`（`toMap` は乗った）、`sortBy` 以外の `*Option` 系（`maxOption` / `minOption` / `reduceOption`）、`patch` / `diff` / `intersect` / `unzip` / `partitionMap` / `tails` / `inits` / `corresponds` / `segmentLength` / `indexWhere` / `lastIndexWhere` / `zipAll` / `padTo` / `mapConserve` / `tapEach` / `sameElements` は未。`Ordering` の implicit インスタンスは `Int` / `Char` / `String` / `Long` / `Boolean` だけ（`Double` は 2.13 の `Ordering.Double.TotalOrdering` / `DeprecatedDoubleOrdering` の切り分けが要る）。`Numeric` は `Int` / `Long` / `Double` だけ。`xs.collect { case … }` のインライン `PartialFunction` リテラルを直接渡す形は typer が未対応（ArrayOps と同じ。型注釈付きの `val pf: PartialFunction[A, B]` を渡す）。私有ランタイム側は `map` / `flatMap` / `foreach` / `withFilter` / 上記コア以外は未（`Function2` classfile が無いので `foldLeft` 系は出せない）
- 他の mutable（`ArrayDeque` / `LinkedHashMap` / `LinkedHashSet` / `HashMap` / `HashSet` / `ArrayBuffer` / `ListBuffer` / `StringBuilder` 以外）と他の immutable（`BitSet` / `SortedMap` / `TreeMap` / `SortedSet` / `TreeSet` / `Set` / `Map` / `Vector` 以外）。`scala.collection.View` の `List.view` / `map` / `toList` と `View.fill` / `View.iterate` は乗った（他の View は未）。`scala.util.control.Breaks` の `breakable` / `break` / `tryBreakable`+`catchBreak` は乗った（他の control は未）。`scala.math.BigInt` / `BigDecimal` の `apply(Int)` / `apply(String)` / `+` / `*` / `int2bigInt` は乗った（他の math は未）。`scala.util.chaining` の `pipe` / `tap` は乗った。`scala.util.Using.resource` / `Using.apply` / `Using.Manager` / `Using.resources`（2–4 引数）は乗った（他の Using は未）
- **`java.lang.String` の素のメソッド**（`trim` / `substring(1/2 引数)` / `lastIndexOf` / `replace(Char,Char)` / `replace(CharSequence,CharSequence)` / `contains(String)` / `equalsIgnoreCase` / `matches` / `strip` / `repeat` / `compareTo` を追加。既存の `startsWith` / `endsWith` / `indexOf` / `split` / `charAt` / `concat` と、StringOps 経由の `toUpperCase` / `toLowerCase` / `isEmpty` と重複させていない）。`chars()` / `codePoints()`（`java.util.stream.IntStream` を返す）は Stream 型のインフラが無く未対応
- **`scala.collection.mutable.StringBuilder`**（bare `StringBuilder`＝`scala.StringBuilder` エイリアスを追加。`append` の全プリミティブ + `String` + `Any` オーバーロード、`+=`（`Char`）、`++=`（`String`）、`insert`、`deleteCharAt`、`setLength`、`reverse`、`clear`、`isEmpty` / `nonEmpty`、`length`、`result`、`charAt`、`apply`、`(Int)`/`(String)` コンストラクタ。`reverse` は `IndexedSeqOps` 由来で erase されるため checkcast している）
- **`Range` のコレクション系**（`withFilter`（for 内包表記の guard に必須）/ `filter` / `filterNot` / `map` / `flatMap` / `foldLeft` / `foldRight` / `sum` / `product` / `min` / `max` / `reverse` / `toList` / `toArray` / `toVector` / `zipWithIndex` / `exists` / `forall` / `count` / `take` / `drop` / `takeWhile` / `dropWhile` / `head` / `last` / `isEmpty` / `nonEmpty` / `size` / `contains` / `by` / `splitAt` / `slice` / `takeRight` / `dropRight` を追加。`sum` / `min` / `max` は `Range` 自身の `int` 返しオーバーロードへ `Numeric$IntIsIntegral$` / `Ordering$Int$` を渡して直接呼ぶ。`filter` / `filterNot` / `flatMap` / `zipWithIndex` / `toArray` は `Object` へ erase された唯一のオーバーロードしか無いため checkcast / `ClassTag` 経由）
- **`scala.math` パッケージオブジェクト関数**（`abs` / `max` / `min` / `signum`（`Int`/`Long`/`Float`/`Double`）/ `pow` / `sqrt` / `cbrt` / `floor` / `ceil` / `round` / `random` / `exp` / `log` を追加。実体は静的フォワーダクラス `scala.math.package` の `invokestatic`）
- **numeric enrichment の穴**（`RichInt`/`RichLong`.`toBinaryString`/`toHexString`/`toOctalString`/`sign`、`RichDouble`.`isNaN`/`isInfinity`/`round`/`floor`/`ceil`/`sign`、`RichChar`.`isLetter`/`isLetterOrDigit`/`isUpper`/`isLower`/`isWhitespace`/`toUpper`/`toLower` を追加。`sign`/`round`/`floor`/`ceil` は `$extension` static が無いため `java.lang.Integer/Long.signum` と `java.lang.Math` に委譲。`RichInt`/`RichLong`/`RichDouble`/`RichChar`/`RichByte`/`RichShort`.`compare` は `$extension` も無く、`RichBoolean.compare` のような実インスタンス化 codegen が要るため未対応のまま — `RichBoolean.compare` のみ既存の codegen を再利用して実装）
- 他の mutable（`ArrayDeque` / `LinkedHashMap` / `LinkedHashSet` / `HashMap` / `HashSet` / `ArrayBuffer` / `ListBuffer` / **新規 `mutable.Map` / `mutable.Set`** 以外）と他の immutable（`BitSet` / `SortedMap` / `TreeMap` / `SortedSet` / `TreeSet` / `Set` / `Map` / `Vector` 以外）。`scala.collection.View` の `List.view` / `map` / `toList` と `View.fill` / `View.iterate` は乗った（他の View は未）。`scala.util.control.Breaks` の `breakable` / `break` / `tryBreakable`+`catchBreak` は乗った（他の control は未）。`scala.math.BigInt` / `BigDecimal` の `apply(Int)` / `apply(String)` / `+` / `*` / `int2bigInt` は乗った（他の math は未）。`scala.util.chaining` の `pipe` / `tap` は乗った。`scala.util.Using.resource` / `Using.apply` / `Using.Manager` / `Using.resources`（2–4 引数）は乗った（他の Using は未）
- **collections スライス**（`ArrayBuffer` / `ListBuffer` / 新規 `mutable.Map` / `mutable.Set` / `immutable.Map` / `immutable.Set` / `Vector` / `Tuple2.swap`）で見つかった既知の穴。いずれも黙って壊れた挙動を出さないよう、この機能自体を未実装のまま残した（該当メンバーは prelude に追加していない）:
  - **`immutable.Map` の `++` / `concat`**: `scala.collection.immutable.Map` は `iterableFactory` を override せず（`mapFactory` のみ）、継承した `IterableOps.++`/`concat` のデフォルト実装が `iterableFactory` 経由で構築するため、`scala-library-2.13.16.jar` に対する実測で `Map(...) ++ Map(...)` が `Map` ではなく `List`（`::`）を返し `ClassCastException` になることを確認した（`Map1`〜`Map4` 特殊化クラスでも `HashMap` バックでも同様）。`immutable.Set.++` は同じ経路で正しく `Set` を返す（実測済み、`coll_immutableset1.scala` でカバー）ので、非対称な穴として記録する
  - **`MapView.mapValues[W]` のメソッド型パラメータ推論**: `W` がラムダの戻り値型からしか決まらない場合（`v => v * 2` のような式）、現状の推論はラムダの本体を型付けする前に呼び出され、`W` が未解決のまま残る。呼び出し側で明示的に `mapValues[Int](...)` と書けば動く（`coll_map_view1.scala` はこの形を使っている）
  - **Tuple2 を分配する単一引数ラムダの受け取り型**: `map.foreach(p => ...)`（`p: (K, V)` 全体を受け取る）は動くが、期待型が具体的な `Tuple2[K, V]`（型引数が既に具体化済み）のときの `MapView.foreach` などでは `p` の型が誤って `K` だけに縮退することがある（`p => p._2` が `_2 is not a member of K`）。回避策として `MapView` のようなケースでは `foreach` の代わりに `toList` / `mkString` を使う
  - **`case (k, v) => ...` の tuple パターン分配**（`Map.foreach { case (k, v) => ... }`）は immutable/mutable どちらの `Map` でも型エラーになる（既存の `Map` にも存在する既存バグで、このスライスが原因ではない）。回避策は `p._1` / `p._2` を使うラムダ
  - **`ArrayBuffer` / `ListBuffer` の `toArray`**: `ClassTag[A]` の暗黙値をメソッド型パラメータから汎用的に導出する仕組みが現状ないため見送った（`StringOps.toArray` のような固定要素型の特殊形のみ既存）
- 下限境界を宣言しているのは今のところ `List.::` だけ。`:::` / `+:` / `:+` / `concat` / `++` / `appended` / `prepended` / `updated` / `max` / `min` / `sum` / `product` / `Option.getOrElse` / `Either.getOrElse` は **prelude にメンバー自体がまだ無い**ので、追加時に同じ `[B >: A]` を宣言すればこの推論経路にそのまま乗る（`crates/typer/src/prelude_lowbound.rs`）
- 上限境界付き型パラメータ経由のメンバー選択（`def f[A <: Named](x: A) = x.name`）は未実装。境界の検査と `Named` 期待位置への適合までがこのスライス。erasure は nsc と違い型パラメータを常に `Object` にする
- 残りの **ArrayOps**（`lengthIs` / `sizeIs` / `indexOf` / `copyToArray` / `iterator` / `zipWithIndex` / `knownSize` / `sizeCompare` / `filterNot` / `headOption` / `lastOption` / `partition` / `splitAt` / `span` / `find` / `contains` / `distinct` / `takeRight` / `dropRight` / `takeWhile` / `indices` / `lengthCompare` / `last` / `init` / `reverse` / `size` / `isEmpty` / `nonEmpty` / `scanLeft` / `count` / `forall` / `foldLeft` / `fold` / `foldRight` / `drop` / `dropWhile` / `exists` / `take` / `collect` / `zip` / `filter` / `slice` / 3 引数 `flatMap` / 4 引数 Array→Iterable `flatMap` と primitive wrappers / `genericArrayOps` の `head`/`map`/`foreach`/`tail` に加え、**変換・集約系**（`toList` / `toSeq` / `toIndexedSeq` / `toSet` / `toVector` / `toBuffer` / `groupBy` / `sortBy` / `sorted` / `sortWith` / `sum` / `product` / `min` / `max` / `minBy` / `maxBy` / `mkString` / `reduce` / `reduceLeft` / `indexWhere` / `lastIndexOf` / `patch` / `updated` / `appended` / `prepended` / `concat` / `++`）も揃った。他メソッド（`sliding` / `grouped` / `distinctBy` / `startsWith` / `endsWith` / `padTo` / `transpose` / `unzip` / `unzip3` / `intersect` / `diff` / `combinations` / `permutations` 等）は未）
- 他の mutable（`ArrayDeque` / `LinkedHashMap` / `LinkedHashSet` / `HashMap` / `HashSet` / `ArrayBuffer` / `ListBuffer` 以外）と他の immutable（`BitSet` / `SortedMap` / `TreeMap` / `SortedSet` / `TreeSet` / `Set` / `Map` / `Vector` 以外）。`scala.collection.View` の `List.view` / `map` / `toList` と `View.fill` / `View.iterate`、**`scala.collection.MapView`**（`Map.view` / `keys` / `values` / `filterKeys` / `mapValues` / `toMap` / `toList` / `toSeq` / `size` / `isEmpty` / `foreach`）は乗った（他の View/MapView メンバーは未）。`scala.util.control.Breaks` の `breakable` / `break` / `tryBreakable`+`catchBreak` は乗った（他の control は未）。`scala.math.BigInt` / `BigDecimal` の `apply(Int)` / `apply(String)` / `+` / `*` / `int2bigInt` は乗った（他の math は未）。`scala.util.chaining` の `pipe` / `tap` は乗った。`scala.util.Using.resource` / `Using.apply` / `Using.Manager` / `Using.resources`（2–4 引数）は乗った（他の Using は未）
- **import の残り**: (a) **jar の package object にある型エイリアス**（`scala/package$` の `type NoSuchElementException = java.util.NoSuchElementException`、cats の `type Eq[A] = cats.kernel.Eq[A]` など）は classfile に出てこず pickle にしか無いため、まだ供給していない。エイリアス自体は名前としては見えるが型パラメータを持たないので、使うと `does not take type parameters` になる。(b) **同一テンプレート内で先に書かれた `val` を接頭辞にする import**（`object O { val h = new H; import h.Inner._ }`）は、import が `val` の型付けより先に走るため `<notype>` になる。別のオブジェクトに置いた `val`（`import O.h.Inner._`）は動く。(c) ワイルドカード import はパッケージのエントリを列挙せず、名前が要求されたときに 1 クラスずつ読むので、**同名の別クラスがあるときの曖昧さ検査**はしていない
- **`-Xsource:3` の残り**: 実装したのは `?` ワイルドカード / `&` 交差型 / 可変長パターン `case Cast(ch*)` / `*` ワイルドカード import / `as` リネーム import だけ。`|` 合併型 / `enum` / `given` / `using` / `extension` / トレイトのパラメータは入っていない（`given` / `using` は 2.13 の構文ではないので対象外）。`-Xsource-features:<feature>` も未実装
- **ケーキパターンのスライス**では 177 ファイルのエラーが **2,901 → 2,581**、エラーを含むファイルは **116 → 114** になった（`not found: type Table` 34 件と `not found: type Sequence` 17 件は 0 に、`no matching overload for constructor` は 42 → 26）。残る `not found: type Ref` / `Async` は cats-effect の package object エイリアスで別件
- **slick の型検査で残っているもの**: import の解決で slick 177 ファイルのエラーは **13,245 → 7,727**（`tests/slick_measure.sh`）。import 由来で残っているのは、ビルド時に `.fm` テンプレートから生成される `slick.util.TupleSupport` / `ProductWrapper` / `slick.jdbc.GetResult` の 4 件だけで、これらはソースセットに存在しないので scalac でも同じく落ちる。残りの上位は `does not take type parameters`（142）、ラムダ本体からの型推論で import とは別の領域。**名前付き引数のスライス**では 177 ファイルのエラーが **6,504 → 6,300**、`unimplemented syntax: named arguments` は **43 → 1** になった（残り 1 件は `slick/jdbc/JdbcModelBuilder.scala` の、`-cp` 上の case class に対する `m.Column(name = …, …)` で、classpath のシンボルにパラメータ名が無いため）
- **slick のパースで残っているもの**: slick 本体 176 ファイルのエラーは **23 → 11**（`-Xsource:3` 付き）。残りは `ShapedValue.scala:21` と `TableQuery.scala:36` の **def マクロ 2 箇所だけ**で、パースエラーはこの 2 ファイルを除くと **0** になる。`try e catch h` / `case Cast(ch*)` / 型位置の `super.T` はこのスライスで潰した
- **`agent/smallgaps` スライス**: 177 ファイルのエラーは **2,901 → 2,560**（`files_with_errors` は **116 → 115**）。4 項目のうち 3 つは根本原因を直した:
  - `@inline` / `@noinline` の配置検証（11 件）: 実 scalac は配置を一切検証しない（`crates/typer/src/check.rs::check_stored_annotations`）ので検証自体を削除。
  - `value length/varying is not a member of FieldSymbol`（23 件）と `value desc is not a member of Direction`（13 件）はカスケードではなく 3 つの独立した根本バグだった: (a) `qualified_type_owner`（`check.rs`）が `Foo.Bar` 型パスの `Foo` を解決するとき、同名の case class とその companion module が両方候補に挙がると宣言順で勝敗が決まっていた（companion を優先するよう修正）、(b) 複数引数リストを持つ case class（`case class F(a: A)(b: B, c: C)`）の companion `apply`/`unapply` が最初の引数リストしか見ておらず curry が崩れていた（`finish_case_apply`）、(c) `object X extends Y(args)` の module `<init>` codegen が常に無引数スーパーコンストラクタを呼んでいて `NoSuchMethodError` になっていた（`crates/backend/src/gen.rs::emit_module_init`）。
  - `value getOrElse is not a member of Any`（16 件）はカスケードで、根本原因は 2 つ: (a) `Option.flatMap` の prelude 宣言がクラス自身の型パラメータ `A` を使い回していて多相でなかった（`crates/typer/src/prelude_sgap.rs::fix_option_flat_map`）、(b) `if`/`else`（`match` も同様）の枝の `lub` が構造的な部分型判定だけで親を辿らない上に非対称だった（`SymbolTable::lub` を親チェーン探索・両側対称に拡張）。副次的に見つかった `None`（companion **module class** ではなく module 自体に `parents` を設定していた既存バグ）も修正。
  - `value apply is not a member of Iterable`（15 件）は prelude の穴で、`List` / `Seq` と同じパターン（companion `apply` が実ライブラリでは `IterableFactory$Delegate` 継承で pickle から見えない）に対処（`add_iterable_apply`、library ABI 限定・`crates/backend/src/gen.rs` に codegen を追加）。
  - カスタム文字列補間子（`value q/tq/pq is not a member of StringContext`、14 件、`ShapedValue.scala` の `mapToImpl` 1 メソッド）は当初 `implicit class` パターンと想定していたが、実際は `scala.reflect.macros`（`scala-reflect.jar`）の **quasiquote**（`q"..."` / `tq"..."`）だった。`docs/macros.md` が既にこれを「JVM ブリッジでは展開できず scala-rs 自身の組み込み実装が要る最大の残作業」と明記しており、この小さなギャップのスライスでは手を付けていない。
  - 副次的に見つけたが未修正: `case object X extends Y(...) { override def m: MoreSpecific = ... }` のように親の抽象メソッドを共変な戻り値型でオーバーライドすると `AbstractMethodError`（ブリッジメソッド未生成）。fixture 構築中に踏んだので `tests/fixtures/sgap.scala` はこのパターンを避けている。別の残課題として記録。
- **型位置 `super.T` の残り**: 親クラスの型メンバーへのパスは通るが、`trait Mid { trait Impl extends super.Impl }` のように**親と同名**の入れ子型を定義すると、ミックス先で継承メンバーの解決が親側を選んでしまうことがある（`super` の解決ではなく、同名ネスト型のメンバー継承側の穴）
- **`Unit` に具体化した多相メソッドの捨て方**: `PartialFunction[A, Unit].apply` のように JVM 上は `(Object)Object` を返すものだけ、statement 位置で `pop` する。`Breaks.catchBreak` / `Using.resource` のように emit 側で既に捨てている intrinsic とは重ならないよう、判定は意図的に狭くしてある（`unit_call_leaves_ref`）
- **`agent/overloadshadow` スライス**（別のクラスを読むと既存のオーバーロード集合が消える）: 177 ファイルのエラーは **1,707 → 1,678**（`files_with_errors` は **111** のまま）。根本原因は 3 つ重なっていた: (a) `PickleSupply::complete` がクラス側で 1 つでも供給できたらコンパニオンを見ずに返していた（`java.math.MathContext` が入っているかどうかという**無関係な大域状態**で答えが変わる）、(b) `check.rs::resolve_overload` が `Type::Overload` の候補シンボルを `fun.sym` の owner から引き直すので、クラスとコンパニオンにまたがる集合の片側が丸ごと落ちる、(c) 一度クラス側に `apply(MathContext)` が入ると以降の `BigDecimal(...)` は `lookup_member` がそれを見つけて止まり、pickle 補完まで届かない。(a) は合併に、(b) は `Check::overload_groups`（引き直しで失われる集合だけ覚える）に、(c) は `Check::widen_with_companion`（**エラーを出す直前だけ**、term 位置のクラス名の選択をコンパニオンのメンバで広げて 1 度だけ解決し直す）で直した。併せて `scala.math.BigDecimal.apply(java.math.BigDecimal)`（JDBC の結果を Scala 値にするのに使う）を prelude に固定した（`crates/typer/src/prelude_oshadow.rs`。`library_abi` のみ）。残件: slick の `value getOrElse is not a member of Product`（16 件）は BigDecimal とは無関係で、`if (c) None else Some(x)` の `lub` が `Option[X]` にならず `Product` に落ちる別のバグ（`Boolean` / `Blob` / `Byte` … でも同じように出る）。`BigDecimal.apply` を eta 展開して `(Double) => BigDecimal` に渡す `new ScalaNumericType[BigDecimal](BigDecimal.apply)` は、オーバーロードの eta 展開を期待型で選べないため未対応
- **`@specialized` codegen** はこのスライスでは開始しない
- **オーバーロード / メソッド適用のスライスで残っているもの**: slick 177 ファイルのエラーは **2,901 → 2,539**（`tests/slick_measure.sh`。エラーを含むファイルは 116 → 115）。`no matching overload for (Type, Any, Boolean)LiteralNode` / `(#N*)(TypedType[T])Rep[T]` / `not found: extractor ==` / `type arg is not a member of OptionMapperDSL$.arg[B1, P1]` は消えた。残る上位は implicit 探索（`could not find implicit value of type TypedType[BR]` など）と、`.fm` テンプレート由来で存在しない型（`Table` / `Sequence` / `Ref`）のカスケード。`no matching overload for (String)String` は最小再現では通るので、別の穴のカスケード
- 高階 `F[_] <% …` は nsc どおり `takes type parameters`（`F[_]: C` は nsc が受理するので実装済み。README の旧記述は誤りだったので実測に合わせて直した）
- placeholder の残り（より深い入れ子の完全再現。unary / Function2 / typed `_ : T` の必要形はこのスライスまで）
- **implicit の導出**（`implicit def optShow[A](implicit s: Show[A]): Show[Option[A]]` のように、implicit パラメータを取る implicit def を型パラメータの単一化つきで再帰的に解決する形）。`implicit_provides` は今のところパラメータリストが空の implicit しか候補にしないので、`Show[Option[Int]]` は `no implicit` になる
- **キャプチャしたクラスの JVM 名**（メソッドの中のクラスは nsc の `Outer$Inner$1` ではなく素の `Inner` / `$anon$N` として出る。既存の匿名クラスと同じ扱いで、同名のローカルクラスが 2 つあると衝突する）
- **`while` 本体で宣言したローカルの StackMapTable**（`while (c) { val s = …; … }` はループ先頭のフレームがそのスロットを含んでしまい `VerifyError: Instruction type does not match stack map` になる。匿名クラスとは無関係で、ループの外で `val` を束ねれば動く）
- **`scala.Product` 本体**（case class / case object は `productPrefix` / `productArity` は持つが、`Product` を親に付けていないので `productElement` / `productIterator` / `productElementNames` は無く、`(x: Product)` にも渡せない）
- **コンストラクタの省略可能引数のうち、先行する ctor パラメータを参照するデフォルト**（`class C(x: Int, y: Int = x + 1)`）。単純なリテラル / `null` のデフォルト（`class C(x: Int, y: Int = 5)` や slick の `SlickException(msg, parent: Throwable = null)`）は動く
- **名前付き引数の残り**: (a) **prelude / classpath のメソッドはパラメータ名を持たない**ので、`List(1,2,3).mkString(sep = "-")` や jar・`-cp` 上の case class への `copy(name = …)` は `unimplemented syntax: named arguments (method parameters not resolved)` になる（scala-library の pickle からパラメータ名を読む経路も、prelude 手書きシグネチャの名前付けも未実装。同一コンパイル単位のメソッド・クラスなら全部動く）。(b) **複数引数リストのコンストラクタ** `class C(a: Int)(b: Int)` は名前付き引数以前に `new C(1)(2)` 自体が未対応（`value apply is not a member`）。(c) 名前と型が同一で順序だけ違うオーバーロード（`h(s: String, n: Int)` と `h(n: Int, s: String)`）は nsc なら `ambiguous reference to overloaded definition` だが、こちらは先に宣言された方を黙って選ぶ
- **`--no-scala-library` での `x == null`（reference 型）**（`scala.runtime.BoxesRunTime.equals` を経由しないため、`x` が実際に `null` だと `Object.equals` の invokevirtual で `NullPointerException`。`--scala-library` 時は正しく動く）
- **lazy completer のスコープ**: namer だけが見た定義（別テンプレートからの前方参照）は、所有者チェーンのメンバーから組み直したスコープで完成させる。ファイル先頭の `import` は typer が処理するまで入らないので、import 名を右辺に使う定義を前方参照した場合は型が付かず `<notype>` のまま（診断はそのまま出る。黙って通すことはしない）
- **trait のメンバークラスから外側インスタンスを読む codegen**（`trait T { def x = 1; class Inner { def y = x } }`）。
  `enclosing_instance` は trait のメンバークラスに `$outer` を渡さないので、`x` は `this` を `T` へ
  checkcast する形になり、実行時に `ClassCastException` になる。自己型エイリアス（`self`）を内部クラス
  から読む形も同じ経路。型検査は通る（scalac と一致する）が、nsc のように interface 型の `$outer` を
  コンストラクタで受け渡す実装が要る。trait の**自分のメソッド**の中で `self` を使う形は正しく動く
- **jar の package object にある型エイリアス**（cats-effect の `cats.effect.Ref` / `Async` など）。
  `import cats.effect.{Async, Ref, Resource}` は package object の pickle にしか無いので解決できず、
  slick の `slick/basic/BasicBackend.scala` に `not found: type Ref` が残っている。
  `import cats.effect.kernel.Ref` のように実クラスを直接指せば通る（「import の残り」(a) と同じ穴）
- **trait の `val` / `lazy val` を継承先から読む codegen**（`IncompatibleClassChangeError: Found interface T, but class was expected`。lazysig 以前からある別件。fixture は trait の `def` を使って回避している）

## ライセンス

Apache-2.0
