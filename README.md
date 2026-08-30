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
- `-cp` / `--class-path` — 先にコンパイルした classfile を読む（`ScalaSignature` pickle subset と JVM メソッド。vals / パラメータ付き defs / 型パラメータ / `$default$n` ゲッター / case class の ctor フィールドを含む。自前 `-cp` は companion `apply` も読む。nsc は companion apply `Point(...)` / term `Point` / extractor `unapply` / `List[_]` の existentials / `List[_ <: AnyRef]` / `List[_ <: List[_]]` / `@deprecated("msg", "2.13.0")` の annotation args / Java `@Deprecated`（SYMANNOT + `java.lang.Deprecated`） / `this.type` / `Int @unchecked` / refinement `A with B { def f: Int }` も読む）。**jar の中の Scala クラス**は `ScalaSignature` pickle をそのまま読みます（`crates/pickle`。高階型パラメータ `F[_]` と `F[A]` を含む。読めなかったメンバだけ JVM signature に落ちる。`scala.*` / `java.*` は対象外。先読みはせず 1 クラスずつ。「jar のクラスを pickle から読む」節）。**Java の `.class`** も同じ `-cp` / jar / jmod / JDK（`java.base.jmod` や `rt.jar`）からオンデマンドで読む（ScalaSignature の無い pickle-less Java は pickle インストーラに載せない。`JAVA` / `protected` / `static` を落とさないため）。prelude に無い JDK クラスのメソッド・フィールド（`java.lang.Math.abs` / `java.util.ArrayList#add`）を解決する。**Signature 属性**があればジェネリックを raw にしない（`ArrayList[String]#get` は `E`＝`String`。無ければ `Object` のまま `String` へは通さない）。**ワイルドカード／型パラメータ境界**（`Class[*]` → `Class[_]`、`Collection<+TT>` → `Collection[_ <: T]`、`<T:Number>` の hi bound）は存在型として残し raw `Object` にしない。`ArrayList[Byte] <: List[_ <: T]` は親ウォークより先にワイルドカードを照合し、継承した `add` は `drop_overridden` する。**静的 inner**（`java.util.Map.Entry` / `AbstractMap.SimpleEntry`）と **Java varargs**（`ACC_VARARGS` の `String.format` / `Arrays.asList`。Scala `Seq` wrap ではなく `Object[]`）も classfile から読む。Java の `throws` 検査例外は Scala と同様チェックしない。**Java `protected`** は同じパッケージかサブクラス（nsc / JLS）から見え、それ以外は診断する。Scala の `Base.secretStatic()` は Java クラスの `MODULE$` を出さず `invokestatic` する。ScalaSignature pickle だけに頼らない。**Java enum**（`ACC_ENUM` のクラスと定数。`values` / `valueOf` は classfile の static。非 enum に `values` を合成しない）。未対応の classfile 機能（未知 CP tag、`ACC_MODULE`、壊れた magic）は診断する（黙って成功にしない）

フィクスチャはデフォルトパッケージ（`package` 句なし）なので、`-cp out` の `Main` でそのまま動く想定です。

## 実装している言語サブセット

Scala **2.13** 構文です。Scala 3 の `then`、トップレベル定義、TASTy はありません。エントリポイントは `def main(args: Array[String]): Unit` です。

パースできる（またはデシュガーする）構文:

- packages / imports
- objects / classes / traits / case classes。**補助コンストラクタ** `def this(...) = this(...)`（連鎖の先頭は `this(...)`。`super(...)` や文のあとの `this` は診断）。サブクラスの `extends C(1)` は primary が親 ctor を呼ぶ。内部クラスの `new Inner` は ctor overload 選択後も `$outer` を `<init>` の第一引数に残す。**case class の `copy(...)`**（positional / 一部省略時は自分自身の対応フィールドを default / 名前付き引数。`copy` は namer 時点ではまだ ctor フィールドの型が確定していないため、フィールド型解決後の typer フェーズで `copy` 自身の引数シンボルと `copy$default$N` を作り直す。private ランタイムでも動く）。**コンストラクタの省略可能引数**（`class C(x: Int, y: Int = 5)` の `new C(1)` / `new C(y = 2, x = 1)`）: 末尾を省略した呼び出しへのデフォルト値の充填は、通常の `def` の default getter 経由ではなく（`this` が無い呼び出し元では使えないため）呼び出し側でその場を型付けする素朴なフォールバックのみ実装（先行引数を参照するデフォルトは非対応）。**名前付き引数での並べ替えは `new C(...)` でも動く**（コンストラクタのオーバーロードはパラメータ名で絞ってから型で決める）
- `val` / `var` / `def`（ネストした `def` はパースする）
- パラメータ、ラムダ（型付き / 期待型から推論）、ブロック。**placeholder `_`**（nsc `withPlaceholders`）: `_ + 1` / `_.abs` / `f(_)` / `xs.map(_ + 1)` / Function2 `_ + _` / 入れ子 `_.map(_ + 1)` に加え **typed `_ : T`**（`(_: Int) + 1` / `(_: Int) + (_: Int)` / `(_: Int).abs` / `xs.map((_: Int) + 1)`）。レキサが `_:` を `Ident("_")` にするので、式位置では Underscore と同じ placeholder にする。bare `(_: Int)` は `unbound placeholder parameter`。`xs.map(_ : Int)` は nsc どおり wrap せず map に Int が渡り mismatch。unary / Function2 の既存 wrap は触らない。**メソッド適用のセクション** `f(_, x)` / `f(_, _)` は期待型が無くても呼び先のシグネチャからパラメータ型を取る（nsc と同じ条件で、呼び先が単一の非ジェネリックメソッドのときだけ。`poly(_, 3)` や overload された `"abc".substring(_)` は `missing parameter type for expanded function` のまま）。合成パラメータはソース順で並べる（`two(_, _)` は `(a, b) => two(a, b)`）。**リテラルの本体は期待型の結果に対して検査する** ── `xs.foreach((x: Int) => x + 1)` は value discarding、`fl((x: Int) => x)` は `Int => Long` への数値拡大。パラメータ型を書いたリテラルはオーバーロード解決のために期待型より先に型付けられるので、そのぶんは `adapt` 側でやる。関数**値**は対象外で、`val h: Int => Int = …; fu(h)` は nsc どおり `type mismatch`
- `if` / `else`、`while`、`do { ... } while (cond)`
- `try` / `catch` / `finally`（catch は `{ case ... }`。`try/finally` と `try/catch/finally`。finally は正常終了と例外（catch からの throw 含む）の両方で走る。JVM 例外テーブルを出す。パーサは `finally` を落とさない）
- `match`（コンストラクタパターン、リテラル、ワイルドカード、Java enum 定数の安定識別子 `Thread.State.NEW`、`x @ Pat` の束縛、`case null`）
- for-comprehension（`map` / `flatMap` / `foreach` / `withFilter` へデシュガー。私有ランタイムでは `List.withFilter` は eager な `List`。`--scala-library` 時は `scala.collection.WithFilter[+A, +CC[_]]` で、`map[B]` は `CC[B]` を返す。`Option.withFilter` は `Option$WithFilter`）。値定義 `q = e` はラムダ本体の `val` になる ── **生成子ではない**ので、その前の生成子はやはり最内で `map` を取る。値定義の**後ろのガード**は nsc のタプル化が要るので診断する
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
- compiled class/object に **ScalaSignature**（クラス属性 `ScalaSig` マーカー + `RuntimeVisibleAnnotations` の pickle subset）。`javap -v` で見える。自前 unpickler が読める範囲で `-cp` による別コンパイルができる。nsc 完全 pickle ではないが、ワイヤ形式は nsc と同じ（nentries、tag/len、ビッグエンディアン Nat、SID-10 は `0x7f→0`）。`val` / パラメータ付き `def` / 型パラメータ `id[T]` / `case class` の `new` と ctor フィールド / **companion apply `Point(3, 4)`（term `Point` / `MODULE$`）** / **extractor `unapply`（`p match { case Point(a, b) => … }`）** / object の `def` / **`List[_]`（EXISTENTIALtpe）** / **`List[_ <: AnyRef]`（量化 TYPEsym の hi bound）** / **`@deprecated("msg", "2.13.0")`（SYMANNOT + LITERALstring）** / **Java `@Deprecated`（SYMANNOT + TypeRef(java.lang, Deprecated)。scalac `-deprecation` がメソッド上のアノテーションを見る）** / **`this.type`（THIStpe をメソッド結果に）** / **`Int @unchecked`（ANNOTATEDtpe）** / **`val one: 1` と `def lit(x: 1)`（CONSTANTtpe + LITERALint）** / **`List[_ <: List[_]]`（入れ子 EXISTENTIALtpe）** / **`A with B { def f: Int }`（REFINEDtpe）** / **`@Ann(foo)` / `@Ann(c.x)` / `@Ann(3)` / `@Ann(this)` / `@Ann(classOf[Int])` / `@Ann(ident(1))` / `@Ann(this.x)` / `@Ann(super.foo)` / `@Ann(ident(ident(1)))` / `@Ann(foo = 1)`（TREE Ident/Select/This/Super/Apply + リテラル / LITERALclass Constant。ネストした Apply と Ident 以外の Select 修飾子を含む。named `@Ann(foo = 1)` は nsc と同じ位置 Constant）** / **`def join(xs: String*)`（VARARGS + `<repeated>`）** / **`Ordered` erasure bridge（BRIDGE）** / **`type T = Int`（ALIASsym。2.13 に ALIAStpe は無い）** は scalac 2.13.16 が読める形（object は CLASSsym+MODULE + MODULESYM、クラス pickle にも companion の MODULESYM を載せる、パッケージ（`hklib` / `slick/ast`）と scala / java.lang の EXTMODCLASSref、デフォルトパッケージだけ `<empty>`、POLYtpe は restpe 先行、val は NullaryMethodType ゲッター、case class は CASE / CASEACCESSOR、ユーザー型は**自分のパッケージ**所有の EXTREF、`Option` / `TupleN` / `FunctionN` / `List` は scala / `scala.collection.immutable` モジュール所有の TypeRef + 型引数、Flags は nsc raw long を `rawToPickledFlags` して出す）。full pickle とは主張しない。残る穴は README Remaining
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
- **コンストラクタ引数のアクセサ**。`class C(val x: Int)` も、キーワード無しで `val` になる **`case class` の第 1 引数リスト**も public なアクセサ `x()` になり、親の抽象メンバーを実装する（親が `def value: T` を `()Object` に erase する場合はブリッジも出す）。第 2 引数リスト以降は nsc と同じく private な状態のまま。`var` 引数は `x()` と `x_$eq(v)` の両方
- **`FunctionN.tupled` / `curried`（arity 2〜22）と `scala.Function.untupled`（2〜5）**。`scala/FunctionN` の default メソッドと `scala/Function$` なので **jar リンク時のみ**（`--no-scala-library` では診断する）。あわせて、引数リストを持たないメソッドの結果が関数ならその引数リストは関数のもの（`def g: Int => Int; g(3)`）、カリー化された**関数値**の `f(1)(2)` は 2 回の `Function1.apply`（メソッドのカリー化とは違って平坦化しない）
- **`scala.collection.mutable.Builder` の `+=` / `++=`**（`Growable` の default メソッド。`this.type` を返すので受け手の型がそのまま返る）。jar リンク時のみ
- `super` / 修飾付き `this`（`Outer.this`）。trait の `super` は、具象クラスなら `T$class`、スタック可能な `abstract override` なら `T$$super$m` 経由
- `sealed` 階層の match 網羅検査（不足は **warning**。`-Xfatal-warnings` でエラー）
- extractor の `unapply`（`Option` / `Boolean` / `Tuple2`）と `unapplySeq`（`List` / `Seq` / `Vector` / `IndexedSeq` / `Array` と可変長 `_*`）。名前付き extractor 引数（`Point(y = b, x = a)`）
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
- **quasiquote（`q"..."`）の reification**: `q"..."` / `tq"..."` / `pq"..."` / `cq"..."` は
  `StringContext` の普通の補間子ではなく、nsc の**コンパイラ内蔵マクロ**である。
  補間文字列の中身を（`$x` / `${…}` / `..$xs` / `...$xss` をプレースホルダに置き換えて）
  **scala-rs のパーサで実際に構文解析し**、`q"..."` については
  `<universe>.internal.reificationSupport.Syntactic*` の呼び出しに脱糖して、
  普通の式として型検査・コード生成する（`crates/typer/src/reify.rs`）。落とせるのは
  リテラル / 名前 / 選択 / 適用（カリー化含む）/ `$x` 穴 / 引数リスト 1 節ぶんの `..$xs`。
  universe は `import <universe>._` から採る。**落とせない形は必ず
  `unimplemented syntax: quasiquote q"..." (どの形か)` で診断する**（黙って通さない）。
  残りの形と `tq` / `pq` / `cq` は未実装で、同じ診断が出る
  （[`docs/macros.md`](docs/macros.md) §7.4 / §7.5）
- **`-cp` から読んだクラスとトレイト**: `-cp` の classfile から読んだ Scala の
  トレイトは**インタフェース**として扱い（`ACC_INTERFACE` を読む）、
  **親はヘッダの `super_class` / `interfaces`** から付ける。以前は前者が無くて
  実行時 `IncompatibleClassChangeError`、後者が無くて継承メンバが `is not a member`
  になっていた。さらに、pickle から補完したメンバの JVM 宣言が
  **バイトコードの経路では届かない**クラスにあるとき（`scala.reflect.api.JavaUniverse`
  は `interfaces: 0` のインタフェースで、`Constant()` を宣言するのは
  `scala.reflect.api.Constants`）、`Symbol::declaring_class` にその内部名を記録し、
  codegen はそのクラスを invoke のオーナーに使ってレシーバをそこへ `checkcast` する
  （nsc と同形）。これで `scala.reflect.runtime.universe` 上の Tree 構築が実際に走る
- **package object のメンバ**: jar の `scala.math.Pi` のような package object の
  `val` / `def`。typer はこれをパッケージシンボルに畳み込むが、パッケージには実行時の値が
  無いので、codegen は `<pkg>/package$.MODULE$` をレシーバに積む
- **引数なし `def` の結果に対する `apply` 挿入**: `def mk: Box` に対する `mk("a")` は
  `mk.apply("a")`。reflect API の抽出子（`def Literal: LiteralExtractor` → `Literal(x)`）が
  この形
- **`import <値>._`**: プレフィクスが object でも package でもなく**値**のとき、
  その値の*型*のメンバを入れ、無修飾の参照を `値.メンバ` に書き戻す
  （`import c.universe._` の形）

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

#### 未確定の型変数（nsc の undetermined type variables）

引数はオーバーロード解決を型で駆動するために**期待型なしで**型付けします。その結果、
`Map.empty` のような多相参照は自分の型パラメータを抱えたまま（`Map[K, V]`）引数位置に
届きます。nsc はこれを **TypeVar**（`Context.undetparams`）として持ち回り、候補を選び終えて
から一度に解きます。scala-rs も同じようにします（`check.rs` の `undet_tvars` /
`undetermined_of` / `undet_compatible` / `instantiate_undet_arg`）。

- 適用可能性の判定（`arg_score`）は、引数が抱えている変数をパラメータ型と単一化して
  解いてから比較します。`take(m: Map[String, Int])` に `Map.empty` を渡せます。
  空の `apply`（`Map()` / `Vector()` / `List()`）も同じ経路です。
- 内側の呼び出しが解けなかった変数は、外側の呼び出しにとってまだ未確定なので
  外へ持ち出します。`take(id(Map.empty))` は外側のパラメータ型が `K` / `V` を決めます。
- 結果型まで届いた変数は**期待型**が決めます（`solve_undet_result`）。
  `val l: List[Map[String, Int]] = f(Map.empty)`（`def f[T](x: T): List[T]`）の
  `K` / `V` は宣言した型から解けます。可変長引数・by-name・デフォルト引数の位置も
  同じ経路です。
- **囲んでいる定義が束縛している型パラメータは変数ではなく確定した型**です。
  スコープにその名前で引けるかどうかで区別します（`tparam_in_scope`）。
  `def g[K](m: Map[K, Int]) = take(m)` や `def rec[T](x: T, m: Map[T, Int]) = take(m)` は
  scalac と同じく拒否します。
- 変数の上下界は無視しません。単一化した解が界に適合しない候補は選びません。
- 解けない変数は `Nothing` で黙って埋めず、診断を出します。

逆向き、つまり**呼び先自身の**型パラメータがまだ未確定な場合も同じ考えです。
`xs.collect { case … }` は `PartialFunction[Int, ?B]` に対して検査され、`?B` はリテラルの
結果型が決めます。以前はここでパラメータ型を `Any` に潰していました
（`relax_open_tparams`）が、結果型を失う場に持ち込むと壊れる場当たりだったので
**削除**し、引数から解いた解に対して適合を見るようにしました（`solve_open_from_arg`）。
引数を型付けする時の期待型としては、未確定の変数をその**宣言された上界**まで開きます
（`open_to_bounds`。上界が無ければ `Any`）。
スコープにあるクラスの型パラメータは確定した型なので開きません。
`def take[T](r: Rep[T])` を `trait Base[P1]` の中で `take(c)`（`c: Rep[P1]`）と
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

`implicit object X` は候補としては**一つ**です。module symbol と module class の両方が
`IMPLICIT` を持ち、型も同じなので、そのままだと自分自身と `ambiguous implicit` になっていました
（slick の `implicit object GetString extends GetResult[String]`）。module class は落とします。

#### 一度埋めた implicit 引数を持つ呼び出しの再型付け

typer は同じ application を二度型付けすることがあります（nsc のタプル適応にあたる
`retry_tupled_args` は、引数を一つのタプルに詰め直してから呼び出しを型付けし直す）。
一度目のパスが埋めた implicit 引数は argument list に残っているので、二度目のパスは
それを「ユーザーが書いた引数」として数えてしまい、`LiteralNode(1)` が
`not found: value intType`（companion の implicit をレキシカルスコープで引き直そうとした）や
`no matching overload …(1, ScalaNumericType[Int])` になっていました。
typer が自分で足した引数には `NodeId::FILLED_ARG` を付け、再解決の前に落とします。

#### 引数位置の残余 implicit 節

`take(a: Array[String])` に `Array.empty` を渡すと、引数は期待型なしで型付けされるので
`(ClassTag[T])Array[T]` というメソッド型のまま届きます。オーバーロード解決にはその**結果型**
`Array[T]` を見せ（`T` は nsc の未決定型変数として扱う）、パラメータ型が決まってから
implicit 節を埋めます。埋める witness は**パラメータ型が要求するもの**で、スコープにある
唯一の implicit ではありません（`take(empty)` で `Tag[Int]` しか無ければ
`could not find implicit value of type Tag[String]`）。

#### implicit 探索だけが決められる型パラメータ

`def mk[T: TT](s: String): Seq[Int] => Rep[T]` の `T` は値引数のどこにも現れないので、
witness を見つける探索そのものが決めるしかありません（slick の `SimpleFunction.nullary`）。
第二節が全部 implicit の呼び出しでは、値引数から解けなかった型パラメータを implicit
パラメータ型から解き、結果型にも反映します。

#### prelude の穴

- `scala.math.Numeric[T]` は `scala.math.Ordering[T]` を継承します（実 ABI の
  `interface scala.math.Numeric<T> extends scala.math.Ordering<T>`）。prelude は
  `sum` / `product` 用に `Numeric` を合成するだけでこの親を張っておらず、
  `Numeric[T]` を `Ordering[T]` の位置に渡せませんでした（slick の
  `ScalaNumericType[T] extends ScalaBaseType[T]()(tag, numeric)`）。
  `crates/typer/src/prelude_numhier.rs`。
- 関数値の `apply` は関数そのものです。prelude の `FunctionN.apply` は消去された
  パラメータで宣言されているので、`f.apply(xs)` は `Any` になっていました（`f(xs)` は正しい）。
- 可変長引数を持つ `case class` の `copy$default$n`（`this.cells`）は `T*` ではなく
  `Seq[T]` として型付けします。nsc はこの形に `copy` を作らないので、`T*` に対して
  検査すると誰も書いていないツリーに対する診断が出ていました。

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

`try` 本体が必ず投げる（`Nothing`）ときの型は、nsc どおりハンドラ側との lub です。`val n = try throw e catch toLen` は `Nothing` ではなく `Int` になります。ハンドラが本体の型に**適合しない**ときも lub です — `try Success(f) catch { case NonFatal(e) => Failure(e) }` は `Success` ではなく `Try` です。ただし全枝が参照型のときだけで、`Int` と `Unit` が混ざる形は本体の型のままにします（結果は 1 つのローカルに置くので、sort が混ざると箱詰めが要ります）。

結果を置くそのローカルには**宣言型**を持たせます（`Assembler::set_local_class`）。枝が `Success` と `Failure` を入れると、クラス階層を持たないアセンブラの合流は `java/lang/Object` になり、続く `areturn` が検証に落ちるためです。参照のスロットにプリミティブが来る枝は箱詰めします（`box_for_result_slot`。既に箱詰め済みかは木の型では分からないので、アセンブラのスタックの実際の型を見ます）。`match` / `if` の合流も同じで、こちらはスタック最上段に対して `Assembler::set_join_class` を使います。宣言が無い合流は `java/lang/Object` です。

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

`unapplySeq` は `List` / `Seq` / `Vector` / `IndexedSeq` / `Array` のコンパニオンと、ユーザー定義の可変長 extractor です。`List(a, b, c)`、`List(h, rest @ _*)`、`Seq(a, b)`、`Vector(a, rest @ _*)`、`Array(a, b)`、`PairSeq(a, b)` が動きます。名前付き引数は case class のコンストラクタパターンで並べ替えます（`Point(y = b, x = a)`）。

`List` だけは cons リストを head / tail で辿ります。それ以外は実 scalac と同じく
添字で読みます（`scala.collection.SeqFactory$UnapplySeqWrapper$` の
`lengthCompare$extension` / `apply$extension` / `drop$extension`。`Array` は
`scala.Array$UnapplySeqWrapper$` の同名 extension）。`Vector` を `Seq` として
渡しても落ちないのはこのためです。`rest @ _*` に付く型は extractor 自身の
結果型のコンテナで、`List` パターンなら `List[A]`、`Seq` / `Array` パターン
なら `Seq[A]`（`drop$extension` の戻り型）です。

スクルーティニの静的型がシーケンスだと保証していないとき（`x: Any` など）は、
scalac と同じく先に型テストを出します（`instanceof`、`Array` は
`ScalaRunTime.isArray(Object, 1)`）。部分パターンの `_: T` は**テスト**なので
`instanceof` で落とし、`checkcast` はしません（`case List((s, _: TableNode))`
がマッチしない値で例外になっていました）。

`SeqFactory$UnapplySeqWrapper$` は私有ランタイム（`--no-scala-library`）には
無いので、jar 無しの `case Seq(…)` / `case Array(…)` は**診断を出します**
（黙って要素型 `Any` のコードを出しません）。`List` パターンは両モードで動きます。

### `x @ Pat` の束縛と `null`

`case n @ N(v, _)` の `n` は**パターン自身の型**（`N`）で束縛します。スクルーティニの型
（`T`）のまま格納していたので、`n.copy(...)` が親の型の値から `N` のフィールドを読みに行き
`VerifyError: Bad type on operand stack` になっていました。型パターンの綴り（`case n: N`）は
動いていたので、`@` だけが壊れていた形です。nsc と同じ順で出します ──
`instanceof` → `checkcast` → `astore`。テストが落ちたら束縛もしません。
`case i @ (_: Int)` のようにプリミティブへ絞る `@` は、参照を int スロットに入れる前に
unbox します。

`null` はどのパターン種別に対しても nsc と同じ扱いです（SLS 8.1.1 / 8.1.2）。

| パターン | 出すコード | `null` は |
| --- | --- | --- |
| `case null` | `ifnonnull`（**参照比較**） | マッチ |
| `case "a"` / `case 1` / `case 1L` | 定数を**左**に置いて比較 | 不一致 |
| `case Nil`（安定識別子） | 同上（`Nil$.MODULE$.equals(x)`） | 不一致 |
| `case s: String` / `case x: Any` | `instanceof`（`Any` / `AnyRef` / 型パラメータでも出す） | 不一致 |
| `case N(v, _)`（case class） | `instanceof` | 不一致 |
| `case Ex(n)`（extractor） | `ifnull` で先に落とす | 不一致 |
| `case Seq(a, b)` | `instanceof` | 不一致 |
| `case _` | テストなし | マッチ |

`case null` を `x.equals(null)` として出していたので、その case が捕まえるはずの唯一の値で
`NullPointerException` になっていました。定数パターンを `x.equals(定数)` の向きで出していたのも
同じ理由で直しています（`case "a"` が `null` スクルーティニで落ちていました）。

定数パターンの比較そのものも直しました。`Long` / `Float` / `Double` のスクルーティニは両オペランドを
`pop` して**無条件にマッチ**していた（`case 1L =>` がすべての `Long` に当たる）ので、
nsc と同じ `lcmp` / `fcmpl` / `dcmpl` + `ifne` にしています。参照スクルーティニに対する
プリミティブ定数は box してから比較します（jar モードは nsc と同じ
`BoxesRunTime.equals`、私有ランタイムは `Object.equals`）。

`Null` はどの値型にも適合しないので、`(x: Int) match { case null => … }` は nsc と同じく
**エラー**です（黙って通らない case を出しません）。

```
type mismatch; found: Null(null)  required: Int
```

`case a: Array[Int]` は配列ディスクリプタで `instanceof` します。`type_jvm_name` が配列に
`Object` を返していたため何もテストせず、`a.length` が `Object` に対する `arraylength` に
なっていました。

同じ話が `==` 演算子にもあります。`x == null` / `null == x`（と `!=`）は nsc と同じく
**参照テスト 1 命令**（`ifnonnull` / `ifnull`）で、`equals` は呼びません。`null` 側は
評価しません（リテラルなので副作用がありません）。値クラスとプリミティブは `null` に
なり得ないので、この近道は取らず従来どおり box して比較します。
一般の `x == y` は jar モードでは nsc と同じ `BoxesRunTime.equals`（null 安全）ですが、
私有ランタイムには `BoxesRunTime` が無く、素の `recv.equals(arg)` は receiver が `null` の
ときに落ちていました。ここは nsc 自身の展開に合わせています。

```
if (recv == null) arg == null else recv.equals(arg)
```

両辺を先にローカルへ落としてから分岐するので、どの分岐先でもオペランドスタックは空です。

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

### slick が生成する 7 本（`.fm` テンプレート）が通るまで

slick は `TupleSupport` / `TupleShapeImplicits` / `SetParameter` / `GetResult` などをビルド時に
FreeMarker テンプレートから生成します。これを計測に含めた（177 → 184 ファイル）ところ、
今まで見えていなかった穴がまとめて出てきました。最小再現は `tests/fixtures/genrep.scala`
（異常系は `genrep_bound_bad` / `genrep_tuple_bad` / `genrep_product_bad`）にあり、
`crates/cli/tests/genrep.rs` が **scalac と scala-rs の両方でコンパイルして `Main` の出力を
比較**します。

**1. クラス型パラメータの境界が import を見ていなかった**。`import slick.lifted._` の下で
`class Boxed[T <: Rep[_]]` と書くと `not found: type Rep` になっていました。原因は
**namer が境界を解決していた**ことです。namer は import 句を処理する前に走るので、
その時点で imported な名前は引けません。`def` の型パラメータは `type_def_sig` が、型メンバーは
`type_type_member` が改めて `enter_tparams` を呼ぶので通っていましたが、クラスだけは
一度も解決し直していませんでした。namer 側は**暫定として黙って**解決し（`enter_tparams_provisional`）、
`type_class` が import の見えるスコープで `resolve_tparam_bounds` を呼び直します。本当に
存在しない境界は、そこで（1 回だけ）診断されます。

**2. `implicit class` の合成変換が型パラメータを落としていた**。
`implicit class RepOps[T <: Rep[_]](c: T)` は nsc では
`implicit def RepOps[T <: Rep[_]](c: T): RepOps[T] = new RepOps[T](c)` にデシュガーされます。
以前は型パラメータを付けずに合成していたので、結果型が裸の `RepOps` になり
`RepOps takes type parameters` で落ちていました。クラスの型パラメータ木を**新しいシンボルで
複製**して（`copy_tparams`）合成 `def` に付け、`new C[T1, …](x)` を作るようにしました。

**3. `TupleN` が `Product` でも `Serializable` でもなかった**。prelude は `Tuple2` を `prelude.rs`
で、`Tuple3`…`Tuple22` を `prelude_tuple.rs` で作りますが、親は `AnyRef` だけでした。そのため
`def buildTuple(…): Product = … new Tuple4(a, b, c, d)`（生成された `TupleSupport`）も、
ただの `val p: Product = (1, 2)` も通りませんでした。`scala.Product` と `java.io.Serializable`
は jar 側（オンデマンドロード）なので、classpath を入れた直後に 2 本だけ先読みしてから
辺を張ります（`prelude_genrep.rs`）。**jar が無ければ何もしません**: 私有ランタイムの
`scala/Tuple2` はどちらも実装していないので、親を名乗ったら嘘になります。
`--no-scala-library` では今までどおり診断します（`genrep_product_bad`）。

**4. 継承したオーバーロードが受け手の型引数を失っていた**。`scala.collection.Seq[A]` の `apply` は
`SeqOps.apply(Int): A` と、`PartialFunction[Int, A]` から継承した `apply` の 2 本です。
`Type::Overload` は型だけを運び、`resolve_overload` は選んだ候補のシンボルを知るために
**宣言をシンボルから読み直して**いたため、2 本目が `apply(A): B` という素の宣言に戻り、
どちらがより特化しているとも言えず `ambiguous overload for apply` になっていました。
選択時に計算済みの「受け手での型」を `overload_member_types` に控えて、読み直しのときに
そちらを使います（`s(0)` が通るようになります）。

**5. 引数リストのタプル化（nsc の tuple adaptation）**。`Some((p._1, p._2), p._3)` は
`Some(((p._1, p._2), p._3))` の意味です。どの候補にも合わない引数リストを、最後の手段として
1 個のタプルに詰め直して**もう一度だけ**型付けします。だめならツリーも診断も元に戻すので、
エラーは書いたとおりのものが出ます。nsc と同じく**オーバーロードされた呼び先には適用しません**
（`inferMethodAlternative` はタプル化しない）。合成した `TupleN(a, b)` 自身が再入しないよう
再入フラグで止めています。

**6. 名前が `Tuple` で始まるだけのクラスをタプル扱いしていた**。`TupleShape[L, M, U, P]`
（slick 自身のクラス）が **4 要素タプル**として読まれ、`TupleShapeImplicits` が全滅していました。
`starts_with("Tuple")` / `starts_with("Function")` を、**`TupleN` / `FunctionN` の N が実際の
引数個数と一致するか**の判定に置き換えました（typer の型解決と backend の pickle の両方）。

**7. 可変長引数のコンストラクタ**。`class C(xs: T*)` に `new C(a, b)` と渡すと
`type mismatch; found: a  required: T*` になっていました。メソッド側は `param_at` で
repeated を展開していましたが、コンストラクタ側は引数を**位置で**引いていたためです。
slick の `new SetTupleParameter[(T1, T2)](c1, c2)` がこれです。codegen 側も直しました:
repeated パラメータは JVM では `Seq` 引数 1 本（`<init>` のディスクリプタは元から
`Lscala/collection/immutable/Seq;`）なので、要素を生のまま積むと `VerifyError` になります。
メソッド呼び出しと同じ `gen_call_args` を通して包みます。

**8. ワイルドカード型引数と反変**。`SetParameter[-T]` に対して `SetParameter[T1]` は
`SetParameter[_]` に適合します。ワイルドカードは「何かの型」を表すので、パラメータの
変位に関わらず相手を**含み**ます。反変のときだけ `_ <: T1` を見に行って落としていました。

**9. `package p { … }` の後ろのトップレベル定義**。`package genrep { … }` に続けて
`object Main` を書くと、`Main` が `genrep` パッケージに入って `genrep/Main.class` が
出ていました。閉じ括弧の後は**兄弟**であってパッケージのメンバーではありません。

計測（`tests/slick_measure.sh`、slick 184 ファイル、`-Xsource:3`）では **2064 → 1300**、
生成 7 本のエラーは **736 → 41** になりました（`TupleSupport` 569 → 2、
`TupleShapeImplicits` 65 → 0、`SetParameter` 46 → 4、`GetResult` 25 → 4）。

**残っているもの**（このスライスでは直していない）:

- **可変長引数のフィールド型**。`class C(val xs: T*)` の `xs` は nsc では `Seq[T]` ですが、
  こちらは `T*` のままなので `c.xs.length` が `value length is not a member of T*` になります
  （黙って通しはしません）。コンストラクタ**呼び出し**は通るようになりました。
- **私有ランタイム（`--no-scala-library`）に可変長引数の裏付けが無い**のは今までどおりです。
  `def f(xs: Int*)` も `class C(xs: T*)` も `scala/collection/immutable/Seq` を参照するので、
  実行時に `NoClassDefFoundError` になります（メソッド側から変わっていない既存の穴で、
  コンストラクタもこれに揃えました）。
- ~~**case class のコンストラクタ引数が抽象メンバーを実装しない**~~ →
  `agent/ctoraccessor` スライスで修正しました。下の
  「コンストラクタ引数のアクセサと `FunctionN.tupled`」を参照。
- **`Vector[T]` が `scala.collection.IndexedSeq[U]` に適合しない**（`immutable.Vector` →
  `collection.IndexedSeq` の辺が無い）。`Vector[Any](1)` のように**明示した型引数**が
  companion の `apply` に伝わらない穴もあります。
- **タプル型の `ClassTag`**。`classTag[(_, _)]` の implicit が見つからず、
  `TupleSupport` に残る 2 件はこれです。

### コンストラクタ引数のアクセサと `FunctionN.tupled`

`agent/ctoraccessor` スライス。フィクスチャは `tests/fixtures/ctacc*.scala`、テストは
`crates/cli/tests/ctoraccessor.rs` です。

**1. `case class` のコンストラクタ引数がアクセサにならなかった**。これは
**型検査は通るのに実行時に落ち、しかも黙って通る**種類の穴でした。

```scala
trait Rep[T] { def value: T }
case class ConstRep[T](value: T) extends Rep[T]   // 実行時 AbstractMethodError
```

`class C(val x: Int)` は `emit_ctor_val_getters` が `x()` を出していましたが、判定が
「パーサが `val` キーワードを見て `Flags::ACCESSOR` を立てたか」だけでした。`case class` は
**キーワード無しで第 1 引数リストを `val` にする**ので、そこを通りません。フィールドだけが
出て `value()` が無く、`Rep` 越しに呼ぶと `AbstractMethodError` になっていました。
nsc は「case class の第 1 引数リストのみ」（第 2 リスト以降は private な状態のまま。
nsc は `case class C(implicit x: Int)` 自体を拒否するので、第 1 リストは常に非 implicit）
なので、そのとおりに条件を足しました。親が erase して `def value: Object` になる場合の
ブリッジは既にある経路がそのまま使われます。`javap -p -s` で実 scalac 2.13.16 と付き合わせ、
アクセサ名・ディスクリプタ・ブリッジの有無が一致することを
`ctacc_case_class_params_get_public_accessors` が固定します。

**2. `FunctionN.tupled` / `curried` と `scala.Function.untupled`**。slick の
`generated/slick/lifted/CompilableFunctions.scala` は 21 通りの `CompiledFunction` を
`f.tupled` で作るので、arity 2〜22 が全滅していました。関数型（`Type::Function`）には
`class_sym_of` が返すシンボルが無く、メンバ探索の行き先が存在しなかったのが原因です。
`scala.FunctionN` を `T1 … Tn, R` という型パラメータ付きで宣言し直し（`prelude_fntuple.rs`）、
`type_select` が関数型の受け手のときだけそこを引くようにしました（置換は受け手の
パラメータ型＋結果型を位置で当てます）。`prelude.rs` からは 1 行呼ぶだけです。
`tupled` / `curried` は `scala/FunctionN` の default メソッド、`untupled` は
`scala/Function$` なので、**`library_abi` 限定**です。私有ランタイムの `scala/Function0` /
`Function1` は `apply` しか持たないので、`--no-scala-library` では
`value tupled is not a member of (Int, Int) => Int` と診断します
（`fixtures_ctacc_fn_without_library_is_error`）。

このとき **3 つの一般的な穴**も直しています（どれも `tupled` 抜きで再現します）:

- `def g: Int => Int` を `g(3)` と呼ぶと `no matching overload` でした。引数リストを
  持たないメソッドの結果が関数なら、引数リストはその関数のものです
  （`auto_apply_nullary_function`）。
- `add(1)(2)`（カリー化された**関数値**）が 1 回の `Function1.apply` に潰れていました。
  uncurry と backend の両方に apply 平坦化があり、どちらも「内側の Apply の結果が
  関数型なら別の呼び出し」を見ていませんでした。`Function.untupled(f)(1, 2)` も同型です。
- erasure が「呼び先ツリーの型が関数型なのに、そのツリーが持つシンボルの結果型」を
  読んで unbox を巻いていました（`f.tupled(t)` のシンボルは `tupled` で、その結果が
  いま適用されている関数そのもの）。

`Function.untupled` の 4 本のオーバーロードは引数の**タプルの arity だけ**が違うので、
オーバーロード採点で関数型どうしを無条件に一致とみなしていたのを、
「引数側のパラメータ型がまだ未推論（`{ case … }` リテラル）でなければ arity と
タプル arity を見る」に絞りました。

**3. `Builder` の `+=` / `++=`**。`scala.collection.mutable.Builder` は prelude が
宣言せず pickle 供給で来ます。`b ++= xs` は 2 つの理由で通っていませんでした:
`try_rewrite_assignment_op`（`x += 1` を `x = x + 1` に書き換える nsc の
`convertToAssignment`）が **pickle を引かずに**「メンバが無い」と判断していたことと、
`Growable` の `+=` / `++=` が `this.type` を返すのを pickle 供給が
「表現できない型」として断っていたことです。前者は補完も試すようにし、後者は
`this.type` を**受け手を自分の型パラメータに適用した型**に写すようにしました
（`type_select` がそこに受け手の型引数を入れるので、`Builder[Int, List[Int]]` に対して
`Builder[Int, List[Int]]` が返り、`.result()` まで繋がります）。

計測（`tests/slick_measure.sh`、slick 184 ファイル、`-Xsource:3`）は
**1279 → 1219**、エラーを含むファイルは **109 → 107** になりました。
`CompilableFunctions.scala` のエラーは 21 → 0、`++= is not a member of Builder` は 6 → 0 です。

**残っているもの**（このスライスでは直していない）:

- **コンストラクタフィールドの可視性**。nsc は `private final`、こちらは `public final` です
  （アクセサ・ブリッジは一致）。パターンマッチの codegen が同名フィールドを直接
  `getfield` するので、private にするならその経路をアクセサ呼び出しに移す必要があります。
- **`Vector.newBuilder` / `List.newBuilder`** が companion に無いので、`Builder` の
  インスタンスは自分で書くしかありません（`ctacc_builder.scala` はそうしています）。
- **`xs.toArray` の `ClassTag` が埋まらない場合がある**。slick の
  `ProductResultConverter`（`(ClassTag[B])Any` のまま `cha(i)` を呼ぶ 6 件）が残ります。
- **コンストラクタ引数（`val` 無し）を外から読んだときの診断**が nsc と違います。
  nsc は `value hidden is not a member of Plain`、こちらは
  `value hidden cannot be accessed as a member of Plain from Main$` です
  （どちらもエラーにはなります。`ctacc_plain_bad`）。

### オーバーロードの候補集合（継承・`private[this]`・`java.lang.String`）

`agent/ovl2` スライス。フィクスチャは `tests/fixtures/ovl2*.scala`、テストは
`crates/cli/tests/ovl2.rs` です。slick に残っていた `no matching overload` の塊は、
**解決の規則**ではなく**候補集合の作り方**が原因でした。

**1. 継承はオーバーライドではない**。`drop_overridden` は「所有者がスーパークラスなら
落とす」だけで、シグネチャを見ていませんでした。

```scala
class Base { def f(x: Int): String = "int:" + x }
class Derived extends Base { def f(s: String): String = "str:" + s }
new Derived().f(1)   // Base.f(Int) を落としていたので no matching overload
```

nsc の `matchingSymbols` と同じく、**シグネチャが一致するときだけ**親側を落とすように
しました（引数リストを平坦化して個数と型を比較。型パラメータ／抽象型メンバを含む
パラメータは as-seen-from を再構成せずに一致とみなします）。あわせて
**コンストラクタは継承されない**ので、`pick_ctor_at` は `lookup_member` が親から拾ってくる
`<init>` を候補から外します。

同じ穴が backend にもありました。`emit_erasure_bridges` は「同名で
ディスクリプタが違う」だけでブリッジを出していたので、上の `Derived` に
`f(I)Ljava/lang/String;` という**検証不能な**ブリッジ（`Integer` を `String` の
位置に積む）が出ていました。ブリッジを出すのは、親のパラメータが erase して
`Object` になる（＝ジェネリックの実装）ときだけにしています。`-Xverify:all` で
固定しました。

**2. `private[this]` は継承されない**。テンプレートのスコープは自分のメンバと継承メンバを
**同じスコープ**に入れるので、素のコンストラクタ引数（nsc では `private[this]`）が親子で
衝突していました。slick の `LoggingPreparedStatement(st: PreparedStatement) extends
LoggingStatement(st: Statement)` は `st` が `<overload Statement | PreparedStatement>` に
なり、`st.execute()` が全滅していました。`enter_inherited_members` が
`PRIVATE | LOCAL` のメンバを入れないようにしています。逆に、親の素の引数を子から
名指しするのは nsc と同じくエラーです（`ovl2_bad`: `not found: value tag`）。

**3. `val` が抽象 `def` を実装したら 1 つのメンバ**。同じ理由で、
`trait InterpolationContext { def symbolName: SymbolNamer }` を実装する
`val symbolName: SymbolNamer` が `<overload SymbolNamer | SymbolNamer>` になり、
`symbolName(s)` が解決しませんでした。`bind_found`（識別子側）も
`drop_overridden` を通します。

**4. `java.lang.String` は `CharSequence` を実装している**。prelude は `String` の親を
`AnyRef` だけにしていたので、`String <: CharSequence` が偽で、`CharSequence` を取る
JDK のオーバーロードが**全て**不適合でした（`Instant.parse(s)` /
`LocalDate.parse(s, fmt)` / `DateTimeFormatter.parse(s)`）。`prelude_strhier.rs` が
JDK から `Comparable` / `CharSequence` / `Serializable` を読んで親に足し、
`is_sub_type` がそれを辿ります。これは JVM の事実なので **`library_abi` に依らず**
両モードで有効です。同じファイルで `indexOf` / `lastIndexOf` の
`(Int)` / `(Int, Int)` / `(String, Int)` を足しています（`s.indexOf(':')` は
`Char` を `Int` に広げて `indexOf(int)` を選ぶので、その候補が無いと落ちます）。

**5. オーバーロードされたメソッドの η 展開**。期待型が関数型のとき、nsc の
`inferExprAlternative` は「その関数型に η 展開できる 1 本」に絞ります。
`constOp[Long]("min")(math.min)` と `val g: (Long, Long) => Long = math.max` の両方
（引数位置と期待型位置）を通すため、`adapt` に `pick_overload_for_function` を足し、
採点側では `Type::Overload` の引数を「どれか 1 本が合えば合う」と見るようにしました。

**6. `new ArrayBuffer[R](g.length)`**。`ArrayBuffer` の
`def this()` / `def this(initialSize: Int)` を prelude が宣言していませんでした
（`prelude_ovl2.rs`）。どちらも 2.13.16 の実クラスにある `<init>()V` / `<init>(I)V` です。

計測（`tests/slick_measure.sh`、slick 184 ファイル、`-Xsource:3`）は
**1059 → 903**、エラーを含むファイルは **105 → 104** になりました。

**残っているもの**（このスライスでは直していない）:

- **`Map[K, V] <: Iterable[T]` から `T` が解けない**。`ConstArray.from(m)` の
  `no matching overload` は候補集合ではなく `infer_method_tparams` 側で、
  `h[T](xs: Iterable[T])` に `Map[String, Int]` を渡すだけで再現します
  （`h2(xs: Iterable[(String, Int)])` と明示 `h[(String, Int)](m)` は通るので、
  適合判定ではなく推論）。`agent/tyvar` の担当範囲なので触っていません。
- **`java.lang.String` の JDK メンバが on-demand で読まれない**。`codePointAt` などは
  prelude が宣言した分しか無く、`value codePointAt is not a member of String` です。
- **`xs.toArray` の `ClassTag`**（slick `ProductResultConverter` の
  `(ClassTag[B])Any`）は上の `agent/ctoraccessor` の残件のままです。

### 型メンバ・`this.type`・未確定変数の後始末（`type mismatch` 第 3 スライス）

`agent/mismatch3` スライス。フィクスチャは `tests/fixtures/mism3*.scala`、テストは
`crates/cli/tests/mismatch3.rs` です。8 つの原因を直しました。

**1. 継承したメンバをスコープに入れる順序が線形化ではなかった**。
`enter_inherited_members` は親を**深さ優先**で辿っていたので、*祖父母*の抽象宣言が
その子クラスの具象宣言より先にスコープに入りました。

```scala
trait N { type Self >: this.type <: N; def self: Self }
abstract class Base[T] extends N { type Self = Base[T] }
trait Extra extends N
new Base[T] with Extra { def self: Self = this }   // Self が N の抽象宣言に解決していた
```

親を**逆順（最後の mixin が先）に幅優先**で辿るようにしました。これは nsc の線形化で
メンバが届く順序そのもので、直接の親については従来どおり「最後の mixin が勝つ」ままです。
slick の `new SimpleFeatureNode[T] with SimpleFunction { … }` がこれでした。

**2. エイリアス型メンバの右辺が as-seen-from されていなかった**。
`type Self = Base3[T]` の `T` は `Base3` 自身の型パラメータなので、
`Base3[String]` 越しに読めば `Base3[String]` でなければなりません。
名前解決側（`Check::type_member_here`）と、レシーバ越しの展開
（`SymbolTable::expand_in_type`）の両方で置換します。

**3. どの引数も決められない型パラメータが型パラメータのまま残っていた**。
`def dbAction[R, S <: NoStream, E <: Effect](f: Session => R): ProfileAction[R, S, E]` の
`S` はどのパラメータ型にも出てこないので、nsc は `solvedTypes` で境界に確定させます
（共変なら下界、反変なら上界）。こちらは `Act[Unit, S, Schema]` のまま報告していました。
`instantiate_leftover_tparams` を足しました。**パラメータ型に現れる**型パラメータは
対象外です（そちらは引数か implicit が決めるもので、潰すと診断が消えるため。
`exptype_unsolved_bad` はそのまま落ちます）。期待型が無い呼び出し（レシーバ位置）も
対象外で、そこは nsc の `Context.undetparams` と同じく後続の適用に委ねます。

**4. `new C with T { … }` の次の行のブロックが引数になっていた**。nsc の
`canApply` が無かったため、

```scala
def build(p: IndexedSeq[Node]): SimpleFeatureNode[T] = new SimpleFeatureNode[T] with SimpleFunction {
  …
}
{ (paramsC: Seq[Rep[?]]) => … }      // これが上の無名クラスへの引数になっていた
```

となり、`build` がブロックの値として η 展開されて型が合いませんでした。
`parse_simple_expr` が `new` の直後は `can_apply = false` にします（`.` と `[…]` を
辿ったら真に戻すのも nsc と同じ）。

**5. `this.type` が受け手の型引数を落としていた**。`def add(v: T): this.type` を
`B[String]` に対して呼ぶと `B`（引数なし）になり、次の `add` のパラメータが裸の `T` に
なっていました。`subst_as_seen_from` がメンバのシグネチャ中の `C.this.type` を
レシーバそのものに置き換えます。self エイリアス（`trait T { self => }` の `self`）は
**囲いのインスタンス**を指すので、そこは置き換えません。

**6. レシーバが持ち越した未確定変数を、その呼び出しの引数が決められなかった**。
`ConstArray.newBuilder()` は `ConstArrayBuilder[?T]` で、`b + from` の `from` が `?T` を
決めます。呼び出しが結果に残した自分の型パラメータを `undet_tvars` に記録し、
引数から解くようにしました（slick の `Comprehension.children` の `+` / `++` の塊）。

**7. protected アクセスが一番内側のクラスしか見ていなかった**。

```scala
class DDL(val stmts: List[String]) { self =>
  protected def phase: List[String] = stmts
  def merge(other: DDL): DDL = new DDL(Nil) {
    override protected def phase = self.phase ++ other.phase   // 「$anon から触れない」
  }
}
```

nsc は**囲いのクラスすべて**について規則を判定するので、`DDL` の本体に書かれている
以上プレフィックスは `DDL` であれば足ります。`protected_subclass_ok` が owner を
外向きに辿ります。あわせて backend の穴も 1 つ塞ぎました: self エイリアスの読み出しは
`load_owner_instance` が「`this` が owner に適合するなら `this`」で止まるため、
**owner のサブクラスでもある**無名クラスの中では `this` を読んでしまい、
上の `self.phase` が自分のオーバーライドを呼んで無限再帰していました
（`load_self_alias_instance` が `$outer` を同一性で辿ります）。

**8. classpath の pickle が型引数と kind を捨てていた**。`unpickle` は `TYPEREFtpe` の
型引数を読み飛ばし、クラスの型パラメータも名前だけでした。`Monad[F[_]]` を
`-cp <ディレクトリ>` 越しに使うと `kinds of the type arguments (F) do not conform`、
`c.as(1)` は `Any` になります。`PickledType` / `PickledTypeParam` を入れて、
読み手（`classpath.rs`）が `Function1[A, B]` / `Tuple2[A, B]` / `Array[T]` を
構造的な `Type` に戻せるようにしました。書き手側も、高階な型パラメータの
`TYPEsym` に `POLYtpe` を書くようにしています（実 scalac 2.13.16 が
`-cp` で読めることを確認済み）。jar のクラスはこの経路を通りません（次節の `adopt_binary_class` を通ります）。

計測（`tests/slick_measure.sh`、slick 184 ファイル、`-Xsource:3`）は
**833 → 772**、`type mismatch` は **201 → 168**、エラーを含むファイルは
**102 → 100** になりました。新たにエラーを出すようになったファイルはありません。

### 早すぎたエイリアス完了と `FunctionN`（`type mismatch` 第 4 スライス）

`agent/mismatch4` スライス。フィクスチャは `tests/fixtures/mism4*.scala`、テストは
`crates/cli/tests/mismatch4.rs` です。6 つの原因を直しました。

**1. 遅延完了した型エイリアスがそのファイルの import を見ていなかった**（最大の塊）。
型エイリアスは「名前を dealias しなければならなくなった瞬間」に完了します。
*入れ子のテンプレート*の親句がまさにそれをやるので、シグネチャパスがエイリアスに
辿り着く前に、ヘッダパス（`parents_pass`）から完了が走ります。

```scala
import slick.sql.FixedSqlAction
trait JdbcActionComponent extends SqlActionComponent { self: JdbcProfile =>
  type ProfileAction[+R, +S <: NoStream, -E <: Effect] = FixedSqlAction[R, S, E]
  abstract class SimpleJdbcProfileAction[+R](…) extends … with ProfileAction[R, NoStream, Effect]
}
```

この時点でエイリアスを記録しているのは namer だけで、namer は**スコープを保存しません**
（`PendingSig.scopes: None`）。`swap_in_scopes` は owner の鎖からスコープを組み直すので、
囲いのテンプレートのメンバは入りますが**ファイルの import は入りません**。結果
`FixedSqlAction` が `Type::Named` のまま解決できず、`ProfileAction` の型は
`<error>` に固定され、以後 `new SimpleJdbcProfileAction[Unit](…) { … }` が
すべて `type mismatch; found: $anon$N required: JdbcActionComponent.ProfileAction[…]`
になっていました（`JdbcActionComponent` だけで 26 件、`MemoryProfile` /
`MemoryQueryingProfile` にも同じものが）。

ヘッダパスは**そのファイルの import を型付け済みで、テンプレートのメンバも
入れ終わっている** —— エイリアスが書かれた語彙そのものです。`refresh_alias_sigs` が、
入れ子テンプレートへ降りる直前に、まだ namer の記録しか持たない `TypeDef` の
`PendingSig` へ現在のスコープスタックを渡します。

**2. compound 型が「適用された抽象型メンバ」に適合しなかった**。

```scala
trait P { type M[+R] <: A[R];  type N[+R] <: A[R] with M[R] }
trait Q extends P { type M[+R] <: B[R];  type N[+R] <: B[R] with M[R] }
```

`B[R] with M[R] <: A[R] with M[R]` を見るとき、右辺の `M[R]` は**抽象**メンバの適用で、
展開する右辺を持ちません。`is_sub_type` の `(other, Applied)` 腕はそこで `false` を
返していました。nsc は右辺で決められないときは**左辺の規則**へ落ちるので、compound は
自分の親の 1 つを通して適合します。

**3. `Map[K, V]` が `K => V` ではなかった**。2.13 の `scala.collection.Map[K, +V]` は
`PartialFunction[K, V]` を継承しています（`javap` の interface 一覧に `scala/Function1`
が並ぶ）。prelude の階層表（`prelude_hier.rs`）には `Iterable` 側の辺しか無く、
slick の `val symbolToIndex: TermSymbol => Int = someMap` が落ちていました。
辺は `crates/typer/src/prelude_mism4.rs` で張ります。

同時に **`scala.FunctionN` という「クラス」と構造的な `(T1, …) => R` を同じ型として
扱う**ようにしました（`SymbolTable::function_class_shape`）。prelude は
`PartialFunction` の親などをクラスで書き、それ以外では構造的な形を使うので、
両方を行き来できないと `PartialFunction[A, B] <: A => B` すら成り立ちません。
これは `is_sub_type` の両方向、関数リテラルの期待型（`type_function`）、
オーバーロードの適用判定（`arg_score`）の 3 か所に効きます。3 番目は
**pickle 由来のシグネチャで関数パラメータがクラスとして書かれる**ため重要で、
`IterableOnceOps.reduceLeft[B >: A](op: Function2[B, A, B]): B` に
リテラルを渡すと `no matching overload … with arguments ((<notype>, <notype>) => <notype>)`
になっていました。

**4. `map` が受け手のコレクションを落としていた**。`IndexedSeq` は `map` を
宣言し直さないので、継承した宣言は `Seq[B]` と言います。しかし実際のシグネチャは
受け手自身の型構成子（`IterableOps.CC[B]`）を返すので、`xs.toSeq.map(f)` が
`IndexedSeq` なら結果も `IndexedSeq` です。従来は「宣言された結果が勝つ」だけでした。
受け手が **`scala.collection` のクラスで、宣言された結果クラスの子孫であるとき**に
限って受け手を優先します。`Range`（自分の型パラメータを持たない）は従来どおり
宣言された `IndexedSeq` のままですし、`Seq` を継承しただけのユーザクラスは
`Seq` の builder を継承するので、こちらも宣言された結果のままです。

**5. 安定識別子パターンが、まだ決まっていないスクルーティニに弾かれていた**。

```scala
def f[T](t: ScalaType[T]) = t match {
  case ScalaBaseType.byteType => …    // found: ScalaNumericType[Byte] required: ScalaType[T]
}
```

`T` は `Byte` かもしれず、パターンは実行時にはただの `==` なので、型引数がまだ
分からないスクルーティニは何も排除しません。`relax_abstract_targs` が、期待型に使う
スクルーティニの**型引数**にある型パラメータ・抽象型メンバを `_` に置き換えます
（先頭のクラスは緩めません）。

**6. `type Self >: this.type <: Node` に `this` が適合しなかった**。

```scala
trait Node { type Self >: this.type <: Node; def mapChildren(…): Self }
trait NullaryNode extends Node {
  override final def mapChildren(f: Node => Node, keepType: Boolean = false): Self = this
}
```

`adapt_singleton` は「抽象型メンバの**下界**が `this.type` なら `this` は通る」を
すでに持っていましたが、`ThisType(cls)` の判定が `tree.sym == cls` の**同一性**でした。
下界は `Node` の語彙で書かれているので、`NullaryNode` から読めば
`NullaryNode.this.type` です。`This` ツリーの指すクラスが `cls` の**子孫**であれば
通すようにしました。**`This` ツリーにしか適用しない**ので、
`def wrong(a: Node, b: Node): a.Self = b` は今も落ちます（scalac も落とします）
——「素直に下界規則を入れると別の `Node` まで通る」という懸念はここで切れます。
あわせて `Node.Self with DefNode` のような compound も、親を 1 つずつ見るように
しました。`val n: Self = if(…) this else rebuild(…)` は、`this` が受理された時点で
`Self` に広がる（`this.type <: Self` なので健全）ので、両枝の lub も `Self` です。

計測（`tests/slick_measure.sh`、slick 184 ファイル、`-Xsource:3`）は
**711 → 635**、エラーを含むファイルは **91 → 87** になりました。新たにエラーを
出すようになったファイルはありません。`type mismatch` は **157 → 127** ですが、
これは 3 番の効果で `no matching overload` だったものが本来の `type mismatch` に
変わった分（`BasicBackend` の cats-effect まわりなど）を含みます。
`type mismatch` だけを見ると 157 → 114 まで落ちたあと、`Function2` の穴を塞いだ
ぶん 131 に戻り、`Self` で 127 になっています。

**残っているもの**（このスライスでは直していない）:

- **`case Seq(a, b)` が使えない**。`unapplySeq` を持つのは prelude の `List` だけで、
  `Seq` には無いので `case Seq((s, _)) => Some(s)` は「クラスパターン」に落ちて
  要素型が付きません（slick `JdbcStatementBuilderComponent` で 4 件）。
  prelude に足すのは簡単ですが、codegen は `gen_unapply_seq_bind` が
  `checkcast List` から始まる **List 専用**なので、`Vector` を `Seq` として渡すと
  実行時に落ちます。`SeqOps.length` / `apply(I)` を使う版か `toList` の挿入が要ります。
  ついでに `case List(a, b, rest @ _*)` の codegen は**現状でも** `VerifyError` を
  出します（星付きパターンの前の要素に checkcast が出ていない）。
  → **`agent/seqpat` で解決**（後述）。`List(a, b, rest @ _*)` の `VerifyError` は
  その前の `41d4bca` で既に直っていました。
- **`StringOps.map[B](f: Char => B): IndexedSeq[B]`** が無く、
  `"…".map(_.toString)` が `found: String required: Char` になります。2.13 は
  `map(Char => Char): String` と 2 つのオーバーロードを持ちますが、prelude に
  2 つ並べるとリテラルの結果型が決まる前に `ambiguous overload` になり、
  1 つに畳むと erasure が結果型を symbol から取り直すため codegen が
  `IndexedSeq` を返す方を呼びます。オーバーロード解決がリテラルの結果型で
  絞れるようになるまで保留です。→ **`agent/seqpat` で解決**（後述）。
- **安定識別子パターンの型検査そのもの**。scalac 2.13.16 は
  `case Ids.other =>`（`other: Other`、スクルーティニ `ST[Int]`）を**通します**が、
  こちらは今も `type mismatch` を出します。今回は型引数が抽象なときだけ緩めました。
  → **`agent/seqpat` で解決**（後述）。
- `Seq("a").map(m)`（`m: Map[String, Int]`）は `Map` が関数になっても通りません。
  適合ではなく推論（`Function2[B, …]` の `B` が未解決）の側です。

### 関数型を継承したトレイトと省略された型引数（`type mismatch` 第 5 スライス）

`agent/mismatch5` スライス。フィクスチャは `tests/fixtures/mism5*.scala`、テストは
`crates/cli/tests/mismatch5.rs` です。8 つの原因を直しました。

**1. 関数型を親に持つトレイトが SAM にならなかった**（最大の塊）。

```scala
trait CanBeQueryCondition[-T] extends (T => Rep[?])
implicit val c: CanBeQueryCondition[Rep[Boolean]] = value => value
```

唯一の抽象メソッドは `Function1.apply` で、それは**構造的に書かれた親**から
継承されます。`class_sym_of` は `Type::Function` をあえてクラスにしません
（適合と erasure が構造的に扱うため）ので、**クラスを要る場所だけ**が
`SymbolTable::function_class_form`（`function_class_shape` の逆）を呼ぶように
しました —— SAM 探索（`abstract_sam_methods`）、メンバ検索（`lookup_member`）、
as-seen-from（`subst_as_seen_from` の `walk`）、JVM の interface 一覧
（backend `split_parents`）、線形化（backend `linearize`）の 5 か所です。

同時に **prelude の `FunctionN` に本物の型パラメータを持たせ、`apply` を
`ABSTRACT` にしました**。従来は `apply(Any): Any` かつ非 abstract だったので、
(a) SAM 探索が抽象メソッドを 1 つも見つけられず、(b) 見つけたとしても
`C[X]` を通して読んだときに置換するものが何もありませんでした。
`self.apply(rs)`（`trait GetResult[+T] extends (PositionedResult => T) { self => }`）
のために `walk` は `ThisType` も歩きます。
`resolve_overload` の `Type::Class` 腕も `type_select` と同じく as-seen-from
するようにしました（生のまま読むと `m(3)` が `found: 3 required: T1`）。
`Select` が**値に解決されたとき**は `.apply` を挿入する受け手になります
（従来は Select というだけで諦めていた）。

**2. 2 回目の推論パスが「呼び出し側の型パラメータ」という解を捨てていた**。

```scala
def mk[T](f: PR => T): GR[T] = …
def const[T](value: T): GR[T] = mk(_ => value)   // found: GR[T] required: GR[T]
```

`mk` の `T` はラムダの**結果**からしか決まらないので 2 回目のパスが解きますが、
そこは `Type::TypeParam` をすべて弾いていました。弾くべきなのは
**その呼び出し自身の**変数（`T := T` は解ではない）だけで、呼び出し側の型
パラメータは立派な解です。

**3. `extends Base(s)` が親の型引数を推論していなかった**。

```scala
class DerbySequenceDDLBuilder[T](seq: Sequence[T])
  extends SequenceDDLBuilder.BuiltInSupport.OverrideActualStart(seq)
```

nsc の `parentTypes` はコンストラクタ引数から親の型引数を推論します。していないと
パラメータが `Sequence[Base.this.T]` のまま残り、両辺が `Sequence[T]` と表示されて
どちらも他方ではない、という診断になります。推論した型引数は**記録される親**にも
なるので `Derived[X] <: Base[X]` も成り立ちます（`Typer::infer_parent_targs`）。

**4. `new C` が期待型から型引数を読んでいなかった**。

```scala
def unit[R]: ResultConverter[R, W, U, Unit] = new UnitResultConverter
```

期待型が**親クラス**を指しているので、`UnitResultConverter[R] <: RC[R, …, Unit]` から
`R` を読みます（コンストラクタパターンがスクルーティニに対してやるのと同じ計算＝
`base_targs_from_pt`）。引数から解ける分と併せる必要があるので、パラメータごとに
`Option` で返します。あわせて **`new C(args)` の頭は適用全体の期待型に適合しなくて
よく**なりました（`type_expr_inner`）。頭だけを見て
`found: ProductResultConverter required: ResultConverter[R, W, U, _]` と言っていた
のはこれです。

**5. パラメータのクラスに引数を揃えてから単一化していなかった**。`unify_one` は
シンボル表を持たず型引数を位置で zip するので、`def id[R, U](c: RC[R, U])` に
`UnitRC[String]`（＝`RC[String, Unit]`）を渡すと `[R, U]` に `[String]` を
zip して `U` が解けませんでした。`unify_tparam_all` が
`base_type_instance` で揃えます。

**6. implicit だけの引数節が期待型で埋まらなかった**。`TreeMap.empty` は
`[K: Ordering, V]: TreeMap[K, V]` で、`V` はどの implicit パラメータにも
現れません。したがって探索だけでは決まらず、`adapt_implicit_apply` は
「`TypeApply` を待つ」ため何もしないまま**メソッド型そのもの**を値の型に
していました（`found: (Ordering[K])TreeMap[K, V]`）。nsc は
`inferExprInstance` を先に走らせるので、**期待型が implicit パラメータに
現れる型パラメータを全部決められるとき**は先へ進みます。

**7. 注釈付きの型に `.apply` が挿入されなかった**。slick の
`val (b, m: Map[…] @unchecked) = …` に続く `m(f)` が
`value apply is not a member of Map[…] @unchecked` になっていました。注釈は
型が**どんなメンバを持つか**については何も言わないので、形を問う場所は
すべて注釈を透過します（`strip_annotations`）。

**8. 同じ要素型の変換が受け手のコレクションを返していなかった**。2.13 は
`filter` / `filterNot` / `take` / `reverse` / `++` / `:+` / `updated` / `sortWith`
などを `C`（受け手自身のコレクション）を返すものとして宣言します。prelude は
`C` を書けないので `Vector[Phase].filterNot(p)` が継承した `Seq[Phase]`、
`phases ++ ps` が `IndexedSeq[Phase]` になっていました。第 4 スライスの `map` と
同じ形の規則ですが、**消去後の descriptor が `Object` を返すメンバに限ります**
（`erases_to_object`）。`TreeMap.filter` は JVM 上 `Map` を返すので、ここで
`TreeMap` に絞ると codegen が `Map` を `TreeMap` のフィールドに積んで
`VerifyError` になります（`to*` 変換は元から対象外＝`v.toSeq` は本当に `Seq`）。

おまけに **コレクションファクトリの要素型を期待型で広げる**ようにしました。
`Set` と `Map` は非変なので `def f(s: AnonSym): Set[Sym] = Set(s)` は
部分型の問題ではなく、ファクトリの近道（引数だけから要素型を決める）が
期待型に訊く必要があります（`factory_targs_from_pt`）。

計測（`tests/slick_measure.sh`、slick 184 ファイル、`-Xsource:3`）は
**620 → 547**、エラーを含むファイルは **87 → 81**、`type mismatch` は
**127 → 98** になりました。新たにエラーを出すようになったファイルはありません。

> 計測の注意: `tests/slick_measure.sh` の `BIN` は**親リポジトリの**
> `target/release/scala-rs` を指しています。git worktree で作業していると
> `cargo build --release` は worktree 側の `target/` に出るので、
> スクリプトはビルドしたバイナリではなく `main` のバイナリを測ります
> （実際に「変更しても数字が 1 ミリも動かない」形で踏みました）。
> worktree では `SCALA_RS=<worktree>/target/release/scala-rs tests/slick_measure.sh`
> と明示してください。

**残っているもの**（このスライスでは直していない）:

- **`MapOps` / `SetOps` の `-` / `removed` / `incl` / `excl` / `filter` は
  受け手のコレクションに絞れない**。これらは JVM 上 `Map` / `Set` という
  **名前のあるクラス**を返すので、typer が `TreeMap` に絞っても codegen の
  Apply 結果型は消去後のシンボルから取り直され、`TreeMap` のフィールドへ
  `Map` を積んで `VerifyError` になります。Apply 自身の結果型が erasure を
  生き残るようになれば外せます（`agent/seqpat` が触っている領域）。
  slick の `ConcurrencyControl` に 2 件残ります。
- **タプルのパターン定義で成分に型を書くと `VerifyError`**（main でも同じ）。
  `val (n: Int, s: String) = if (b) (1, "x") else (0, "y")` は
  `Bad local variable type`（int を参照ローカルに入れている）になります。
  slick の `HoistClientOps` の
  `val (bl2: Bind, lrepl: Map[…] @unchecked) = …` がこの形です。
  → **`agent/mismatch6` で解決**（`_: T` の部分パターンは参照のまま束ねる）。
- **タプル成分への期待型の伝播**。`(new Sel, Map(s -> a))` を
  `(Node, Map[Sym, Int])` に対して型付けると、成分の `Map(s -> a)` は
  期待型なしで型付けられて非変な `Map[AnonSym, Int]` になります。
  nsc の `protoTypeArgs`（引数を型付ける前に期待型から型引数の見込みを立てる）
  を入れてみましたが、by-name パラメータが `() => T` のまま渡って
  611 → 604 と悪化したので巻き戻しました。by-name を除外した形なら通る
  見込みです。
- **`def wrong[A, B](v: B): GR[A] = mk(_ => v)` を通してしまう**（main でも同じ）。
  期待型が非変位置で `T := A` を強制したあと、ラムダの本体が `A` に対して
  再検査されません。scalac は `found: v.type required: A` を出します。

### 合流型・型検査を通ってから落ちる 3 件（`type mismatch` 第 6 スライス）

`agent/mismatch6` スライス。フィクスチャは `tests/fixtures/mism6*.scala`、テストは
`crates/cli/tests/mismatch6.rs` です。README の Remaining に記録されていた
**3 件**（うち 2 件は型検査を通ってから `VerifyError` になるコード生成のバグ、
1 件は型検査で落ちる明示型ラムダ）と、`type mismatch` の 6 原因を直しました。
以下の 9 項目です。

**1. 分岐の合流型が `java/lang/Object` になっていた**（codegen）。

```scala
h.cur = (3: Int) match { case 0 => None; case n => Some(n) }
```

`match` の枝は `scala/Some` と `scala/None$` という**別のクラス**を積みます。
アセンブラはクラス階層を持たないので、`merge_vtype` はその 2 つを
`java/lang/Object` に潰していました。結果、`putfield Lscala/Option;` は
`VerifyError: Bad type on operand stack` になります。

**式の静的型そのものが全枝の上界**なので、生成側がそれを渡すようにしました
（`Assembler::set_join_class`。`gen_match` / `gen_int_switch` / `gen_if` が
`join_class_of(result_ty)` を合流ラベルに宣言する）。合流はスタック最上段
——つまり合流の対象になっている値——にだけ適用します。下の段は分岐の前に
積まれたもので、どの経路でも同じだからです。

`try` は結果を**ローカル**に置くので同じ手当てが要ります
（`Assembler::set_local_class`。宣言したスロットに入る参照はすべてその
クラスとして記録されます）。

同時に `ret_object` を**外しました**。これは「参照どうしの合流は**メソッドの
戻り型**にする」という、`areturn` だけのために置かれた当て推量です。宣言する
型が本当の上界である保証がどこにも無く、
`Some(n match { case 1 => "one"; case _ => n })` のように `Option` を返す
メソッドの**内側**で `String` と `Integer` が合流すると、フレームが
`scala/Option` を名乗って `VerifyError: Inconsistent stackmap frames` に
なっていました（main でも同じ）。宣言が無い合流は `java/lang/Object` ——
どんな参照でも代入可能な、常に正しいフレーム型——にし、本当の型が分かる場所
だけが `set_join_class` で言うようにしました。

**2. `try` の型が本体の型のままだった**。上と対になる型検査側の穴です。

```scala
try Success(f) catch { case NonFatal(e) => Failure(e) }
```

コメントは「nsc は本体とハンドラの lub を取る」と書いてありましたが、実装は
本体が `Nothing` のときしか lub を取らず、それ以外は本体の型をそのまま
使っていました。ハンドラが本体に**適合しない**ときは lub が要ります。
`try n catch { case _: Exception => "x" }` は `Any` であって `Int` ではなく、
`Int` のままだと `int` のスロットに `Integer` を `istore` して
`VerifyError` になっていました（main でも同じ）。

結果は 1 つのローカル（1 つの JVM sort）に入るので、参照のスロットに
プリミティブが来たら箱詰めします（`box_for_result_slot`）。枝が既に箱詰め
済みかどうかは木の型では分からない——型検査側の adapt が箱詰めしていることが
ある——ので、**アセンブラのスタックの実際の型**を見ます
（`Assembler::top_is_reference`）。

lub を取らないのは枝に `Unit` がある形だけです。nsc は文の位置の
`try f() /* Int */ catch { println }` を `Any` に lub しますが、`gen_try` には
その形のために「本体の sort の既定値を積む」経路が既にあります。

**3. `_: T` の部分パターンが参照のままでなかった**（codegen）。

```scala
val (n: Int, s: String) = if (b) (1, "x") else (0, "y")
```

`Tuple2._1` を読んだあと `emit_from_erased_object` が `int` に開けてしまい、
続く型テストがそのローカルを `aload` して
`VerifyError: Bad local variable type` になっていました。`_: T` は
**テスト**なので `instanceof` が読む参照が要ります —— 開けるのは
`gen_pattern` の `Typed` 枝がテストを通ったあとにやります。
部分パターンを束ねる 7 か所（`bind_subpattern`）は、パターンの型ではなく
**スタックにある値の sort** を受け取るようにしました。スクルーティニが
すでにプリミティブなら型テストは静的に決まっているので、`Typed` 枝は
`instanceof` を出さずに素通しします。

**4. 明示型ラムダの本体が期待結果型で検査されない**。

```scala
xs.foreach((x: Int) => x + 1)   // found: (Int) => Int  required: (Int) => Unit
```

nsc は関数リテラルの本体を期待型の**結果**に対して型付けます（value discarding も
数値拡大もそこで起きます）。パラメータ型を書いたリテラルは、オーバーロード解決が
その結果型を必要とするので**期待型が決まる前に**型付けられ、本体は期待結果型を
見ないままでした。`adapt` に `adapt_function_literal_result` を足して、
リテラルのときだけ本体を期待結果型に adapt します。**リテラルのときだけ**です ——
`val h: Int => Int = …; fu(h)` は nsc と同じく `type mismatch` のままです
（`tests/fixtures/mism6_bad.scala`）。

**5. `Map` は自分が宣言している関数である**。2.13 の
`MapOps[K, +V, …] extends IterableOps[…] with PartialFunction[K, V]` なので
`on.map(columnIndexes)` はキー引きです。prelude に `MapOps` は無く、
`Map` に `PartialFunction` の親を張ると継承メンバの走査順が変わって
`toMap` の `A <:< (K, V)` が壊れる（`A` が `Char` になる）ので、
`Typer::function_view`（`arg` を継承した構造的関数型として読む）に事実を
書きました。使うのは 3 か所で、いずれも**それ以外が何も決まらなかったとき**の
フォールバックです:

- `arg_score` の最後（早い段階で採点すると slick の `map` 呼び出しが
  軒並み `ambiguous overload` になりました）
- `unify_tparam_all`（引数のままでは型パラメータが 1 つも決まらなかったとき）
- `map` の「受け手のコレクションを保つ」近道（要素型は関数の戻り型）

`scala.FunctionN` **クラス**自身はこの view の対象外です。すでにどこでも
関数として扱われており、ここで構造的に書き直すと `map` の両オーバーロードが
同時に applicable になります。

**6. `WithFilter` が型構築子を持っていなかった**。2.13 は
`class WithFilter[+A, +CC[_]]` で `map[B](f: A => B): CC[B]` です。prelude は
`CC` に**適用済み**のコレクション（`List[A]`）を入れて `map: CC` としていたので、
`for (x <- xs if p) yield x.toString` は `List[Int]` のままでした
（jar モードのみ。私有ランタイムでは `withFilter` が受け手をそのまま返します）。
`CC` を kind 1 の型パラメータにし、`map` / `flatMap` に自分の `B` を持たせて
`Type::Applied { ctor: CC, args: [B] }` を返すようにしました。

**7. for 内包の値定義が生成子として数えられていた**。

```scala
for { m <- ms if m > 0; q = f(m) } yield q
```

`q = e` はラムダ本体の `val` になるので、その前の生成子は**やはり最内**で
`map` を取ります。列挙子の位置で数えていたため `flatMap` になり、関数が
コレクションではなく要素を返す形になっていました。値定義の後ろに**ガード**が
付く形（nsc はタプルに組んでストリームを絞る）は、この desugaring では
表現できないので**診断します**（`tests/fixtures/mism6_forval_bad.scala`）。

**8. コレクション階層に `scala.collection.IndexedSeq` と mutable の背骨が無かった**。
`ArrayBuffer` はどこでも `IndexedSeq` ではなかったので、slick の
`def and(ns: scala.collection.IndexedSeq[Node])` は自分が組み立てた
`ArrayBuffer` を受け取れませんでした。`prelude_hier.rs` に
`collection.IndexedSeq` / `mutable.Seq` / `mutable.IndexedSeq` /
`mutable.Buffer` を足し、`ArrayBuffer` と `ListBuffer` をそこに繋ぎました。

**9. `Success` / `Failure` の `apply` に型パラメータが無かった**。`apply` は
生の `Success` / `Failure` を返していたので、`def a[R](…): Try[R] = Success(f)` は
`found: Success required: Try[R]` でした。`Failure.apply[T]` の `T` は
どのパラメータにも現れないので、期待型（か `Nothing`。`Try` は共変なので無害）
だけが決められます。

slick: `errors 537 → 526`、`type mismatch 90 → 83`、`files_with_errors` は 80 のまま。
エラーが出るファイルの集合は変わっていません。

### jar のクラスを pickle から読む

`load_classpath` はディレクトリしか歩きません。つまり **jar の中のクラスは
`ScalaSignature` ではなく JVM の generic signature から**読まれていました。この形式は
**高階の kind を書けません**。`trait Monad[F[_]]` は `<F:Ljava/lang/Object;>` として
届くので `F` はただの型、`def pure[A](a: A): F[A]` は `(TA;)TF;` として届くので結果は
`F[A]` ではなく `F` です。結果として `Monad[F]` はすべて
`kinds of the type arguments (F) do not conform`、`F.pure(v)` はすべて
`found: F required: F[Int]` になっていました。cats / cats-effect を使う
`BasicBackend.scala` と `ConcurrencyControl.scala` はまるごとこれが原因です。

pickle には本当のシグネチャが書いてあります。`crates/pickle` は 2.13.16 の pickle を
読み切れる（scala-library の 799 個すべて）ので、**足りていたのは jar のエントリに
それを使う経路だけ**でした。`PickleSupply::adopt_binary_class` がそれです。

- classfile が `ScalaSig` を持つとき（＝ Scala のクラスのとき）だけ、
  `install_java_class_in` が組んだシンボルを pickle で**上書きします**。
  クラスの親・フラグ・フィールドは classfile から来たものをそのまま使い、
  - 型パラメータの **kind**（`F[_]` の arity）と、
  - pickle が宣言するメンバ 1 つずつのシグネチャ
  を pickle から取ります。**pickle で表現できなかったメンバは classfile 読みの
  ままにします**（`erased_desc` が決まらない、型が `Type` に落ちないなど）。
  つまり精度は上がっても、メンバが消えることはありません。
- `scala.*` / `java.*` は対象外です。標準ライブラリは prelude ＋ `complete` という
  検証済みの経路を通ります（prelude が勝つ規則を壊さないため）。
- **先読みはしません**。classfile が 1 つ読まれたときに、そのクラスだけを見ます。
  slick の依存 classpath（cats / cats-effect / slf4j ほか 40 個超の jar）での計測時間は
  1:58 → 1:51（user 101.5s → 107.5s）でした。

あわせて 3 か所塞ぎました。

1. **型パラメータの適用**（`conv_ref`）。`F[A]` は `Type::Applied` で書けるのに
   「高階なので表現できない」と落としていました。`F` の kind arity が引数の数と
   一致するときだけ `Applied` にします（存在型のワイルドカードや kind の分からない
   ものは、間違った型を作るより落とす方がましなので落とします）。
2. **プレースホルダのシンボルに kind を後付けする**（`give_stub_its_kinds`）。
   `find_or_stub_java_class` は親リストやディスクリプタが名指した名前に
   「中身の無いシンボル」を入れます。標準ライブラリの外ではこれが至る所で当たり、
   `cats.effect.kernel.Sync` は型パラメータ 0 個のまま `Sync[F]` が
   「applied to 1 argument but the symbol has 0」になって、`Ref.of` /
   `Ref.ofEffect` / `Ref.lens` が全部落ちていました。まだ誰も埋めていない
   シンボルにだけ、pickle が宣言する型パラメータを与えます。
3. **erasure bridge の override 判定**（`bridge_overrides`）。同じディスクリプタに
   erase する 2 つのパラメータは JVM から見て同じパラメータなので、オーバーロードを
   区別する材料にはなりません。`def bind[A, B](fa: F[A], f: A => F[B])` を
   `F = Option` で実装すると `f: A => Option[B]` になりますが、構造比較では
   「override ではない」と見えてブリッジが出ず、インターフェース越しの `bind` が
   実行時 `AbstractMethodError` になっていました。

**pickle ライタ側**も 2 つ直しました。どちらも「自前で出した jar を読み戻せない」
原因でした（ディレクトリなら通るのは、読み手がパッケージをファイルパスから
復元しているからです）。

- **トップレベルのクラスの所有者が `<empty>`** でした。unpickler は所有者を pickle から
  読むので、`package hklib` の中のクラスは自分を `hklib.Monadic` ではなく `Monadic`
  と名乗っていました。実 scalac 2.13.16 も自前リーダも見つけられません
  （`not found: type Monadic`）。パッケージの module class を `EXTMODCLASSref` の
  連鎖として書くようにしました。
- **`FunctionN` に型引数が付いていません**でした（`TupleN` は付いていました）。
  引数の無い `Function1` は読み手が落とすしかないので、`f: A => F[B]` を含む
  シグネチャはすべて供給されませんでした。

計測（同上）は **772 → 766**、エラーを含むファイルは **100 → 100** です。数字が
小さいのは、`Monad[F]` が通るようになると今度はその先（cats の `implicits` 経由の
implicit 探索、`Ref.Make[F]` の導出）で止まるからです。エラーの中身は
`kinds of the type arguments (F) do not conform` のような「読み違え」から、
`could not find implicit value of type Make[F]` のような
「本当に足りていない機能」に変わりました。

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
- **quasiquote の展開（reification）**。`q"..."` / `tq"..."` / `pq"..."` / `cq"..."` は
  **認識して診断する**ところまでです（上の「実装している言語サブセット」参照）。中身は
  scala-rs のパーサで実際に構文解析するので、通らない構文は
  `unimplemented syntax: quasiquote ...` として**その場で**報告されます。
  残りは、解析結果を `internal.reificationSupport.Syntactic*` の呼び出しに落とすことと、
  その受け皿である reflect ABI の型検査・コード生成です。何が要るかは
  [`docs/macros.md`](docs/macros.md) §7.3 に列挙しました。
  なお slick の `ShapedValue.mapToImpl` にある 14 箇所の quasiquote は
  **すべて構文解析できて**おり、`unimplemented syntax` は 1 件も出ません
- full nsc pickle（出しているのは TERMname / TYPEname / TYPEsym / CLASSsym / MODULESYM / VALsym / EXTref / EXTMODCLASSref / METHODtpe / POLYtpe / TYPEREFtpe / CLASSINFOtpe / TYPEBOUNDStpe / THIStpe / SINGLEtpe / NOPREFIXtpe / CONSTANTtpe / LITERALint / LITERALboolean / LITERALstring ほかリテラル / EXISTENTIALtpe / REFINEDtpe / SYMANNOT / ANNOTATEDtpe / ANNOTINFO / TREE（IDENTtree / SELECTtree / THIStree / SUPERtree / APPLYtree）のサブセット。ByteCodecs は SID-10。ワイヤ形式は nsc と同じ nentries + ビッグエンディアン Nat。vals は METHOD|STABLE|ACCESSOR ゲッター + NullaryMethodType。case class は CASE + フィールド CASEACCESSOR。Flags は nsc raw long を `rawToPickledFlags`（VARARGS / BRIDGE / JAVA を適用箇所で出す）。scalac 2.13.16 が `val` / パラメータ付き `def` / `id[T]` / `new Point` + `p.x` / companion apply `Point(...)` / term `Point` / extractor `unapply` / object の `def` / `def f(xs: List[_]): Int` / `@deprecated("msg", "2.13.0") def g` / `def me: this.type` / `def f(xs: List[_ <: AnyRef])` / `def h(x: Int @unchecked)` / `val one: 1` / `def lit(x: 1)` / `def nest(xs: List[_ <: List[_]])` / `def idRef(x: MixA with MixB { def f: Int })` / `@Ann(foo)` / `@Ann(c.x)` / `@Ann(this)` / `@Ann(classOf[Int])` / `@Ann(ident(1))` / `@Ann(this.x)` / `@Ann(super.foo)` / `@Ann(ident(ident(1)))` / `@Ann(foo = 1)` / `@Ann(foo = this.x)` / `@Ann(foo = bar)` / `Lib.join("a","b")` / `new OrdBox(1).compare(...)` を typecheck できる範囲。**パラメータ節は 1 つに潰れる**（`uncurry` がシンボル上の `paramss` を平坦化したあとに pickle するので、`def bind(fa)(f)` は `bind(fa, f)` として読まれる）。**CLASSINFOtpe の親は `Object` だけ**（`trait Monadic[F[_]] extends Functor[F]` の継承関係は pickle に載らない。classfile の interfaces には載るので、こちら側の `-cp` 読みでは効く）。full pickle ではない。残る穴は Remaining）

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
- **ライブラリ（`agent/seqpat` の追加分）**: `Seq$` / `Vector$` / `IndexedSeq$` の `unapplySeq`（実体は identity。読み出しは `scala/collection/SeqFactory$UnapplySeqWrapper$` の `lengthCompare$extension` / `apply$extension` / `drop$extension`）と `Array$.unapplySeq`（`scala/Array$UnapplySeqWrapper$` の同名 extension）。`StringOps.map` は 2 本になり、`Char => Char` が `map$extension(String, Function1)String`、それ以外が `map$extension(String, Function1)IndexedSeq`。いずれも **library のみ**で、`--no-scala-library` では診断する。dual-run: `seqpat` / `seqpat_map` / `seqpat_ids`（`seqpat_ids` は私有ランタイムでも同じ出力）。
- **object**: scalac と同様、`Main$`（モジュール）と静的フォワーダ `Main` を出します。`java Main` が動くのはそのためです。
- **プリミティブ**: `Int` の `+` などは `scala.Int` のボックスメソッドではなく、JVM 命令（`iadd` など）として出します。
- **trait**: 抽象メンバーだけの trait は JVM interface です。具象メンバーは `T$class` 静的実装と、C3 線形化順のインスタンスフォワーダです。Java 8 default method は使いません。`val` は getter/setter + `$init$` です。`abstract override` は `T$$super$m` です。
- **名前付き引数**: 呼び出し側で `f(b = 2, a = 1)` を並べ替えます。巨大な rewrite フェーズはありません。メソッド・`apply`・`copy`・コンストラクタ・オーバーロードのある呼び出しのすべてで並べ替え、省略されたデフォルト引数はその場で埋めます（通常のメソッドは `{method}$default$n` ゲッター経由、コンストラクタは呼び出し側でデフォルト式を型付けします）。extractor パターンでも case class なら並べ替えます。パーサは `x = e` を一律に代入としてパースし、**引数位置のそれを名前付き引数として扱うのは typer** です（nsc と同じ作り）。
- **try**: Code 属性に例外テーブルと StackMapTable を出します。
- **ラムダ**: `FunctionN` を実装する合成クラス（`Main$$$anonfun$0` など）です。SAM 期待位置ではその Java インタフェース（`Runnable` / `Comparator` / `java.util.function.Function`）を実装します。`PartialFunction` 期待位置の `{ case }` は `scala/PartialFunction` を実装し、`isDefinedAt` / `apply` / `applyOrElse` を出します。invokedynamic / LambdaMetaFactory は使いません。囲いのメソッドのローカルは `$captured$n` フィールドに、**囲いの `this`** は nsc と同じ `$outer` フィールドに捕まえます。`this` が要るのは、明示的に書かれたとき（`this.f` / `super.f`）だけでなく、**囲いのクラスのメソッドを呼ぶだけ**（`xs.map(a => base(a))`）のときも同じです（`object` のメンバは `MODULE$` 経由なので要りません）。
- **フェーズ**: nsc の mixin などの独立パスはありません。**uncurry**、**lambda-lift**（ネスト def）、erasure、ラムダのクロージャ変換はあります。
- **sealed**: 非網羅 match は scalac と同様 warning です。`-Xfatal-warnings` でエラーになります。
- **AnyVal**: scalac は値クラスのクラスファイルと拡張メソッドの両方を出します。scala-rs も同じで、`new C(x)` は underlying に消え、呼び出しは `$extension` 静的メソッドです。参照が要る位置（`Any` / universal trait / 型引数 / 配列要素）では nsc と同じく `new C(u)` で box し、`equals` / `hashCode` も underlying から合成します。違いは `$extension` の本体の置き場所で、nsc はコンパニオン `C$` に置いてクラス側をフォワーダにしますが、scala-rs はクラス側に直接出します。
- **Predef / StringOps**: 私有では `assert` / `require` / `???` / `->`（`Tuple2` 直結）/ `identity` / `locally` / `implicitly` / `any2stringadd` と String の `length`/`toInt`/`isEmpty`。library では `Predef$.println/assert/require/???/identity/locally/implicitly`、`any2stringadd.$plus$extension`、`ArrowAssoc.$minus$greater$extension`、`intWrapper` → `RichInt.abs$extension` / `max$extension` / `to$extension` / `until$extension`、`longWrapper` → `RichLong.abs$extension` / `max$extension` / `to` / `until`（`NumericRange$.inclusive` / `apply` + `Numeric$LongIsIntegral$`）、`doubleWrapper` → `RichDouble.abs$extension` / `max$extension`、`floatWrapper` → `RichFloat.abs$extension` / `max$extension`、`charWrapper` → `RichChar.isDigit$extension` / `intValue$extension`（`.toInt`）/ `to` / `until`（`NumericRange$.inclusive` / `apply` + `Numeric$CharIsIntegral$`）、`byteWrapper` → `RichByte.abs$extension` / `max$extension` / `to` / `until`（`NumericRange$.inclusive` / `apply` + `Numeric$ByteIsIntegral$`）、`shortWrapper` → `RichShort.max$extension` / `to` / `until`（`NumericRange$.inclusive` / `apply` + `Numeric$ShortIsIntegral$`）、`booleanWrapper` → `RichBoolean.compare(Object)`、`augmentString` → `StringOps.toInt$extension` / `size$extension`（`.length`）/ `$times$extension` / `take$extension` / `drop$extension` / `stripPrefix$extension` / `split$extension` / `stripSuffix$extension` / `padTo$extension`（`Int, Char`）/ `linesIterator$extension` / `toIntOption$extension` / `stripMargin$extension` / `lines$extension` / `capitalize$extension` / `reverse$extension` / `slice$extension` / `takeRight$extension` / `dropRight$extension` / `contains$extension`（`.isEmpty` / `.toUpperCase` / `.toLowerCase` は StringOps 経由で `String` にインライン。`startsWith` / `endsWith` / `indexOf` は nsc どおり `java.lang.String`。`head$extension` / `last$extension` / `stripLineEnd$extension` / `replaceAllLiterally$extension` / `tail$extension` / `init$extension` / `distinct$extension` / `mkString$extension` / `filter$extension` / `reverseIterator$extension`）。`intArrayOps` → `ArrayOps.head$extension` / `tail$extension` / `foreach$extension(Object,Function1)V` / `map$extension(Object,Function1,ClassTag)Object`。`longArrayOps` → 同じ `head` / `foreach`（`[J]`）。`refArrayOps` → 参照配列の `map`。**`StringOps` / `ArrayOps` / `RichInt` / `RichLong` / `RichDouble` / `RichFloat` / `RichChar` / `RichByte` / `RichShort` / `RichBoolean` / `ArrayBuffer` / `ListBuffer` / `StringBuilder` / `HashMap` / `HashSet` / `LinkedHashMap` / `LinkedHashSet` / `ArrayDeque` / `NumericRange` classfile は出していません。**
- **unapplySeq**: `List` / `Seq` / `Vector` / `IndexedSeq` / `Array` とユーザー定義 extractor、`_*`、名前付き case class パターン。library リンク時の `List.unapplySeq` は `SeqOps` 戻りで、`List` 以外は nsc と同じく `UnapplySeqWrapper` の `$extension` で添字読みします。`Seq` / `Array` のシーケンスパターンは jar リンク時のみ（`--no-scala-library` では診断する）。

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

**jar のクラスを pickle から読む**テストは `crates/cli/tests/jarpickle.rs`
（fixture 接頭辞 `jarpk`）です。

- `jarpk_fixture_dual_run`: `jarpk.scala`（`Functor[F[_]]` / `Monadic[F[_]]` と
  `Option` / `List` / 自作 `Ident` の 3 インスタンス）を実 scala-library に対して
  コンパイルし、実 scalac 2.13.16 の出力（`tests/fixtures/expected/jarpk.txt`）と
  **完全一致**することを見る。
- `jarpk_bad_is_still_rejected`: `Monadic2[Int]` の kind エラーと
  `F.pure(1): F[String]` の型不一致。nsc 2.13.16 も両方拒否する。
- `a_higher_kinded_trait_survives_a_jar_round_trip`: 高階トレイトを含むライブラリを
  コンパイル → `jar cf` で jar に固める → **jar しか見えない**プログラムを
  コンパイルして実行する。渡るのは `ScalaSignature` だけ。`jar` が無ければスキップ。
- `a_higher_kinded_type_class_from_a_real_jar_typechecks` /
  `a_proper_type_is_still_rejected_where_a_real_jar_wants_a_constructor`:
  ローカルの Coursier キャッシュに cats-core / cats-kernel があれば、
  実物の `cats.Monad` に対して `F.pure` / `F.flatMap` / `F.map` が通ることと、
  `Monad[Int]` が kind エラーになることを見る。無ければスキップ（何もダウンロードしない）。

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

`agent/catsimpl` スライス（ラムダが囲いの `this` を捕まえる、cats の syntax 形の暗黙変換、コンパニオンの implicit スコープ、デフォルト引数を省いた呼び出しの by-name 引数）のフィクスチャは接頭辞 `cats`（`cats_lambda` / `cats_lambda2` / `cats_syntax` / `cats_syntax_bad` / `cats_byname`）で、同じ理由から `crates/cli/tests/catsimpl.rs` に置いています。`cats_lambda.scala` は `List.map` / `flatMap` を使うので library dual-run 専用、`cats_lambda2.scala` は同じ捕捉をライブラリのコレクション抜きで書いてあるので**私有ランタイムと `--scala-library` の両方**で走ります。`cats_syntax.scala` は `implicit def toFlatMapOps[F[_], A](fa: F[A])(implicit F: FlatMap[F])` を自前で書いた 1 ファイル版で、抽象 `F[_]` と具象 `Box` の両方の受け手を通します。`cats_syntax_bad.scala` は、変換のパラメータを「1 引数に適用された任意の型」まで広げたことで**witness の無い型にまで変換が挿さらない**こと（scalac と同じ `value flatMap is not a member of Bag[Int]`）を固定します。`a_higher_kinded_companion_implicit_crosses_a_jar` はライブラリを自分でコンパイルして jar に詰め、`ScalaSignature` だけを通して `Async[Box]` ＝ `Box.asyncForBox` が見つかることと、**witness の無い型は依然として hard error**（`could not find implicit value of type Async[Crate]`）であることを両方見ます。

`agent/catsyntax` スライス（cats の syntax による拡張メソッドが本物の cats に届くまで）のフィクスチャは接頭辞 `csyn`（`csyn_ops` / `csyn_ops_bad`）で、同じ理由から `crates/cli/tests/catsyntax.rs` に置いています。`csyn_ops.scala` は cats の `Ops[F[_], A]` と同じ形の受け手に `map` / `flatMap` / `foreach` を呼ぶもので、**暗黙変換を一切使わずに**（`new Ops[Box, Int](b)`）ラムダの引数型が第 1 型引数の `Box` になっていたずれを固定します。私有ランタイムと `--scala-library` の両方で走ります。`csyn_ops_bad.scala` は、ラムダに宣言どおりの引数型を与えても witness の無い呼び出しは通らないこと（`could not find implicit value of type FlatMap[Bag]`）を固定します。`a_simulacrum_style_syntax_layer_crosses_a_jar` は **実 scalac** で小さな cats（`Ops[F, A] { type TypeClassType = FlatMap[F] }` という refinement 結果型、パッケージオブジェクトの入れ子 `object all`、その `all` を `InnerClasses` に載せるだけの無関係なクラス）をコンパイルして jar に詰め、`ScalaSignature` だけを通して `b.flatMap(…)` と `b >> …` が解決し、`java -Xverify:all` で走ることを見ます。自前の pickle ライタは `REFINEDtpe` を出さないので、この fixture は scalac が書いたものでなければ意味がありません（scalac が無い環境では skip します）。同じテストで、witness の無い `Crate` には変換が挿さらないこと（`value flatMap is not a member of Crate[Int]`）も見ます。

`agent/genrep` スライス（slick が `.fm` テンプレートから生成する 7 本を通すための穴: import を見ないクラス型パラメータ境界、型パラメータ付き `implicit class`、`TupleN extends Product`、継承したオーバーロードの受け手での型、引数リストのタプル化、`Tuple` で始まるだけのクラス名、可変長引数コンストラクタ、ワイルドカード型引数と反変、`package p { … }` の後ろのトップレベル定義）のフィクスチャは接頭辞 `genrep`（`genrep` / `genrep_bound_bad` / `genrep_tuple_bad` / `genrep_product_bad`）で、同じ理由から `crates/cli/tests/genrep.rs` に置いています。`genrep.scala` は `--scala-library` dual-run に加えて real scalac 2.13.16 との実行結果 diff（`real_scalac_dual_run_genrep`）でも見ます。異常系は 3 本: `genrep_bound_bad` は namer が黙るようにした境界でも**存在しない型はきちんと診断される**こと、`genrep_tuple_bad` はタプル化が**間違った呼び出しを通さない**こと、`genrep_product_bad` は `--no-scala-library` で `Product` の辺を張らない（私有ランタイムに裏付けが無い）ことを固定します。

`agent/ctoraccessor` スライス（コンストラクタ引数のアクセサ、`FunctionN.tupled` / `curried` / `Function.untupled`、`Builder` の `+=` / `++=`）のフィクスチャは接頭辞 `ctacc`（`ctacc` / `ctacc_fn` / `ctacc_builder` / `ctacc_plain_bad`）で、同じ理由から `crates/cli/tests/ctoraccessor.rs` に置いています。`ctacc.scala` は**私有ランタイムと `--scala-library` の両方**で `java -Xverify:all` の下に走らせ、`real_scalac_dual_run_ctacc` で real scalac 2.13.16 の出力とも比較します（`expected/ctacc.txt` は scalac の出力そのもの）。`ctacc_case_class_params_get_public_accessors` は `javap -p -s` でアクセサのディスクリプタ（`ConstRep.value()Object` / `NumRep.n()I` / `IntBox.unwrap` の `()I` ＋ `()Object` ブリッジ / `StringBox.label` の `()String` ＋ `()Object` ブリッジ）と、**第 2 引数リストがアクセサにならない**こと（`Multi.extra`）を固定します。`ctacc_fn.scala` と `ctacc_builder.scala` は library ABI 限定（`scala/FunctionN` の default メソッド、`scala/Function$`、`Growable`）なので library dual-run と real scalac dual-run のみで、`fixtures_ctacc_fn_without_library_is_error` / `fixtures_ctacc_builder_without_library_is_error` が `--no-scala-library` で**きちんと診断される**ことを見ます。`ctacc_plain_bad.scala` は `val` の無いコンストラクタ引数が外から読めないままであること（case class の第 1 引数リストだけがアクセサになる）を固定します。
オーバーロード集合が別のクラスの読み込みで消える回帰のフィクスチャは接頭辞 `oshadow`（`oshadow` / `oshadow_java_first` / `oshadow_java_last` / `oshadow_bad`）で、同じ理由から `crates/cli/tests/overloadshadow.rs` に置いています。`oshadow.scala` は `--scala-library` dual-run に加えて real scalac 2.13.16 の実行結果とも直接比較します（`oshadow_matches_scalac`）。`oshadow_java_first.scala` と `oshadow_java_last.scala` は `java.math.BigDecimal` の位置だけを入れ替えた同じプログラムで、`oshadow_order_independent` が両方通ることと stdout が一致することを固定します。`oshadow_bad.scala` は `BigDecimal(Some(1))`（real scalac も拒否）が `no matching overload` になり、しかも**候補一覧が丸ごと**出る（`(String)BigDecimal` を含む）ことを見ます。`oshadow_without_library_is_error` は `--no-scala-library` で `not found: value BigDecimal` の診断が残ることを見ます。
`agent/parentimpl` スライス（親コンストラクタの implicit 節・デフォルト引数の補完）のフィクスチャは接頭辞 `pimpl`（`pimpl` / `pimpl_bad`）で、同じ理由から `crates/cli/tests/parentimpl.rs` に置いています。`pimpl.scala` は slick の `ConstColumn` 形（`class ConstColumn[T : TT] extends TypedRep[T]`）、明示節＋2 引数の implicit 節、全部デフォルト／末尾だけデフォルト、デフォルト節＋implicit 節、匿名クラスの親、引数無しの `new` を 1 本にまとめ、**私有ランタイムと `--scala-library` の両方**で `java -Xverify:all` の下に走らせます。`real_scalac_dual_run_pimpl` は real scalac 2.13.16 でも同じソースを走らせて stdout が一致することを見ます（`expected/pimpl.txt` は scalac の出力そのもの）。`pimpl_late_a.scala` / `pimpl_late_z.scala` は**子を親より先にコンパイル**して、親の context bound の evidence がシグネチャパス時点で未生成でも埋まる（＝ファイル順に依存しない）ことを見ます。`pimpl_bad.scala` は witness の無い親 implicit 節が**黙って通らない**ことを固定し、`pimpl_bad_reports_the_extends_clause_once` で診断が `extends` の行に 1 件だけ出る（3 パス分に増えない）ことも見ています。

`agent/patbind` スライス（`x @ Pat` の束縛型、`null` パターン、`==` の null）のフィクスチャは接頭辞 `pb`
（`pb_bind` / `pb_null` / `pb_lit` / `pb_eqnull` / `pb_nullseq` / `pb_null_bad`）で、コンフリクト回避のため
`crates/cli/tests/patbind.rs` に置いています。どれも**型検査は通って実行時に落ちていた**バグなので、
テストは 3 通り（私有ランタイム / `--scala-library` / 実 scalac 2.13.16）走らせて
stdout が全部一致することを見ます。`expected/pb_*.txt` は実 scalac の出力そのものです。
`pb_bind.scala` は入れ子の `@`、型パターンとの併用（`case n @ (_: N)`）、ガード付き、抽出子、
タプル、`catch` の中、`Any` からプリミティブへの絞り込みを 1 本にまとめてあります。
`pb_null.scala` はリテラル / 安定識別子 / 型パターン / `AnyRef` / 抽出子 / case class /
タプル / `case _` の 8 種類に `null` を通します。`pb_lit.scala` は `Long` / `Float` / `Double` /
`Char` のスクルーティニと、参照スクルーティニに対する box した定数です。`pb_eqnull.scala` は
`==` / `!=` の側で、`x == null` / `null == x` の参照テストと、私有ランタイムでの
null 受け手（`(null: String) == "a"` が NPE になっていた）を見ます。`pb_nullseq.scala` は
`Seq` / `::` / `Array[Int]` / `()` で、`SeqFactory$UnapplySeqWrapper$` が要るので
library dual-run 専用です。`pb_null_bad.scala` は `(x: Int) match { case null => … }` が
nsc と同じく `type mismatch; found: Null(null)` になることを固定します。

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
| `pb_bind.scala`（`crates/cli/tests/patbind.rs`、私有ランタイム・library・実 scalac の 3 通り） | `x @ Pat` はパターン自身の型で束縛する。`case n @ N(v, _) => n.copy(...)` が `VerifyError`（`n` が親の型のまま）だった。入れ子 `@`、`case n @ (_: N)`、ガード、抽出子、タプル、`catch`、`Any` → プリミティブの絞り込み | `N(11,L)` … `caught boom` |
| `pb_null.scala`（`crates/cli/tests/patbind.rs`、3 通り） | `null` を 8 種類のパターンに通す。`case null` は `ifnonnull`（以前は `null.equals` で NPE）、定数は左オペランド、型パターン / case class / タプルは `instanceof`、抽出子は `ifnull` で先に落とす | `null` `A` `one` … `A` |
| `pb_lit.scala`（`crates/cli/tests/patbind.rs`、3 通り） | `Long` / `Float` / `Double` / `Char` の定数パターン（以前は両オペランドを `pop` して無条件にマッチ）と、参照スクルーティニに対する box した定数 | `one` `o` `1.5` … `o` |
| `pb_eqnull.scala`（`crates/cli/tests/patbind.rs`、3 通り） | `==` / `!=` の null。`x == null` / `null == x` は `ifnonnull` / `ifnull` の 1 命令（以前は `x.equals(null)` で NPE）。私有ランタイムの一般の `x == y` は `if (x == null) y == null else x.equals(y)`（`BoxesRunTime` が無いため） | `true` `false` … `o` |
| `pb_nullseq.scala`（`crates/cli/tests/patbind.rs`、library dual-run のみ） | `Seq(a, b)` / `a :: b :: Nil` / `case a: Array[Int]` / `case ()` に `null` と実値を通す。`SeqFactory$UnapplySeqWrapper$` が要るので jar 限定 | `o` `seq 1 2` … `o` |
| `cats_lambda.scala`（`crates/cli/tests/catsimpl.rs`、library dual-run のみ） | `agent/catsimpl` スライス: ラムダが囲いの `this` を捕まえる（trait のデフォルトメソッド内、クラスの暗黙 `this`、明示 `this.`、フィールド読み、入れ子ラムダ、`object` のメンバは `MODULE$` 経由で `this` が要らないこと）。`List.map` / `flatMap` を使うので library 限定 | `List(2, 4, 6)` … `List(3, 6, 9)` |
| `cats_lambda2.scala`（`crates/cli/tests/catsimpl.rs`、私有ランタイム・library dual-run） | 同じ `$outer` 捕捉をライブラリのコレクション抜きで書いたもの。無ければ `M2$$anonfun$0 cannot be cast to M2` で落ちる（型検査は通る） | `10` `10` `6` `6` `105` `7` `15` |
| `cats_syntax.scala`（`crates/cli/tests/catsimpl.rs`、library dual-run） | cats の syntax 形の暗黙変換 `implicit def toFlatMapOps[F[_], A](fa: F[A])(implicit F: FlatMap[F])`: 受け手の**型構築子**から `F` を解く（以前は `AnyRef`）、変換自身の implicit 節を適用する（以前は落として descriptor より 1 引数少ない呼び出しになり `VerifyError`）、コンパニオンの `implicit def flatMapForBox` | `3` `41` `14` |
| `cats_syntax_bad.scala`（`crates/cli/tests/catsimpl.rs`、異常系） | witness の無い型（`Bag`）には変換が挿さらず、scalac と同じ `value flatMap is not a member of Bag[Int]` のまま | （コンパイルエラー） |
| `cats_byname.scala`（`crates/cli/tests/catsimpl.rs`、library dual-run） | デフォルト引数を省いた呼び出しは 2 度型付けされる（`name$default$n` ゲッターが先行パラメータを取るため）。2 度目に by-name 引数は既に `Function0` の thunk になっており、`() => <notype>` として何にも一致しなかった（slick の `copy(where = w2.orElse(where), …)`） | `Comp(1,Some(1),Some(2),None)` `Some(7) None` ×3 |
| `csyn_ops.scala`（`crates/cli/tests/catsyntax.rs`、私有ランタイム・library dual-run） | `agent/catsyntax` スライス: 高階クラスの第 1 型引数は「要素」ではない。`Ops[F[_], A]` の `map` / `flatMap` / `foreach` でラムダの引数型が `Box` になっていた（暗黙変換抜き、`new Ops[Box, Int](b)` で再現）。抽象 `F[_]` の受け手も通す | `4` `6` `103` `40` |
| `csyn_ops_bad.scala`（`crates/cli/tests/catsyntax.rs`、異常系） | ラムダに宣言どおりの引数型を与えても、`FlatMap[Bag]` の witness が無い呼び出しは scalac と同じく通らない | （コンパイルエラー） |
| `ctacc.scala`（`crates/cli/tests/ctoraccessor.rs`、私有ランタイム・library dual-run・real scalac dual-run） | `agent/ctoraccessor` スライス: コンストラクタ引数が public アクセサになり親の抽象メンバーを実装する（`case class ConstRep[T](value: T) extends Rep[T]`、`case class NumRep(n: Int)`、`()Object` へのブリッジが要る `IntBox` / `StringBox`、`class Person(val name: String, …)`、`class Cell(var c: Int)` の getter/setter、第 2 引数リストがアクセサにならない `Multi`） | `42` `hi` `7` `5` `tag` `bob` `3` `11` `1` `x` `42` |
| `ctacc_fn.scala`（`crates/cli/tests/ctoraccessor.rs`、library dual-run と real scalac dual-run） | `FunctionN.tupled` / `curried`（arity 2 / 3 / 5 / 22）と `scala.Function.untupled`、引数リストを持たないメソッドの結果を直接呼ぶ（`def adder: (Int, Int) => Int; adder(7, 8)`）。私有ランタイムには裏付けが無いので `--no-scala-library` では診断のまま | `7` `11` `7` `30` `1x2` `1y3` `4z5` `15` `15` `20` `22` `15` |
| `ctacc_builder.scala`（`crates/cli/tests/ctoraccessor.rs`、library dual-run と real scalac dual-run） | `scala.collection.mutable.Builder` の `+=` / `++=`（`Growable` の default メソッド、`this.type` 返し）を pickle 供給から引く。`--no-scala-library` では `not found: type Builder` | `List(1, 2, 3, 4)` |
| `genrep.scala`（`crates/cli/tests/genrep.rs`、library dual-run と real scalac dual-run） | `agent/genrep` スライス: import を見ないクラス型パラメータ境界（`class Boxed[T <: Rep[_]]`）、型パラメータ付き `implicit class` の合成変換、`new TupleN(…): Product`（jar でしか作られない arity 込み）、`scala.collection.Seq` の一意な `apply`、`Some(a, b)` のタプル化、`Tuple` で始まるだけのクラス名（`TupleOps2`）、`package p { … }` の後ろの `object Main` | `Rep(1)` `(Rep(1),Rep(x))` … `Some((1,x))` |
| `oshadow.scala`（`crates/cli/tests/overloadshadow.rs`、library dual-run のみ） | 別のクラスを読んでも既存のオーバーロード集合が消えないこと: `java.math.BigDecimal` を**前にも後にも**置いた上での `BigDecimal(Int)` / `(Long)` / `(String)` / `(BigInt)` / `(java.math.BigDecimal)`、`Option[BigDecimal].getOrElse` | `2` `3` `4.25` `6` `12.5` `12.5` `-1` `7` `8.75` `9` |
| `oshadow_java_first.scala` / `oshadow_java_last.scala`（`crates/cli/tests/overloadshadow.rs`、library dual-run のみ） | 同じプログラムを `java.math.BigDecimal` の位置だけ入れ替えた 2 本。両方通り、stdout が一致すること（順序依存の回帰テスト） | `1` `2` `3.5` |
| `pimpl.scala`（`crates/cli/tests/parentimpl.rs`） | `agent/parentimpl` スライス: 親コンストラクタの implicit 節・デフォルト引数の補完（`class ConstColumn[T : TT] extends TypedRep[T]`、明示節＋2 引数の implicit 節、context bound の親への受け渡し、全部／末尾だけデフォルト、デフォルト節＋implicit 節、匿名クラスの親、引数無しの `new`）。私有ランタイム・library dual-run・real scalac dual-run の 3 通り | `rep[Int]` `rep[String]` … `anon:Int` `Int` |
| `vcls.scala`（`crates/cli/tests/valclass.rs`） | 値クラス + universal trait（`Meters` / `Name` が `Univ`）、trait 位置と `Any` への代入で `new Meters` に box、`toString` / `isInstanceOf` / `case x: Meters` / `==` / `asInstanceOf`、`}` の次行の `-1`、行末 `+` の継続 | `5m` `5m5m` `<ada><ada>` `5m` `Meters@5` `true` `meters 5` `true` `false` `8m` `5` `-1` `-1` |
| `vcls_nl.scala`（`crates/cli/tests/valclass.rs`） | 改行が文を切る条件: `}` / `if` / `)` / 識別子の直後の `-`、行末演算子の継続、括弧内は継続、文の位置の `if` / `match` | `-1` `-2` `-3` `-4` `-1` `4` `y` `` |
| `vcls_arr.scala`（`crates/cli/tests/valclass.rs`、library dual-run のみ） | `Array[Meters]`（`[LMeters;`、`mkString`、`new Array` + 代入）、`List[Meters]` / `map(_.n)`、`Option[Meters]`、ジェネリックメソッド、case class のフィールド、`Set` | `2` `1` `Meters@1,Meters@2` `7m` `7` … `1` |
| `vcls_hnil.scala`（`crates/cli/tests/valclass.rs`、library dual-run のみ） | `import syntax._` が型名 `HNil` を隠したうえでの `HNil.type`、前方参照、パッケージ修飾 `hl.HNil.type`、型引数位置、ネストした object の `ColumnOption.AutoInc.type` | `0` `2` `0` `0` `1` `1` `PrimaryKey` `AutoInc` `1` |
| `pkgalias.scala`（`crates/cli/tests/pkgalias.rs`、library dual-run のみ） | jar の package object にしかない**型エイリアス**（`scala/package$` の pickle）: `new NoSuchElementException(...)` と `catch`、`Throwable` / `UnsupportedOperationException` / `IllegalArgumentException` / `Exception`、型パラメータ付きの `IterableOnce[Int]` / `Seq[Int]` | `gone` `java.lang.UnsupportedOperationException` `java.lang.IllegalArgumentException` `3` `r` `9` |
| `java_cp.scala` | JDK の Java `.class` から `Math.abs` / `Byte.MAX_VALUE` / `ArrayList.add` を解決して実行 | `3` `127` `true` `1` |
| `ovl2.scala`（`crates/cli/tests/ovl2.rs`、私有ランタイム・library dual-run・real scalac dual-run） | `agent/ovl2` スライス: 継承はオーバーライドではない（`Base.f(Int)` と `Derived.f(String)` が両方候補に残り、erasure ブリッジも出ない）、素のコンストラクタ引数は `private[this]` なので継承されない、`val` が抽象 `def` を実装したら 1 つのメンバ、`String <: CharSequence`、`indexOf(':')` / `indexOf(':', 2)` / `lastIndexOf(':')` | `int:7/str:z` `42` `outer` `named!` `1` `1` `3` `3` `a` `5` |
| `ovl2_lib.scala`（`crates/cli/tests/ovl2.rs`、library dual-run と real scalac dual-run） | オーバーロードされたメソッドの η 展開（`constOp[Long]("min")(math.min)`、`val g: (Double, Double) => Double = math.max`）、`new ArrayBuffer[Int](8)` と `new ArrayBuffer[String]()`、`Instant.parse` / `LocalDate.parse(s, fmt)` / `DateTimeFormatter.parse(s)`。私有ランタイムには裏付けが無いので `--no-scala-library` では診断のまま | `3` `4` `2.5` `1,2` `x` `2020-01-02T03:04:05Z` `2020-01-02` `true` |
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

`agent/reify2` スライス（宣言クラスでの呼び出しと quasiquote の reification）のフィクスチャは接頭辞 `reify`（`reify` / `reify_bad` / `reify_qq` / `reify_qq_bad`）で、コンフリクト回避のため `crates/cli/tests/reify.rs` に置いています。`reify.scala` は 1 コンパイル単位で trait-extends-class のディスパッチ（宣言クラスへの `checkcast` + `invokevirtual` と、トレイト自身の `invokeinterface`）を private ランタイム・library ABI の両方で見るもので、期待出力は実 scalac 2.13.16 の出力です。`reify_qq.scala` は **scala-reflect.jar を `-cp` に置いて**quasiquote を実行し、実 scalac の出力と毎回その場で比較します（`reify_qq_quasiquotes_build_the_same_trees_as_scalac`）。`reify_runtime_universe_builds_a_tree` は `scala.reflect.runtime.universe` 上で `SyntacticTermIdent` / `SyntacticSelectTerm` / `Literal(Constant(42))` を組み立てて**実行**します（以前は `NoSuchMethodError`）。`reify_classpath_trait_is_an_interface_and_inherits` は `-cp` 越しのトレイトのメンバと継承メンバ（以前は `IncompatibleClassChangeError` と `is not a member`）。異常系は `reify_bad.scala`（トレイトにもクラスにも無い名前）と `reify_qq_bad.scala`（reification が落とせない 6 つの形が、どれも形の名前つきで診断されること）。

quasiquote と、その受け皿である reflect ABI の下地は `crates/cli/tests/quasi.rs` にまとめています。正常系 `tests/fixtures/quasi.scala` は `scala_library_dual_run_quasi`（jar リンクで実行し `expected/quasi.txt` と一致）と `real_scalac_dual_run_quasi`（**実 scalac 2.13.16** の stdout・期待値・scala-rs の出力の三者一致）の 2 通りで回し、package object のメンバ（`scala.math.Pi` / `abs` / `max`）、`import <値>._`、引数なし `def` の結果に対する `apply` 挿入（`Literal(1)` = `Literal.apply(1)`）、そして**ユーザ定義の `q` 補間子が quasiquote に横取りされないこと**を実行結果まで固定します。異常系 `quasi_bad.scala` は `fixtures_quasi_bad_is_error` が `q` / `tq` / `pq` / `cq` の 4 種すべてに診断が出ること、`q""` は `unimplemented syntax: quasiquote q"..." (empty quasiquote)` になることを見ます。`quasiquote_is_not_reported_as_a_stringcontext_member` は、以前の**誤った**診断 `value q is not a member of StringContext` が戻らないことを固定します。

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

| `mism2.scala` | 型引数が解けないまま残る／宣言した結果型が上書きされる一群の修正（library dual-run のみ）：後続ユニットのメンバを参照するデフォルト引数、型パラメータが 3 つある型の `map` の結果型、ラムダの結果から解くメソッド型パラメータ、引数リスト無しの `RepShape[L, M, U]`、期待型から決まる `Coll.empty`、タプルや可変長引数の中の関数リテラル、package object の implicit 節（`classTag[Short]`）、引数を取らない `def` の値位置での適用、ブロック内のローカル `def` の前方参照 | `hi later` `Some(7)` `rep` `0` `5` `42` `short` |

| `reify.scala` / `reify_bad.scala`（`crates/cli/tests/reify.rs`） | クラスを継承したトレイト越しのメンバ呼び出し: 宣言クラスの `checkcast` + `invokevirtual` と、トレイト自身の `invokeinterface`。異常系はどちらにも無い名前が黙って通らないこと | `gear` `gear/gear` `6` `gear` `3` |
| `reify_qq.scala` / `reify_qq_bad.scala`（`crates/cli/tests/reify.rs`、scala-reflect.jar が要る） | quasiquote の reification（実 scalac 2.13.16 と dual-run）: リテラル / 名前 / 選択 / 適用（カリー化含む）/ `$x` 穴 / `..$xs` 穴 / 引数ゼロ。異常系は落とせない 6 形が形の名前つきで診断されること | `1` `greet` `true` `"hi"` `a.b.c` `f(1)` `a.b(1)(2)` `g(x)` `h(x, 2)` `x.size` `k(p, q)` `k()` |
| `quasi.scala` | quasiquote の下地（実 scalac 2.13.16 と dual-run）：jar の package object のメンバ（`scala.math.Pi` / `abs` / `max`）、`import <値>._` とその書き戻し、引数なし `def` の結果に対する `apply` 挿入（`Literal(1)` = `Literal.apply(1)`）、ユーザ定義 `q` 補間子が横取りされないこと | `3.141592653589793` `7` `9` `<1>` `<x>` `small` `<via-path>` `a$1b$2c` `user-q:a\|b` `user-tq:c` |

`mism2.scala` は `crates/cli/tests/mismatch2.rs` から回します。同ファイルには最小形の
受理テスト（`a_default_argument_may_name_a_later_units_member` /
`map_keeps_a_result_type_with_more_than_one_argument` /
`a_lambdas_result_solves_a_nested_type_parameter` /
`a_parameterless_module_apply_is_a_value` /
`empty_takes_its_type_arguments_from_the_expected_type` /
`a_function_literal_gets_its_parameter_type_from_the_expected_type` /
`a_package_objects_implicit_clause_survives` /
`a_parameterless_method_is_applied_where_a_function_is_expected` /
`a_constructor_argument_is_solved_against_the_parameter_it_fills` /
`a_local_def_may_be_called_before_it_is_written`）も置いてあります。
逆に、`map` が宣言どおりの結果型で検査される（コレクションの近道が結果型を
作り変えない）ことは `mism2_bad.scala`（`Act[Int, …]` を `Act[String, …]` に渡す）で
固定しています。

| `mism3.scala` | 抽象型メンバとエイリアス、どの引数も決められない型パラメータ、`this.type`、引数ではないブロック、内側の無名クラスからの protected アクセス（library dual-run のみ）：線形化で「継承した抽象宣言」より「具象エイリアス」が勝つこと、エイリアスの右辺がプレフィックス側の型引数で読めること、パラメータ型に出てこない型パラメータが下界（反変なら上界）に確定すること、`new C with T { … }` の次行のブロックが別の文であること、`this.type` が受け手の型引数を保つこと、レシーバが持ち越した未確定変数をその呼び出しの引数が決めること、self エイリアスが囲いのインスタンスを指すこと | `n` `4` `1` `List(x, y)` `f2` `List(a, b)` |

`mism3.scala` は `crates/cli/tests/mismatch3.rs` から回します。同ファイルには最小形の
受理テスト（`an_alias_overrides_the_abstract_member_it_inherits_twice` /
`an_alias_type_member_is_read_through_its_prefix` /
`a_parameter_no_argument_mentions_is_instantiated_to_its_bound` /
`a_block_after_an_anonymous_class_is_not_an_argument` /
`this_type_keeps_the_receivers_arguments` /
`protected_access_counts_the_enclosing_class` /
`a_classpath_pickle_carries_kinds_and_type_arguments`）も置いてあります。最後の 1 本は
**2 段コンパイル**（ライブラリを別ディレクトリに出してから `-cp` で使う）で、
`ScalaSignature` pickle 越しに高階型パラメータの kind と型引数が渡ることを見ます。
逆に、緩めた規則が診断を飲み込まないことは `mism3_bad.scala`
（定義側の外から protected メンバを別インスタンス経由で触る／`this.type` を
別の型引数の同名クラスに渡す）で固定しています。どちらも実 scalac 2.13.16 も拒否します。

| `mism4.scala` | シグネチャパスより前に完了した型エイリアス、適用された抽象型メンバへの compound の適合、`Map` が `K => V` であること、`map` が受け手のコレクションを保つこと、型引数がまだ決まっていないスクルーティニへの安定識別子パターン、親が宣言した `type Self >: this.type` への `this`（library dual-run のみ） | `schema.create` `2` `10` `Vector(1, 2, 3)` `List(1, 2)` `Vector(1, 2)` `Vector(2, 4, 6)` `1` `4` `0` `leaf` `0` `up` `other` |

`mism4.scala` は `crates/cli/tests/mismatch4.rs` から回します。同ファイルには最小形の
受理テスト（`an_alias_completed_early_still_sees_the_units_imports` /
`a_compound_conforms_to_an_applied_abstract_member` /
`a_map_is_a_function_and_so_is_a_function_class` /
`map_keeps_the_receivers_own_collection` /
`this_conforms_to_a_self_member_declared_by_a_parent` /
`a_stable_id_pattern_may_meet_an_abstract_scrutinee`）も置いてあります。
逆に、緩めた規則が診断を飲み込まないことは `mism4_bad.scala`
（解決するようになったエイリアスの型引数が合わない／抽象型メンバの上界を
親より広く上書きする／`Map[String, Int]` を `Int => Int` に渡す／
`this` ではない別の `Node` を `a.Self` に渡す／
`Seq.map` の結果を `IndexedSeq` に渡す）で固定しています。実 scalac 2.13.16 も
すべて拒否します（nsc は typer で止まるので refchecks 側の 1 件は出しません）。

| `mism5.scala` | 関数型を親に持つトレイトの SAM 変換、呼び出し側の型パラメータを解にする 2 回目の推論、`extends Base(s)` と `new C` が省略した型引数、パラメータのクラスへ引数を揃える単一化、implicit だけの引数節を期待型で埋めること、注釈付きの型への `.apply` 挿入、同じ要素型の変換が受け手のコレクションを返すこと、ファクトリの要素型を期待型で広げること（library dual-run のみ） | `true` `false` `k` `3` `unit` `prod1` `tmunit` `0` `x2` `Vector(1, 3)` `Vector(1, 2, 3, 4)` `Vector(1, 2)` `Vector(3, 2, 1)` `Vector(1, 2, 3, 5)` `Vector(9, 2, 3)` `Vector(3, 2, 1)` `Set(2)` `Vector(1, 2, 3)` `Set(anon)` `Map(anon -> 1)` |

`mism5.scala` は `crates/cli/tests/mismatch5.rs` から回します。同ファイルには最小形の
受理テスト（`a_trait_that_extends_a_function_type_is_a_sam` /
`a_callees_parameter_may_be_solved_to_the_callers` /
`a_parent_gets_its_type_arguments_from_the_ctor_args` /
`a_new_gets_its_type_arguments_from_a_base_expected_type` /
`an_argument_is_lined_up_with_the_parameters_class` /
`an_implicit_only_clause_is_filled_from_the_expected_type` /
`apply_is_inserted_through_an_annotated_type` /
`a_transformation_keeps_the_receivers_own_collection` /
`a_factorys_element_type_is_widened_by_the_expected_type`）も置いてあります。
逆に、緩めた規則が診断を飲み込まないことは `mism5_bad.scala`
（抽象メソッドが 2 つあるトレイトは SAM ではない／親の型引数がどう推論しても
合わない引数／期待型が親クラスのインスタンスですらない `new`／`Seq.filter` の
結果を `Vector` に渡す／`Set[String]` に `Int` を入れる／`apply` を持たない型を
`@unchecked` 越しに関数として呼ぶ）で固定しています。実 scalac 2.13.16 も
すべて拒否します。

| `mism6.scala` | `match` / `if` の合流型を式の静的型にすること、`_: T` の部分パターンを参照のまま束ねること、明示型ラムダの本体を期待結果型に adapt すること、for 内包の末尾の値定義、`withFilter(…).map(f)` の要素型、`try` の lub と結果スロットへの箱詰め（**私有ランタイムと library の両モード**、`java -Xverify:all`） | `3` `-1` `true` `7` `-1` `5` `-1` `9` `1` `x` `4` `n1;n2;` `2;3;` `true` `one` `7` `5` `6` |

`mism6.scala` は `crates/cli/tests/mismatch6.rs` の
`mism6_fixture_runs_in_both_modes` から**両モードで**回し、どちらの stdout も
**実 scalac 2.13.16 の出力そのもの**（`tests/fixtures/expected/mism6.txt`）と
突き合わせます。同ファイルには最小形の受理テスト
（`an_annotated_lambdas_body_is_adapted_to_the_expected_result` /
`a_map_is_the_function_it_declares` /
`with_filter_carries_a_type_constructor` /
`a_for_comprehensions_value_definition_is_not_a_generator` /
`a_try_is_the_lub_of_its_body_and_its_handlers` /
`the_mutable_collections_reach_indexed_seq` /
`patch_keeps_the_receivers_own_collection` /
`a_branchs_merge_type_is_the_expressions_own` /
`a_type_test_sub_pattern_keeps_the_erased_reference`）も置いてあります。
逆に、緩めた規則が診断を飲み込まないことは `mism6_bad.scala`
（関数**値**を別の結果型に adapt しない／`Map` をキーの型が違う `map` に渡す／
`ArrayBuffer` を `Vector` に渡す／`Success[String]` を `Try[Int]` に渡す）で
固定しています。実 scalac 2.13.16 も 4 件すべて拒否します。for 内包の値定義に
続くガードは `mism6_forval_bad.scala` で、**nsc は通すがこちらは診断する**ことを
固定しています（`a_guard_after_a_for_value_definition_is_diagnosed`）。

| `tyvar.scala` | 未確定の型変数（nsc の undetermined type variables）。引数位置の `Map.empty` / `Vector.empty` / `Set.empty` / `List.empty` / `Nil` / `Seq.empty`、空の `apply`（`Map()` / `Vector()` / `List()`）、入れ子の呼び出しから漏れる変数（`take(id(Map.empty))`）、期待型が結果型の変数を決める形（`val l: List[Map[String, Int]] = f(Map.empty)`）、可変長引数・by-name・デフォルト引数の位置、複数引数・複数節、オーバーロード選択、コンストラクタ引数、そして逆向きの「呼び先自身の未確定な型パラメータ」（`xs.collect { case … }`）（library dual-run のみ） | `0`×9 `List(Map())` `2` `0`×3 `1` `2` `0` `0` `List(2, 4)` `List(2, 3, 4, 5)` `Some(6)` |

`tyvar.scala` は `crates/cli/tests/tyvar.rs` から回します。同ファイルには最小形の
受理テスト（`a_polymorphic_reference_in_argument_position_is_solved_by_the_parameter` /
`an_empty_apply_is_solved_by_the_parameter` /
`a_variable_leaks_out_of_a_nested_call` /
`the_expected_type_solves_a_variable_that_reached_the_result` /
`overload_selection_sees_through_an_undetermined_variable` /
`a_constructor_argument_is_solved_by_its_parameter` /
`a_callees_open_type_parameter_is_solved_from_the_argument`）と、
**解けない変数を黙って埋めない**ことの拒否テスト
（`an_enclosing_methods_type_parameter_is_not_a_variable` /
`a_recursive_call_does_not_solve_its_own_type_parameter` /
`a_variable_the_parameter_cannot_pin_stays_an_error` /
`solving_a_callees_type_parameter_does_not_widen_the_result`）を置いてあります。
まとまった拒否側は `tyvar_unsolved_bad.scala`（5 か所。実 scalac 2.13.16 も
すべて拒否することを確認済み）で固定しています。

### implicit の残件と prelude の穴（`agent/impltail`）

slick に残っていた implicit 関連のエラーを追ったスライスです。フィクスチャは
`tests/fixtures/itail.scala`（正常系。実 scalac 2.13.16 と stdout がバイト一致）と
`tests/fixtures/itail_bad.scala`（異常系）、テストは `crates/cli/tests/impltail.rs` です。

| `itail.scala`（`crates/cli/tests/impltail.rs`、library dual-run） | 一度 implicit を埋めた呼び出しの再型付け（タプル化リトライ）、`Numeric[T] <: Ordering[T]`、implicit 探索だけが決められる型パラメータ、関数値の `apply`、引数位置の残余 implicit 節（`take(Array.empty)`）、可変長引数を持つ case class | `Pair(Lit(1, …))` `Int 42 true` `-1` `a:str0` `b:bool1` `n=2 n=0` `0` `r 6 3` `true` `0` |

最小形の受理／拒否テストは同じファイルにあります
（`an_implicit_filled_call_survives_being_typed_twice` /
`numeric_is_an_ordering` / `a_numeric_type_parameter_is_an_ordering` /
`an_implicit_only_type_parameter_is_solved_by_the_witness` /
`apply_on_a_function_value_is_the_function` /
`a_residual_implicit_clause_is_applied_in_argument_position` /
`the_parameter_decides_which_witness_a_residual_clause_needs` /
`an_implicit_object_is_not_ambiguous_with_itself` /
`a_repeated_case_class_parameter_has_a_sequence_default`）。
`itail_bad.scala` は、証拠が無い残余 implicit 節と、候補がまったく無い
implicit-only 型パラメータの両方に nsc と同じ趣旨の診断が出ることを固定します。

計測は `files=184 errors=833 files_with_errors=102` → `errors=777 files_with_errors=93`。

### シーケンスパターン / `StringOps.map` / 安定識別子パターン（`agent/seqpat`）

`case Seq(a, b)` が使えず、`StringOps.map` が 1 つしか無く、安定識別子パターンが
nsc より厳しかった 3 件を片付けたスライスです。フィクスチャは
`tests/fixtures/seqpat.scala` / `seqpat_map.scala` / `seqpat_ids.scala`
（いずれも実 scalac 2.13.16 と stdout がバイト一致）と、拒否側の
`seqpat_bad.scala` / `seqpat_star_bad.scala` / `seqpat_nolib_bad.scala`。
テストは `crates/cli/tests/seqpat.rs` です。

**1. `unapplySeq` を持つのが `List` のコンパニオンだけだった。**
`Seq` / `Vector` / `IndexedSeq` / `Array` のコンパニオンに
`unapplySeq[A](x: CC[A]): Option[Seq[A]]` を足しました
（`crates/typer/src/prelude_seqpat.rs`）。codegen 側は
`gen_unapply_seq_bind`（`checkcast List` から始まる **List 専用**の head/tail
walk）に加えて `gen_unapply_wrapper_bind` を持ち、`SeqPatShape` で
`scala/collection/SeqFactory$UnapplySeqWrapper$` と
`scala/Array$UnapplySeqWrapper$` を切り替えます。実 scalac の `javap -p -c` と
同じ `lengthCompare$extension` / `apply$extension` / `drop$extension` を呼ぶので、
`Vector` を `Seq` として渡しても、`"abc".map(_.toString)` が返す `ArraySeq` を
`case Seq(a, b, c)` で受けても落ちません。

なお、README が「`case List(a, b, rest @ _*)` は main でも `VerifyError` を出す」
としていた件は、**その後の `41d4bca`（extractor の checkcast）で既に直っていました**。
`seqpat.scala` の `listShape` / `caseElems` で固定してあります。

**1b. ついでに見つかった 2 つの黙って壊れていたもの。**

- **`Any` のスクルーティニ**。`case Seq(a, b)` / `case List(a, b)` /
  `case Array(a, b)` を `Any` に対して書くと、型テストなしで
  `checkcast` / wrapper の extension に入っていました
  （`ClassCastException` / `IllegalArgumentException: Argument is not an
  array`）。scalac と同じく `instanceof`（`Array` は
  `ScalaRunTime.isArray(Object, 1)`）を先に出し、静的型がすでに保証している
  ときだけ省きます。
- **`_: T` の部分パターン**。`case List((s, _: TableNode))` は、要素を束ねる
  前に `checkcast TableNode` を出していたので、**マッチしない値が例外に
  なっていました**（次の case に落ちない）。型アスクリプションは *テスト* で
  あって cast ではないので、`gen_pattern` の `instanceof` に任せます
  （`is_type_test_pat`）。case class のコンストラクタパターン
  （`case Some((s, _: TableNode))`）にも同じ穴がありました。

**2. `StringOps.map` の 2 つのオーバーロード。** 2.13 の `StringOps` は
`map(Char => Char): String` と `map[B](Char => B): IndexedSeq[B]` を持ち、
JVM descriptor は戻り型だけが違います（`javap -s` で確認）。prelude にも
**2 つのシンボル**として持たせるのが正しく（`crates/typer/src/prelude_strmap.rs`）、
1 つに畳むと `value_extension_desc` がシンボルの結果型から descriptor を作るため
`Char => Char` のときにも `IndexedSeq` を返す方を呼んでしまいます。
2 つ並べたときに `ambiguous overload` になっていた原因は、オーバーロード解決の
3 か所でした:

- `is_as_specific_method` が相手の型パラメータを未確定として扱っていなかった。
  `map[B](Char => B)` の `B` を `Char` に決められないと、どちらも
  「相手と同じくらい specific」になります。
- 逆向きに、自分の型パラメータが**剛体**でなかった。`Char => B` の `B` は
  `Char` ではないので、上界（既定は `Any`）に置き換えてから比べます。
- `arg_score` が「パラメータの形が合えば関数型どうしは一致」と採点していた。
  これは結果型がまだ無いラムダのための緩和なので、**両辺が確定している**
  ときだけ本当の適合を要求するようにしました（`Unit` / `Any` パラメータの
  value discarding と、数値拡大は従来どおり）。

さらに nsc の `Infer.pretypeArgs` を入れました。どのオーバーロード候補も同じ
関数パラメータ型を要求するなら、解決の前にラムダを型付けできます。これが無いと
`"abc".map(_.toString)` は `(<notype>) => <notype>` のまま両方に applicable で、
より specific な `Char => Char` 版が誤って勝ちます。

**3. 安定識別子パターンの型検査。** nsc は適合ではなく
**同時に住めること**しか要求しません。開いたクラスどうしは常に住めるので
`case Ids.other =>`（`Other`）を `ST[Int]` のスクルーティニに書けます。
`final` なクラス（`String`、値クラス、配列、object）とプリミティブだけが
排除の根拠で、そこは scalac も `type mismatch` を出します
（`stable_pattern_compatible` / `is_final_like`）。

**おまけ: `final` / `abstract` / `sealed` がパーサに落ちていた。** クラスの
省略可能なコンストラクタ修飾子（`class C private (x: Int)`）を読む
`parse_modifiers` は改行を読み飛ばすので、`class Other` の直後に来る
**次の定義の修飾子を食べていました**。ファイル中 2 つ目以降のクラスの
`final` / `abstract` / `sealed` / `implicit` が全部消えていたということです
（`FinalOther` が final でないので 3 の判定も効きませんでした）。
コンストラクタ修飾子はクラス名と同じ行にあるので、**改行を読み飛ばす前に**
`private` / `protected` / `@` が来ているかだけを見るようにしました。

| `seqpat.scala`（library dual-run） | `Seq` / `List` / `Vector` / `IndexedSeq` / `Array` のシーケンスパターン（固定長・`_*`・入れ子・タプル要素・case class 要素）、`ArraySeq` を `Seq` で受ける形、`Any` スクルーティニ、`_: T` の部分パターン | `empty` `one 1` `two 3` `many 3 2` `xyz\|w` `q` `ab` `a2` `24` `3` `3` `xy\|z` `4` `k7` `5` `abc` `arr 12` `seq 12` `seq 12` `lst 9` `?` `?` `table a` `plain a` `table b` `plain b` `table c` `plain c` |
| `seqpat_map.scala`（library dual-run） | `StringOps.map` の 2 つのオーバーロード（`Char => Char` は `String`、それ以外は `IndexedSeq[B]`） | `Ab` `ABC` `ArraySeq(a, b, c)` `ArraySeq(97, 98, 99)` `a-b` `abc` `3` `false,false,true,true,false` |
| `seqpat_ids.scala`（library + 私有ランタイム dual-run） | 安定識別子パターン（無関係なクラス／trait／`Any` のスクルーティニ）と、クラスの後に続く定義の修飾子 | `st` `?` `tr` `?` `other` `?` `7` `true` `true` |

拒否側は `seqpat_bad.scala`（`final` クラス／`String`／プリミティブが絡む 5 件。
実 scalac 2.13.16 も同じ 5 件を出します）、`seqpat_star_bad.scala`（`_*` が
最後でない）、`seqpat_nolib_bad.scala`（`--no-scala-library` での
`case Array(…)` は診断）です。最小形の受理テストも `seqpat.rs` に置いてあります
（`a_seq_pattern_binds_the_scrutinees_element_type` /
`a_star_pattern_takes_the_extractors_own_container` /
`a_user_unapply_seq_is_untouched` /
`string_ops_map_picks_the_alternative_by_the_literals_result` /
`a_stable_id_pattern_only_has_to_be_inhabitable` /
`modifiers_after_a_class_are_not_swallowed` /
`a_constructor_access_modifier_still_parses`）。

計測は `files=184 errors=620 files_with_errors=87` → **変わらず
`errors=620 files_with_errors=87`**（エラーの多重集合が 1 件も動きません）。
slick 側の `case Seq((s, _: TableNode))`（`JdbcStatementBuilderComponent.scala`
164-165 行）はまだ `found: A required: TermSymbol` のままです。同じ形を単体で
書くと通る（`crates/cli/tests/seqpat.rs` の
`a_seq_pattern_binds_the_scrutinees_element_type`）ので、slick の側は
**同じファイルの別のエラーのカスケード**です。そのすぐ下の
`currentUniqueFrom = from match { … }` には別の（main から続く）穴があり、
下の Remaining に書きました。

### cats の syntax による拡張メソッド（`agent/catsyntax`）

`import cats.syntax.all._` が入れる `fa.flatMap(…)` / `a >> b` / `fa.attempt` が
**本物の cats（cats-core 2.13.0 / cats-effect 3.7.1）で解決するようになりました**。
`agent/catsimpl` が原因まで特定していた refinement の件を含め、5 つの穴があります。
テストは `crates/cli/tests/catsyntax.rs`、fixture の接頭辞は `csyn` です。

1. **高階クラスの第 1 型引数は「要素」ではない**（`agent/catsimpl` が別バグとして
   報告していたもの）。`map` / `flatMap` / `foreach` / `withFilter` / `pipe` /
   `tap` は、ラムダの引数型をレシーバの**第 1 型引数**で置き換えていました。
   `List[A]` では正しく、cats の `Ops[F[_], A]` では誤りです。
   `new Ops[Box, Int](b).flatMap(n => …)` の `n` が `Box` になり、
   `n + 1` が `any2stringadd` に落ちていました（`csyn_ops`。暗黙変換抜きで再現）。
   第 1 型引数の kind arity が 0 のときだけ要素として使います。結果型の側の
   「未確定な `B` を含むなら `Any` に緩める」処理はそのまま残します
   （両方消すと `fa.flatMap(_ => fa)` が `F[Any]` になりました）。

2. **pickle の `REFINEDtpe` を `Type::Refined` に変換する**。simulacrum が生成する
   `toFooOps` の結果型はすべて `Foo.Ops[F, A] { type TypeClassType = Foo[F] }` で、
   `PickleSupply::conv` がこの形を表現できずメンバごと供給されませんでした
   （`unmappable result type Refined { … }`）。`conv_refined` / `conv_refine_decl`
   を足しました。**黙って落とさない**方針は変えていません。親か宣言が 1 つでも
   変換できなければ refinement 全体を declined にし、`SCALA_RS_PICKLE_DEBUG=1` で
   理由（どの親／どの宣言か）を出します。型パラメータを持つ `def` のように
   `RefineDecl` に入らない形は `None` を返します。親 1 つ・宣言なしの
   refinement だけは、失うものが無いのでその親そのものにします。
   受け側も 2 か所要りました。`subst_as_seen_from` の `walk` に `Type::Refined`
   の腕（親をたどらないと `Ops[F, Int]#flatMap` の `A` が素のままになる）と、
   `elem_type` が refinement の親を見ること。
   `cats.FlatMap.Ops` のような**入れ子クラスは所有者の直下宣言だけを探す**ように
   もしました（`find_or_stub_java_class`）。親までたどっていたので、
   `cats/FlatMap$Ops` が `FlatMap` に `Ops` を訊き、線形化の先にいた
   `Functor.Ops` が返ってきていました。

3. **`import o._` は `o` が *持つ* メンバを入れる**（SLS 4.7）。`cats.syntax.all` は
   自分ではほとんど何も宣言せず、`toFlatMapOps` も `catsSyntaxApplicativeId` も
   mixin した約 60 のトレイト側にあります。直下のメンバしか入れていなかったので、
   cats の syntax 層は 1 つもスコープに入っていませんでした。
   親を幅優先でたどって入れます。ただし**同じ拡張が 2 経路で来たときは 1 つ**に
   します（`Typer::drop_inherited_duplicates`）。prelude はライブラリの一部の変換を
   継承先のオブジェクトにも直接置いているので、そのままだと `xs.asJava` が
   「同じメンバを同じ結果型で返す 2 つの変換」で決着せず、
   `scala.jdk.CollectionConverters._` が壊れました。
   codegen 側もレシーバが要ります（`Typer::wildcard_module_for`）。継承した変換を
   裸の名前で出すと `this` を積んで宣言元のトレイトに checkcast するので、
   `Main$ cannot be cast to tinycats.FlatMap$ToFlatMapOps` になります。
   受け手は import したオブジェクトです。

4. **`InnerClasses` は「宣言した入れ子クラス」の表ではない**。
   `cats/effect/kernel/MonadCancel.class` は `cats/syntax/package$all$` を挙げます
   （参照しているから）。これを採用していたので `cats.syntax.all` が
   `MonadCancel` のメンバとして入り、`load_binary_into` は classfile を 1 度しか
   完了させないので、後から来た `import cats.syntax.all._` は
   `value all is not a member of <notype>` になっていました。
   **`import cats.effect.… ` を先に書いた時だけ**起きるので import 順の癖に
   見えていましたが、slick の `BasicBackend.scala` はまさにその順です。
   自分の JVM 名を接頭辞に持つエントリだけ採用します。

5. **変換自身の implicit 節から型引数を解く**（`solve_conv_targs_from_implicits`）。
   `catsSyntaxApplicativeError[F[_], E, A](fa: F[A])(implicit F: ApplicativeError[F, E])`
   の `E` は `F[A]` のどこにも現れないので `AnyRef` に落ち、`fa.attempt` が
   `F[Either[AnyRef, A]]` という「解決はするがどこにも適合しない」型になって
   いました。スコープの証人（`Async[F] <: MonadError[F, Throwable]`）が
   `E = Throwable` を決めます。結果型が言及する型引数のときだけ探索します
   （候補ごとに implicit 探索を回すため）。
   あわせて `search_extension` の先頭でレシーバの implicit スコープを暖めます。
   高階の変換が適用可能かどうかは「自分の implicit 節に証人があるか」で決めており
   （`agent/catsimpl`）、その探索は `&self` なので自分では何も読み込めません。
   `FlatMap[Box]` の証人は `Box` のコンパニオン＝誰も要求しない別 classfile です。

計測は `files=184 errors=537 files_with_errors=80` → **`errors=529
files_with_errors=80`**。生の件数はあまり動きませんが、**このスライスが狙った
エラーの種類は消えています**。`… is not a member of F[…]`（`flatMap` / `>>` /
`attempt` / `map` / `void` / `timeoutTo` / `guarantee` …）は **42 → 8**、
`value all is not a member of <notype>`（項 4）は **2 → 0** です。
残る 8 件（`value flatMap is not a member of F` 4 件と
`value >> is not a member of F` 4 件）は**すべてレシーバが素の `F`** で、
下の Remaining の「jar のメンバの結果型が素の `F` になる」です。
差し引きが 8 件にしかならないのは、拡張メソッドが解決するようになったことで
**その先で止まっていたカスケードが表に出た**ためです（`found: F required: F[Unit]`、
`no matching overload for (Function0[A])F` など。どれも同じ「素の `F`」が原因）。

### Remaining

- **`Unit` を型に持つパラメータのディスクリプタが `(V)` になる**
  （`agent/patbind` で見つけた、パターンとは無関係の別件）。
  `def unit2(x: Unit): String` が `(V)Ljava/lang/String;` になり
  `ClassFormatError: illegal signature` で落ちます。nsc は
  `(Lscala/runtime/BoxedUnit;)` です。erasure 側の話なので触っていません。

- **私有ランタイムに `scala.runtime.BoxedUnit` が無い**（`agent/patbind`）。
  `--no-scala-library` では `Unit` を `Any` に入れると `null` になるので、
  `(x: Any) match { case () => … }` は `null` にも当たります。jar モードは
  nsc と一致します（`pb_nullseq.scala`）。私有ランタイムに `BoxedUnit` を
  足すのが本筋ですが、`Unit` の box 表現全体を変える話になります。

- **for 内包の値定義に続くガード**（`agent/mismatch6` で診断だけ入れた、未実装）。
  `for { m <- ms; q = f(m); if q > 0 } yield q` は nsc では通ります。nsc は
  値を生成子の要素とタプルに組んで**そのストリームを絞り**、後続の列挙子は
  そのタプルをパターンで受けます。こちらの desugaring は値をラムダ本体の
  `val` にするので絞る相手がおらず、
  `unimplemented: a guard after a value definition in a for-comprehension` を
  出します（`tests/fixtures/mism6_forval_bad.scala`）。直すにはタプル化の
  desugaring そのものが要ります。

- **`scala.collection.Seq` を名指しすると `patch` / `filterNot` などが
  受け手のコレクションに絞れなくなる**（`agent/mismatch6` で確認、main でも
  同じ）。`val c: scala.collection.Seq[Int] = …` と書いた**だけ**で jar から
  本物の `scala/collection/Seq` が読まれ、`patch` の宣言結果である生の `Seq`
  に型引数が付かなくなります。「受け手のコレクションを保つ」近道は
  `dargs.is_empty()` で降りるので、`Vector("a").patch(0, Seq("b"), 0)` が
  `found: Seq required: Vector[String]` になります。単体では通ります
  （`crates/cli/tests/mismatch6.rs` の
  `patch_keeps_the_receivers_own_collection`）。

- **`Map.toMap` が実装されていない**（`agent/mismatch6` で確認、main でも同じ）。
  `m.toMap` は `IterableOnceOps.toMap[K, V](implicit ev: A <:< (K, V))` の
  implicit 節が埋まらず、eta 展開されて**関数オブジェクトそのもの**が値に
  なります（`println(m.toMap)` が `Main$$$anonfun$2@…` を出す）。`A` が
  `Char` として届いている（`<:<[Char, Tuple2[K$, V$]]`）ので、
  `lookup_member` が `StringOps` 系のメンバを供給しているのが疑わしいです。
  slick では `columns.map(…).toMap` の形で数件出ます。

- **`Map` に `PartialFunction` の親を張れない**（`agent/mismatch6`）。
  `Map[K, V] <: PartialFunction[K, V]` は 2.13 の事実ですが、
  `prelude_hier.rs` にその辺を足すと継承メンバの走査順が変わり、上の
  `toMap` の `A` が `Tuple2[…]` から `Char` に化けて slick が 526 → 570 に
  悪化します。いまは `Typer::function_view` に事実だけ書いてあるので、
  `val pf: PartialFunction[String, Int] = aMap` はまだ通りません。

- **`scala.collection.immutable.ArraySeq` / `mutable.ArraySeq` を名指しした
  シーケンスパターン**（`case ArraySeq(a, b)`）。`agent/seqpat` で
  `Seq` / `Vector` / `IndexedSeq` / `Array` のコンパニオンには `unapplySeq` を
  足しましたが、`ArraySeq` のコンパニオンは prelude にありません。`ArraySeq` の
  値を `case Seq(a, b)` で受けるのは動きます（実行時に添字で読むので、
  `"abc".map(_.toString)` の戻り値でも落ちません）。足すときは
  `prelude_seqpat.rs` の `SEQ_FACTORY_MODULES` と `gen.rs` の
  `SEQPAT_SEQOPS_MODULES` の**両方**に JVM 名を書きます。
- **`MapOps` / `SetOps` の `-` / `removed` / `incl` / `excl` / `filter` を
  受け手のコレクションに絞れない**（`agent/mismatch5` で原因まで特定、未修正）。
  これらは JVM 上 `Map` / `Set` という**名前のあるクラス**を返すので、typer が
  `TreeMap` に絞っても codegen は Apply の結果型を消去後のシンボルから取り直し、
  `TreeMap` のフィールドへ `Map` を積んで `VerifyError` になります。
  そのため `erases_to_object` で「消去後 `Object` を返すメンバ」だけに
  限定しました。Apply 自身の結果型が erasure を生き残れば外せます。

- **タプル成分への期待型の伝播**（`agent/mismatch5` で試して巻き戻し）。
  `(new Sel, Map(s -> a))` を `(Node, Map[Sym, Int])` に対して型付けると、
  成分の `Map(s -> a)` は期待型なしで型付けられて非変な `Map[AnonSym, Int]` に
  なります。nsc の `protoTypeArgs`（引数を型付ける前に期待型から型引数の
  見込みを立てる）を入れると by-name パラメータが `() => T` のまま渡り、
  slick が 575 → 604 に悪化したので巻き戻しました。by-name を除外した形なら
  通る見込みです。

- **`case Seq(a, b)` が使えない**（`agent/mismatch4` で原因まで特定、未修正）。
  `unapplySeq` を持つのは prelude の `List` だけなので、`case Seq((s, _))` は
  `type_pattern` の「クラスパターン」枝に落ちて要素型が付かず、
  `Some(s)` が `Some[A]`（extractor 自身の型パラメータ）になります。
  prelude に `Seq.unapplySeq` を足すのは簡単ですが、codegen の
  `gen_unapply_seq_bind` は `checkcast scala/collection/immutable/List` から
  始まる **List 専用**なので、`Vector` を `Seq` として渡すと実行時に落ちます。
  `SeqOps.length` / `apply(I)` を使う版か `toList` の挿入が要ります。
  ついでに `case List(a, b, rest @ _*)` の codegen は **main でも**
  `VerifyError: Bad type on operand stack` を出します（星付きパターンの前の
  要素を束ねるローカルに checkcast が出ていない）。

- **抽象クラスの `new` が診断されない**（`agent/seqpat` で気づいた、未修正）。
  `abstract class A { def n: Int }` に対する `new A` を通してしまいます
  （nsc は `class A is abstract; cannot be instantiated`）。修飾子が
  パーサから落ちていた件を直したので `Flags::ABSTRACT` は正しく載るように
  なりましたが、`new` 側の検査はまだありません。

- **オーバーロード解決の specificity で、自分の型パラメータを上界に潰している**
  （`agent/seqpat`）。nsc は skolem を作りますが、こちらは `bound_hi`（既定
  `Any`）で置き換えます。`def f[T <: A](x: T)` と `def f(x: B)` のような、
  上界が効く形では nsc と結論が変わりうるはずです。slick では出ていません。

- **`Seq.toArray` / `Seq.zipWithIndex` が、あるファイルを一緒にコンパイルすると
  消去されたシグネチャに化ける**（`agent/impltail` で原因まで特定、未修正）。
  slick の `ProductResultConverter` の `elementConverters.toArray` は
  `(ClassTag[B])Any` というメソッド型のまま残り、`cha.length` / `cha(i)` が
  そのカスケードになります（5 件）。`ResultConverter.scala` を単体で
  コンパイルすると `Seq#toArray` は prelude の `(ClassTag[A])Array[A]` に
  解決されるのに、`slick/util/ConstArray.scala` を**先に**コンパイルすると
  `IterableOnceOps#toArray : (ClassTag[B])Any` に解決されます（`Seq` の
  symbol は同一で、`lookup_member(Seq, "toArray")` の結果が
  `Seq` 自身のものから `IterableOnceOps` のものに入れ替わる）。
  `Array[B]` は classfile 上 `Object` に消去されるので、classfile 由来の
  メンバが prelude のメンバを覆うと結果型が `Any` になります。
  implicit の問題ではなく、クラス完了時のメンバ供給の問題です。

- **`implicitly[C[T]]` の結果に checkcast が入らない**（`agent/impltail` で確認、
  未修正。main でも同じ）。`def f[T: C](…) = implicitly[C[T]].name` は
  `implicitly` の戻り値（消去して `Object`）を `getfield` の receiver に
  そのまま積むので `VerifyError: Bad type on operand stack` になります。
  context bound の evidence を名前で受ける形（`def f[T](…)(implicit c: C[T])`）
  なら通ります。

- **`Integral[T]` / `Fractional[T]` が `Numeric[T]` にならない**。
  `Numeric[T] <: Ordering[T]` は `crates/typer/src/prelude_numhier.rs` で
  張りましたが、`Integral` / `Fractional` は prelude を組み立てる時点では
  symbol table におらず（ソースが名前を出したときに jar から読まれる）、
  同じ場所では親を張れません。

- **cats の syntax（`import cats.syntax.all._`）による拡張メソッド**は
  `agent/catsyntax` で**本物の cats に届くようになりました**（上の節）。
- **jar のメンバの結果型が素の `F` になる**（`agent/catsyntax` で原因まで確認、未修正）。
  `ctx.update(…) >> …` は `value >> is not a member of F`、
  `asyncF.pure(x)` は `no matching overload for (Function0[A])F` になります。
  `Ref[F[_], A]#update` の結果型は `F[Unit]` ですが、これが**pickle からではなく
  classfile の generic signature から**来ています（`TF;` は高階の適用を書けないので
  素の `F`）。pickle 供給が落ちる理由は
  `cats/effect/kernel/Ref$#update/1: no unambiguous erased descriptor` で、
  **`Ref` のシンボルの `jvm_name` がコンパニオンの `cats/effect/kernel/Ref$`**
  だからです。`find_or_stub_java_class` は `cats/effect/kernel/Ref$` を渡されると
  `java_simple_name` で末尾の `$` を落として `Ref` という名前の
  **`SymKind::Class`** を作り、`jvm_name` にはコンパニオンの名前を入れます。
  以後 `ensure_class("cats.effect.kernel.Ref")` は
  「その名前のシンボルはあるが `jvm_name` が key と違う」で `None` を返すので、
  本物のトレイトはシンボルを持てません。
  `$` 付きなら `ModuleClass` を `Ref$` という名前で作る、と直すと
  この経路は通りましたが、今度は `Async[F]` から `FlatMap[F]` が引けなくなり
  （`agent/catsyntax` のスクラッチで確認）、差し引きで悪化したので戻しました。
  slick に残る `F` 系のエラー 8 件はすべてこれです。
- **jar のクラスを pickle から読む（`agent/jarpickle`）で残ったもの**。
  - **cats の `implicits` 経由の implicit 探索**。`Monad[F]` のシグネチャは
    正しく届くようになったが、`import cats.implicits._` から `Monad[Option]` を
    見つける（`cats.instances.*` の深い継承をたどる）ところは通らない。
    slick の `BasicBackend.scala` に残る `value flatMap is not a member of F[Any]`
    などは cats の syntax 拡張メソッドで、同じ実装が要る。
  - **`Ref.Make[F]` のような導出 implicit**。`Ref.of[F, Int](0)` はシグネチャが
    通り、`could not find implicit value of type Make[F]` で止まる
    （`MakeLowPriorityInstances#syncInstance` から `Sync[F]` 経由で導く）。
    なお**コンパニオンに直接置かれた implicit**（`Async[IO]` ＝
    `cats.effect.IO.asyncForIO`）は届くようになりました。SLS 7.2 の implicit
    スコープにあるコンパニオンを探索前に読み込む（`Typer::warm_implicit_scope`。
    jar のコンパニオンは誰も要求しない別 classfile なので、そのままでは
    スコープが空だった）、pickle の `IMPLICIT` フラグをメソッドに載せる
    （classfile にはこのビットが無い）、pickle → Type 変換が**まったく適用されて
    いないクラス参照**（`Async[F[_]]` の引数としての `IO`）を arity エラー扱い
    しない、の 3 点。コンパニオン全体を `adopt_binary_class` すると
    cats-effect の推移閉包を引き込んで数分かかるので、implicit メンバだけを
    供給します（`PickleSupply::supply_implicit_members`）。
    3 点目は**位置が高階を要求しているときだけ**許します（`conv_ref` に
    `want_arity` を渡す）。どこでも許すと、素の位置に現れた `Iterable` が
    型引数ゼロの `Iterable` になって本物の `map` を隠し、slick が
    745 → 844 エラーに悪化しました。
  - **ソースレベルでも高階の引数節をまたぐ推論が効かない**。
    `F.flatMap(fa)(a => F.pure(a))` の `a` が `Any` になる。これは jar とは無関係で、
    同じ形をソースに書いても同じく落ちる（`trait MyMonad[F[_]]` で確認）。
  - **pickle ライタのパラメータ節と親**。上の「実装していないもの」参照。
    どちらも自前で出した jar を自前で読み戻すときの上限で、
    `-cp <ディレクトリ>` 経路（classfile の interfaces を読む）には影響しない。
  - **`-cp` がディレクトリのときと jar のときで結果が違いうる**。ディレクトリは
    `install_classpath`（backend の unpickler、親は `Object` のまま）、jar は
    `adopt_binary_class`（`crates/pickle` ＋ classfile の interfaces）を通る。
    今は jar の方が正確で、`Monadic[Option] <: Functor[Option]` はディレクトリ側だけ
    通らない。統一するならディレクトリ側も `adopt_binary_class` に寄せる。

- **`List.newBuilder` / `Vector.newBuilder` がコンパニオンに無い**。`Builder[A, To]`
  自体は pickle から供給されて動く（`ctacc_builder` が通る）が、companion の
  `newBuilder` は多相メソッドのため供給されない。prelude に `Builder` を自前宣言
  して足そうとすると、pickle 側の `Builder`（`Growable` を継承し `addOne` が
  abstract）を隠してしまい、`class ListB extends Builder[...]` が `addOne` を
  実装しなくなって実行時 `AbstractMethodError` になる。試して巻き戻した。

- **slick の計測は `.fm` テンプレートを展開してから行う**。slick は `GetResult` /
  `SetParameter` / `TupleSupport` など 7 本を FreeMarker テンプレートとして持ち、
  ビルド時に生成します。生成せずに計測すると、その 7 本に依存する 7 ファイルが
  「scalac でも落ちる」エラーを出すため、`tests/expand_fm.py` で展開して一緒に
  コンパイルします（`tests/slick_measure.sh` が自動で実行）。この 7 本を含めた
  時点で計測対象は 177 → 184 ファイルになり、エラー数も一段増えます（1371 → 2064）。
  数字が増えたのは退行ではなく、計測が実際のコンパイル対象に追いついたためです。
  その後 `agent/genrep` スライスで **2064 → 1300**（生成 7 本は 736 → 41）になりました。
  内訳と残りは「slick が生成する 7 本（`.fm` テンプレート）が通るまで」を参照。
  `agent/ctoraccessor` スライスでさらに **1279 → 1219**（エラーを含むファイルは 109 → 107、
  `CompilableFunctions.scala` の `tupled` 21 件と `Builder` の `++=` 6 件がゼロ）。
  さらに `agent/mismatch2` スライスで **1279 → 1123**（`type mismatch` は 320 → 227、
  エラーを含むファイルは 109 → 107）になりました。残る `type mismatch` を機械分類すると、
  「解けないままの型パラメータがそのまま出ている」81、「同じクラスで型引数だけ違う」27、
  「`Any` に広がった」14、「found と required が同じ字面」11 で、残りは細かい単発です。
  さらに `agent/tyvar`（未確定の型変数）スライスで **1059 → 1029**（エラーを含む
  ファイルは 105 → 104、`no matching overload` は 280 → 266、`type mismatch` は
  231 → 217）。減ったのは「多相参照が型パラメータを抱えたまま引数位置に届く」形
  （`Vector[A]` / `Map[K, V]` / `Set[A]` が `found` に出るもの）です。
  新たにエラーを出すようになったファイルはありません（もともとエラーのあった行に
  カスケードが 1 本増えた箇所が数か所）。同スライスで `relax_open_tparams`
  （未確定の型パラメータを `Any` に潰す場当たり。README の記録では 3 回別々の
  バグの原因になっていた）を**削除**しました。
  `agent/ovl2` スライス（オーバーロードの候補集合）でさらに **1059 → 903**、
  エラーを含むファイルは 105 → 104 になりました。
  `agent/mismatch3` スライスで **833 → 772**（`type mismatch` は 201 → 168、
  エラーを含むファイルは 102 → 100、新たにエラーを出すようになったファイルは
  ありません）。原因は 8 つで、`type mismatch` そのものより「その手前で落ちていた
  カスケード」の方が多く消えました。残る 168 件の機械分類は、単発 46、
  「`found` が裸の型パラメータ」36（うち `F` は cats の HK シグネチャ、後述）、
  「自己型ごしの型メンバ／`ProfileAction`」25、「同じクラスで型引数だけ違う」21、
  「タプルの成分が解けない」11、「コレクションの結果型が広がる」11、
  「`found` と `required` が同じ字面」8、「`Some`/`Failure` の要素型」6、
  「`type Self >: this.type` に `this` が適合しない」4 です。

- **jar のクラスは `ScalaSignature` ではなく JVM の generic signature から読んでいる**。
  `-cp` に**ディレクトリ**を渡したときだけ pickle を読み（`load_classpath` は jar の中を
  歩かない）、jar のクラスは `install_java_class` が classfile の `Signature` 属性から
  作ります。JVM の signature は高階型の適用を書けないので、cats の
  `def pure[A](a: A): F[A]` は `<A:Ljava/lang/Object;>(TA;)TF;` として届き、
  `F.pure(v)` が `found: F  required: F[R]` になります。slick で最もエラーの多い
  `BasicBackend.scala`（54 件）と `ConcurrencyControl.scala`（16 件）はまるごとこれで、
  残る `type mismatch` の「裸の型パラメータ」36 件の大半を占めます。直すなら
  `crates/pickle`（すでにフル機能の unpickler）を jar のクラスにも使うことになります。

- **`p.State` のような依存メソッド型が置換されない**。`def get[P <: Phase](p: P): Option[p.State]`
  の結果は `Option[Phase.State]` のまま届き、`state.get(Phase.assignUniqueSymbols)
  .map(_.aggregate).getOrElse(true)` が `found: Any  required: Boolean` になります
  （4 件）。`Type` にプレフィックス付きの型メンバ（`p.State`）を表す変種が無いのが原因。

- **`type Self >: this.type <: Node` に `this` が適合しない**（4 件）。下界が
  `C.this.type` の抽象型メンバに対する適合規則（`X <: lo ⇒ X <: Self`）が無く、
  `val n: Self = if(…) this else rebuild(…)` が
  `found: BinaryNode  required: Node.Self` になります。`this` の型が
  `ThisType` ではなく素のクラス型なので、素直に入れると
  「別の `Node` を `Self` に渡す」まで通ってしまうのが難所です。

- **`Map[K, V]` を `Iterable[T]` に渡したとき `T` が解けない**。適合判定ではなく推論の
  穴で、`def h[T](xs: Iterable[T]) = xs.size` に `Map[String, Int]` を渡すだけで
  `no matching overload` になります（`h[(String, Int)](m)` と
  `def h2(xs: Iterable[(String, Int)])` は通ります）。slick の
  `ConstArray.from(newDefsM.map(…))` 5 件がこれです。

- **`java.lang.String` の JDK メンバが on-demand で読まれない**。prelude が宣言した分
  （`prelude.rs` の `add_string_members` / `prelude_text.rs` の `add_string_extra` /
  `prelude_strhier.rs` の `indexOf` 群）しか無く、`s.codePointAt(0)` は
  `value codePointAt is not a member of String` になります。他の Java クラスと違い
  `Type::String` は受け手のクラスシンボルを持つのにメンバ探索が prelude で当たって
  しまうため、`ensure_java_loaded` に到達しません。

- **override 検査が無い**。`override` 修飾子の要否も、override 時の型適合も検査していない。
  scalac が拒否する次の 2 つを黙って通す:
  `trait T { def f: Int = 1 }; class D extends T { def f: Int = 2 }`（`override` 無し。
  scalac: ``` `override` modifier required to override concrete member ```）、
  `class D extends T { override def f: String = "x" }`（親は `Int`。scalac:
  `incompatible type in overriding`）。`val` も同様。受け入れすぎる側の穴。
- **`Array[T]` から `Seq[T]` への暗黙変換**。`def k(x: Array[Int]): Seq[Int] = x` は scalac
  では（deprecation 警告つきで）通るが、こちらは type mismatch になる。`Predef` の
  `copyArrayToImmutableIndexedSeq` / `wrapIntArray` 相当の暗黙変換が prelude に無い。
- **`Vector[T]` が `scala.collection.IndexedSeq[T]` に適合しない**。prelude の
  コレクション階層に `immutable.Vector`（および `immutable.IndexedSeq`）から
  `collection.IndexedSeq` への辺が無い。`immutable.IndexedSeq` を書けば通るので、
  足りないのは辺だけ。
- **`F` が型パラメータ名と implicit 値名を兼ねると型側が勝つ**。
  `def f[F[_]](implicit F: Sync[F]) = F.pure(x)` の `F.pure` で、値の `F` ではなく
  型パラメータ `F` を選んでしまい `found: F  required: F[R]` になる
  （slick の `BasicBackend.scala`）。名前解決が項と型を分けていない。
- **残りの型変数の穴は、値になっていない引数のほう**。引数を期待型なしで型付けすること
  自体は変えていない（オーバーロード解決が引数の型を先に必要とする）が、そこから出てくる
  未確定の型変数は `agent/tyvar` スライスで持ち回って解くようにした
  （「未確定の型変数」の節）。残っているのは、引数の型が**まだ値の型になっていない**場合:
  - `Array.empty` は `(ClassTag[T])Array[T]`、つまり implicit 節が残ったメソッド型のまま
    引数位置に届く。scalac は `take(a: Array[String])` に `Array.empty` を渡せるが、
    こちらは `no matching overload … with arguments ((ClassTag[T])Array[T])` になる。
    残っている implicit 節を引数位置で適用していないのが原因で、型変数の側ではない。
    `Array.empty[String]` と書けば通る。
  - `f(("x", n => n + 1))` のようにタプル要素の関数リテラルは、
    **scalac 2.13.16 も拒否する**（`missing parameter type` ＋
    `no type parameters for method apply … exist so that it can be applied to
    arguments (String, ? => ?)` ＋ `undetermined type`）。以前ここに書いてあった
    「scalac は通す」は誤りだった。`f("abc", s => s.length)` のような
    同じ節の中での「先の引数から後の引数のラムダのパラメータ型を決める」形も
    scalac は拒否する（こちらは通してしまうので、受け入れすぎている側の穴）。
  - `h(new Box(Map.empty))`（`def h[A](b: Box[Map[String, A]])`）も
    **scalac が拒否する**（`Box` が非変なため）。

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
- **quasiquote の reification の残り**。`q"..."` はリテラル / 名前 / 選択 /
  適用（カリー化含む）/ `$x` 穴 / 引数リスト 1 節ぶんの `..$xs` を
  `internal.reificationSupport.Syntactic*` に落として実行できる
  （`crates/typer/src/reify.rs`、実 scalac と dual-run 済み）。宣言クラスでの呼び出しも
  済んでいて、`scala.reflect.runtime.universe` 上の Tree 構築は実際に走る。
  残るのは `docs/macros.md` §7.5 の 5 つ:
  (a) `tq` / `pq` / `cq` 全体、
  (b) `q` の残りの形（ブロック / `new` / 関数リテラル / `if` / `match` / 型注釈 /
  定義 / `this` / `super`）、
  (c) `..$` と普通の引数の混在（`q"f(a, ..$xs)"`）、
  (d) `Liftable`（`$x` の `x` が `Tree` でないとき nsc は implicit で持ち上げる。
  `mapToImpl` は `$rTag` / `${c.prefix}` でこれを使う）、
  (e) `c.Expr[T]` のようなパス依存型 — いまは `prelude_reflect.rs` が**空の `Context`** を
  入れており、classpath 上の本物より優先されてしまう。マクロ実装の*中で*
  quasiquote を書くにはこれが要る。
  落とせない形は**すべて `unimplemented syntax: quasiquote ...` で診断する**
- **`import <値>._` のスコープ**。プレフィクスが値のときの書き戻し
  (`term_import_prefixes`) はコンパイル単位をまたいで持ち越される。名前が
  そのクラスのメンバに解決できたときだけ使うので実害は見ていないが、
  本来はスコープと一緒に push / pop すべきである
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
  - カスタム文字列補間子（`value q/tq/pq is not a member of StringContext`、14 件、`ShapedValue.scala` の `mapToImpl` 1 メソッド）は当初 `implicit class` パターンと想定していたが、実際は `scala.reflect.macros`（`scala-reflect.jar`）の **quasiquote**（`q"..."` / `tq"..."`）だった。**`agent/quasi` スライスで診断を正した**（以前の文面は誤りで、`q` は `Quasiquotes.Quasiquote` のメンバである）。中身は scala-rs のパーサで実際に構文解析するようにしたので、14 件すべてが構文解析できていること（`unimplemented syntax` は 0 件）が実測で分かった。残りは reification と reflect ABI で、`docs/macros.md` §7.3 に列挙した。
  - 副次的に見つけたが未修正: `case object X extends Y(...) { override def m: MoreSpecific = ... }` のように親の抽象メソッドを共変な戻り値型でオーバーライドすると `AbstractMethodError`（ブリッジメソッド未生成）。fixture 構築中に踏んだので `tests/fixtures/sgap.scala` はこのパターンを避けている。別の残課題として記録。
- **型位置 `super.T` の残り**: 親クラスの型メンバーへのパスは通るが、`trait Mid { trait Impl extends super.Impl }` のように**親と同名**の入れ子型を定義すると、ミックス先で継承メンバーの解決が親側を選んでしまうことがある（`super` の解決ではなく、同名ネスト型のメンバー継承側の穴）
- **`Unit` に具体化した多相メソッドの捨て方**: `PartialFunction[A, Unit].apply` のように JVM 上は `(Object)Object` を返すものだけ、statement 位置で `pop` する。`Breaks.catchBreak` / `Using.resource` のように emit 側で既に捨てている intrinsic とは重ならないよう、判定は意図的に狭くしてある（`unit_call_leaves_ref`）
- **`agent/overloadshadow` スライス**（別のクラスを読むと既存のオーバーロード集合が消える）: 177 ファイルのエラーは **1,707 → 1,678**（`files_with_errors` は **111** のまま）。根本原因は 3 つ重なっていた: (a) `PickleSupply::complete` がクラス側で 1 つでも供給できたらコンパニオンを見ずに返していた（`java.math.MathContext` が入っているかどうかという**無関係な大域状態**で答えが変わる）、(b) `check.rs::resolve_overload` が `Type::Overload` の候補シンボルを `fun.sym` の owner から引き直すので、クラスとコンパニオンにまたがる集合の片側が丸ごと落ちる、(c) 一度クラス側に `apply(MathContext)` が入ると以降の `BigDecimal(...)` は `lookup_member` がそれを見つけて止まり、pickle 補完まで届かない。(a) は合併に、(b) は `Check::overload_groups`（引き直しで失われる集合だけ覚える）に、(c) は `Check::widen_with_companion`（**エラーを出す直前だけ**、term 位置のクラス名の選択をコンパニオンのメンバで広げて 1 度だけ解決し直す）で直した。併せて `scala.math.BigDecimal.apply(java.math.BigDecimal)`（JDBC の結果を Scala 値にするのに使う）を prelude に固定した（`crates/typer/src/prelude_oshadow.rs`。`library_abi` のみ）。残件: slick の `value getOrElse is not a member of Product`（16 件）は BigDecimal とは無関係で、`if (c) None else Some(x)` の `lub` が `Option[X]` にならず `Product` に落ちる別のバグ（`Boolean` / `Blob` / `Byte` … でも同じように出る）。`BigDecimal.apply` を eta 展開して `(Double) => BigDecimal` に渡す `new ScalaNumericType[BigDecimal](BigDecimal.apply)` は、オーバーロードの eta 展開を期待型で選べないため未対応
- **`agent/quasi` スライス**（quasiquote と reflect ABI の下地）: slick 184 ファイルのエラーは **1,059 → 1,050**（`files_with_errors` は **105 → 104**）。数字が小さいのは、このスライスの主眼が「誤った診断を正しい診断に置き換えること」と「reflect ABI に至る道の穴を塞ぐこと」だったからである。塞いだ穴は、pickle が指す**ネストしたクラス**（`Names.TermNameExtractor` = `Names$TermNameExtractor`。しかもネストした classfile は `ScalaSignature` を持たない）、**バイトコード上の親を持たないトレイト**（`Universe` が abstract class なので `trait JavaUniverse extends Universe` の classfile は `interfaces: 0`）、**抽象型メンバ**（`type Tree >: Null <: TreeApi` — reflect API の語彙のほぼ全部）、**引数なし `def` の結果に対する `apply` 挿入**（reflect に限らない一般の欠落）、**package object のメンバのコード生成**（`scala.math.Pi` が `VerifyError` になる既存バグ）、**`import <値>._`**。詳細と残件は `docs/macros.md` §7.2 / §7.3
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
