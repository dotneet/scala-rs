# scala-rs

Rust で書いた、Scala 2.13（nsc）サブセットのコンパイラです。JVM classfile を出力します。

scalac のソースを移植したものではありません。オリジナルの再実装です。Scala 3 / TASTy は対象外です。

## これは何か

scala-rs は、Scala 2.13 の構文と意味論のごく一部を、Rust から JVM バイトコードへ落とす実験的コンパイラです。

- フロントエンドは nsc の `Tree` に近い AST を持ちます。
- ターゲットは Java 8 相当の classfile（major version 52）です。Code 属性に StackMapTable（full_frame）を出します。ローカルのフレーム型は scalac と同じく**そのスロットの宣言型の消去**です（`var c: Option[Int]` はループ先頭でも `scala/Option`。`Some` と `None$` の最小上界を計算するのではありません — 「ループ先頭のフレームとオペランドスタックの上の `try`」節）。
- デフォルトでは scala-library を同梱しません。Option / List / FunctionN は **scala-rs 独自のランタイム classfile**（`scala/Option` など）です。
- `--scala-library [<jar>]`（または `SCALA_LIBRARY_JAR`）を付けると、Option / List / FunctionN / Tuple2 に加え、`Predef$`（`println` / `assert` / `require` / `???` / `identity` / `locally` / `implicitly`）、`any2stringadd`（`1 + "x"`）、`ArrowAssoc` の `->`、`intWrapper` / `RichInt`（`1.abs` / `1.max` / `1.to`）、`longWrapper` / `doubleWrapper` / `charWrapper`（`(-3L).abs` / `1.0.max` / `'9'.isDigit`）、`StringOps`（`augmentString` 経由の `toInt` / `length` / `*` / `take` / `drop` / `isEmpty` ほか。**prelude に無いメンバは jar の `ScalaSignature` から補完**します — `agent/stringops8` の節を参照）、`WithFilter` / `Iterator`、`Map` / `Vector` / `List` / `Set`（varargs `apply` を含む）、**`scala.jdk.CollectionConverters` の `asScala` / `asJava`** は **scala-library 2.13.16 の ABI** にリンクし、衝突する私有 classfile は出しません。jar パスを省略すると `SCALA_LIBRARY_JAR`、`/tmp/scala-rs-lib`、cwd を探します。**`scala-rs compile` と `scala-rs run` は、jar が自動検出できればそれを既定で使い**、見つからなければ私有ランタイムに落ちます。**`--no-scala-library` は私有ランタイムを強制**します。jar リンク時はさらに **right-biased な `Either`**（`map` / `flatMap` / `fold` / `swap` / `toOption` / `filterOrElse` / `left` の `LeftProjection`）と **`scala.util.Try`**（`recover` / `recoverWith` / `transform` / `toEither` / `withFilter`）も乗り、どちらも `for` 内包表記で使えます。

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
- `-cp` / `--class-path` — 先にコンパイルした classfile を読む（`ScalaSignature` pickle subset と JVM メソッド。vals / パラメータ付き defs / 型パラメータ / `$default$n` ゲッター / case class の ctor フィールドを含む。自前 `-cp` は companion `apply` も読む。nsc は companion apply `Point(...)` / term `Point` / extractor `unapply` / `List[_]` の existentials / `List[_ <: AnyRef]` / `List[_ <: List[_]]` / `@deprecated("msg", "2.13.0")` の annotation args / Java `@Deprecated`（SYMANNOT + `java.lang.Deprecated`） / `this.type` / `Int @unchecked` / refinement `A with B { def f: Int }` も読む）。**jar の中の Scala クラス**は `ScalaSignature` pickle をそのまま読みます（`crates/pickle`。高階型パラメータ `F[_]` と `F[A]` を含む。読めなかったメンバだけ JVM signature に落ちる。`scala.*` / `java.*` は対象外。先読みはせず 1 クラスずつ。「jar のクラスを pickle から読む」節）。**Java の `.class`** も同じ `-cp` / jar / jmod / JDK（`java.base.jmod` や `rt.jar`）からオンデマンドで読む（ScalaSignature の無い pickle-less Java は pickle インストーラに載せない。`JAVA` / `protected` / `static` を落とさないため）。prelude に無い JDK クラスのメソッド・フィールド（`java.lang.Math.abs` / `java.util.ArrayList#add`）を解決する。**Signature 属性**があればジェネリックを raw にしない（`ArrayList[String]#get` は `E`＝`String`。無ければ `Object` のまま `String` へは通さない）。**ワイルドカード／型パラメータ境界**（`Class[*]` → `Class[_]`、`Collection<+TT>` → `Collection[_ <: T]`、`<T:Number>` の hi bound）は存在型として残し raw `Object` にしない。`ArrayList[Byte] <: List[_ <: T]` は親ウォークより先にワイルドカードを照合し、継承した `add` は `drop_overridden` する。**静的 inner**（`java.util.Map.Entry` / `AbstractMap.SimpleEntry`。入れ子側の `Signature` にある**入れ子自身の型パラメータ**を含む — `Map.Entry[K, V]`）と **Java varargs**（`ACC_VARARGS` の `String.format` / `Arrays.asList`。Scala `Seq` wrap ではなく `Object[]`）も classfile から読む。**インタフェースの `static` メソッド**（`Map.entry` / `List.of`）は JVMS 4.4.2 のとおり定数プールで `CONSTANT_InterfaceMethodref`（`invokestatic` 命令はそのまま）。Java の `throws` 検査例外は Scala と同様チェックしない。**Java `protected`** は同じパッケージかサブクラス（nsc / JLS）から見え、それ以外は診断する。Scala の `Base.secretStatic()` は Java クラスの `MODULE$` を出さず `invokestatic` する。ScalaSignature pickle だけに頼らない。**Java enum**（`ACC_ENUM` のクラスと定数。`values` / `valueOf` は classfile の static。非 enum に `values` を合成しない）。未対応の classfile 機能（未知 CP tag、`ACC_MODULE`、壊れた magic）は診断する（黙って成功にしない）

フィクスチャはデフォルトパッケージ（`package` 句なし）なので、`-cp out` の `Main` でそのまま動く想定です。

## 実装している言語サブセット

Scala **2.13** 構文です。Scala 3 の `then`、トップレベル定義、TASTy はありません。エントリポイントは `def main(args: Array[String]): Unit` です。

パースできる（またはデシュガーする）構文:

- packages / imports
- objects / classes / traits / case classes。**補助コンストラクタ** `def this(...) = this(...)`（連鎖の先頭は `this(...)`。`super(...)` や文のあとの `this` は診断）。サブクラスの `extends C(1)` は primary が親 ctor を呼ぶ。内部クラスの `new Inner` は ctor overload 選択後も `$outer` を `<init>` の第一引数に残す。**case class の `copy(...)`**（positional / 一部省略時は自分自身の対応フィールドを default / 名前付き引数。`copy` は namer 時点ではまだ ctor フィールドの型が確定していないため、フィールド型解決後の typer フェーズで `copy` 自身の引数シンボルと `copy$default$N` を作り直す。private ランタイムでも動く）。**コンストラクタの省略可能引数**（`class C(x: Int, y: Int = 5)` の `new C(1)` / `new C(y = 2, x = 1)`）: 末尾を省略した呼び出しへのデフォルト値の充填は、通常の `def` の default getter 経由ではなく（`this` が無い呼び出し元では使えないため）呼び出し側でその場を型付けする素朴なフォールバックのみ実装（先行引数を参照するデフォルトは非対応）。**名前付き引数での並べ替えは `new C(...)` でも動く**（コンストラクタのオーバーロードはパラメータ名で絞ってから型で決める）
- `val` / `var` / `def`（ネストした `def` はパースする）
- **テンプレート本体の式文**（`class A { println("ctorA") }`）。SLS 5.1 / 5.3 どおり、class なら主コンストラクタ、trait なら `$init$`、`object` ならモジュール初期化の一部として、`val` / `var` の初期化と**宣言順に交互に**走る。早期の `require(...)` / `assert(...)`、`if` / `match` / `try` / ループ / ラムダ、`case class` / ローカルクラス / 匿名クラス / メンバ `object` の本体でも同じ。詳細は「テンプレート本体の式文」節
- パラメータ、ラムダ（型付き / 期待型から推論）、ブロック。**placeholder `_`**（nsc `withPlaceholders`）: `_ + 1` / `_.abs` / `f(_)` / `xs.map(_ + 1)` / Function2 `_ + _` / 入れ子 `_.map(_ + 1)` に加え **typed `_ : T`**（`(_: Int) + 1` / `(_: Int) + (_: Int)` / `(_: Int).abs` / `xs.map((_: Int) + 1)`）。レキサが `_:` を `Ident("_")` にするので、式位置では Underscore と同じ placeholder にする。bare `(_: Int)` は `unbound placeholder parameter`。`xs.map(_ : Int)` は nsc どおり wrap せず map に Int が渡り mismatch。unary / Function2 の既存 wrap は触らない。**メソッド適用のセクション** `f(_, x)` / `f(_, _)` は期待型が無くても呼び先のシグネチャからパラメータ型を取る（nsc と同じ条件で、呼び先が単一の非ジェネリックメソッドのときだけ。`poly(_, 3)` や overload された `"abc".substring(_)` は `missing parameter type for expanded function` のまま）。合成パラメータはソース順で並べる（`two(_, _)` は `(a, b) => two(a, b)`）。**リテラルの本体は期待型の結果に対して検査する** ── `xs.foreach((x: Int) => x + 1)` は value discarding、`fl((x: Int) => x)` は `Int => Long` への数値拡大。パラメータ型を書いたリテラルはオーバーロード解決のために期待型より先に型付けられるので、そのぶんは `adapt` 側でやる。関数**値**は対象外で、`val h: Int => Int = …; fu(h)` は nsc どおり `type mismatch`
- `if` / `else`、`while`、`do { ... } while (cond)`
- `try` / `catch` / `finally`（catch は `{ case ... }`。`try/finally` と `try/catch/finally`。finally は正常終了と例外（catch からの throw 含む）の両方で走る。JVM 例外テーブルを出す。パーサは `finally` を落とさない）
- `match`（コンストラクタパターン、リテラル、ワイルドカード、Java enum 定数の安定識別子 `Thread.State.NEW`、`x @ Pat` の束縛、`case null`、入れ子の抽出子 `case P(v) :: t`。どの case にも当たらなければ `scala.MatchError`）
- for-comprehension（`map` / `flatMap` / `foreach` / `withFilter` へデシュガー。私有ランタイムでは `List.withFilter` は eager な `List`。`--scala-library` 時は `scala.collection.WithFilter[+A, +CC[_]]` で、`map[B]` は `CC[B]` を返す。`Option.withFilter` は `Option$WithFilter`）。値定義 `q = e` はラムダ本体の `val` になる ── **生成子ではない**ので、その前の生成子はやはり最内で `map` を取る。値定義の**後ろのガード**は nsc のタプル化が要るので診断する
- apply / select / infix（`:` 終わりの演算子は右結合で、レシーバは右オペランド。`1 :: Nil` → `Nil.::(1)`）。代入 `xs(i) = v` は nsc どおり `xs.update(i, v)`。代入でない `c(1)` で `apply` が無ければ診断する（黙って `update` にしない）
- リテラル、タプル
- 名前付き型・ジェネリック型（`Array[String]`、`def id[T](x: T): T` など）。infix 型 `A Either B` は `Either[A, B]`。`Map[K, V]` の applied 構文はそのまま。**高階型** `trait Functor[F[_]]` / `class Box[F[_], A](val fa: F[A])`。具象は `Id[_]` など。kind 不一致（`F[_]` を proper 位置で使う、proper 型を型コンストラクタとして使う）は診断する（黙って捨てない）。**高階型メンバー** `trait M { type F[_] }` とパス依存適用 `m.F[Int]`。具象は subclass で `type F[X] = Id[X]`（または `List[X]`）。メンバーの kind 不一致（`type F[_]` を `type F = Int` で束縛、逆も）は診断する。**refinement の高階型メンバー** `M { type F[X] = Id[X] }` と適用。**HK 境界** `type F[_] <: Bound`（proper な境界。`type F[_] <: List` は nsc どおり `takes type parameters`）。**refinement の境界** `{ type A <: Int }`。クラス / トレイトの nullary `type A <: T` は未実装のまま診断する。**入れ子型射影** `Outer#Inner#X` / `Holder#Inner#T`。違法な `Int#X` と抽象 `B#U#T`（メンバー無し）は nsc どおり `is not a member`
- 2.13 の early field defs: `class C extends { val x = 1 } with T`。`x` は親 ctor / trait `$init$` の前にフィールドへ書く（nsc と同じ）。具象フィールド以外（`def` / 文 / 抽象 val）は nsc どおり `only concrete field definitions allowed in early object initialization section`。early 内の `this` は `this can be used only in a class, object, or template`
- SIP-23 定数型のサブセット: `val x: 1 = 1`、`def f(x: 1): Int`。式のリテラルは定数型（`1 <: Int`）。不一致 `val y: 1 = 2` は type mismatch。classfile の pickle は nsc `CONSTANTtpe` + `LITERALint`（scalac 2.13.16 が `-cp` で `def f(x: 1)` / `val one: 1` を typecheck できる）
- `scala.Dynamic`: `d.foo` → `selectDynamic("foo")`、`d.foo(args)` → `applyDynamic("foo")(args)`、`d.foo = v` → `updateDynamic("foo")(v)`、`d.foo(a = x)` → `applyDynamicNamed("foo")(("a", x))`。`import scala.language.dynamics`（または `-language:dynamics`）が必要。`--scala-library` 時は jar の `scala/Dynamic` に対して実行する
- XML リテラルのサブセット（2.13）: `<a>t{e}</a>` / `<a/>` / `<a b={e} c="t"/>` / `<a xmlns:p="u" p:b={e} c="t"/>` / `<p:a xmlns:p="u"/>` / `<p:b xmlns:p="u">t</p:b>` / `<a><!--c--></a>` / `<a><![CDATA[x]]></a>` / `<a><?pi t?></a>` / `<a>&amp;</a>` / `<a>&#65;</a>`（elem / text / splice / 非プレフィックス属性 / `xmlns:p` とプレフィックス属性 `p:b` / プレフィックス付き要素名 / コメント / CDATA / PI / 定義済みエンティティ `&amp;` `&lt;` `&gt;` `&quot;` `&apos;` / 数値 `&#N;` `&#xN;`）。属性は nsc と同じ `UnprefixedAttribute` / `PrefixedAttribute` チェーンと `NamespaceBinding`。プレフィックス付き `Elem` は `prefix` に文字列、`label` にローカル名。コメント / CDATA / PI は `scala.xml.Comment` / `PCData` / `ProcInstr`。定義済みエンティティは `EntityRef`、数値参照は `Text`。レキサは `><!--` を `>` と `<` に分ける。未知のエンティティは診断する。`scala-rs run` は検出できた scala-xml jar を `java -cp` に足す
- `scala.Enumeration`: `object Color extends Enumeration { val Red, Blue = Value }`（複数 `val` で連番の id）。`--scala-library` 時は jar の `Enumeration` に対して実行し、`Value` の 4 オーバーロード（`Value` / `Value(i)` / `Value(name)` / `Value(i, name)`）・`values: ValueSet`（`toList` / `filter` / `size` / `contains`）・`withName` / `apply` / `maxId`・`Value.id` / `toString`・`case Color.Red =>` の安定識別子パターンが使える。`values` 以下は jar の `ScalaSignature` から読む（`agent/uniteq`）
- 適合（conformance）まわり: **コレクションの継承関係**（`Vector[A] <: IndexedSeq[A] <: Seq[A] <: collection.Seq[A] <: Iterable[A] <: IterableOnce[A]`、`List` / `LazyList` / `Queue` / `Range` / `ArraySeq`、`Set[A] <: Iterable[A]`、`Map[K, V] <: Iterable[(K, V)]`、mutable 側も同様）を `crates/typer/src/prelude_hier.rs` の 1 枚の表で型引数つきに張る。**アノテーション付き型**は下の型と同じに適合する（`Node` は `Node @uncheckedVariance`）。**モジュールの `.type`** はそのモジュール自身の型（`Some(Nil): Some[Nil.type]`）。反変パラメータを持つクラスの lub はそのパラメータだけ glb を取る（`Act[+R, -E]` の lub は `Act[R lub R2, E glb E2]`）。型パラメータの lub はその上限境界まで辿る。`extends Base[T](y)` の親コンストラクタ引数は`extends` 節が書いた型引数で読む。`type Self >: this.type <: Nd` に対して `this` は適合し（`class Leafy extends Nd { type Self = Leafy }` のように下限の `this.type` をサブクラス側で読み直す）、任意の `Nd` は適合しない
- 言語フラグ `implicitConversions` と `postfixOps` は nsc 2.13 どおり。ユーザー定義の `implicit def` / `implicit class` は import / `-language:implicitConversions` なしだと **warning**。postfix `42 bang` / `42 abs` は `import scala.language.postfixOps`（または `-language:postfixOps`）なしだと **warning**（`-Xfatal-warnings` でエラー）
- 存在型のよくある形: `List[_]`、`T forSome { type X }`、`List[_]` を取るメソッド、境界付き `List[_ <: AnyRef]` と `List[X] forSome { type X <: AnyRef }`（名前付き量化は `BoundedWildcard` に落として既存の pickle/erase 経路を使う）。ワイルドカードは Object 相当に erase する。入れ子の `List[_ <: List[_]]` は hi bound 側の EXISTENTIALtpe として pickle する。`p.Inner forSome { val p: Outer }` は `Outer#Inner` にパックして実行する。その他の `forSome { val … }` は診断する（黙って捨てない）
- compiled class/object に **ScalaSignature**（クラス属性 `ScalaSig` マーカー + `RuntimeVisibleAnnotations` の pickle subset）。`javap -v` で見える。自前 unpickler が読める範囲で `-cp` による別コンパイルができる。nsc 完全 pickle ではないが、ワイヤ形式は nsc と同じ（nentries、tag/len、ビッグエンディアン Nat、SID-10 は `0x7f→0`）。`val` / パラメータ付き `def` / 型パラメータ `id[T]` / `case class` の `new` と ctor フィールド / **companion apply `Point(3, 4)`（term `Point` / `MODULE$`）** / **extractor `unapply`（`p match { case Point(a, b) => … }`）** / object の `def` / **`List[_]`（EXISTENTIALtpe）** / **`List[_ <: AnyRef]`（量化 TYPEsym の hi bound）** / **`@deprecated("msg", "2.13.0")`（SYMANNOT + LITERALstring）** / **Java `@Deprecated`（SYMANNOT + TypeRef(java.lang, Deprecated)。scalac `-deprecation` がメソッド上のアノテーションを見る）** / **`this.type`（THIStpe をメソッド結果に）** / **`Int @unchecked`（ANNOTATEDtpe）** / **`val one: 1` と `def lit(x: 1)`（CONSTANTtpe + LITERALint）** / **`List[_ <: List[_]]`（入れ子 EXISTENTIALtpe）** / **`A with B { def f: Int }`（REFINEDtpe）** / **`@Ann(foo)` / `@Ann(c.x)` / `@Ann(3)` / `@Ann(this)` / `@Ann(classOf[Int])` / `@Ann(ident(1))` / `@Ann(this.x)` / `@Ann(super.foo)` / `@Ann(ident(ident(1)))` / `@Ann(foo = 1)`（TREE Ident/Select/This/Super/Apply + リテラル / LITERALclass Constant。ネストした Apply と Ident 以外の Select 修飾子を含む。named `@Ann(foo = 1)` は nsc と同じ位置 Constant）** / **`def join(xs: String*)`（VARARGS + `<repeated>`）** / **`Ordered` erasure bridge（BRIDGE）** / **`type T = Int`（ALIASsym。2.13 に ALIAStpe は無い）** は scalac 2.13.16 が読める形（object は CLASSsym+MODULE + MODULESYM、クラス pickle にも companion の MODULESYM を載せる、パッケージ（`hklib` / `slick/ast`）と scala / java.lang の EXTMODCLASSref、デフォルトパッケージだけ `<empty>`、POLYtpe は restpe 先行、val は NullaryMethodType ゲッター、case class は CASE / CASEACCESSOR、ユーザー型は**自分のパッケージ**所有の EXTREF、`Option` / `TupleN` / `FunctionN` / `List` は scala / `scala.collection.immutable` モジュール所有の TypeRef + 型引数、Flags は nsc raw long を `rawToPickledFlags` して出す）。full pickle とは主張しない。残る穴は README Remaining
- compiled class/object に **`InnerClasses`（JVMS §4.7.6）と `EnclosingMethod`（§4.7.7）**。以前は一切出しておらず、`getClass.getSimpleName` が `Circle` ではなく `Main$Circle` を返す、`isMemberClass` が常に `false`、`getEnclosingClass` / `getDeclaringClass` が常に `null` になっていた（すべてこの属性を読む）。ネストしたクラス / トレイト / object（`class Circle extends Shape` が両方とも `object Main` の直下）は自己エントリ（`outer_class_info` = 外側クラス、`inner_name` = ソース上の単純名）と、その classfile 自身の定数プールに現れる**他の**ネストクラス（`implements` / `checkcast` / フィールドや `$outer` の型など）を両方載せる。加えて、そのクラスが自身の直下に宣言しているネストクラス／object は、実際に参照していなくても**無条件で**載る（`javap -v` で確認した実 scalac の `Outer` / `Outer$Level1` の挙動と同じ）。ローカルクラスと無名クラス（`new Shape { ... }`）は `outer_class_info` を 0 のままにし（`isMemberClass` は `false`）、代わりに `EnclosingMethod` を出す。`inner_name` は無名クラスだけ 0（`getSimpleName` が空文字列）。`access_flags` は**ソース上の**修飾子（`public`/`private`/`protected`、`$outer` フィールドを持たない＝`static`、`final`）で、classfile 自身の `access_flags` とは別物（module class 自身の `final` は暗黙なので載せない。value class の `final` は書いていなくても `extends AnyVal` から来るので載る）。ネストしたオブジェクトの `object Main` 自身が生成する static フォワーダ（"mirror" class `Main`。`object` に `def main` があるときに出る）は自分自身が入れ子ではないので自己エントリは持たないが、実 scalac の mirror class と同じく、リンクした object の直下メンバーを無条件で載せる。case class のコンパニオンや value class の `$extension` を持つコンパニオンも普通のネストした module class として同じ経路を通る。ローカルクラスの `LocalC$1` のような**曖昧回避の数値サフィックス**は nsc にはあるが scala-rs にはまだ無い（この属性の話とは無関係な既存のギャップ）。fixture 接頭辞 `inner`（`crates/cli/tests/innerclasses.rs`）
- `s"..."` / `f"..."` / `raw"..."` 文字列補間。`f"$n%02d"` は `String.format` に落とす。`raw` はエスケープを解釈しない。日付時刻（`%t`/`%T`）、引数インデックス、相対 `% <` は診断する。`--scala-library` 時はカスタム interpolator（`implicit class Q(sc: StringContext) { def q(args: Any*) }` の `q"a$x"`）を `StringContext.apply(parts*).q(args*)` へデシュガーして実行する。私有ランタイムでは `s`/`f`/`raw` 以外は診断する
- コンテキストバウンド `T: ClassTag` / `T: Ordering` / `T: scala.reflect.ClassTag`（メソッド型パラメータ）と **クラス型パラメータ** `class C[T: Ordering](x: T)`。nsc と同様、implicit evidence `C[T]` へデシュガーする（クラスは primary ctor の extra implicit 節）。トレイトの `: C` / `<%` は nsc どおり `traits cannot have type parameters with context bounds ': ...' nor view bounds '<% ...'`。evidence が無ければ `no implicit`。`--scala-library` 時は jar の `scala.math.Ordering` を classfile から読み、companion の `implicit object Int`（`Ordering$Int$.MODULE$` / InnerClasses）と `ClassTag` にリンクして動く。ジェネリック `Array[T].length` は jar の `ScalaRunTime.array_length` に落とす
- `lazy val`。メンバは `bitmap$0` + アクセサ、**メソッドローカルは `scala.runtime.LazyRef`（プリミティブは `LazyInt` などの専用セル、`Unit` は `LazyUnit`）+ 持ち上げたアクセサ**で、宣言位置ではセルを作るだけ。初期化子は最初の読み取り時に高々 1 回、セルのモニタの下で走る（nsc の `lazyvals` フェーズと同じ形）。ブロックの中では `lazy val` だけ前方参照できる
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
- **case class / case object の合成メンバー**: case class は `toString` / `equals` / `hashCode` / `canEqual` / `productPrefix` / `productArity` / `productElement` / `productElementName`。**case object** は module class 側に nsc と同じ定数畳み込みの `toString`（`Foo`。`Foo$@1a2b3c` ではない）/ `productPrefix` / `hashCode`（`"Foo".hashCode`）/ `productArity`（0）/ `canEqual` / `productElement` を出す。`equals` は nsc と同じくシングルトンの参照等価（`Object` 由来）のまま。手書きの定義があればそちらが勝つ
- **case class / case object は `scala.Product with java.io.Serializable`**（jar リンク時）。`val p: Product = P(1, 2)` も `List[Product]` も通り、`productIterator` / `productElementNames` は nsc と同じく `Product` から継承する。**合成コンパニオンは `scala.runtime.AbstractFunctionN` を継承する**ので `P.tupled` / `P.curried` / `val f: (Int, String) => P = P` が動く。詳しくは「case class を `Product` にする」節
- **`val` への再代入の診断**（`val x = 1; x = 2` も `d.v = 5`（trait の `val`）も nsc と同じ `reassignment to val`）。Java のフィールドとコンパイラ生成の synthetic な項は対象外
- 内部クラス（`$outer`）とネストした object。匿名クラス `new Trait { def f = ... }` と `new { def x = 1 }`（合成 classfile。型は refinement ではなく `$anon$N`）
- **クラス / trait のメンバである `object`**。トップレベルの `object` と違って静的シングルトンではなく、**外側インスタンスごとに 1 つ**です。nsc と同じく `$outer` フィールドと外側インスタンスを取る `<init>`（`MODULE$` も `<clinit>` も無い）を出し、外側テンプレート側に `private volatile <name>$module` フィールドと、初回参照時に作る `<name>()` アクセサを出します。trait のメンバのときは interface が `<name>()` を abstract で宣言し、実装クラス側がフィールドとアクセサを持ちます（`lazy val` の mixin と同じ形）。非 static な `object` の中の `object` も同じく非 static です（`class Outer { object P { object N } }` の `N` は `$outer: Outer$P$`）。クラスにネストした `case class` のコンパニオンも同じ扱いで、`copy` は自分の `$outer` を新しいインスタンスへ渡します。詳細は「ネストした型」節
- メソッド本体の中で定義したクラス（匿名クラス `new T { … }` と**ローカル `class` / `object`**）が、**囲みメソッドのパラメータ / ローカルをキャプチャ**する。nsc と同じ形で、自由変数ごとに `x$1` という public final フィールドと、末尾に付く追加のコンストラクタ引数を出す。各インスタンスメソッドの先頭でそのフィールドをローカルスロットに読み戻すので、キャプチャした `var` の `scala.runtime.*Ref` 経由の読み書きも、匿名クラス内のラムダによる二重キャプチャ（`$captured$N`）も、既存の経路のまま動く。メソッドの中のクラスにも `$outer` が付き、囲みクラスのメンバは `$outer` チェーンで読む
- eta-expansion `foo _` と、FunctionN が期待される位置への未適用メソッド（`xs.map(inc)`）。ネストしたパラメータリストは **uncurry** で 1 リスト + クロージャになる。SIP-21 の SAM: ラムダ / 未適用メソッドを `Runnable` / `java.util.Comparator[Int]` / `java.util.function.Function[A,B]`（単一抽象メソッド）に適合。SAM でない型へは type mismatch（黙ってラップしない）。`def go(): Unit` を `_` なしで `Runnable` に渡すのは nsc と同じく auto-apply して mismatch。合成クラスは既存の anonfun と同じく invokedynamic は使わない
- **コンストラクタ引数のアクセサ**。`class C(val x: Int)` も、キーワード無しで `val` になる **`case class` の第 1 引数リスト**も public なアクセサ `x()` になり、親の抽象メンバーを実装する（親が `def value: T` を `()Object` に erase する場合はブリッジも出す）。第 2 引数リスト以降は nsc と同じく private な状態のまま。`var` 引数は `x()` と `x_$eq(v)` の両方
- **`FunctionN.tupled` / `curried`（arity 2〜22）と `scala.Function.untupled`（2〜5）**。`scala/FunctionN` の default メソッドと `scala/Function$` なので **jar リンク時のみ**（`--no-scala-library` では診断する）。あわせて、引数リストを持たないメソッドの結果が関数ならその引数リストは関数のもの（`def g: Int => Int; g(3)`）、カリー化された**関数値**の `f(1)(2)` は 2 回の `Function1.apply`（メソッドのカリー化とは違って平坦化しない）
- **`scala.collection.mutable.Builder` の `+=` / `++=`**（`Growable` の default メソッド。`this.type` を返すので受け手の型がそのまま返る）。jar リンク時のみ
- `super` / 修飾付き `this`（`Outer.this`）。trait の `super` は、具象クラスなら `T$class`、スタック可能な `abstract override` なら `T$$super$m` 経由
- **オーバーライドの適合検査**（SLS 5.1.4 / 5.2.6。`crates/typer/src/override_check.rs`）。結果型の共変性（`incompatible type in overriding`）、パラメータ型の不変性（違えばオーバーロード。`override` を付けていれば `method f overrides nothing.` ＋ scalac と同じ `Note:`）、`override` 修飾子の要否、deferred な再宣言が下の具象実装を打ち消すこと、`final`、可視性を狭められないこと（`weaker access privileges in overriding`）、`val` は `def` を覆えるが逆は不可・具象 `var` は覆えないこと、型パラメータの個数と境界、そして**抽象メンバの実装漏れ**（`class X needs to be abstract.` / `object creation impossible.`）。文面は実 scalac 2.13.16 のもので、オーバーライドされた側は**オーバーライド地点から見た形**でエコーする。prelude と pickle 由来のメンバはフラグ（`FINAL` / `DEFERRED`）を運んでいないので、`final` と実装漏れの検査は**ソースと Java classfile 由来のメンバに限る**（詳細と残件は「オーバーライドの適合検査」節）
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
- compound types: `A with B` を値 / パラメータの型として使い、両側のメンバーを呼ぶ。**型**としてはクラスが二つあっても通る（nsc と同じ。値が無いだけ）。テンプレートに二つ目のクラスを混ぜる（`class C extends A with B`）のは `class B needs to be a trait to be mixed in` で診断する
- 構造的 refinement: `{ def foo: Int }` / `T { def foo: Int }`。実行時は **Java reflection**（`getClass` / `Class.getMethod` / `Method.invoke` + unbox）。2.13 の reflective call と同じ実行意味論のサブセット。`scala.language.reflectiveCalls` は要求しない。**構造的代入** `x.foo = v`（`{ var foo: T }` または getter + `foo_=`）と構造的 `x(i) = v`（`update`）。nsc どおり reflective `foo_=` / `update`。違法な `{ def foo: Int }; x.foo = 1` は `foo_= is not a member`。本体付き `def` は診断する
- self type: `trait T { self: Foo => ... }` の typecheck と mixin。実装クラスが self type に適合しないと `illegal inheritance`
- 変性: `class C[+A]` / `class Box[+A](val x: A)` は合法。`class Bad[+A](var x: A)` は nsc と同様 covariant-in-contravariant で拒否。`A @uncheckedVariance`（メソッド引数や型引数位置）は nsc と同じくその出現の変性検査を外す

- **def マクロの定義**: `def f: T = macro Impl.method[A]`。パースし、実装参照を解決して
  `Impl$` / `method` のバインディングをシンボルに記録し、マクロ def のバイトコードは
  nsc と同じく**出さない**（だから Java から呼べない）。戻り値型の省略 / object のメソッド
  でない実装 / `Context` を第 1 引数に取らない実装 / 解決できない参照 / whitebox は診断する。
  設計は [`docs/macros.md`](docs/macros.md)
- **def マクロの展開（JVM ブリッジ）**: nsc と同じく、マクロ実装の classfile を
  **JVM 上で本当にロードして呼ぶ**。`java` と scala-reflect.jar があれば
  `def f(): Int = macro Impl.m` の呼び出しが展開され、展開後のプログラムが走る。
  engine は Java 1 ファイル（`crates/typer/java/ScalaRsMacroEngine.java`）で、
  初回展開時に `javac` して `$TMPDIR` にキャッシュし、コンパイル 1 回につき
  1 プロセスを常駐させる。`Context` は `Proxy` で作り、`universe` には
  `scala.reflect.runtime.universe` を差す。**nsc と同じく、マクロ実装は
  前の run でコンパイル済みでなければならない**（同じ run にあると
  `is not on the macro classpath` という理由つきで診断する）。渡せる引数の形
  （`Literal` / `Ident` / `Select` / `Apply` / `this`）、作れるタグ（型引数の無い
  クラス、明示された型引数のみ）、戻せる木の種類は**部分集合**で、外れる形は
  すべて名指しで診断する（黙って違う木に展開しない）
  （[`docs/macros.md`](docs/macros.md) §7.11）
- **quasiquote（`q"..."`）の reification**: `q"..."` / `tq"..."` / `pq"..."` / `cq"..."` は
  `StringContext` の普通の補間子ではなく、nsc の**コンパイラ内蔵マクロ**である。
  補間文字列の中身を（`$x` / `${…}` / `..$xs` / `...$xss` をプレースホルダに置き換えて）
  **scala-rs のパーサで実際に構文解析し**、`q"..."` については
  `<universe>.internal.reificationSupport.Syntactic*` の呼び出しに脱糖して、
  普通の式として型検査・コード生成する（`crates/typer/src/reify.rs`）。
  universe は `import <universe>._` から採る。落とせるのは
  リテラル / 名前 / 選択 / 適用（カリー化含む）/ `$x` 穴 / `..$xs` 穴に加えて、
  **`tq"..."`（型識別子・選択・型適用・関数型・タプル型・特異型・型射影・複合型）、
  `pq"..."`（`Bind` / 抽出子 / `|` / `_: T` / 安定識別子）、`cq"..."`（`CaseDef`）、
  そして `q"..."` の型注釈 / eta 展開（`f _`）/ 型適用 / ブロックと `val` 定義 /
  `new` / `match` / 部分関数 `{ case … }` / 関数リテラル / `this` / 代入 /
  `if`-`else` / タプル**。演算子名は `NameTransformer` で符号化する。
  **定義**も落とせる（`crates/typer/src/reify_defs.rs`）: `class` / `case class` /
  `trait` / `object` / `def` / 修飾つきの `val`・`var`（`SyntacticClassDef` /
  `SyntacticTraitDef` / `SyntacticObjectDef` / `SyntacticDefDef` /
  `SyntacticValDef` / `SyntacticVarDef`）。`Modifiers` のフラグは
  `scala.reflect.internal.Flags` のビット（パーサの番号とは**別物**）に翻訳し、
  nsc のパーサが補う親（`AnyRef`、`case` なら `Product with Serializable`）と
  クラス・パラメータのアクセサ・フラグ（`PARAMACCESSOR` / `CASEACCESSOR` /
  `PRIVATE | LOCAL`）、末尾の implicit 節（`ImplicitParams`）、
  匿名クラスの本体（`new C { … }`）まで再現する。
  形はすべて実 scalac 2.13.16 の `-Ymacro-debug-lite` から読み取り、
  `showRaw` まで実 scalac と一致することを確認している。**落とせない形は必ず
  `unimplemented syntax: quasiquote q"..." (どの形か)` で診断する**（黙って通さない）。
  残っているのは、パーサが nsc の保つ区別ごと正規化してしまう形
  （`else` の無い `if`、by-name 型）と、`..$` と普通の引数の混在、`type` 定義
  （[`docs/macros.md`](docs/macros.md) §7.4 / §7.7）
- **fresh 名を要する 3 形**: `_` プレースホルダ関数リテラル（`q"_.get"`）、
  `_` 型引数＝存在型（`tq"P[_, _]"`）、右結合演算子（`q"a :: b"`）は、
  nsc の展開が 1 個の式ではなく
  `val n = rs.freshTermName("x$")` を先に置く**ブロック**である
  （名前は実行時に universe のカウンタから引く）。scala-rs も同じブロックごと組む。
  中置 `a :: b` とドット呼び `b.::(a)` はパース後に同じ木になるので、
  選択の span のテキストが演算子で始まるかで見分ける。パターンの中の裸の `_`
  型引数は型変数パターン（`u.Bind(u.TypeName("_"), u.EmptyTree)`）で fresh 名を
  使わず、境界つきはパターンの中でも存在型になる
  （[`docs/macros.md`](docs/macros.md) §7.10）
- **`Liftable`（`Tree` でない穴）**: 穴の引数は `Tree` でなくてよい。nsc は
  implicit `Liftable[T]` を探して `Liftable.liftX[T](arg)` を差す。scala-rs は
  implicit 探索はせず、**引数の型から標準インスタンスを選び、そのインスタンスが
  作るのと同じ木を直接組む**。`Int` / `String` などのリテラルは
  `u.Literal(u.Constant(v))`、`Constant` は `u.Literal(c)`、`Type` は
  `rs.mkTypeTree(t)`、`WeakTypeTag` / `TypeTag` は `rs.mkTypeTree(tag.tpe)`、
  `Expr[T]` は `e.tree`、`Symbol` は `rs.mkRefTree(u.EmptyTree, sym)`、
  `Name` は立っている位置しだいで `SyntacticTermIdent` / `SyntacticTypeIdent` /
  `Bind`、`..$xs` は要素ごとに `xs.toList.map(v => …)`。
  型を知るために reify の前に引数を投機的に型付けする（診断は巻き戻す）。
  **標準インスタンスの無い型は名指しで診断する**
  （`a hole of type X is not lifted (…)`）。ユーザ定義の `Liftable` は探さない。
  slick の `ShapedValue.mapToImpl` の `q"($rModule.tupled) : ($uTag => $rTag)"` が
  この形（[`docs/macros.md`](docs/macros.md) §7.8）
- **`symbolOf[T]` / `weakTypeOf[T]` / `typeOf[T]` が見つかること**: 型パラメータを
  implicit 節にしか書かないメンバ（materialiser の形）は
  `pin_undetermined_tparams` が丸ごと落としていたので `not found: value symbolOf`
  だった。節が implicit だけで、その implicit が当の型パラメータを要求する形に
  限って残す（`classTag[Short]` と同じく常に明示型引数で呼ばれる）。
  マクロ実装の中では `implicit rTag: c.WeakTypeTag[R]` が implicit を埋めるので
  `symbolOf[R]` が実際に解ける
- **`TypeTag` / `WeakTypeTag` の materialization**: `typeOf[T]` の implicit は
  プログラムのどこにも書かれていない。nsc は「見つからない」と言わず、
  コンパイラ内蔵マクロ `materializeTypeTag[T](u)` を展開して**その場でタグを作る**。
  scala-rs も同じ位置（`fill_implicit_params_in` の `ClassTag` フォールバックの隣）で、
  `TypeCreator` の無名クラスを含むブロックを組んで普通の式として型検査する
  （`crates/typer/src/materialize.rs`）。作るのは
  `<universe>.TypeTag.apply[T](<universe>.rootMirror, new $typecreator1())` で、
  creator の本体は `$m$untyped.staticClass("Foo").asType.toTypeConstructor`。
  universe は `import <universe>._` の prefix から決める（quasiquote と同じ読み方）。
  **木は nsc と同じでなくてよく、`tag.tpe` の実行結果が一致する**ことを
  実 scalac 2.13.16 との dual-run で見る（`tests/fixtures/tt_tags.scala`、30 行一致）。
  `staticClass` はクラスを 1 つ名指しする呼び出しなので、組めるのは
  **型引数の無いトップレベルのクラス型**（＋基本型 / `Unit` / `String` / `Any` /
  `AnyVal` / `Nothing` / `Null`）だけで、`List[Int]` / 入れ子クラス / `AnyRef` /
  型パラメータ / singleton 型は
  `materialisation is not implemented: cannot build a TypeTag for ...` と
  **その形を名指しして**断る（黙って別の型を作らない）。
  ついでに、`TypeTags$TypeTag$` のシンボルが無いこと（トレイトの入れ子オブジェクトの
  classfile は自分の pickle を持たない）、`typeOf` の implicit パラメータが
  未解決の `Type::Named` だったこと、`TypeTags#TypeTag` アクセサが供給されないことを
  直した。slick の `c.typeOf[HList]` / `typeOf[Tag]` はこれで通る
  （[`docs/macros.md`](docs/macros.md) §7.10）
- **`reify { … }` の診断**: `reify` は quasiquote と同じコンパイラ内蔵マクロで、
  scala-reflect.jar に実装が無い。`value reify is not a member of JavaUniverse`
  という**誤った**診断をやめ、
  `macro expansion is not implemented: cannot expand reify { ... }` と言う
  （`else` の無い `if`、by-name 型、by-name / 可変長パラメータ、
  手続き構文 `def f() { … }`、パターン定義、自分型、early definition）と、
  `..$` と普通の引数の混在、`type` 定義
  （[`docs/macros.md`](docs/macros.md) §7.4 / §7.7 / §7.8 / §7.10）
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
  この形。**オーバーロード集合でも働く**: reflect API は
  `val Ident: IdentExtractor` と `def Ident(name: String): Ident` を並べて置くので、
  `Ident(TermName("x"))` はどちらの候補にも当たらず `Ident.apply(...)` になる
  （`Bind` / `This` / `New` も同じ形。slick の `TableQuery` のマクロ実装は
  これだけで書かれている）
- **同名の型メンバに項の選択が食われないこと**: reflect API は
  `type Modifiers` と `def Modifiers(flags: FlagSet)` を両方置く。jar のメンバは
  名前ごとに遅延ロードされるので、型メンバが先に入ると項のオーバーロードが
  読まれないまま `u.Modifiers(flags)` が `<notype>` に解決していた
- **`import <値>._`**: プレフィクスが object でも package でもなく**値**のとき、
  その値の*型*のメンバを入れ、無修飾の参照を `値.メンバ` に書き戻す
  （`import c.universe._` の形）。**jar のクラスの継承メンバにも届く**:
  そのメンバは名前ごとに pickle から遅延ロードされるので、
  `import scala.reflect.runtime.universe._` の `TermName` / `Literal` /
  `Constant` / `termNames`（いずれも linearization の上の方の宣言）は
  以前ひとつも入っていなかった。**型名前空間も別に露出する**
  （reflect API は `val TermName` と `type TermName` を両方置く）。
  書き戻しに使うプレフィクスは**そのスコープの中だけ**で有効
  （メソッドローカルの `import u._` を別のメソッドで使うと、
  死んだローカルへの `getfield` になっていた）
- **マクロ実装のシグネチャ（`c.Expr[T]` / `c.Tree` / `c.WeakTypeTag[T]`）**:
  `blackbox.Context` が `scala.reflect.macros.Aliases` から継承する型別名。
  jar のクラスの**型メンバ**を pickle から読めるようにしたので
  （`PickleSupply::complete_type_member`）、マクロ実装のソースが
  scala-reflect.jar 越しに型検査できる。精製型のレシーバ
  （`blackbox.Context { type PrefixType = … }`、slick の `mapToImpl` の形）
  からも引ける。別名は透過で、`c.Tree` は `Trees.Tree` そのもの。
  **前置詞（prefix）は落とす**ので、別々の `c` の `Expr` はここでは同じ型になる
  （nsc では別の型。バイトコードに出る消去後のシグネチャは同じ
  `scala/reflect/api/Exprs$Expr` なので、出力は変わらない）。
  scala-reflect.jar が classpath に無いときは空の `Context` のまま
  `value universe is not a member of Context` と診断する
  （`--scala-library` は scala-reflect.jar を含まない）

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

`Unit` が `V` になるのは**メソッドの戻り値だけ**です。パラメータ・フィールド・配列要素・型引数では nsc と同じく `scala/runtime/BoxedUnit` に erase し、値は `BoxedUnit.UNIT` シングルトンです（`Nothing` は同様に `scala/runtime/Nothing$`）。詳しくは「`Unit` の引数と `scala.runtime.BoxedUnit`」を参照してください。

### メソッド型パラメータの推論（引数＋期待型）

nsc の `instantiateExpecting` と同じく、メソッドの型パラメータは**引数と期待型の両方**を制約として解きます（`crates/typer/src/check.rs` の `add_expected_constraints`）。

- 結果型の**不変位置**では期待型が引数の解より優先します。`Array` は非変なので `val a: Array[AnyRef] = Array("x", "y")` は `T = AnyRef`（`[Ljava.lang.Object;`）、`val b: Array[Any] = Array(1, 2)` は `T = Any` でボックスされます。
- **共変位置**の期待型は上界にすぎないので引数の解が勝ちます（`cov("q"): List[Any]` は `T = String`）。
- 解いた型引数は**implicit 引数リストの解決より前**に確定します。`def column[T](n: String)(implicit tt: TypedType[T]): Rep[T]` を `Rep[Int]` の位置で呼ぶと `TypedType[Int]` を探しに行きます。
- どちらでも決まらない型パラメータは `Nothing` で埋めず、nsc と同じ診断（`could not find implicit value …`）を出します。
- 引数は、パラメータのクラスにおける**基底型**に直してから突き合わせます（nsc の
  `Types.baseType`。`check.rs` の `align_to_param_class` / `base_type_instance`）。
  `object OD extends D[Int]` を `def u[A](d: D[A])` に渡すと、引数の型は `OD.type`
  なので `D[Int]` は基底型としてしか見えません。`this.type` / `p.type` も同じで、
  単一型はまず**それが広がる先**を読みます（`agent/hkinfer`）。

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

#### 期待型は引数の**プロトタイプ**でもある（nsc `protoTypeArgs`）

引数が一つも型付けされる前から、期待型は呼び先の型パラメータの一部を言い当てています。
nsc の `Infer.protoTypeArgs` はそれを引数の期待型（プロトタイプ）として渡します。
scala-rs も同じことをします（`check.rs` の `proto_arg_type`）。

```scala
def f(s: AnonSymbol, a2: Aggregate): (Node, Map[TermSymbol, Aggregate]) =
  (Select(...).infer(), Map(s -> a2))
```

`Map` はキーについて**非変**なので、`Map(s -> a2)` を期待型なしで型付けすると
`Map[AnonSymbol, Aggregate]` になり、`Map[TermSymbol, Aggregate]` には適合しません。
プロトタイプがあれば nsc と同じ `Map[TermSymbol, Aggregate]` になります。

- 対象は**パラメータ型がそのまま型パラメータ**である位置だけです（`Tuple2.apply[T1, T2]`）。
  それ以外に広げると、オーバーロードの候補をプロトタイプが選び始めてしまいます。
- 呼び先がオーバーロードのときはやりません。
- プロトタイプは**ヒントであって制約ではありません**。それで型付けするとエラーになる引数
  （implicit 節が残っている `kvs.toMap` など）は、診断ごと巻き戻して期待型なしで
  型付けし直します。

#### 空の可変長引数と `xs: _*`

可変長パラメータに**何も渡さなかった**呼び出しには、要素の型を決める材料がありません。
nsc はそれを制約なしと見て下界（`Nothing`）へ最小化します。scala-rs は呼び先のシグネチャに
その型パラメータが「出てくる」ことだけを見て「引数が解くはず」と判断していたので、
`List()` / `Seq()` / `Map()` は呼び先の型パラメータを抱えたままになり、何にも適合しません
でした。渡した引数の数を見て、空の可変長パラメータは無かったことにします。

`xs: _*` の引数は、要素型に `Repeated` の印を付けた型で届きます。パラメータ側は
`param_at` が既に要素へ剥がしているので、**片側だけ剥がす**と
`def mk[A](xs: A*)` が `A = Int*` に解けて `mk(xs: _*)` が `List[Int*]` になっていました。
両側を剥がします（`unify_tparam_all` / `unify_one`）。`Map(kvs: _*)` /
`Seq(xs: _*)` / `Vector` / `Set` / `Array` の各ファクトリも同じ扱いです。

#### 依存メソッド型（nsc `dependentTypeMap`）

```scala
def get[P <: Phase](p: P): Option[p.State]
```

`p.State` は**引数**の `State` であって、`Phase` が宣言している抽象型メンバではありません。
scala-rs の `Type::TypeMember` は接頭辞を持たないので、`get(Phase.assignUniqueSymbols)` の
結果は `Option[Phase#State]` のままになり、`.map(_.aggregate)` が `Any` に落ちていました。

接頭辞が型に無いので、**接頭辞になり得たパラメータをその境界から探します**
（`check.rs` の `subst_dependent_members`）。抽象型メンバの所有者を基底型に持つ
パラメータが**ちょうど一つ**のときだけ、その引数の同名メンバで置き換えます。
引数側でも抽象なら何も変わりません。置き換えたあとも普通に型検査されるので、
`val bad: Option[String] = (new CS).get(new P1)` は
`type mismatch; found: Option[Int]  required: Option[String]` のままです。

#### 高階の適用（`F[B]`）

`F` が抽象な型構築子（`F[_]`）のとき、`def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]`
の結果型は `Type::Class` ではなく `Type::Applied` です。期待型から型パラメータを
解く `collect_expected` はこの形を見ておらず、`B` は期待型 `F[String]` からも
引数からも決まらないまま `Any` になっていました（cats 風の
`F.flatMap(fa) { … }` がすべて `F[Any]`）。`Applied` 同士は構築子と引数を
位置で突き合わせます。型構築子の引数位置には変位注釈が無いので**不変**扱いです
（＝期待型が引数側の解を上書きできる位置）。期待型がすでに実クラスに落ちている
形（`F[B]` 対 `List[String]`）では、構築子を**適用前**の `List` として
突き合わせ、`F` 自体が `List[String]` に解けないようにします。

### Implicit 解決

nsc に寄せた探索順です。偽の「何でも変換」はありません。

1. 現在のスコープと、囲んでいるクラス / object の `implicit` メンバー（親 class / trait から inherited したメンバーと、`import Foo._` で入れたメンバーを含む）
2. 囲んでいるパッケージのパッケージオブジェクト（`package object p` の implicit メンバー）
3. 目標型の部分（型コンストラクタ・型引数・ネストした prefix）と、その **基底クラス** のコンパニオン（`Option[T]` なら `Option`、`Outer.Inner` なら `Inner`、`A =:= B` なら `=:=` が継承している `<:<` のコンパニオン）。変換なら元の型の部分も見る。コンパニオンが jar にしかないときは、探索の直前にその classfile を読み込んで
   pickle から implicit だけを補います（**`scala.*` も含みます** — prelude が
   describe するのはプログラムが名前で書くものだけで、`scala.collection.BuildFrom`
   のように prelude がコンパニオンを与えていないクラスの witness は、
   コンパニオンを入れない限りどのスコープにも現れません）。コンパニオンが
   mixin したトレイトの宣言も同じように辿ります
   （`object BuildFrom extends BuildFromLowPriority1 extends BuildFromLowPriority2`）

呼び出し側で implicit パラメータ節を明示できます: `add(5)(3)` / `foo(x)(ev)`。探索で埋めるのは、その節が省略されたときだけです。

数値の widening（`Int` → `Long` / `Double` など）は **implicit 探索の前** に特別扱いします。scalac の implicit ではなく、typer の組み込みです。

継承した implicit メンバーは**親の型引数を通して**見ます（as-seen-from）。
`trait Base[P1] { protected[this] implicit def p1Type: TT[P1] }` を
`trait Mid[P1] extends Base[P1]` から使うとき、候補の型は `Base` の `P1` ではなく
`Mid` の `P1` です（`Typer::implicit_candidate_ty`）。ここを素の宣言型のままにすると
`implicitly[TT[P1]]` が自分の親の実装を見つけられません（slick の
`Library.Abs.column[P1](n)`）。

`import <値>._` で入れた implicit も同じで、**その値の型を通して**見ます
（`Typer::at_import_prefix_of`）。`class Box[T] { implicit def mkOps(lhs: T): Ops[T] }`
を `b: Box[Int]` から使えば `Int => Ops[Int]` です。`Box` の `T` のままだと候補が
何にも当たりません。結果が「その generic クラスに**ネストした**クラス」のときも同じ
prefix で置換します（`Ordering[T]#OrderingOps` の `def <(rhs: T)` の `T` は
`OrderingOps` ではなく `Ordering` のパラメータです）。
さらに、この implicit は**インスタンスメンバ**なので、参照はその値を receiver として
名前修飾します。素の名前で出すと codegen が `this` を積んでキャストし、
`class Main$ cannot be cast to class NoTp` になっていました。
サブクラスが**オーバーライド**した変換は候補 1 個です
（`Integral[T]` は `Numeric[T]#mkNumericOps` の結果を `NumericOps` から
`IntegralOps` に狭めます。結果クラスが違うので「同じ変換に 2 経路」の規則では
落ちず、探索が諦めていました）。

jar のメンバーは**名前を 1 つずつ**読みます。ところが implicit は
「スコープを探して見つける」ものなので、プログラムがその名前を書くことは決してなく、
`Numeric#mkNumericOps` も `Option.option2Iterable` もどのメンバー一覧にも
入っていませんでした（slick の `import seq.integral._` と `where.reduceLeft(f)`）。
`import <値>._` と「型の implicit スコープにあるコンパニオン」の両方で、pickle に
**どの名前が implicit か**を聞いて、その名前だけを通常の on-demand 経路で
補完します（`PickleSupply::implicit_member_names`）。クラスがすでにメンバーを
持っている名前は聞かないので、手書き prelude の宣言が勝つのは従来どおりです。
プリミティブのコンパニオンは対象外です（`object Int` の implicit は数値 widening
そのもので、typer が組み込みで持っています。view として並べると `n + ":"` が
ambiguous になるだけです）。

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

#### 関数型の implicit パラメータ（view）を implicit def から埋める

SLS 7.2 / 6.26.5 の view です。`A => B` 型の implicit パラメータは、`A => B` の **値**が
無くても、`A` から `B` への **implicit conversion** を **eta 展開した関数値**で埋まります。
実 scalac 2.13.16 は `def h[A](x: A, y: A)(implicit ev: A => Ordered[A])` の呼び出しに
`$anonfun$main$1(int) = Predef.intWrapper(x)` /
`$anonfun$main$2(String) = Ordered.orderingToOrdered(x)(Ordering.String)` を渡します。

scala-rs にはこの経路がありませんでした。`fill_implicit_params_in` は `A => B` 型の
**値**だけを探し、見つからなければ `identity_view`（`A <: B`）と `array_wrap_view` という
二つの決め打ちしか試さないので、implicit def は候補にすら入らず、
view bound（`def f[A <% B]`、同じ implicit パラメータに脱糖される）ごと
`no implicit: could not find implicit value of type (Int) => Ordered[Int]` でした。

`crates/typer/src/views.rs` の `Typer::conversion_view` が塞ぎます。`Ordered` 専用の
特別扱いではありません。普通の view 探索 `search_conversion(A, B)` に訊き、見つかったら
`(x$n: A) => x$n` を組み立てて **本体を `adapt` に `B` へ適応させる**だけです。
`val b: B = (a: A)` が通るのとまったく同じ経路・同じ候補選択で、任意の `A => B` に効きます。
ラムダの型付けは診断マーク付きで行い、本体の適応が何か報告したら巻き戻して `None` を返すので、
探索が実際に witness を出さなかったものを受け入れることはありません
（`def h[A](x: A)(implicit ev: A => Ordered[A])` に `new Object` を渡せば、
実 scalac の `No implicit view available from Object => Ordered[Object]` と同じく拒否）。

あわせて `search_conversion` の候補判定（`implicits.rs` の `conversion_provides`）が
**多相な implicit def** を見るようになりました。それまでは宣言型のまま比較していたので、
自分の型パラメータを持つ変換は view 探索から丸ごと見えず、
`implicit def boxit[T](x: T): Box[T]` があっても `val b: Box[Int] = 3` は通りませんでした。
いまはメンバー選択側の探索（`conversion_result` / `conv_targs`）と同じやり方で、
引数型から候補の型パラメータを解いてから結果型を比べます。自分の implicit 節に witness が
無い変換は nsc と同じく適用対象外です（そうしないと `orderingToOrdered` が
`Box[Int] => Ordered[Box[Int]]` を名乗って、あとから `Ordering[Box[Int]]` が作れずに落ちます）。

#### ローカルスコープの implicit 変換（view）

`agent/localconv` スライス。実 scalac との差分テストで見つかった非対称です:
implicit パラメータの探索（`fill_implicit_params_in` → `Typer::implicits_in_scope`）は
メソッド本体 / ブロック / ラムダ本体に書いた `implicit val` / `implicit def` をちゃんと
見つけるのに、view 探索（`search_conversion` / `search_extension`）は SLS 7.3 の言う
「implicit パラメータと同じ候補プール」を実際には見ていませんでした。両方とも
`implicits_in_scope` を呼んでいるので同じスコープ連鎖を歩くはずなのに、根本原因は
3 つとも view 探索そのものではなく、その手前にありました。

1. **`Typer::type_def_sig` が `Flags::IMPLICIT` をローカルの `def` にコピーしていなかった。**
   クラス / object のメンバーは namer（`namer_member`）が `type_def_sig` より前に
   完全な flag（`implicit` 込み）でシンボルを確保しているので問題が出ませんが、
   ブロック内のローカル `def` には namer パスが無く、`type_def_sig` 自身が
   `tree.sym.is_none()` を見て `Flags::EMPTY` で新規にシンボルを確保します。
   その後 `LOCAL` / `PRIVATE` / `PROTECTED` はコピーするのに `IMPLICIT` だけ
   コピーしていなかったため、ローカル `implicit def` はブロックのスコープに
   正しく入っているのに、どの探索（`implicits_in_scope` は `Flags::IMPLICIT` で
   絞り込む）にも一切見えていませんでした。
2. **`implicit class` の desugar（`Typer::implicit_class_conversions`。
   `implicit class C(x: P) { … }` → 合成 `implicit def C(x: P): C = new C(x)`）が
   クラス / module のメンバーに対してしか走っていなかった。** ブロックの
   `TreeKind::Block` 処理はローカル `class` / `object` を名前解決するだけで、
   このデシュガーを一度も呼んでいなかったので、ローカル `implicit class` は
   変換メソッドそのものが存在せず、探索する以前の問題でした。
3. **`implicits_in_scope` のスコープ探索がシャドーイングをしていなかった。**
   SLS 7.2 の候補は「プレフィックス無しで参照できる識別子」、つまり普通の
   非修飾名前解決の対象で、これは**シャドーする**はずです。ところが実装は
   スコープスタックの全レベルを just walk して `Flags::IMPLICIT` を持つシンボルを
   重複除去なしで集めていたので、外側の `implicit def i2s` と同名のローカル
   `implicit def i2s` が「シャドーされて 1 個だけ見える」ではなく
   「2 個の候補があって `ambiguous implicit: i2s, i2s`」になっていました。
   いまは内側から外側へスコープを歩きながら「このスコープで初めて見る名前」だけを
   採用し、一度採用した名前は外側のスコープでは無視します（同名のインスタンス
   メンバー / package object メンバーも同様にシャドーされます）。

3 つとも直したうえで、副作用として見つかったもう一つの独立したバグも直しています:

4. **`crates/typer/src/lambda_lift.rs` の自由変数解析が、ローカルクラスを
   `new` するネストしたローカル `def` に、そのクラス自身が要求する capture を
   伝播していなかった。** `implicit class` のデシュガー結果（`new C(x)`）は
   必ず合成メソッドという「別のネストしたローカル `def`」の中に置かれるので、
   `class C(...) { ... factor ... }` のようにローカルを capture するクラスを
   ローカル `implicit class` にすると必ず踏みます。scalac が受理する
   `val factor = 10; class F(...) { def scaled = n * factor }; def helper() = new F(3).scaled`
   のような**素の**（implicit と無関係な）コードでも同じ形で再現し、
   `RuntimeException: cannot capture factor` を実行時に投げていました。
   `Symbol::captures`（どのローカルを capture するか）は `mark_anon_captures` が
   計算しますが、ドライバはそれを `lambda_lift` の**あと**に走らせるため、
   `lambda_lift` 自身の自由変数解析（`collect_captures`）が `new F(x)` を見た
   時点ではまだ空でした。`lambda_lift` の入口で `mark_anon_captures` を先に
   一度呼んで（ドライバの 2 回目の呼び出しはリフト後の木に対して再計算するだけで
   無害）`Symbol::captures` を先に埋め、`collect_captures` の `New` の枝で
   参照先クラスの capture を（`own` でフィルタしながら）自分の capture にも
   加えるようにしました。

優先順位はローカル > import（スコープに入るのでローカルと同列） > コンパニオン
のまま変えていません（`search_conversion` / `search_conversion_open` /
`view_undet_bindings` はどれも先に `implicits_in_scope`、空ならコンパニオン、
という順序）。

#### 埋まらなかった implicit 節を黙って eta 展開しない

implicit しか取らないメソッドは値ではありません。nsc はその節を適用するか、
足りない implicit を報告するかのどちらかで、三つ目の結末はありません。
scala-rs には三つ目がありました。`adapt_implicit_apply` は何箇所かで諦めます
（`TypeApply` 待ち、あるいは期待型がまだ分からない引数の型付け中）。誰も後から
節を適用しなかったとき、**メソッド型がそのまま式の型として残り**、`adapt` が
それを**関数値へ eta 展開**していました。
`println(List(Some(1), None, Some(3)).flatten)` はエラー無しで通り、実行時に
`Main$$$anonfun$0@7a765367` を印字します。**サイレントな誤コンパイル**です。
型が見える書き方（`List(Some(1)).flatten.sum`）にすると同じ木が
`value sum is not a member of ((Some[Int]) => IterableOnce[B])List[B]`
として表に出ていました。

`Typer::reject_unapplied_implicit_clause` がその歯止めです。`adapt` は、木が
既知の期待型のもとで**値として**使われるときにしか走らず、その同じ期待型で
`adapt_implicit_apply` は既に一度試しています。だからここまで残った第一節は
もう誰も埋めません。足りない implicit として報告し、eta 展開はしません。
期待型がメソッド型のとき（＝外側の `Apply` が適用する途中）と、第一節に
非 implicit のパラメータがあるとき（＝本当に eta 展開できる）は対象外です。

あわせて、関数型の implicit パラメータが**呼び出し側の未決定型パラメータ**を
持つ場合、その view から解けるようにしました
（`crates/typer/src/implicits.rs` の `view_undet_bindings`）。
`flatten[B](implicit asIterable: A => IterableOnce[B])` の `B` は呼び出しの
どこにも現れないので witness だけが決められますが、その witness は値ではなく
変換です。変換の結果型を期待型と単一化して `B` を解きます
（候補側は `Unify` が基底型へ広げるので `Iterable[Int]` を `IterableOnce[B]`
に合わせられます）。

`scala.math.Ordered` のコンパニオンと
`implicit def orderingToOrdered[T](x: T)(implicit ord: Ordering[T]): Ordered[T]` は
`crates/typer/src/prelude_durrange.rs` で宣言します（`javap -p -s scala.math.Ordered$` の
とおり `Ordered$` のメンバーはこれ一つ）。`--scala-library` 専用です。私有ランタイムは
`scala/math/Ordered` は出しますが `Ordered$` も `Ordering` も出さないので、jar なしでは
これまでどおり `no implicit: …` を出します。view 経路そのものは jar に依存しません
（`tests/fixtures/dr_viewuser.scala` は `--no-scala-library` でも通ります）。

#### prelude の穴

- `scala.concurrent.duration` の後置単位（`5.seconds` / `100.millis` /
  `1.second + 500.millis`）。`package object duration` の
  `implicit def DurationInt(n: Int): DurationInt`（`DurationLong` / `DurationDouble` も）と、
  `DurationConversions` の単位メソッド 20 本（`nanoseconds` / `nanos` / `nanosecond` /
  `nano`、`micro` 系 4 本、`milli` 系 4 本、`seconds` / `second`、`minutes` / `minute`、
  `hours` / `hour`、`days` / `day`）です。`Duration(5, SECONDS)` と `Duration.Inf` は
  もともと jar から読めていました。
  これらは value class なので、`javap` 上の conversion は `DurationInt(int)int` と
  **消去された恒等**であり、classfile リーダはそれを `Int => Int` として読み、`IMPLICIT`
  も付きません（pickle からなら読めますが `PickleSupply` は `scala/` を除外します）。
  それが `value seconds is not a member of 5` の全部です。単位メソッドは箱側の
  `package$DurationInt` に**普通のインスタンスメソッド**として実在する（`$extension` は
  `durationIn` / `hashCode` / `equals` だけ）ので、箱クラスは classfile から読み、
  足すのは conversion だけです。scalac は `5.seconds` を
  `new package$DurationInt(5).seconds()` に落とすので、conversion の codegen は
  `Intrinsic::NewWrapper`（`new <箱>(引数)`）です。
  package object は遅延ロードなので、この導入も `Typer::package_object_of` から遅延で
  行います（`FiniteDuration` は jar を読むまで symbol が存在しないため）。
  `crates/typer/src/prelude_durrange.rs`。`--scala-library` 専用。
- `Range` のコンパニオン `Range$`。prelude はクラス `Range` しか宣言しておらず、
  term 位置の識別子 `Range` はクラスシンボルに解決されていました。`Range(0, 5)` は
  その**クラス自身の** `apply(i: Int): Int`（要素アクセサ）を見つけてしまい、
  `no matching overload for (Int)Int` になっていました。`javap -p -s
  scala.collection.immutable.Range$` のとおり `Range$` にあるのは `apply` 2 本 /
  `inclusive` 2 本 / `count` 2 本（すべて `Int` 版）だけで、`BigInt` / `Long` /
  `BigDecimal` 版は入れ子オブジェクト `Range.Long` などの側にあります（別スライス）。
  `apply` は `Range$Exclusive`、`inclusive` は `Range$Inclusive` を返すので、
  JVM ディスクリプタを明示します（`RichInt.to` が `gen.rs` で必要としたのと同じ理由）。
  `--scala-library` 専用（prelude はクラス `Range` 自体を `library_abi` で gate しており、
  jar なしでは `1 until 10` も診断です）。
- **pickle からのメンバ供給を view 探索より先に**行うようになりました
  （`type_select`）。SLS 6.26.1 の view は「選択が型検査を通らないとき」にだけ
  挿さるもので、nsc はその時点で全メンバを読み終えています。scala-rs はメンバを
  遅延で読むので、供給を view 探索の**後**に置くと「まだ読んでいないだけのメンバ」が
  暗黙変換に負けていました。`1.second + 500.millis` がその例で、`FiniteDuration` の
  classfile は `+` を `$plus` と綴るためメンバー探索が外し、`any2stringadd` が
  選択を横取りして `no matching overload for (String)String with arguments
  (FiniteDuration)` になっていました。いまは pickle が `+` を供給し、
  `FiniteDuration.$plus` が呼ばれます。「何も見つかっていないとき」という条件は
  そのままなので、既存のメンバを隠すことはありません。
- `scala.math.Numeric[T]` は `scala.math.Ordering[T]` を継承します（実 ABI の
  `interface scala.math.Numeric<T> extends scala.math.Ordering<T>`）。prelude は
  `sum` / `product` 用に `Numeric` を合成するだけでこの親を張っておらず、
  `Numeric[T]` を `Ordering[T]` の位置に渡せませんでした（slick の
  `ScalaNumericType[T] extends ScalaBaseType[T]()(tag, numeric)`）。
  `crates/typer/src/prelude_numhier.rs`。
- **第 1 引数リストが implicit の method は view ではありません**（SLS 7.3。view は
  「引数 1 個の *explicit* な implicit method」）。`implicit def Option[T](implicit
  ord: Ordering[T]): Ordering[Option[T]]` は導出規則であって
  `Ordering[T] => Ordering[Option[T]]` の変換ではないのに、暗黙変換の探索が
  引数リストの implicit 性を見ずに拾っていました。`val o: Ordering[Option[Int]] =
  Ordering.Int` が**黙って通り**、メンバが見つからなかった選択の受け手も
  この変換で書き換えられて（`value Int is not a member of
  Ordering[Option[AnyRef]]`）診断が化けていました。method の**型**では
  どの節が implicit か分からないので、パラメータ**シンボル**の
  `Flags::IMPLICIT` で判定します（`crates/typer/src/implicits.rs` の
  `first_clause_is_implicit`）。導出規則としての利用（`List(Some(2), None).sorted`）は
  そのまま通ります。
- **高階の候補**。候補の型パラメータが**型構築子**のとき
  （`buildFromIterableOps[CC[X] <: Iterable[X] with IterableOps[X, CC, _], A0, A]:
  BuildFrom[CC[A0], A, CC[A]]`）、`CC[A0]` を `List[String]` に照合して
  `CC := List` / `A0 := String` を読み、同じ束縛で `CC[A]` を答えます。
  これで `LazyZip2.map[B, C](f)(implicit bf: BuildFrom[C1, B, C]): C` のように
  **implicit 節にしか現れない** `C` が解けます
  （「[`BuildFrom` の高階 implicit 照合](#buildfrom-の高階-implicit-照合lazyzipagentbuildfrom2)」）。
  構築子に立てられるのは**候補自身の**型パラメータだけで、呼び出し側の未確定な
  `M[_]` は引数からの通常の推論が決めます。
- **候補自身の型パラメータ境界を検査します**（nsc `Infer#checkBounds`）。
  `BuildFrom` の witness は境界以外は同じ型なので、これが唯一の区別です。
  高階の境界は型に畳み込まれて届くので交差型の単一化がそれを担い
  （`BuildFrom[CC[A0] with SortedSet[A0], …]`）、一階の F-bound
  （`buildFromBitSet[C <: BitSet with BitSetOps[C]]`）は `bound_hi` を見ます。
  検査するのは境界が名指す**クラス**が解の基底クラスにあるかどうかだけで、
  引数の位置には触れません（nsc より緩い方向にだけ外します）。
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

#### trait がクラスを継承する（SLS 5.3.3）

`trait Loud extends Animal` のように **trait の親がクラス**でもかまいません。この親は
「制約」であって初期化ではないので、trait は `Animal` のコンストラクタを**呼びません**。
したがって trait の親に**引数リストは書けず**（`trait T extends C(x)` は scalac 2.13.16 と
同じく `parents of traits may not have parameters`）、コンストラクタのオーバーロード解決も
一切しません。親がコンストラクタ引数を取るだけで `no matching overload for constructor` に
なっていたのを直しました。

制約なので、その trait を mixin できるのは**そのスーパークラスのサブクラスだけ**です。
`class Plain` に `Loud` を混ぜると scalac と同じ文面で拒否します。

```
illegal inheritance; superclass Plain
 is not a subclass of the superclass Animal
 of the mixin trait Loud
```

classfile 上では、trait の interface は**そのスーパークラスを継承しません**（scalac も
`Main$Loud` の super は `java/lang/Object` です）。なので `T$class` の本体が継承メンバを
`$this` 経由で読むときは `checkcast` を先に出します（`$this` の JVM 型は `LT;` なので、
これが無いと `Type 'T' is not assignable to 'C'` で verify に落ちます）。

逆に、**クラス側の親にクラスが 1 つも無いとき**は trait のスーパークラスがそのクラスの
スーパークラスになります（SLS 5.1）。`class X extends Loud` は classfile でも
`Main$Animal` を継承し、`val a: Animal = new X` が verify を通ります。

`abstract override` は**線形化上の次の実装**を指すので、`new Dog with Polite with Loud` と
`new Dog with Loud with Polite` は結果が変わります（`LOUD-please-woof` と `please-LOUD-woof`）。
その連鎖が具象実装に届かない場合は、実行時に落とさず**コンパイル時に**拒否します。

```
object creation impossible.
abstract override def speak: String (defined in trait Loud) is marked `abstract` and `override`, but no concrete implementation could be found in a base class
```

クラス自身の定義は線形化で trait より**上**にあるので super の受け皿にはなれません
（scalac と同じく `` `abstract override` modifiers required to override `` です）。
`abstract override` をクラスのメンバに付けるのも scalac と同じく拒否します
（`` `abstract override` modifier only allowed for members of traits ``）。

線形化（C3）は `crates/typer/src/lin.rs` に 1 つだけ置き、型検査（`abstract override` の
接地判定）とコード生成（super アクセサ / mixin フォワーダ）の両方がこれを使います。

#### 解決できない親は診断する

`extends` の頭・`with` の各項・適用された親の型引数・自分型注釈・`new X` / `new X {}` で
名前が解決できなければ、実 scalac 2.13.16 と同じ文面で拒否します（`not found: type X`、
修飾付きなら `type X is not a member of package p` など）。以前は**両モードとも無言で**
`java/lang/Object` を継承した classfile を書いていました。詳しくは
下の「存在しない親クラス／トレイトを黙って受理していた（`agent/parentcheck`）」節を参照してください。

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

**クラス / trait のメンバである `object`** は静的シングルトンではありません。`javap -v -p -c`
で確かめた scalac 2.13.16 の形は次のとおりで、こちらも同じものを出します。

- `Main$Outer$P$` に `$outer` フィールドと `public <init>(LMain$Outer;)V`
  （先頭で引数の null チェック）。`MODULE$` も `<clinit>` も無い。フィールドの可視性
  だけは nsc の `private final` ではなく、内部クラスの `$outer` と揃えて
  `public final` です（既存の `$outer` チェーン読みがそのまま効くため）。
- 外側 `Main$Outer` に `private volatile Main$Outer$P$ P$module` と、`null` なら
  `synchronized` で作る `public Main$Outer$P$ P()` アクセサ。参照側は `getstatic MODULE$`
  ではなく `<外側インスタンス>.P()` を呼びます。だから `o.P eq o.P` は `true`、
  別の `Outer` の `P` とは `false` になります。
- trait のメンバのときは interface に `public abstract <name>()` だけを置き、
  実装クラスごとにフィールドとアクセサを出します（trait の `lazy val` と同じ mixin）。
- **クラスにネストした trait**（`class Outer { trait T { def d = v } }`）は interface に
  フィールドを持てないので、nsc と同じ展開名のアクセサ `Main$Outer$T$$$outer()` を
  abstract で宣言し、実装クラス / `object` 側がそれを実装します。trait の実装
  （`Main$Outer$T$class`）は `getfield $outer` ではなくこのアクセサを呼びます。

**メソッド本体の中の `object`**（ローカル `object`）は別の形で、nsc は呼び出しごとに 1 つを
`scala.runtime.LazyRef` に持ち、`$outer` とキャプチャした局所を `<init>` に渡します。これは
まだ実装していないので、外側インスタンスや囲みメソッドの局所を読むローカル `object` は
**コンパイル時に診断**します（黙って壊れた静的シングルトンを出しません）。外に何も
読まないローカル `object` はこれまでどおり静的シングルトンとして通ります。

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

**メソッド本体の中の宣言**（ローカル `trait` / `class` / `object`）も、テンプレートの中
の宣言と同じだけのものを出します。binary name は nsc と同じく索引つき
（`Main$Same$1` / `Main$Same$2`）で、ローカル trait の捕捉はアクセサ経由です。
詳しくは「メソッド本体の中の宣言（ローカル trait / class / object）」の節を参照。

### lazy val

**クラス・trait・object のメンバ**は、フィールドに加えて `bitmap$0: Int` と、同期した
アクセサを出します。初期化は最初の読み取りまで遅延します。

trait の `lazy val` は（nsc の mixin フェーズと同じく）実装クラス／オブジェクトごとに
フィールド・`bitmap$0` のビット・アクセサを複製します。ビットはクラス自身の `lazy val`
と継承したものを 1 本のリストにして採番するので衝突しません。interface 側は abstract
宣言だけなので、呼び出しは `invokeinterface` です。

**メソッドの中の `lazy val`**（ローカル）は、フィールドを吊るすインスタンスが無いので
nsc の `lazyvals` フェーズと同じく **`scala.runtime.LazyRef` 系のセル**になります
（`crates/typer/src/lazy_local.rs`）。宣言位置ではセルを 1 個作るだけで、初期化子は
**最初の読み取り時**に、セルのモニタの下で高々 1 回だけ走ります。

```scala
def f(n: Int) = {
  lazy val s = { println("mk"); "v" + n }   // ここでは new LazyRef() だけ
  s + s                                     // 読むたびに s$1(s$lzy) を呼ぶ（初期化は 1 回）
}
```

- セルの型は結果型で決まります: `Boolean`/`Byte`/`Char`/`Short`/`Int`/`Long`/`Float`/
  `Double` はそれぞれ `LazyBoolean` … `LazyDouble`（値をボックスしない）、`Unit` は
  `LazyUnit`（フラグのみ）、それ以外は `LazyRef`。
- アクセサは普通のネストした `def` としてラムダリフトに渡すので、初期化子が捕捉した
  ローカル・引数・`var` はそのまま追加引数として渡ります。`lazy val` 同士の依存
  （`lazy val a = b + 1; lazy val b = 2`）も、`a` のアクセサが `b` のセルを捕捉する形で
  そのまま通ります（scalac の `a$lzycompute$1(LazyInt, LazyInt)` と同じ）。
- ブロックの中では `lazy val` だけ**前方参照**できます（素の `val` は今までどおりエラー）。
- 初期化子が例外を投げた場合、`_initialized` は値を格納した後にしか立てないので、
  セルは未初期化のまま残り、次の読み取りで再試行します（scalac と同じ）。
- ループの本体で宣言すると反復ごとに別のセルになります。
- `--no-scala-library` では `scala/runtime/Lazy*` を私有ランタイムとして出します
  （`crates/backend/src/runtime.rs`）。jar モードでは本物を使うので出しません。

これ以前はローカルの `lazy val` の初期化子が**宣言位置で先行評価**されていました。
型検査は通り値も合っていたので、`println` の出る順番でしか分からない誤コンパイルでした。

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

**呼び出しの型パラメータを期待型から解く経路**（`collect_expected`）でも展開します。
`object Type { type Scope = Map[TermSymbol, Type] }` に対して `val s: Type.Scope = Map.empty`
と書くと、展開しないままでは `Map[K, V]` と `Type$.Scope` が構造的に噛み合わず、
`Map[Nothing, Nothing]` になっていました。展開するのはエイリアスだけで、
抽象型メンバはそのまま期待型として使います。

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

### 入れ子のパターン（`case P(v) :: t`）

**取り出した値を、部分パターン自身の型へ先に `checkcast` してはいけません。** nsc は
取り出した値を**取り出し元の静的型**へ絞り（`List[C]` の `$colon$colon.head` なら
`checkcast C`）、そのあとで部分パターンの `instanceof P` → `ifeq <次の case>` →
`checkcast P` を出します。scala-rs は部分パターンの型へ無条件に `checkcast` していたので、
`case P(v) :: t` は head が `P` でないリストすべてで `ClassCastException` になっていました
（型検査は通るので、実行するまで分かりません）。`case h :: t` 単体が動いていたのは、
束縛だけの部分パターンには絞り込みが要るからです。

判定は `reads_erased_value`（`crates/backend/src/gen.rs`）にまとめ、部分パターンを束ねる
すべての経路（case class のコンストラクタパターン、`unapply` の結果、`unapplySeq` の要素）で
共有しています。**テストする**部分パターン（`P(...)` / `Foo(...)` / `_: T` / 定数 /
安定識別子 / `x @ Pat`）は取り出したままの値を受け取り、**束縛だけ**の識別子パターンだけが
絞り込みを受けます。これで次が全部直りました。

| 形 | 直る前 |
| --- | --- |
| `case P(v) :: t` / `case P(a) :: P(b) :: _` / `case h :: P(v) :: _` | `ClassCastException` |
| `case (p @ P(v)) :: t` | 同上（`@` の内側がテストでも絞っていた） |
| `case Some(P(v))` / `case Some(Nil)` | 同上 |
| `case Some(1)`（`Option[Any]`） | `Integer` へ unbox して `ClassCastException`。nsc と同じく box したまま `BoxesRunTime.equals` で比べます |

`unapply` の呼び出し口も 2 か所直しました。入れ子の extractor は消去された `Object` を
受け取るので、`unapply` のディスクリプタが要求する型へ `checkcast` します。そのうえで、
スクルーティニの静的型が extractor の引数型に適合していないとき（`Option[Any]` に対する
`case Some(Two(a, b))`）は、nsc と同じく `instanceof` → `ifeq` を前に置いて**次の case へ
落とします**（以前は検証すら通りませんでした）。また `Option[(A, B)]` の結果を `dup` で
スタックに載せたまま部分パターンが次の case へ飛んでいたため、ユーザー定義の中置 extractor
（`case P(v) ~ _`）は `VerifyError: Inconsistent stackmap frames` でした。タプルはローカルへ
落としてから読みます。`Tuple3` 以上を返す `unapply` も、アリティに関わらず
`checkcast scala/Tuple2` していたのを、`scala/TupleN` の `_1()` … `_n()` に直しました
（`Tuple2` だけは 2.13 でもフィールドが public なので `getfield` のままです）。

コンストラクタパターンのアリティは typer が見ます。`case P(a, b)` を 1 フィールドの `P` に
当てると、以前は `b` が `Any` で通ってしまい backend が実行時に
`RuntimeException("pattern arity")` を投げていました。可変長の最終パラメータは対象外です。

### 尽きた `match`（`MatchError`）

どの case にも当たらなかった `match` は、nsc と同じく **`scala.MatchError` にスクルーティニを
持たせて** 投げます（以前は `RuntimeException("match error")` で、`case _: MatchError` では
捕まらず、どの値で落ちたのかも分かりませんでした）。プリミティブのスクルーティニは box して
渡します。私有ランタイムには `scala/MatchError` を生成するので（`crates/backend/src/runtime.rs`）、
両モードで同じクラス・同じメッセージ書式（`<値> (of class <クラス名>)`、`null` なら `null`）です。

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

### `Ordering` コンパニオンと summon（`agent/ordsummon`）

`package object scala` は型クラスを**型と項の両方**で無修飾に見せています。

```scala
type Ordering[T] = scala.math.Ordering[T]
val  Ordering    = scala.math.Ordering
```

prelude（`add_scala_aliases`）は前者だけを入れていて、**項**位置の `Ordering` も
trait そのものに解決されていました。そのため

- `Ordering.Int` は trait のメンバを探して落ちる（`scala.math.Ordering.Int` と
  完全修飾すれば通っていた）。
- `Ordering[String]` は「trait を項に置いた型適用」として**型検査を黙って通り**、
  codegen が `Ordering$.MODULE$` を積んで `Ordering` に checkcast していました
  （実行時 `ClassCastException: scala.math.Ordering$ cannot be cast to
  scala.math.Ordering`）。`Ordering[Int].reverse` は
  `IncompatibleClassChangeError` の形で出ます。

`crates/typer/src/prelude_ordsummon.rs` がコンパニオン module を項の名前空間にも
入れます（`Ordering` / `Numeric` / `Equiv` / `Fractional` / `Integral` / `BigInt` /
`BigDecimal`）。`SymbolTable::lookup` は class と module の両方を返し、項位置
（`type_ident`）は module を、型位置（`resolve_type_name`）は class を選びます。
`Integral` / `Fractional` は `prelude_numhier` が jar を読まずに trait だけ生やして
いたのでコンパニオンが無く、ここで作ります（jar の `scala/math/Integral$` は実在し、
`apply:(Lscala/math/Integral;)Lscala/math/Integral;` を持ちます）。作る前は
`val i: Integral[Int] = Integral[Int]` が黙って通って実行時に落ちていました。

summon（`Ordering[String]` = `Ordering.apply[String]`。nsc では
`def apply[T](implicit ord: Ordering[T]): Ordering[T] = ord` の恒等）は
`check.rs` の `Module[T]` → `Module.apply[T]` リダイレクトが受け持ちます。
2 つ足りていませんでした。

- ライブラリのコンパニオンの `apply` は**選択されたとき**に pickle から読まれるので、
  `.apply` と書かない `Ordering[String]` では見つかりませんでした。ここで
  pickle から供給します。prelude が自前の `apply` を書いているコレクションの
  ファクトリと並べても安全なのは、`PickleSupply` が**同じ erasure の
  手書きメンバのコピーを断る**ようになったから（`agent/setapply`）で、その門が
  入る前は `List[Int](1, 2)` が `ambiguous overload` になっていました。
- 参照が module シンボルとは限りません。パッケージオブジェクトの別名は
  アクセサ（`def Equiv(): Equiv$`）として届くので、**module クラス型の安定値**も
  同じ扱いにします（`module_class_of_value`）。

項 `Ordering` が module になったことで、**オーバーロードの復旧経路も 1 本増えます**。
`BigDecimal(3L)` は今まで「項 `BigDecimal` はクラス → `apply` はそのクラスのメンバでは
ない → `type_select` の `found.is_empty()` 枝が pickle からコンパニオンの 7 本を読む →
`widen_with_companion` が両スコープを合わせる」という遠回りで通っていました。別名が
module に解決されると module クラスには prelude 手書きの `apply` が 3 本あるので
`found` が空にならず、pickle が読まれません（`no matching overload for <(Int) |
(String) | (BigDecimal)> with arguments (3L)`。このスライスが一度 revert された回帰が
これです）。`widen_module_from_pickle` を `widen_with_companion` の隣に足して、
**module 受け手でも**「どの候補にも当てはまらなかった」ときだけ pickle を読みます。
足す方向にしか働きません（同 erasure のコピーは `agent/setapply` の門が断る）。

`--no-scala-library` では `scala/math/Ordering` の classfile も `Ordering$` も無く、
`not found: value Ordering` の診断のままです（`prelude_ordsummon` は `library_abi`
でゲート）。

### `Equiv[T]` の summon と `Ordering <: PartialOrdering <: Equiv`（`agent/eqtail`）

`implicitly[Equiv[Int]]` / `Equiv[Int]` は real scalac が通しますが、`could not
find implicit value` で落ちていました。実 ABI（`javap -p -s`）は

```text
interface scala.math.Ordering<T>        extends java.util.Comparator<T>, scala.math.PartialOrdering<T>
interface scala.math.PartialOrdering<T> extends scala.math.Equiv<T>
interface scala.math.Equiv<T>           extends java.io.Serializable
```

という階層ですが、prelude はこれを張っていませんでした。原因は 2 つ:

1. `Ordering[T] <: PartialOrdering[T] <: Equiv[T]` の辺が無く、`val e:
   Equiv[Int] = Ordering.Int` のような劣化代入が `type mismatch` になって
   いました。
2. `object Equiv` は implicit instance を 1 つも持っていませんでした。real
   scalac は `implicitly[Equiv[Int]]` に `Ordering.Int` 経由の派生ではなく
   `Equiv` 専用の instance（`Equiv$Int$`）を選びます
   （`implicitly[Equiv[Int]].getClass.getName` で確認）。

`crates/typer/src/prelude_eqtail.rs` が両方を足します。`Equiv` / `PartialOrdering`
は他の `scala.math` 型クラス（`Ordering` / `Numeric` / `Integral` / `Fractional`）
と同じ穴を踏むので、同じ手で塞ぎます: jar の遅延ロードを待つと `find_by_jvm` が
まだ何も見つけられない時点（`install_prelude` は `install_classpath` より前に
走ります）なので、prelude の時点で自前の class + companion module を作って
現在スコープに `enter_in_current` してしまいます。あとから `Equiv` /
`PartialOrdering` を参照した `check.rs` の `expose_unqualified` は「もうスコープに
ある」ので通らず、この prelude シンボルだけが使われ、`equiv` 以外のメンバ
（`fromComparator` / `by` / `TupleN` 等）は `jvm_name` が実クラスと一致してさえ
いれば `pickle_supply` がオンデマンドで供給します（`Ordering` の `lt` / `gt` /
`lteq` / `gteq` / `max` / `min` が今も同じやり方で効いているのと同じ）。

`implicitly[PartialOrdering[Int]]` には real scalac にも instance が無いので、
階層辺を足しても summon できるようになってはいけません。`PartialOrdering` には
companion module を作らず、`object Equiv` の implicit instance だけを手書きします
（`Unit` / `Boolean` / `Byte` / `Char` / `Short` / `Int` / `Long` / `BigInt` /
`BigDecimal` / `String` と、2.13 で名前空間 object になった `Double` / `Float`
の非推奨版 `DeprecatedDoubleEquiv` / `DeprecatedFloatEquiv`）。`--no-scala-library`
では `scala/math/Equiv` の classfile が無く、`not found: type Equiv` の診断のまま
です（`prelude_eqtail` は `library_abi` でゲート）。

#### `Ordering#compare` の prelude 型（同じスライス）

`crates/typer/src/prelude.rs` の `add_ordering` は `Ordering[T]#compare` を
`(Any, Any): Int` で手書きしていました。`Ordering[String].compare(1, 2)` の
ような **本来 real scalac が拒む** 呼び出しを scala-rs だけが黙って通して
しまいます（受け入れすぎ）。`lt` / `gt` / `lteq` / `gteq` / `equiv` / `max` /
`min` は手書きされておらず `pickle_supply` がオンデマンドで実 ABI の
`(T, T)` シグネチャを供給するので、`compare` だけがこの穴を踏んでいました。
`method()` に渡す引数を `Type::Any` から `Type::TypeParam(t)`（`Ordering`
自身の型パラメータ）に変えるだけで直ります。`Type::TypeParam` は
`Type::Any` と同じく `Ljava/lang/Object;` に erase される
（`crates/backend/src/gen.rs` の `jvm_desc`）ので、`sorted` / `sortBy` の
codegen が期待する erased descriptor `(Ljava/lang/Object;Ljava/lang/
Object;)I` は変わりません。変わるのは型検査での**見え方**だけです。

#### `new T` / `new A` の黙認（`agent/parentcheck` 残件、同じスライス）

`agent/parentcheck`（上の節）が Remaining に残していた 2 形です。

```scala
def f[T] = new T   // scalac: class type required but T found
trait X { type A; def f = new A }   // scalac: class type required but X.this.A found
```

`new` は SLS 5.3.2 でクラス型を要求しますが、型パラメータも（`=` の無い）抽象型メンバも
クラス型ではありません。`check.rs` の `New { tpt }` の `Ident` 分岐（`new T` / `new A` の
ような、型引数も修飾も無い裸の名前の形）は、`new_alias_target` が「エイリアスの右辺を
構築する」変換を試みたあと、`found`（名前解決の結果）に `SymKind::Class` も型エイリアスも
無ければそのまま `type_expr` に流していました。`found` が空でなければ「見つからなかった」
扱いにもならないので、`new T` は黙って `Type::TypeParam` を、`new A` は黙って
`Type::TypeMember` を身にまとった `new` 式になっていました。

直したのは、`new_alias_target` が `None` を返した**あと**（＝ jar 由来のエイリアスは
すでに一度 dealias を試されている）に、`found` の中身がなお `SymKind::TypeParam` /
`SymKind::TypeMember` である symbol を探す 1 段です。**「解決済みで、かつクラスでない」**
だけを見るので、`agent/parentcheck` の `strict_type_names`（「本当に見つからない」ときだけ
発火し、jar 由来で遅れて解決される正当な型は素通りさせる）と同じ慎重さで、pickle から
まだ読んでいない jar の型エイリアスを誤って「抽象型メンバ」と判定することはありません。

メッセージは nsc の文面をそのまま再現します。型パラメータは裸の名前（`T`）、抽象型メンバは
**`this` 修飾つき**（`X.this.A`）——nsc は無修飾の型メンバ参照を暗黙の `this.` 前置として
表示するので、そこも合わせています（`Typer::class_type_required_name`）。

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

型の中でも改行は無条件には飛ばしません。`parse_compound_type` が `with` と refinement の
`{` を探すところは nsc の `newLineOptWhenFollowedBy` と同じで、**改行の次が本当に
`with` / `{` のときだけ**読み飛ばします。無条件に飛ばしていたころは

```scala
trait A {
  val p: String
  println("x")     // ← String println "x" という中置型に食われていた
}
```

のように、右辺の無い宣言の次の行の文が型に飲まれていました。詳しくは
「テンプレート本体の式文」節。

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

**可変長引数の splat `f(xs*)`** も `-Xsource:3` / `-Xsource:3-cross` のときだけ受け付け、2.13 の `f(xs: _*)` と**同じ木**（`Typed` + `<repeated>[_]`）に落とします。中置ループは `)` の前で右辺を諦めて `xs.*` という後置 `Select` にするので、引数リストを閉じる `*` だけを splat と読みます。varargs は引数の最後にしか置けないので、この位置以外に splat はありません。フラグ無しでは 2.13 素のまま後置演算子で、scalac 2.13.16 と同じ文面になります。

```
value * is not a member of List[Int]
```

slick では `Map(elems*)` など 3 箇所で使われています。

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
エラーは書いたとおりのものが出ます。合成した `TupleN(a, b)` 自身が再入しないよう
再入フラグで止めています。オーバーロードされた呼び先の扱いは
`agent/hkinfer` で直しました（下の「引数の基底型と自動タプル化」）。当初は
「オーバーロードには一切適用しない」としていましたが、それでは `println(1, "a")` が
通りません。正しくは**書いた引数個数を取る候補が 1 本でもあればタプル化しない**です。

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

### case class を `Product` にする（`agent/product`）

`agent/product` スライス。フィクスチャは `tests/fixtures/prod*.scala`、テストは
`crates/cli/tests/product.rs` です。

`productPrefix` と `productArity` だけが合成されていて、`scala.Product` を親に付けて
いなかったので、**中途半端に `Product` に見える**状態でした。実 scalac が通す次の 6 つが
すべて落ちていました。

```scala
case class P(x: Int, y: Int)
val p = P(1, 2)
p.productIterator.toList     // value productIterator is not a member of P
p.productElement(0)          // 同上
p.productElementName(0)      // 同上（2.13 で追加）
P.tupled((5, 6))             // value tupled is not a member of P$
P.curried(5)(6)              // 同上
(p: Product).productArity    // type mismatch; found: P required: Product
```

**何を出すべきかは推測せず、scalac 2.13.16 の classfile を `javap -v -p` で読んで**
決めました。読み取った規則はそのまま `crates/typer/src/prelude_product.rs` の
ドキュメントコメントに残してあります。

**1. `case class` / `case object` は `scala.Product with java.io.Serializable`**。
これは無条件です。親を持つ case class もそのあとに付きます
（`class E$L implements E$T, scala.Product, java.io.Serializable`）。この辺が無いと
`val p: Product = P(1, 2)` も `List[Product]` も通らず、`productIterator` /
`productElementNames` の来る先もありません（nsc は 4 つのうち `productIterator` /
`productElement` / `productElementName` / `productPrefix` / `productArity` だけを
case class 側で上書きし、`productElementNames` は `Product` の default 実装を継承します）。

**2. `productElement` / `productElementName` は自前で出す**。どちらも
`0 … arity-1` の **`tableswitch`** で、フィールドが 1 本でも表になります
（`tableswitch { // 0 to 0 }`）。フィールドが 0 本の case class では switch 自体が
無く、範囲外の道だけが残ります。範囲外は
`scala.runtime.Statics.ioobe(I)`（＝`throw new IndexOutOfBoundsException(String.valueOf(i))`）で、
`productElementName` はそのあとに `checkcast java/lang/String` が付きます。
フィールドが値クラスのときは `toString` と同じく**インスタンスに包み直して**返します
（`new G$Meters(this.m())`）。

**3. `productIterator` は継承ではなく上書き**で、
`ScalaRunTime$.MODULE$.typedProductIterator(this)` を呼びます。`productElementNames`
は逆に `Product` の default 実装への **mixin フォワーダ**
（`invokestatic InterfaceMethod scala/Product.productElementNames$`）です。

**4. `case object` の `productElementName` だけ例外**。nsc は case object には
`productElementName` を合成しないので、module class には `Product` の default 実装への
フォワーダが載ります。その default はメッセージが違い、
`case class Zero()` の `productElementName(0)` が `IndexOutOfBoundsException: 0` を投げるのに対し
`case object Solo` は `IndexOutOfBoundsException: 0 is out of bounds (min 0, max -1)` を投げます。
**同じプログラムの中で 2 つのメッセージが出る**ので、両方そのまま再現しています
（`prod.scala` の最後の 4 行）。

**5. コンパニオンは `scala.runtime.AbstractFunctionN` を継承する**。`tupled` /
`curried` はここから来ます（`FunctionN` の default メソッド。prelude 側の
`FunctionN` には `prelude_fntuple.rs` が同じ 2 つを入れています）。メソッドを直接
生やすのではなく継承にしたのは、それが**実物だから**です。おかげで
`val f: (Int, String) => P = P` と `List(1, 2, 3).map(One)` も通るようになりました。
継承する条件も 4 つとも classfile から読み取ったものです。

- **自分で書いた `object P` には付かない**。何を継承していようが関係なく、
  `object P extends Base` は `class F$Plain$ extends E$Base`、
  `object P extends SomeTrait` ですら `class F$WithTrait$ implements E$Mix` で
  `AbstractFunction1` はどこにもありません（ただし `java.io.Serializable` は付きます）。
- **型パラメータのある case class には付かない**。`case class Gen[A](a: A, b: Int)` は
  `class E$Gen$ implements java.io.Serializable` だけ。
- **引数節が 2 つ以上なら付かない**。implicit 節も数に入ります
  （`case class Impl(a: Int)(implicit o: Ordering[Int])` も
  `case class Curr(a: Int)(b: String)` も素の `Serializable`）。
- **arity 23 以上には付かない**。`AbstractFunctionN` は 22 までで、
  22 個ちょうどの兄弟には `AbstractFunction22` が付きます。

可変長引数の case class には付きます（`case class Vararg(a: Int, rest: String*)` →
`AbstractFunction2<Object, scala.collection.immutable.Seq<String>, F$Vararg>`）。
`AbstractFunctionN` を継承する以上、erase された `apply(Object, …)Object` を実装する
必要があるので、コンパニオンにはそのブリッジも出します（nsc も同じ位置に出します）。

**全部 `library_abi` ゲート**です。`scala.Product`・`java.io.Serializable`・
`scala.runtime.AbstractFunctionN`・`scala.collection.Iterator`・`scala.runtime.ScalaRunTime` は
どれも jar 側で、私有ランタイム（`crates/backend/src/runtime.rs`）には 1 つもありません。
`--no-scala-library` では親を張らず、`p.productIterator` は
`value productIterator is not a member of P` のまま診断します
（`fixtures_prod_lib_without_library_is_error`）。ただし `productElement` /
`productElementName` は `java.lang` しか要らないので**両モードで出します**。
私有ランタイム側では `Statics.ioobe` の代わりに同じ throw を、case object 側では
`Product` の default と同じメッセージを、その場で書き出しています。
`prod.scala` と `prod_vc.scala` は**私有ランタイム・jar・real scalac の 3 つが
バイト単位で一致**します。

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

同じ規則の**選択側**（nsc の `nonLocalMember`）も入れました。`private[this]` は
`this` 以外のどの接頭辞のメンバでもないので、他のインスタンスに対する選択では候補から
外し、同名の**継承メンバ**の方を読みます。

```scala
class Sym(val name: String)
class Fun(name: String) extends Sym(name) {   // `name` はコンストラクタ引数
  override def equals(o: Any) = o match {
    case o: Fun => name == o.name             // `o.name` は Sym の val
    case _      => false
  }
}
```

素の引数が継承メンバを覆い隠していたので、slick の `Library.JdbcFunction` /
`SqlOperator` / `SqlFunction` の `o.name` は
`value name cannot be accessed as a member of JdbcFunction from JdbcFunction` でした。

#### `private[p]` は**定義側**の外側から解決する

`private[X]` の `X` は、その定義を囲むクラスかパッケージの名前です。使用側のスコープで
名前を引くと、slick の `private[util] def copySliceTo`（`package slick.util`）が
`scala.util` に当たって、同じパッケージからの参照まで全部
`cannot be accessed` になっていました。`X` はメンバの所有者から外へ辿って解決します
（`check.rs` の `access_within_of`）。パッケージ境界そのものは変わっていないので、
`mism8_access_bad.scala` は今までどおり拒否されます。

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

**7. 宣言クラスの部分クラス関係（nsc `isInProperSubClassOf`）**。どちらも相手と
等しく特定的なとき、nsc の `relativeWeight` は**所有者が真の部分クラスである方**を
選びます。2.13 の `SortedSetOps.map[B](f)(implicit ord)` と `IterableOps.map[B](f)`
はまさにこれで、これが無いと `TreeSet.map(f)` は `ambiguous overload` でした
（nsc の `isAsSpecific` は implicit 節を素通しするので、明示パラメータだけを見ると
2 本は等しく特定的です）。詳しくは「高階の期待型…（第 9 スライス）」の節。

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

### 捕まえたパラメータと不変引数の lub（`type mismatch` 第 7 スライス）

`agent/mismatch7` スライス。フィクスチャは `tests/fixtures/mism7*.scala`、テストは
`crates/cli/tests/mismatch7.rs` です。8 つの原因を直しました。

**1. メソッドのパラメータが、匿名クラス越しに as-seen-from されていた**。

```scala
trait It[T] { self =>
  def next(): T
  def map[B](f: T => B): It[B] = new It[B] { def next(): B = f(self.next()) }
}
```

`bind_found` は「見つけたシンボルの owner が `this_class` と違えば
as-seen-from する」でした。しかし `f` の owner は**メソッド `map`** であって
クラスではありません。匿名クラスの中では `this_class` がその匿名クラス
（親は `It[B]`）なので、`f: T => B` に `T := B` が代入されて `(B) => B` に
なっていました。**クラスのメンバだけが接頭辞越しに読まれる**——owner が
`Class` / `ModuleClass` / `Module` のときだけ as-seen-from します。
匿名クラス自身の `this.next()` を渡す形は nsc と同じく mismatch のままです
（`tests/fixtures/mism7_capture_bad.scala`）。

**2. 複合*型*にテンプレートの規則を当てていた**。`compound_to_type` は
「クラス親が 2 つ以上あって、そのうち最も特殊なものが無ければ
illegal inheritance」と診断していました。nsc の
`typedCompoundTypeTree` にそんな検査はありません。`def f(x: A with B)` は
**A と B が無関係なクラスでも通ります**（その型に値が無いだけです）。
slick の `Query[B, BU, C] & TableQuery[B]`（一方が他方のサブクラス）が
これで落ち、`Executable` の 3 つの implicit が丸ごと `<notype>` に
なっていました。

代わりに nsc が本当に持っている規則——`validateParentClasses`、
**テンプレート**の 2 番目以降の親はトレイトでなければならない——を
入れました（`check_mixin_parents`）。`class C extends A with B` は
`class B needs to be a trait to be mixed in` です。scalac 2.13.16 の
メッセージそのものです。既存の `compound_bad.scala` はこの「型としては
正しい」形を固定していたので、複合型のメンバ解決（どちらの親も宣言して
いない名前）に差し替え、テンプレート側は `mism7_mixin_bad.scala` に
分けました。

**3. eta 展開が、期待関数型の*結果*から型パラメータを解いていた**。

```scala
xs.map(identity)   // found: CA[Any]  required: CA[T]
```

関数のパラメータは反変、結果は共変なので `A => A <: T => ?U` は
`T <: A` と `A <: ?U` です。nsc は `A` を**パラメータ側**から解き、結果は
上界としてしか使いません。両方を一度に取ると、まだ推論中の `map` が
期待する結果は `Any` なので lub が `T` を飲み込んでいました。
パラメータで解き、そこで決まらなかったもの（結果にしか現れない
パラメータ）だけを結果からも解きます（`Typer::solve_eta_tparams`）。
明示的な `f _` 形（`type_eta`）も同じ経路を通るようにしたので、
`val h: String => String = identity _` も通ります。

**4. 抽象型の*下界*が `<:` の右辺で使われていなかった**。

```scala
def f[E, O >: E](x: E): O = x
```

nsc は右辺が抽象型のとき `tp1 <:< tr2.lo` を試します。こちらには
上界（`(TypeParam(id), b)` で `bound_hi`）の規則しか無く、下界を見る
規則が **1 つも** ありませんでした。`is_sub_type` の頭——どの枝も
`a` だけで match するか「同じパラメータか」しか見ないので、その前——に
入れました。これで slick の
`ShapedValue[_ <: E, U]` を `ShapedValue[_ <: O, U]` に渡す形
（`Query.scala` に 5 件）が通ります。逆向き（`def wrong[E, O >: E](x: O): E`）は
nsc と同じく mismatch です（`tests/fixtures/mism7_lobound_bad.scala`）。

**5. 2.13 の `SeqOps` は `indexWhere` を 2 つ宣言している**。

```scala
def indexWhere(p: A => Boolean, from: Int): Int
def indexWhere(p: A => Boolean): Int
```

pickle 供給には「関数を取るオーバーロードは 1 つだけ入れる」規則が
あります（ラムダのパラメータ型は 1 つの期待型からしか推論できないので、
同名の関数オーバーロードが 2 つあると `xs.segmentLength(_ < 3)` が
解けなくなるため）。線形化順で最初のもの＝ 2 引数版だけが入り、
`xs.indexWhere(p)` は arity エラーでした。**引数の個数はラムダを型付ける
前に分かる**ので、規則を「名前**と arity** ごとに 1 つ」に変えました。
`indexOf` / `lastIndexWhere` / `segmentLength` も同じ形です。

**6. モジュール → `apply` の付け替えが、誰も完了していないシグネチャを
読んでいた**。`Module[T1, T2]` と `.apply` を省いて書くと、
`TreeKind::TypeApply` の枝がモジュールのコンパニオンの `apply` に
シンボルを付け替えます。ところがこの経路は**選択（select）を通らない**ので、
`bind_found` がやる `complete_lazy_sig` が走りません。自分の定義より前に
名指された、結果型が推論の `apply` は `<notype>` のままでした
（slick の `Executable.queryIsExecutable = StreamingExecutable[…]`。
`object StreamingExecutable` はその 25 行下）。付け替え先を完了させます。

**7. implicit 節だけが残った引数が、呼び出しを制約したあとで埋められていた**。

```scala
def one[A2](a2: A2): Int = 0
one(kvs.toMap)                 // found: Map[String, Int]  required: Map[K, V]
(1, kvs.toMap)                 // 同じ
```

`toMap[K, V](implicit ev: A <:< (K, V))` は、期待型なしで型付けると
`(A <:< (K, V))Map[K, V]` というメソッド型のまま残ります。`A2` はその
**未解決のまま**の結果から決められ、そのあとで witness が `K`/`V` を
`String`/`Int` に決めるので、引数が適合すべき型は `Map[K, V]` のまま
取り残されていました。nsc は引数を adapt してから呼び出しを制約します
——引数ループの中で、パラメータの未確定変数を代入する**前に**
implicit 節を埋めるようにしました。埋まった結果は既存の
「レシーバの未確定変数を引数から解く」経路がパラメータ・結果・レシーバに
運びます。

**8. 不変な型引数の lub がどちらの側も受け付けない型になっていた**。

```scala
Seq(new Inv[Boolean], new Inv[Int])
```

同じクラスで引数が違うときは引数ごとに join していましたが、**不変**な
パラメータでは `Inv[Boolean]` も `Inv[Int]` も `Inv[Any]` ではありません。
nsc の lub はここで存在型（`Inv[_ >: Int with Boolean <: AnyVal]`）を
作ります。こちらも上界付きワイルドカードにしました。

同時に、**可変長引数の呼び出しはタプルに包み直さない**ようにしました
（`callee_takes_repeated`）。nsc の `tryTupleApply` は formals と引数の
個数が食い違うときにしか走らず、repeated パラメータは比較の前に引数の
個数まで展開されるので、両者は常に一致します。上の `Seq(a, b)` が
applicable でなくなった瞬間に `Seq[(Inv[Boolean], Inv[Int])]` へ化けて
いたのはこれです。

slick: `errors 518 → 495`、`type mismatch 96 → 84`、`files_with_errors`
80 → 77。エラーが増えたファイルはありません。

### 期待型・可変長引数・依存メソッド型（`type mismatch` 第 8 スライス）

`agent/mismatch8` スライス。フィクスチャは `tests/fixtures/mism8*.scala`、テストは
`crates/cli/tests/mismatch8.rs` です。7 つの原因を直しました。詳しくは上の
「型エイリアス」「メソッド型パラメータの推論」「`-Xsource:3`」「オーバーロードの
候補集合」の各節にあります。

1. **期待型がエイリアスのとき dealias していなかった**。`collect_expected` は
   `Map[K, V]` と `Type$.Scope` を構造的に突き合わせるので、
   `val s: Type.Scope = Map.empty` は `Map[Nothing, Nothing]` でした。
2. **空の可変長引数が「未解決」のままだった**。`List()` / `Seq()` / `Map()` は
   要素型を決める材料が無い＝**制約なし**であって、呼び先の型パラメータを
   抱えたままにしてよいわけではありません。
3. **`xs: _*` を片側だけ剥がしていた**。`def mk[A](xs: A*)` が `A = Int*` に解け、
   `mk(xs: _*)` が `List[Int*]` になっていました。
4. **`-Xsource:3` の splat `f(xs*)` が未対応**だった（後置演算子として
   `value * is not a member of Seq[…]`）。slick の `Map(elems*)` 3 箇所。
5. **nsc の `protoTypeArgs` が無かった**。タプルの各成分は期待型の成分に対して
   型付けされるべきで、非変な `Map` はそれが無いと引数側のキー型で固まります。
6. **依存メソッド型**（`def get[P <: Phase](p: P): Option[p.State]`）。
   `Type::TypeMember` に接頭辞が無いので、接頭辞になり得たパラメータを境界から
   一意に決まるときだけ探し、その引数の同名メンバで置き換えます。
   `Any` に落ちていた 4 件の `if(…getOrElse(true))` とそのカスケードが消えます。
7. **`private[p]` を使用側のスコープで解決していた**。`private[util]` が
   `scala.util` に当たり、`slick.util` の中からの参照まで拒否されていました。
   併せて nsc の `nonLocalMember`（`private[this]` は `this` 以外の接頭辞の
   メンバではない）を選択側にも入れました。

slick: `errors 411 → 378`、`type mismatch 58 → 49`、`files_with_errors`
72 → 67。エラーが増えたファイルはありません。

このスライスで**原因まで分かって直していない**もの:

- `mutable.ArrayBuilder[T]` / `StringBuilder` / `ListBuffer` が
  `Builder[…]` / `Growable[…]` の基底型を持っていません
  （`x.result()` が `Any`）。可変コレクションの階層なので `agent/mutcoll` の領域です。
- `BasicBackend.scala` / `ConcurrencyControl.scala` の
  `found: F[Any] required: F[R]` 13 件は、ラムダ本体の**本当のエラー**の
  カスケードだと書いていましたが、**この読みは誤りでした**。ラムダの中に
  エラーは無く、高階の適用（`Applied`）を期待型に突き合わせる腕が
  `collect_expected` に無かっただけです。第 9 スライスで直しました
  （下の「高階の期待型…」の節）。

### 高階の期待型・ソート済みコレクションの多重定義・クラス内の `copy`（`type mismatch` 第 9 スライス）

`agent/mismatch9` スライス。フィクスチャは `tests/fixtures/mism9_*.scala`、テストは
`crates/cli/tests/mismatch9.rs` です。5 つの原因を直しました。

1. **高階の適用が期待型から型パラメータを解けなかった**。
   `def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]` の `F` が抽象な型構築子
   （`F[_]`）のとき、結果型は `Type::Class` ではなく `Type::Applied` です。
   `collect_expected`（nsc の `instantiateExpecting`）には `Class` / `Tuple` /
   `Function` / `Array` の腕しかなく、`Applied` を `Applied` に突き合わせる腕が
   ありませんでした。`B` は期待型 `F[String]` からも引数からも決まらず、
   cats 風の `F.flatMap(fa) { … }` はすべて `F[Any]` になっていました。
   型構築子の引数位置には変位注釈が無いので、**不変**として扱います。
   期待型がすでに実クラスに落ちている形（`F[B]` 対 `List[String]`）も、
   構築子を**適用前**の形で突き合わせて `F` 自体が `List[String]` に解けないように
   します。

   これは第 8 スライスが「ラムダ本体の**本当のエラー**のカスケード」と記録した
   13 件そのものです。**記録が誤りでした**。ラムダの中にエラーは無く、
   6 行に最小化できる純粋な推論の穴です（`crates/cli/tests/mismatch9.rs` の
   `mism9_hk_result_comes_from_the_expected_type`）。実 scalac 2.13.16 は通します。

2. **暗黙引数リストだけが違う 2 つの候補が両方残っていた**。2.13 は
   `map[B](f)(implicit ord: Ordering[B]): CC[B]` を `SortedSetOps` に、
   `map[B](f): CC[B]` を `IterableOps` に宣言します。nsc の `isAsSpecific` は
   implicit 節を**素通し**する（`case mt: MethodType if mt.isImplicit =>
   isAsSpecific(restpe, …)`）ので、この 2 つは等しく特定的で、決着をつけるのは
   `relativeWeight` の `isInProperSubClassOf`＝**宣言クラスの部分クラス関係**
   だけです。ところが `pickle_supply` は継承メンバを receiver のクラスに降ろす
   ので、その owner は失われます（`TreeSet` の `collect` は 2 本とも
   `owner=scala/collection/immutable/TreeSet`）。

   直したのは 3 箇所です。
   - 供給側の「同じ引数列の候補は 1 本だけ」の鍵を、**明示パラメータだけ**から
     作るようにしました。線形化順で先に来る＝より派生した宣言（`Ordering` の
     witness を取る方）が残ります。
   - その鍵は**記述子が引けてから**確保します。`TreeMap.collect(pf)` は
     classfile 上で一意に引けず、鍵だけ取って落ちると
     `collect(pf)(Ordering)` の席まで奪っていました。
   - 型検査側にも nsc の `isInProperSubClassOf` を入れました（同じ owner に
     降りていない、普通の継承の場合はこちらで決まります）。
   - codegen の `scala.collection` 早見表は `map:(Lscala/Function1;)…` という
     `IterableOps` の形を焼き込んでいるので、**pickle 由来で implicit 節を持つ**
     メンバはこの表を通さず、pickle が記録した記述子で呼びます。これが無いと
     `TreeSet.map(f)` は引数 2 つを積んで 1 引数の呼び出しを出し、
     `IncompatibleClassChangeError` になります（`ambiguous overload` が消えた
     ぶん、**黙って誤コンパイルする**方に変わっていました）。

   `TreeSet.map` / `flatMap` / `collect` と `TreeMap.map` / `flatMap` が通り、
   静的型を `TreeSet[Int]` に絞っても実行時に `TreeSet` が返ります。

3. **クラスの中に書いた `copy(…)`**。`p.copy(y = 3)` は
   コンストラクタ呼び出しに書き換えて型パラメータを推論し直していましたが、
   受け手を省いた `copy(from = f2, …)`（`TreeKind::Ident`）は書き換え対象では
   なく、合成メンバの引数型＝**クラス自身の**型パラメータのままでした。
   slick の `case class Comprehension[+Fetch <: Option[Node]](…, fetch: Fetch =
   None, …)` は `Fetch` を変えて組み直せず、`found: Option[Node] required:
   Fetch` になります。nsc は `copy[Fetch <: Option[Node]](…): Comprehension[Fetch]`
   を合成するので、型パラメータは呼び出しごとに解き直されます。
   書き換えるのは、その名前が本当にこのクラス自身の合成 `copy` に解決する
   ときだけです（ローカルの `def copy`、import、継承したものは普通の呼び出し）。

4. **`foreach` が関数の結果に対して多相でなかった**。2.13 は
   `IterableOnceOps.foreach[U](f: A => U): Unit` です（`javap -s`:
   `<U:Ljava/lang/Object;>(Lscala/Function1<-TA;+TU;>;)V`）。prelude は
   `A => Unit` と書いていて、`Function1[Int, R]` はそれに適合しません。
   ラムダ**リテラル**は本体が捨てられるので通っていましたが、
   `def foreach[R](f: Int => R): Unit = r.foreach(f)` のような**関数値**は
   通りませんでした。宣言が二十箇所以上あるので、規則を 1 箇所に書くために
   `crates/typer/src/prelude_mism9.rs` で prelude が書いた形
   （型パラメータ無し・1 引数・`A => Unit` を取り `Unit` を返す `foreach`）
   だけをその場で多相化します。`U` は `Object` に消去され、引数は
   どちらにせよ `Function1` なので、記述子は変わりません。

5. **型の付かなかった木を二度報告していた**。`adapt` の最後の
   `type mismatch` は、`found` が `<notype>`＝**typer がその木に型を付けられ
   なかった**ときにも出ていました。原因はその木のところで必ず報告されている
   ので、これは繰り返しです（nsc の `ErrorType` は同じように吸収します）。
   同じ関数の他の腕（オーバーロード・コンストラクタ）はすでに失敗した被演算子
   について黙る作りだったので、それに揃えました。

slick: `errors 327 → 308`、`type mismatch 44 → 26`、`files_with_errors` 64
（変わらず）。**新しい種類のエラーは 1 つも出ず、新しくエラーになったファイルも
ありません。**

このスライスで**原因まで分かって直していない**もの、および**最小化できなかった**もの:

- `TreeMap.collect { case (k, v) => … }` の `K2` が `Any`（`Ordering[Any]` を
  探しに行く）。`TreeSet.collect { case x => x }`（型パラメータ 1 本）は通ります。
  対の分解と implicit 節が絡む形だけが残っています。**第 10 スライスで直しました**
  （下の「クラスヘッダの 2 パス…」の 3 番目）。なお当時ここに書いた
  「`tm.collect(pf)`（型注釈付きの値）は通ります」は**誤りでした** ── 型検査は
  通っていましたが、実行時に `List` が返っていました（同節の 4 番目）。
- `MemoryProfile` の `found: DDL required: SchemaDescriptionDef` 2 件。
  `class DDL extends SchemaDescriptionDef` と
  `type SchemaDescription = SchemaDescriptionDef` は同じ trait を指しているのに
  適合しません。継承の菱形・自分型・別ファイルを足しても**最小再現が作れません
  でした**（`tests/fixtures` には入れていません）。
- `HeapBackend` / `DistributedBackend` の
  `found: ActionListener[F] required: ActionListener[F]`（同じ表示で違う symbol）。
  `override val al: AL[F] = AL.noop[F]` をコンストラクタ既定値に書いた形ですが、
  これも**最小再現が作れませんでした**。→ **第 10 スライスで最小化して直しました**
  （`class HkBox[F[_]](val cell: Cell[F] = Cell.empty[F])` の 1 行。`found` 側の
  `F` は別 symbol ですらなく、**解決されていない名前**でした）。
- `OptionMapper.scala` の `found: TypedType[Option[Option[Any]]] required:
  TypedType[Option[Any]]` 2 件（`agent/buildfrom` が持ち込んだもの）。
  `trait OptionTypedType[T] extends TypedType[Option[T]]` の階層を写しても
  再現しません。
- `ExtensionMethods.scala` の `BP` / `P` 3 件は、直前の
  `No matching Shape found`（slick の `Shape` の implicit 探索）のカスケードです。
- `mutable.ArrayBuilder` に `Builder[E, Array[E]]` の基底型が無い（第 8
  スライスから続く可変コレクション階層の穴）。第 10 スライスで、これが
  「スタブに親を付けない」制約そのものだと分かりました（`ArrayBuilder` /
  `Iterator.GroupedIterator` は**メンバを一度も尋ねられていない**スタブなので
  親鎖がありません）。→ **この読みは誤りでした**。第 11 スライスで再検証した
  ところ、`GroupedIterator` は `withPartial` を尋ねた時点で親（`AbstractIterator
  [Seq[B]]`）が付いており、原因は**線形化置換の捕獲**でした。`ArrayBuilder` も
  親は classfile 由来で付いていて、原因は**その親の引数が消去されていた**
  ことでした（下の「継承メンバの型パラメータ捕獲…」を参照）。

### クラスヘッダの 2 パスと、ソート済みマップの `collect`（`type mismatch` 第 10 スライス）

`agent/mismatch10` スライス。フィクスチャは `tests/fixtures/mism10_*.scala`、テストは
`crates/cli/tests/mismatch10.rs` です。4 つの原因を直しました。うち 2 つは
**型検査を通って実行時に別物が返る／`VerifyError` になる**サイレントな誤コンパイル
でもありました。

1. **親コンストラクタの実引数を、シグネチャパスの診断ごと報告していた**。
   親の実引数はただの式です。`typecheck_units` は「シグネチャだけのパス」を
   全ユニットに対して 1 回まわしてからボディを型付けしますが、その前半では
   *後ろのファイル*のメンバにまだ型がありません。slick の

   ```scala
   case class ColumnOrdered[T](column: Rep[T], ord: Ordering)
     extends Ordered(Vector((column.toNode, ord)))
   ```

   は `Rep.scala` がコマンドラインの後ろにあるので、シグネチャパスでは
   `toNode` がまだメンバでなく、対が `(?T1, Ordering)` になって
   `found: Vector[Tuple2[T1, Ordering]] required: IndexedSeq[(Node, Ordering)]`
   を出していました。ボディパスは**同じ木をもう一度**型付けして正しく
   `(Node, Ordering)` を得ます。ヘッダパスの診断を捨てるのと同じ理屈で、
   親コンストラクタ適用についてはシグネチャパスの診断を捨てます。本当に
   間違っている親引数は、全シグネチャが揃ったパスがそのまま報告します
   （`mism10_wrong_parent_argument_is_rejected`）。**同一ファイル内でも
   宣言順で起きます**（`mism10_parent_argument_sees_a_later_member`）。

2. **プライマリコンストラクタの既定引数が、クラスの型パラメータを名前で引けなかった**。
   プライマリコンストラクタは自分の型パラメータを持ちません（`A` は*クラス*のもの）。
   さらに、コンストラクタの既定値には `name$default$n` ゲッタがありません
   （`new Foo(1)` の時点でレシーバが無い）。そのため namer が保存した木を
   **呼び出し側のスコープ**でそのまま型付けしていて、そこには `A` の束縛が
   ありません。

   ```scala
   class Box[A](val one: List[A] = List.empty[A])   // found: List[A]  required: List[A]
   class HkBox[F[_]](val cell: Cell[F] = Cell.empty[F])
   ```

   `found` 側の `A` は**解決されていない名前**でした（`Type::Named`）。既定値の
   本体を型付けする前に、そのパラメータの型が書かれている型パラメータを名前で
   束縛します。これが slick の `HeapBackend` / `DistributedBackend` の
   `found: ActionListener[F] required: ActionListener[F]`（同じ表示で違う symbol）
   の正体で、第 9 スライスが「最小再現が作れなかった」と記録していたものです。
   通常のメソッドの既定引数は自分の型パラメータを持つので影響を受けません
   （`mism10_method_default_still_works`）。

3. **未決定の型変数が対の中にあると、部分関数リテラルの本体がそれを決められなかった**。
   呼び出しが解いていない callee の型変数は、引数の位置には*宣言された上限*で
   届きます（`open_to_bounds`）。`SortedMapOps.collect[K2, V2](pf: PartialFunction
   [(K, V), (K2, V2)])(implicit Ordering[K2])` はリテラルに
   `PartialFunction[(Int, String), (Any, Any)]` として届きます。**裸の**型変数は
   すでに「何も言っていない」と見なして本体に決めさせていましたが、**対の中の**
   型変数はそうしていなかったので `case` 本体が `(Any, Any)` として型付けされ、
   `Ordering[Any]` を探しに行っていました。上限まで開かれた型変数だけからなる
   *タプル*も同じく「何も言っていない」と扱います。タプルの要素は必ず参照なので、
   期待型 `Any` が強いていた箱詰めを落とす心配がありません。

4. **ピクルからのメンバ供給がレシーバではなく祖先に載り、順序で結果が変わっていた**。
   ライブラリのメンバは必要になった時点でピクルから読まれ、**それを宣言している
   クラス**に載ります。祖先に載った時点で以降の継承ルックアップが当たるので、
   派生クラス自身のオーバーロードは二度と尋ねられません。

   ```scala
   val plain = Map(1 -> "a")
   println(plain.collect { case (k, v) => (k, v) })      // ここで MapOps.collect が Map に載る
   val pf: PartialFunction[(Int, String), (Int, Int)] = { case (k, v) => (k, v.length) }
   TreeMap(1 -> "a").collect(pf)                          // → List((100,1)) が返っていた
   ```

   `TreeMap.collect` は `MapOps.collect(pf)` に解決され、呼び出しは
   `IterableOps.collect` として出ていました。その既定実装は `iterableFactory`
   経由で組み立てるので、**`List` が返ります**。診断はどこにも出ず、しかも
   同じファイルの前に `Map.collect` があるかどうかで結果が変わりました。
   継承したメンバしか見つからなかったときは、**レシーバのクラスファイルが同名で
   別アリティのメソッドを宣言している場合に限り**ピクルにもう一度尋ね、両方を
   突き合わせます（`TreeMap.collect(PartialFunction, Ordering)` 対
   `MapOps.collect(PartialFunction)`）。同じディスクリプタの単なるオーバーライド
   （`List.length` 対 `Seq.length`）は仮想ディスパッチで解決されるので尋ねません
   ── prelude は `aSet.toSeq` を `List` と型付けする一方で呼び出す
   `toSeq` は `Seq` を返すので、その値に `invokevirtual List.length` を出すと
   `VerifyError` になります。

slick: `errors 257 → 241`、`type mismatch 25 → 22`、`files_with_errors 63 → 61`。
**新しい種類のエラーは 1 つも出ず、新しくエラーになったファイルもありません。**
（第 9 スライスが記録した 327 / 44 / 64 は、その後 `agent/tail1`・`agent/quasi`
などが入った現在の `main` では 257 / 25 / 63 が基準値です。）

このスライスで**分かっているが直していない**もの:

- `mutable.ArrayBuilder` に `Builder[E, Array[E]]` の基底型が無い、
  `Iterator.GroupedIterator[B]` の要素型が `Seq[B]` でなく `B` になる、といった
  「**スタブに親を付けない**」制約の系列（上の「まだできないこと」を参照）。
  `xs.iterator.grouped(2).map { case Seq(i, t) => (i, t) }` は要素型を取り違えた
  ラムダを出すので、`VerifyError` になるサイレントな誤コンパイルでもあります。
  → **第 11 スライスで直しました**。原因はこの制約ではありません（次節）。
- `MemoryProfile` の `found: DDL required: SchemaDescriptionDef` 2 件。
  抽象型メンバ `type SchemaDescription <: SchemaDescriptionDef` を継承先で
  `= SchemaDescriptionDef` に固定する形は書けましたが、slick と同じ症状には
  ならず（別の `Basic.SchemaDescription` 未解決になる）、まだ最小化できていません。
- `OptionMapper` の `TypedType[Option[Option[Any]]]` 2 件、
  `ExtensionMethods` の `BP` / `P` 3 件、`Query.scala` の 3 件、
  `JdbcActionComponent` の `E with Effect` 2 件、`Type.scala:388` の
  `BigDecimal.apply` のイータ展開（**単独ファイルでは再現しません**。
  `java.math.MathContext` をシンボル表に入れても再現しないので、多ファイル依存です）。

### 継承メンバの型パラメータ捕獲と、消去された親（`type mismatch` 第 11 スライス）

`agent/mismatch11` スライス。フィクスチャは `tests/fixtures/mism11_*.scala`、
テストは `crates/cli/tests/mismatch11.rs` です。3 つの原因を直しました。うち 2 つは
**型検査を通って実行時に別物を分解する／呼べるはずの呼び出しを断る**もので、
第 10 スライスが「**スタブに親を付けない**制約」と記録した 2 件は、
**どちらもその制約ではありませんでした**（引き継いだ診断の再検証で分かりました）。

1. **ピクルの線形化置換が、メソッド自身の型パラメータに捕獲されていた**。
   継承メンバは各ホップで「子が親に渡した引数」を代入して、尋ねたクラスの
   語彙に直されます（`SigCache::lookup`）。
   `Iterator.GroupedIterator[B] extends AbstractIterator[Seq[B]]` はその置換が
   `A := Seq[B]` で、当たる先の `Iterator.map[B](f: A => B): Iterator[B]` は
   **自分の `B`** を束縛します。名前で代入していたので*クラスの* `B` が
   *メソッドの*束縛子の下に落ち、`map` は `Seq[B]` ではなく `B` を取る、という
   一つの型になっていました。`apply_subst` を捕獲回避にして、置換の**値側**に
   自由に現れる名前を束縛している型パラメータだけ改名します
   （`crates/pickle/src/sym.rs` の `avoid_capture`）。

2. **「コレクションの要素型はレシーバの第 1 型引数」という規則が、宣言が
   はっきり言っている引数型を上書きしていた**。`grouped(n)` が返すのは
   `GroupedIterator[B]` で、要素は `Seq[B]` です。推測が宣言に勝ってはいけない
   ので、上書きするのは**引数が 1 つで、その型がまだ決まっていない**ときだけに
   しました。ついでに `LazyZip2[A, B, C].map(f: (A, B) => R)` のような
   **2 引数**の関数を 1 引数に潰していたのも直ります
   （`xs.lazyZip(ys).map((a, b) => …)` は `found: (String, Int) => String
   required: (String) => Any` になっていました）。
   1 と 2 が揃って、`clauses.iterator.grouped(2).map { case Seq(i, t) => (i, t) }`
   （slick `Node.scala:724`）が通ります。これは**要素型を取り違えたラムダを
   出していた**ので、`VerifyError` になるサイレントな誤コンパイルでもありました。

3. **`scala.` のプレースホルダにピクルの型パラメータを付けていなかった**。
   `find_or_stub_java_class` は classfile の親リストが名指した名前をすべて
   空のシンボルとして入れます。`give_stub_its_kinds` はそこに型パラメータを
   付ける役ですが、`scala/` で始まる名前を一律に断っていました。断る理由は
   「**prelude が組んだ**シンボルを作り替えない」ことなので、線は `scala.`
   パッケージではなく `prelude_end` です。`ArrayBuilder` の親リストから入った
   `scala/collection/mutable/ReusableBuilder` がまさにそれで、
   `ReusableBuilder[T, Array[T]]` が「引数 2 個だがシンボルは 0 個」になり、
   `ArrayBuilder` は親を得られませんでした。
   さらに、classfile の総称シグネチャは `ArrayBuilder<T> implements
   ReusableBuilder<T, Object>` としか書けません。`To` は非変なので、これでは
   `Builder[E, Array[E]]` になりません。**同じクラスを指す親が、引数だけ違って
   すでに載っている**ときは、ピクルの側（scalac 自身の記録）で**精密化**します
   （prelude のクラスには一切触れません）。これで
   `mutable.ArrayBuilder.make[E]` を `mutable.Builder[E, Array[E]]` として
   返せます（slick `Type.scala:203`）。

4. **未決定の*型構築子*が、引数の期待型に上限として届いていた**。
   `Any` は構築子の**種**の住人ではありません。slick の
   `flatMap[F, T, D[_]](f: E => Query[F, T, D])` はラムダに
   `Query[F, T, Any]` として届き、本体の `Query[G, T, Seq]` が
   `found: Query[G, T, Seq] required: Query[G, T, Any]` になっていました
   （`Query.scala:37`）。`open_to_bounds` は、型パラメータ自身が型パラメータを
   持つ（＝構築子である）ときはワイルドカードで開きます。「まだ決まっていない
   どれか」を `is_sub_type` が既に理解している形で書くだけです。

slick: `errors 237 → 234`、`type mismatch 20 → 17`、`files_with_errors 60`
（変わらず）。`tests/slick_subset.sh` は `verified=204 failed=0` で変化なし。
**新しい種類のエラーは 1 つも出ず、新しくエラーになったファイルもありません。**

このスライスで**分かっているが直していない**もの:

- `LazyZip2.map` は、上の 3 で `BuildFrom[C1, B, C]` が書けるようになった結果
  **供給されるようになりました**が、`C` を決められるのは implicit 探索だけで、
  こちらの探索は「手にしている型を探す」ものなので探している型の中の変数は
  解けません。`implicitly[BuildFrom[Seq[String], String, Seq[String]]]`
  （完全に適用した形）でさえ見つかりません。結果、slick の 5 箇所は
  `value map is not a member of LazyZip2[…]` 1 本から
  `no implicit: could not find implicit value of type BuildFrom[…]` ＋
  未解決の `C` に対するカスケード 1 本に変わっています（種類は既存のもの
  だけで、新しくエラーになったファイルはありません）。
  `BuildFrom` の implicit を本当に見つけるには
  `buildFromIterableOps[CC[X] <: Iterable[X] with IterableOps[X, CC, _], A0, A]`
  を高階で照合できる implicit 探索が要ります。**供給を断つ**方向も試しました
  （「型パラメータが implicit 節でしか決まらないメンバは供給しない」）が、
  それは効きすぎて `errors 235 → 309` になったので入れていません。
- 残る `type mismatch` 17 件のうち、`MemoryProfile` の `DDL /
  SchemaDescriptionDef` 2 件、`ExtensionMethods` の `BP` / `P` 3 件
  （`No matching Shape found` のカスケード）、`JdbcActionComponent` の
  `E with Effect` 2 件、`Query.scala` の残り 2 件、`Type.scala:388` の
  `BigDecimal.apply` のイータ展開は、いずれも第 9・第 10 スライスからの
  引き継ぎで、単独ファイルでは再現しません。
  `JdbcModelBuilder` / `SQLiteProfile` の `found: Product required:
  Option[Option[Any]]` 2 件は、`if (v == "NULL") None else Some(…)` の lub が
  `Option` にならない形に見えますが、**その形だけを書いても再現しません**
  （同じファイルの他のエラーからのカスケードです）。
- `ConcurrencyControl.scala:202` は 4 の変更で `found: State[Any]` が
  `found: State[_]` に変わっただけで、まだエラーです（cats の
  `Ref.of[F, State[F]]` 側の話）。

### 構築子の上限・自前の `apply`・コンパニオンが継いだ implicit（`type mismatch` 第 12 スライス）

`agent/mismatch12` スライス。フィクスチャは `tests/fixtures/mism12_*.scala` と
`tests/multi/mism12_*.scala`、テストは `crates/cli/tests/mismatch12.rs` です。
6 つの原因を直しました。うち 2 つは**型検査を通ったうえで別のメンバ・別の型を
選んでいた**もので、引き継いだ診断のうち「`(Double)` オーバーロード未供給」
（第 11 スライス）は正しく、「`Shape` の implicit 導出が本丸」（第 11 スライス）も
正しかったのですが、真因は導出ではなく**コンパニオンが継承した implicit を
そもそも候補にしていなかった**ことでした。

1. **型構築子パラメータの上限を、適用の引数で具体化していなかった**。
   `M[A]`（`M[+X] <: IterableOnce[X]`）は `IterableOnce[A]` です。上限は
   構築子自身のパラメータで書かれているので、それを置き換えるまで意味を
   持ちません。`widen_type_param` は**裸の** `M` しか広げていなかったので、
   slick の `DBIOAction.traverse[A, B, M[+X] <: IterableOnce[X]]` の
   `in.iterator` は `IterableOnce` 自身の `A` を返し、要素を使うたびに
   `found: A required: A`（**表示が同じ別シンボル**）になっていました
   （`DBIOAction.scala:349`）。

2. **case class のコンパニオン `apply` に、クラスの型パラメータをそのまま
   渡していた**（「クラスの型パラメータがメソッドのものを兼ねる」）。
   1 つのシンボルが「ここでは決まっている」と「この呼び出しではこれから
   推論する」の両方を意味してしまい、**クラスの内側からの呼び出し**は
   `U := U` を代入した結果、引数型がまだ callee の型パラメータを含んだまま
   ＝「未決定」と読まれ、引数は**上限**に対して検査されました
   （`found: Bx[U] required: Bx[Any]`）。`fresh_method_tparams` で `apply` に
   自前の型パラメータ（名前・種・境界は同じ、変位は付けない）を与えます。
   slick の `ShapedValue.packedValue`（`ShapedValue.scala:16`）が通ります。

3. **`scala.math.BigDecimal` のコンパニオンに `apply` が 17 個中 3 個しか
   なかった**。手書きの prelude メンバはピクルのコピーを断る
   （`agent/setapply`）ので、足りない分は**存在しません**。
   `new ScalaNumericType[BigDecimal](BigDecimal.apply)`（`Type.scala:388`）は
   `Double => BigDecimal` でイータ展開するので、選ぶ相手が無かったのです。
   `crates/typer/src/prelude_mism12.rs` に `javap` が出す 17 個ぶんを
   書きました（`library_abi` のみ。私有ランタイムは `scala/math/BigDecimal$`
   を出さないので、非 jar モードでは診断が出ます）。

4. **コンパニオンが*継承*した implicit を候補にしていなかった**。SLS 7.2 が
   言うのはコンパニオン**オブジェクト**で、オブジェクトのメンバには継承した
   ものも含まれます。slick は `Shape` のインスタンスをすべて
   `trait RepShapeImplicits` / `ConstColumnShapeImplicits` /
   `TupleShapeImplicits` に書き、`object Shape extends
   ConstColumnShapeImplicits with …` としているので、**1 つも候補に
   なっていませんでした**。`companion_implicits_of_class` を親までたどります。
   継承したものは裸の名前で出すと `this` を積んで宣言元の trait に
   キャストするコード（`Main$ cannot be cast to ConstShapes`）になるので、
   **通ってきたオブジェクトを受け手に**します（`implicit_via_module`。
   ワイルドカード import に対する既存の `wildcard_module_for` と同じ扱い）。

5. **implicit の単一化が `_` と反変位置を扱えなかった**。求める型の中の `_` は
   「そこは訊いていない」という意味なので何にでも一致します
   （`packedValue[R](implicit ev: Shape[? <: FlatShapeLevel, T, ?, R])` の
   `?` に候補の `U` を構造的に突き合わせて「不一致」と言っていました）。
   反変パラメータは向きが逆で、**求める型のほうが部分型**です
   （`constColumnShape: Shape[L, ConstColumn[T], T, ConstColumn[T]]` が
   `Shape[FlatShapeLevel, LiteralColumn[Boolean], ?, ?BP]` に答える）。
   4 と 5 で `ExtensionMethods` の `fold`（`BP`）2 件と `Query.scala:290` が
   通り、そこからの `value toNode/zip is not a member of (…)…` 3 件も消えます。

6. **遅延解決した型エイリアスが、ヘッダパス 1 巡目のスコープで固定されていた**。
   `refresh_alias_sigs` は保留中のエイリアスに「そのテンプレートのスコープ」を
   覚えさせますが、**最初の 1 回だけ**でした。ヘッダパスは親チェインが
   変わらなくなるまで繰り返す設計で、**自分より後のファイルに祖父がいる**
   クラスの継承メンバは 2 巡目以降にしか見えません。slick の
   `trait MemoryProfile extends RelationalProfile`（`slick/memory/` は
   `slick/relational/` より前）は `type SchemaDescription =
   SchemaDescriptionDef` と書き、`SchemaDescriptionDef` は
   `BasicProfile` の入れ子 trait で、`MemoryProfile.scala` はその名前を
   import していません。入れ子クラスのコンストラクタ引数
   （`class MemorySchemaActionExtensionMethodsImpl(schema: SchemaDescription)`）
   がヘッダパス中にエイリアスを完成させるので、右辺は**未解決の
   `Type::Named`** のまま残り、`new DDL(…)` が
   `found: DDL required: SchemaDescriptionDef`（**両方 `SchemaDescriptionDef`
   と表示される別物**）になっていました。**最後の**巡回のスコープを使います
   （どの巡回のスコープもそのテンプレート自身のものなので、後のほうが
   常により完全です）。第 9〜11 スライスが 3 回続けて最小化に失敗していた
   2 件で、`tests/multi/mism12_*.scala` の 4 ファイルで再現します。

slick: `errors 223 → 209`、`type mismatch 17 → 9`、`files_with_errors 60`
（変わらず）。`tests/slick_subset.sh` は `verified=204 failed=0` で変化なし。
**新しい種類のエラーは 1 つも出ず、新しくエラーになったファイルもありません。**

このスライスで**分かっているが直していない**もの:

- `a ++ b` で `++` が `SchemaDescriptionDef` の宣言（引数は抽象型メンバ
  `SchemaDescription`）のとき、`MemoryProfile` から見た `++` の引数型が
  `BasicProfile.SchemaDescription` のままで、`no matching overload for
  (BasicProfile.SchemaDescription)…` になります。6 とは別の穴
  （**抽象型メンバの as-seen-from**）なので、`tests/multi/mism12_*.scala`
  からは外してあります。
- 残る `type mismatch` 9 件: `Node.scala:636` の
  `found: <overload String | <error>>`、`ConcurrencyControl.scala:202`、
  `JdbcActionComponent` の `E with Effect` 2 件、`JdbcModelBuilder` /
  `SQLiteProfile` の `found: Product required: Option[Option[Any]]` 2 件、
  `ExtensionMethods.scala:210`（`flatten` の `P <:< Rep[Option[QO]]`）、
  `Query.scala:153`、`RelationalProfile.scala:72`。
- 第 11 スライスが記録した `LazyZip2.map` の `BuildFrom` 高階照合は
  そのままです（4 と 5 では届きません）。

### `import <値>._` で入れた view（`agent/tail2`）

フィクスチャは `tests/fixtures/t2_*.scala`、テストは
`crates/cli/tests/tail2.rs` です。slick の `MySQLProfile` /
`JdbcStatementBuilderComponent` が書く

```scala
import seq.integral._
val desc = increment < zero
val beforeStart = start - increment
if (desc) "…" + (-increment) + "…"
```

は全部 `value <op> is not a member of T` でした。原因は 4 つあり、いずれも
「**generic クラスのインスタンスメンバである変換を、値を通して使う**」という
同じ形です。

1. **jar クラスの implicit がそもそもスコープに入らない**。メンバーは pickle から
   名前 1 つずつ読みますが、implicit は誰も名前を書かない（スコープを探して
   見つけるもの）ので、`Numeric#mkNumericOps` / `Ordering#mkOrderingOps` は
   一度も要求されませんでした。同じ理由で `Option.option2Iterable` もどこにも
   無く、`where.reduceLeft(f)` / `c.where.toSeq ++ on`（`Option[Node]`）が
   `value reduceLeft is not a member of Option[Node]` でした。`import <値>._`
   と「型の implicit スコープのコンパニオン」の両方で、pickle に **どの名前が
   implicit か** を聞き、その名前だけを通常の on-demand 経路で補完します
   （クラスが既にメンバーを持つ名前は聞かないので、prelude が勝つのは従来どおり。
   プリミティブのコンパニオンは対象外 — `object Int` の implicit は数値 widening
   そのもので、view として並べると `n + ":"` が ambiguous になります）。
2. **候補が owner の型パラメータのままだった**。`b: Box[Int]` を通した
   `class Box[T] { implicit def mkOps(lhs: T): Ops[T] }` は `Int => Ops[Int]`
   です。値だけがそれを言えます（`Typer::at_import_prefix_of`）。
3. **オーバーライドした変換が 2 個に数えられていた**。`Integral[T]` は
   `Numeric[T]#mkNumericOps` の結果を `NumericOps` から `IntegralOps` に
   狭めます。import 後は両方の名前がスコープにあり、結果クラスも宣言する
   `unary_-` シンボルも違うので、既存の「同じ変換に 2 経路」の規則では落ちず、
   探索が諦めていました。nsc ではメンバーは 1 個（派生側）です。
4. **generic クラスにネストしたクラスのメンバーが読めなかった**。
   `Ordering[T]#OrderingOps` の `def <(rhs: T)` の `T` は *`Ordering`* の
   パラメータで、`OrderingOps` 自身は 1 つも持ちません。マップできない名前として
   メンバーの install ごと失敗していました。外側のパラメータで読み、変換と同じ
   prefix で置換します。

これとは別に、**型検査を通ったうえで実行時に落ちる**バグが 1 つありました。この
変換は値のインスタンスメンバなのに素の名前で出していたので、codegen が `this` を
積んでキャストし、`class Main$ cannot be cast to class NoTp` になっていました。

slick: `errors 203 → 196`、`files_with_errors 60`（変わらず）。
`tests/slick_subset.sh` は `verified=204 failed=0` で変化なし。新しい種類の
エラーは出ず、新しくエラーになったファイルもありません（既にエラーのある 2 ファイルで、
先行するエラーの後続が 1 行ずつ増えています）。

このスライスで**分かっているが直していない**もの:

- generic な親から継承した内部クラスを、サブクラスの内部クラスが継承するとき
  （`class SubBox[T] extends Box[T] { class Sharper(lhs: T) extends Inner(lhs) }`）、
  `Inner` の構築子パラメータが `Box` の `T` のままで `found: T required: T` に
  なります（as-seen-from の別の穴）。
- ブリーフが挙げていた `a ++ b`（引数が抽象型メンバ `SchemaDescription`）の
  `no matching overload for (BasicProfile.SchemaDescription)…` は、現在の
  計測ログには**もう出ていません**。
- `LazyZip2.map` の `BuildFrom` 高階照合（`toSeq` / `mkString is not a member of C`
  4 件 ＋ `could not find implicit value of type BuildFrom[…, C]` 4 件）は
  次の節（`agent/buildfrom2`）で塞ぎました。

### `BuildFrom` の高階 implicit 照合（`LazyZip2`、`agent/buildfrom2`）

`agent/mismatch11` と `agent/tail2` が原因まで書いて未着手にしていた残件です。
フィクスチャは `tests/fixtures/bf2_lazyzip.scala` / `bf2_lazyzip_bad.scala`、
テストは `crates/cli/tests/buildfrom2.rs`。

2.13 の `LazyZip2` は

```scala
class LazyZip2[+El1, +El2, C1] {
  def map[B, C](f: (El1, El2) => B)(implicit bf: BuildFrom[C1, B, C]): C
}
```

で、`C` は **implicit 節にしか現れません**。つまり結果型を決められるのは
witness だけで、汎用の witness は 1 つしかありません。

```scala
implicit def buildFromIterableOps[CC[X] <: Iterable[X] with IterableOps[X, CC, _], A0, A]
  : BuildFrom[CC[A0], A, CC[A]]
```

両者の間に 5 つ穴があり、**手前の穴が奥の穴を隠していました**。

1. **`BuildFrom` のコンパニオンがシンボル表に無かった**。jar クラスの
   コンパニオンを読む `load_companion_module` は `scala/` を一律に断って
   いました。理由は「標準ライブラリを describe するのは prelude だ」ですが、
   prelude が describe するのは**プログラムが名前で書くもの**で、implicit は
   誰も名前を書きません（スコープを探して見つけるもの）。だから
   `import scala.collection.BuildFrom` とたまたま書いたプログラム以外では、
   `BuildFrom` の witness はどのスコープにも入っていませんでした。
   手書きの宣言は何も置き換えません: すでにコンパニオンを持つクラスは先頭の
   早期 return で素通りし、同じ JVM 名のコンパニオンが既に入っていれば二重に
   入れず、`scala.*` については**入れるのは implicit だけ**で、それ以外は
   これまで通り pickle からの on-demand です（classfile が入れたメンバは
   落とします。Java の総称シグネチャは `CC[A]` を綴れないので、pickle 由来の
   宣言の隣に**消去された別のオーバーロード**として並んでしまいます）。
2. **低優先の半分がまだ足りなかった**。
   `object BuildFrom extends BuildFromLowPriority1 extends BuildFromLowPriority2`
   で、`buildFromIterableOps` は**一番下**のトレイトが宣言します。
   コンパニオンだけ読んでも見えないので、親も辿って implicit を供給します。
3. **供給した implicit を、その場で消していた**。`supply_implicit_members` は
   pickle 由来のシグネチャで置き換えた classfile 由来のメンバを落としますが、
   補完は**一度出した名前を覚えている**ので、答えがすでに pickle 由来の
   メンバだったとき、それが「落とす側」と「入れる側」の両方になり、クラスは
   その名前のメンバを 1 つも持たなくなっていました。
4. **二方向の単一化が、未知の*型構築子*を照合できなかった**。`CC[A0]` は頭が
   型パラメータの `Applied` で、`List[String]` は `Class` です。両者を結ぶ枝が
   無く `a == b` に落ちていました。**完全適用した** `implicitly[BuildFrom[…]]`
   が通っていたのは、そこだけ一方向の `unify_one`（構築子を読める）に
   フォールバックするからで、そのフォールバックは**呼び出し側に未確定
   パラメータがあるときだけ飛ばされます** — それがまさに `LazyZip2.map` です。
   `xs.lazyZip(ys).map(f)` が
   `could not find implicit value of type BuildFrom[…, C]` ＋
   `value mkString is not a member of C` だったのはこれです。
5. **witness を区別するものが無かった**。`BuildFrom` の witness たちは
   **境界以外は同じ型**です。高階の境界は型の中に畳み込まれて届くので
   （`buildFromSortedSetOps` は
   `BuildFrom[CC[A0] with SortedSet[A0], A, CC[A] with SortedSet[A]]`）、
   交差型を単一化することがそのまま境界検査になります。ただし
   `immutable.TreeSet` が `collection.SortedSet` だと prelude の階層が
   言っていなかったので（`val x: scala.collection.SortedSet[Int] = TreeSet(1)`
   も `type mismatch` でした）、ソート版が当たらず**非ソート版が答えて**
   `iterableFactory` で組み立て、`TreeSet(1,2).lazyZip(ys).map(f)` が
   `class Set$Set3 cannot be cast to class TreeSet` になっていました。
   一階の F-bound は `bound_hi` に残るので、そちらは nsc の `checkBounds`
   相当を足します。検査しないままだと
   `buildFromBitSet[C <: BitSet with BitSetOps[C]]: BuildFrom[C, Int, C]`
   が `List` に対して答えてしまい（コンパニオン直下なので origin で勝つ）、
   `List(1, 2).lazyZip(…).map(_ + _)` は**型検査を通ってから**
   `class ::$ cannot be cast to class scala.collection.BitSet` で落ちました。

**フィクスチャを読むのではなく走らせて**見つかったバグが 3 つあります。

- **自分の implicit 節を持つ witness が、素の名前で出ていた**。
  `implicit_tree` はその枝だけ `ref_implicit` を通さず `Ident` を組んでいたので、
  コンパニオンが mixin したトレイトの宣言（`buildFromSortedSetOps` はまさに
  これで、しかも `Ordering` を取ります）が `this` を積んでキャストされ、
  `class Main$ cannot be cast to class BuildFromLowPriority1` になりました。
- **呼び出し側の未知の型構築子を、変換が決めてはいけない**。候補**自身**の
  型パラメータ `CC` を解くのが 4 の目的で、呼び出し側の `M[_]` は引数からの
  通常の推論が決めるものです。区別せずに開いたところ、
  `firstLength[A, M[+X] <: Iterable[X]](in: M[A])` が、すでに `M := List` で
  適合している `List[Int]` に対して
  `IterableOnce.iterableOnceExtensionMethods` を「`M[A]` に届く変換」として
  受け入れました（`tests/fixtures/mism12_lib.scala` が
  `ClassCastException` で捕まえました）。単一化の未知は 2 種類に分け、
  **構築子に立てるのは候補自身の型パラメータだけ**にします。

- **標準ライブラリのコンパニオンを classfile から入れると、pickle の宣言と
  二重になる**。`object Option` はこれまで pickle の空スタブとして届いていて、
  `apply` も pickle から来ていました。classfile 側の消去された `apply` が
  隣に並んだ結果、`Option(2)` が `ambiguous overload for apply` になりました
  （`tests/fixtures/jarpk.scala`）。`scala.*` のコンパニオンは classfile の
  メンバを捨てて、これまで通り pickle に任せます。

slick: `errors 177 → 166`、`files_with_errors 57 → 56`
（`QueryInterpreter.scala` が丸ごと通るようになりました）。
`tests/slick_subset.sh` は `verified=204 failed=0` / `subset_files=38 classes=204`
で変化なし。**新しい種類のエラーは 1 つも出ず、新しくエラーになったファイルも
ありません**（消えたのは `BuildFrom[…]` の `no implicit` 4 件、`C` に対する
`is not a member` 4 件、それに巻き添えだった `Function0[…] IO[…]` と
`NotGiven[…]` が 1 件ずつ）。

このスライスで**分かっているが直していない**もの:

- `scala.collection.immutable.ArraySeq(1, 2, 3)` は
  `no implicit: could not find implicit value of type AnyRef[AnyRef]` ＋
  `value lazyZip is not a member of Builder[A, ArraySeq[A]]` になります
  （`ArraySeq.apply` の `ClassTag` 側の別件で、`lazyZip` / `BuildFrom` には
  届いていません）。フィクスチャからは外してあります。
- 高階パラメータの F-bound のうち `IterableOps[X, CC, _]` の部分は検査して
  いません。prelude のコレクションは `IterableOps` の引数まで持っていないので、
  検査すると nsc が受け入れる候補まで落ちます。nsc がこの部分で
  `buildFromIterableOps` を退けるのはソート済みコレクションのときだけで、
  そちらは 5 の交差型と階層で同じ結論になります。
- `collection.SortedSet` / `collection.SortedMap` は今回 `prelude_hier.rs` の
  リンク（メンバを持たない中継ノード）として入れました。`firstKey` などを
  これらの型の値に対して直接呼ぶ形は、pickle からの on-demand 供給に任せて
  います。

### ブロックの値を二重に箱詰めしていた（消去）

`agent/anonbridge` スライス。**型検査は通り、実行時に `VerifyError` になる**
サイレントな誤コンパイルでした。

```scala
val i = new It[Int] { def next(): Int = { val z = 1; z } }   // VerifyError
val j = new It[Int] { def next(): Int = z }                  // これは動いていた
```

```text
java.lang.VerifyError: Bad type on operand stack
  Location: Main$$anon$1.next()Ljava/lang/Object; @6: invokestatic
  Reason:   Type 'java/lang/Integer' is not assignable to integer
```

消去は `Block` / `If` / `Match` / `Try` の**期待型を、その値を作る部分式に
そのまま渡します**（`{ …; z }` なら `z`、`if` なら両枝、`match` なら各 case
本体、`try` なら本体と各 handler）。したがってそれらは**すでに箱詰め済み**
なのですが、`erase_tree` の末尾は続けてノード自身にも `adapt_box_unbox` を
かけていました。ノードの `ty` は箱詰め前の `Int` のままなので条件が成立し、
`boxToInteger(boxToInteger(z))` が出ます。式本体は降りる先のノードが無いので
1 回で済んでいた、というのが「ブロックのときだけ壊れる」の正体です。

直したのは `crates/typer/src/erasure.rs` の 1 か所です。上の 4 種のノードでは
**変換を二度目にかけるのをやめ、枝が持つに至った型を記録するだけ**にしました
（`box_adaptation` が返す変換の結果型をノードの `ty` に入れる）。判定そのものは
`adapt_box_unbox` と同じ関数を共有しているので、値クラスの `new Meters(n)` /
`((Meters) x).n()` を含めて挙動は 1 か所で決まります。

実 scalac は同じ匿名クラスを**メソッド 2 本**にします（本体を持つ `next()I` と、
それを呼んで箱詰めするブリッジ `next()Ljava/lang/Object;`）。こちらは 2 本を
畳んで消去後シグネチャの 1 本だけを出します。呼ぶ側から見た入口
`next()Ljava/lang/Object;` は両者にあり、**そこで箱詰めがちょうど 1 回**という
のが正しい形です。`crates/cli/tests/anonbridge.rs` の
`scalac_and_ours_agree_on_the_erased_entry_point` が `javap -p -c -s` で
両方を並べて固定しています。

匿名クラスに限った話ではありませんでした。同じ二重箱詰めは
`val x: Any = { val z = 1; z }`、`id({ val z = 1; z })`、`if` / `match` / `try`
を本体に持つ形、名前付きクラス（`class C extends It[Int]`）、`abstract class`
の実装、SAM 変換したラムダ、値クラス、そして逆向きの二重**開き**
（`val n: Int = { val z: Any = 1; z.asInstanceOf[Int] }`）にも出ていました。

slick の数字は動きません（`files=184 errors=378 files_with_errors=67` のまま）。
型検査を通り抜けるバグなので、エラー数には現れない種類の修正です。

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
- `java.*` は対象外です。`scala.*` は **prelude が組み立てたシンボルだけ**が対象外
  です（`SymbolTable::prelude_end` より前の id）。標準ライブラリのうち prelude が
  手で書いている部分は prelude ＋ `complete` という検証済みの経路を通り、
  prelude が名前を出していないもの（`scala.concurrent.Future` など）は
  classfile しか情報源が無かったので、ここで pickle から読みます。
  詳しくは「コンパニオンとクラスは別のシンボル（`agent/companionkind`）」。
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
- **`scala.collection.mutable` のコレクション一式**（`agent/mutcoll` スライス、jar リンク時のみ）。
  `f(args) = v` の `f.update(args, v)` への desugar（SLS 6.15）は配列・ユーザークラス・
  多引数 `update`・選択された受け手・ジェネリックな `update`・`Unit` 以外を返す `update` の
  どれでも効く（**私有ランタイムでも動く**。`update` を持たない受け手は
  `value update is not a member of …` で拒否する）。**コンパニオンの varargs `apply` が
  同名の immutable コレクションを返していたバグ**を直した（`mutable.Set(1,2,3)` が
  `scala.collection.immutable.Set` と推論され、`+=` / `-=` / `++=` / `--=` / `add` が
  「not a member」になっていた。`check.rs::factory_result_class`。ファクトリの
  ショートカットは型引数だけを差し替えるもので、クラスは宣言された結果型のものを使う）。
  新しく `mutable.Queue` / `Stack` / `TreeMap` / `TreeSet` / `PriorityQueue` / `ArraySeq` の
  コンパニオン（0 引数を含む varargs `apply` と `empty`。`TreeMap` / `TreeSet` /
  `PriorityQueue` は `Ordering`、`ArraySeq` は `ClassTag` の implicit 証拠つき）を
  `crates/typer/src/prelude_mutcoll.rs` に宣言した。これらは `IterableFactory` /
  `SortedIterableFactory` / `EvidenceIterableFactory` から `apply` を継承していて、
  classfile シグネチャでは可変長パラメータが既に `Seq[A]` に、結果が抽象 `CC` に
  なっているため、`Queue[Int]()` すら
  `no matching overload for (Seq[Int])CC with arguments ()` だった。あわせて
  `ArrayDeque.append`（`Buffer` の default メソッドなので戻り値は `Buffer`）、
  `PriorityQueue.enqueue(elems: A*)`、`ArraySeq` の `apply` / `update` / `length` /
  `size` / `toList`、`mutable.StringBuilder` のコンパニオン `newBuilder`（以前は
  型検査を通って実行時に `RuntimeException: select StringBuilder` を投げていた）、
  `Growable` / `Shrinkable` の `++=` / `--=` / `-=` を新しい型にも（`prelude_mutops.rs`）。
  `new Queue[Int]()` / `new Stack[Int]()` / `new ArrayDeque[Int]()` は 2.13 では
  `class Queue[A](initialSize: Int = ArrayDeque.DefaultInitialSize)` なので `<init>()V` が
  存在せず、以前は型検査を通って実行時に `NoSuchMethodError` になっていた
  （合成デフォルトゲッター `$lessinit$greater$default$1` を呼ぶようにした。
  `gen.rs::has_default_sized_ctor`）。`new TreeMap[K, V]()` /
  `new TreeSet[A]()` / `new PriorityQueue[A]()` は `Ordering` の implicit 節つき
  コンストラクタとして宣言した。**診断**: `op=` が受け手のメンバーでないときは nsc と
  同じく**1 つのエラー**（2 行目が
  `Expression does not convert to assignment because receiver is not assignable.`）に
  まとめた。以前は 2 つ別々に出ていて、直前の `m("a") = 1` が失敗したように読めた

## 実装していないもの

次は実装していません。スタブで「動いたことにする」こともしていません。言語側の残りとライブラリ側の残りを分けます。

言語:

- **def マクロの展開の残り**。展開そのものは動きます（上の「def マクロの展開
  （JVM ブリッジ）」）。まだできないのは:
  **whitebox マクロ** / **macro bundle**（`class B(val c: Context)`）/
  **マクロバインディングの pickle**（`MACRO` フラグと `@macroImpl`。だから
  マクロ def を*別 run*から展開することはできず、「マクロ def は現在の run、
  実装は前の run」という形だけが通ります）/ **`c.Expr[T](tree)` を返す実装**
  （`Context.Expr` のオーバーロードに解決しない）/ **推論された型引数のタグ**
  （明示された `f[T]` だけ）/ **`c.prefix` / `c.enclosingPosition` /
  `c.typecheck` / `c.inferImplicitValue`**（呼ばれると engine が
  `UnsupportedOperationException` を投げ、その名前が診断に出ます）/
  **ブロック・関数リテラル・`new` などの引数を実装に渡すこと** /
  **型引数のある型のタグ**。どれも「黙って別の木に展開する」ことはせず、
  `macro expansion is not implemented: cannot expand f (implementation Impl$.m):
  <理由>` と理由つきで診断します
  （**[`docs/macros.md`](docs/macros.md)** §7.11）
- **quasiquote の展開（reification）の残り**。`q"..."` / `tq"..."` / `pq"..."` /
  `cq"..."` は `internal.reificationSupport.Syntactic*` の呼び出しに落として
  実行でき、型注釈 / eta 展開 / ブロックと `val` / `new` / `match` / 部分関数 /
  関数リテラル / 型・パターン・`case` 節まで**実 scalac 2.13.16 と `showRaw` が
  一致**します（`tests/fixtures/qr_forms.scala`）。`Tree` でない穴も標準の
  `Liftable` 相当の木に持ち上がります（`tests/fixtures/lf2_lift.scala`）。
  fresh 名を要する 3 形（`_` プレースホルダ、`_` 型引数＝存在型、
  右結合演算子）も、nsc と同じ `freshTermName` / `freshTypeName` のブロックごと
  組みます（`tests/fixtures/fn2_fresh.scala`）。
  残っているのは、パーサが nsc の保つ区別ごと正規化してしまう形
  （`else` の無い `if`、by-name 型）、
  `..$` と普通の引数の混在、`type` 定義、標準インスタンスの無い型の穴
  （`liftList` / `liftTuple*` など）、そして `reify { … }` と
  `TypeTag` の materialization です。いずれも
  （右結合演算子 `a :: b`、`else` の無い `if`、`_` プレースホルダ、by-name 型）、
  `..$` と普通の引数の混在、`class` / `def` 定義、標準インスタンスの無い型の穴
  （`liftList` / `liftTuple*` など）、そして `reify { … }` です
  （`TypeTag` / `WeakTypeTag` の materialization は**単相型について実装済み**。
  型引数のある型・入れ子クラスは名指しで断ります。§7.10）。いずれも
  `unimplemented syntax: quasiquote ... (どの形か)` / `a hole of type X is not
  lifted (…)` / `cannot expand reify { ... }` と**名指しして**報告します
  （黙って通しません）。何が要るかは
  一致**します（`tests/fixtures/qr_forms.scala`）。定義（`class` / `case class` /
  `trait` / `object` / `def` / 修飾つき `val`・`var`）も同じく一致します
  （`tests/fixtures/dq_defs.scala`、93 行）。残っているのは
  パーサが nsc の保つ区別ごと正規化してしまう形（`else` の無い `if`、
  by-name 型、by-name / 可変長パラメータ、手続き構文、パターン定義、自分型、
  early definition）、`..$` と普通の引数の混在、`type` 定義、
  reflect API のコレクション操作（`MemberScope#collect`）、
  `TypeTag` の materialization、そして `reify { … }` です。いずれも
  `unimplemented syntax: quasiquote ... (どの形か)` と**その形を名指しして**
  報告します（黙って通しません）。何が要るかは
  [`docs/macros.md`](docs/macros.md) §7.7 / §7.8 / §7.10 に列挙しました。
  slick の `ShapedValue.mapToImpl` は、scala-reflect.jar を `-cp` に置くと
  エラーが 20 件 → 7 件になりました（`Liftable` / `symbolOf[R]` /
  fresh 名を要する 3 形）
  エラーが 20 件 → 9 件になりました（`Liftable` / `symbolOf[R]` /
  `TypeTag` の materialization）
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
- `Range.Int` / `Range.Long` / `Range.BigInt` / `Range.BigDecimal` の入れ子オブジェクト（`Range$Long$` ほか。`NumericRange` を返す `apply` / `inclusive` を持つ。`Range$` 自体には `Int` 版しか無いことは `javap` で確認済み）
- 期待型が関数型のときの `implicitly`（`implicitly[Int => Ordered[Int]]`）。`adapt_implicit_apply` は期待型が `Type::Function` だと eta 展開のために早期 return するので、implicit 節が埋まらずメソッド型のまま残ります。関数型の implicit **パラメータ**（`def f(implicit ev: A => B)` と view bound）は実装済みで、これは `implicitly` 側の別の穴です
- `List[Option[A]].flatten`（`List(Some(1), None, Some(3)).flatten`）。witness は `scala.Option.option2Iterable[A](xo: Option[A]): Iterable[A]` で、classfile からは正しいシグネチャで読めています（`scala.Option.option2Iterable(Some(1))` は動く）が、classfile に `IMPLICIT` は無く、pickle から読める `PickleSupply::supply_implicit_members` は `scala/` を除外するので、探索から見えません。`scala/Option$` をこちらから `load_binary_into` で読み込んで flag を立てる方法は試しましたが、`Option(5)` 自身が通る経路と競合してモジュールクラスに `apply` が二つ入り、`Option(Option(5))` が `ambiguous overload for apply` になるため入れていません。**現状はサイレントな誤コンパイルではなく診断**（`no implicit: could not find implicit value of type (Option[Int]) => IterableOnce[…]`）です
- `Array[Array[A]].flatten`（`value flatten is not a member of Array[Array[Int]]`）。`ArrayOps.flatten[B](implicit asIterable: A => IterableOnce[B], m: ClassTag[B]): Array[B]` が prelude に無い
- 直接引数の位置にある、未決定型パラメータを持つ implicit 節（`println(xs.flatten)`）。`instantiate_undet_arg` が探索より先に未決定変数を下限（`Nothing`）へ確定させるので、診断が `IterableOnce[Nothing]` を名指しします。`val v = xs.flatten` と書けば正しく解けます

- ソート済み `Map` の `collect` にパターンマッチのリテラルを直接渡す形
  （`treeMap.collect { case (k, v) if … => (k, v) }`）。`K2` が `Any` のままで
  `Ordering[Any]` を探しに行きます。型パラメータが 1 本の `TreeSet.collect { … }`、
  型注釈を付けた `PartialFunction` の値、明示型引数 `collect[K2, V2] { … }` は
  通ります（`agent/mismatch9`）
- `mutable.ArrayBuilder[T]` に `Builder[T, Array[T]]` の基底型が無い
  （`ArrayBuilder.make[E]` を `mutable.Builder[E, Array[E]]` の位置に渡せない）
- `Equiv[Int]`（`agent/ordsummon`）。summon 自体は `Equiv.apply[T]` に解決される
  ようになりましたが、実 ABI の `Ordering[T] extends PartialOrdering[T] extends
  Equiv[T]` を prelude が張っていないので `could not find implicit value of type
  Equiv[Int]` になります（**診断であって誤コンパイルではありません**。実 scalac は
  `Ordering.Int` を渡します）。`Numeric[T] <: Ordering[T]` と同じ形の辺を
  1 本足す話ですが、`Ordering` の implicit スコープが変わるので別スライス扱い
- `Ordering#compare` は prelude では `(Any, Any): Int` のままです。
  `Ordering[String].compare(1, 2)` を real scalac は拒否しますが、こちらは通します
  （`agent/ordsummon` の `os2_summon_bad.scala` はこの行を含めていません）

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
  第 11 スライスで 2 つだけ広げました。(a) `find_or_stub_java_class` が入れた
  **空のプレースホルダ**には、`scala/` の名前でも `prelude_end` より後に
  確保されたものならピクルの型パラメータを付けます（`give_stub_its_kinds`。
  prelude が組んだシンボルは今までどおり触りません）。(b) 同じクラスを指す親が
  **引数だけ違って**すでに載っているときは、ピクルの側で精密化します
  ── classfile の総称シグネチャは `ReusableBuilder<T, Object>` としか書けず、
  `To` は非変なので `ArrayBuilder[E]` が `Builder[E, Array[E]]` になりません。
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

`agent/product` スライス（`case class` / `case object` が `scala.Product` を実装し、合成コンパニオンが `scala.runtime.AbstractFunctionN` を継承するまで）のフィクスチャは接頭辞 `prod`（`prod` / `prod_lib` / `prod_vc` / `prod_bad`）で、同じ理由から `crates/cli/tests/product.rs` に置いています。`prod.scala` は 4 つの上書きアクセサ（`productPrefix` / `productArity` / `productElement` / `productElementName`）と範囲外 3 種（正の外・負の・arity 0）を **私有ランタイムと `--scala-library` の両方**で `java -Xverify:all` の下に走らせ、`real_scalac_dual_run_prod` で real scalac 2.13.16 の stdout とも比較します（`expected/prod.txt` は scalac の出力そのもの。`case class Zero()` と `case object Solo` で **範囲外メッセージが違う**ところまで一致します）。`prod_vc.scala` は値クラスのフィールドが `productElement` でインスタンスに包み直されることを同じ 3 モードで固定します。`prod_lib.scala` は `Product` という**型**、`productIterator` / `productElementNames`、`tupled` / `curried`、`val f: (Int, String) => P = P`、arity 22 を扱うので library dual-run と real scalac dual-run のみで、`fixtures_prod_lib_without_library_is_error` が `--no-scala-library` でそれらが**きちんと診断される**ことを見ます。`prod_lib_classfile_shape` は `javap -p -c` で出した形そのもの（`implements scala.Product,java.io.Serializable`、`tableswitch`、`Statics.ioobe`、`ScalaRunTime$.typedProductIterator`、`Product.productElementNames$`、コンパニオンの `extends AbstractFunction2` と erase された `apply` ブリッジ、`AbstractFunction22`、case object が `AbstractFunctionN` を**継承しない**こと、case object の `productElementName` が `Product.productElementName$` へのフォワーダであること）を固定します。`prod_bad.scala` は real scalac も拒否する 4 つ（case class でないクラスの `productArity` / `productElement`、`productElement("0")`、`val bad: Product = new Plain(1)`）を診断します。

`agent/smallgaps` スライス（`@inline` / `@noinline` の配置、curried case class companion、companion への後方参照、`Option.flatMap` の多相性、`None`/`Some` の `lub`、`Iterable.apply`）のフィクスチャは接頭辞 `sgap`（`sgap` / `sgap_lib`）で、同じ理由から `crates/cli/tests/smallgaps.rs` に置いています。`sgap.scala` は `--no-scala-library` で `check` 済み、`sgap_lib.scala` は `Iterable.apply` が library ABI（`IterableFactory$Delegate.apply` の継承）にしか無いため library dual-run 専用（`fixtures_sgap_lib_without_library_is_error` で `--no-scala-library` が診断のまま残ることも見ています）。

`agent/anonbridge` スライス（`Block` / `If` / `Match` / `Try` の値が消去後に二重に箱詰めされていた件）のフィクスチャは接頭辞 `ab`（`ab` / `ab_bad`）で、同じ理由から `crates/cli/tests/anonbridge.rs` に置いています。`ab.scala` は 8 つのプリミティブすべてのブロック本体、`abstract class` と名前付きクラスの実装、プリミティブ引数、型パラメータ 2 つ、ジェネリックに適用したジェネリック、`val` による実装、SAM 変換したラムダ、`while` / `if` / `match` / `try` 本体、捕捉した `var`、匿名クラス抜きの `val x: Any = { … }` / `id({ … })`、逆向きの開き（`val n: Int = { val z: Any = 1; z.asInstanceOf[Int] }`）を 1 本にまとめてあり、**私有ランタイムと `--scala-library` の両方**で `java -Xverify:all` の下に走らせて `expected/ab.txt`（real scalac 2.13.16 の stdout）と比較します（`real_scalac_dual_run_ab`）。`erased_next_boxes_its_block_exactly_once` と `scalac_and_ours_agree_on_the_erased_entry_point` は `javap -p -c -s` で **`next()Ljava/lang/Object;` の中の箱詰めがちょうど 1 回**であることと、実 scalac が同じ入口を（`next()I` ＋ ブリッジという別の形で）持つことを直接見ます。実行出力だけでは二重箱詰めは見えないので、`javap` 側を別に固定しています。`ab.scala` に `Unit` が入っていないのは、`()` は参照位置に来ないので箱詰めが起きず、代わりに**別件の未修正**（下の Remaining）に当たるためです。`ab_bad.scala` は箱詰めが型の不一致を飲み込まないこと（real scalac と同じ `type mismatch; found: String  required: Int`）を固定します。

`agent/stringops8` スライス（`StringOps` を jar の `ScalaSignature` から補完する経路）のフィクスチャは接頭辞 `so8`（`so8` / `so8_bad`）で、同じ理由から `crates/cli/tests/stringops8.rs` に置いています。`so8.scala` は `StringOps` が library ABI にしか無いため library dual-run 専用で、**期待値は実 scalac 2.13.16 の stdout そのもの**です（`java -Xverify:all` で一致を確認）。`fixtures_so8_without_library_is_error` は `--no-scala-library` で 40 件の診断が出続けること（黙って通さないこと）を、`fixtures_so8_bad_collect_result_type_is_error` は戻り型だけのオーバーロードが「解決はする」だけでは不十分で、`Int` を返す case ブロックの `collect` を `String` に束縛できないことを固定します。
`agent/durrange` スライス（`scala.concurrent.duration` の後置単位、`Range` コンパニオンの `apply` / `inclusive`、関数型の implicit パラメータを implicit def から埋める view 経路）のフィクスチャは接頭辞 `dr`（`dr_duration` / `dr_range` / `dr_view` / `dr_viewuser` / `dr_view_bad`）で、同じ理由から `crates/cli/tests/durrange.rs` に置いています。`dr_duration.scala` は `DurationInt` / `DurationLong` / `DurationDouble` の単位メソッド 20 本すべてと `FiniteDuration` の算術、`dr_range.scala` は `Range$` の `apply` / `inclusive` / `count` 全多重定義（`javap` 上 `Int` 版のみ）、`dr_view.scala` は `Ordered.orderingToOrdered` を eta 展開して渡す経路と view bound を見ます。この 3 本は実ライブラリの jar にしか裏付けが無いので library dual-run 専用で、`expected/*.txt` は real scalac 2.13.16 の stdout そのものです。`fixtures_dr_*_without_library_is_error` が `--no-scala-library` で**きちんと診断される**ことを見ます。`dr_viewuser.scala` は同じ view 経路を利用者が書いた `implicit def`（単相・多相・自分の implicit 節を持つもの・view bound・入れ子の implicit パラメータ）だけで書いたもので、**私有ランタイムと `--scala-library` の両方**で走ります（この経路が jar 依存でないことの確認）。`dr_view_bad.scala` は witness の無い型（`Plain` / `Object`）が両モードで拒否されること（real scalac の `No implicit view available from Plain => Ordered[Plain]` に対応）を固定します。`dr_noimpl_bad.scala` は、**implicit しか取らないメソッドは埋まらなければ型エラー**であること（黙って eta 展開して関数値にしない）を両モードで固定します。

`agent/catsimpl` スライス（ラムダが囲いの `this` を捕まえる、cats の syntax 形の暗黙変換、コンパニオンの implicit スコープ、デフォルト引数を省いた呼び出しの by-name 引数）のフィクスチャは接頭辞 `cats`（`cats_lambda` / `cats_lambda2` / `cats_syntax` / `cats_syntax_bad` / `cats_byname`）で、同じ理由から `crates/cli/tests/catsimpl.rs` に置いています。`cats_lambda.scala` は `List.map` / `flatMap` を使うので library dual-run 専用、`cats_lambda2.scala` は同じ捕捉をライブラリのコレクション抜きで書いてあるので**私有ランタイムと `--scala-library` の両方**で走ります。`cats_syntax.scala` は `implicit def toFlatMapOps[F[_], A](fa: F[A])(implicit F: FlatMap[F])` を自前で書いた 1 ファイル版で、抽象 `F[_]` と具象 `Box` の両方の受け手を通します。`cats_syntax_bad.scala` は、変換のパラメータを「1 引数に適用された任意の型」まで広げたことで**witness の無い型にまで変換が挿さらない**こと（scalac と同じ `value flatMap is not a member of Bag[Int]`）を固定します。`a_higher_kinded_companion_implicit_crosses_a_jar` はライブラリを自分でコンパイルして jar に詰め、`ScalaSignature` だけを通して `Async[Box]` ＝ `Box.asyncForBox` が見つかることと、**witness の無い型は依然として hard error**（`could not find implicit value of type Async[Crate]`）であることを両方見ます。

`agent/catsyntax` スライス（cats の syntax による拡張メソッドが本物の cats に届くまで）のフィクスチャは接頭辞 `csyn`（`csyn_ops` / `csyn_ops_bad`）で、同じ理由から `crates/cli/tests/catsyntax.rs` に置いています。`csyn_ops.scala` は cats の `Ops[F[_], A]` と同じ形の受け手に `map` / `flatMap` / `foreach` を呼ぶもので、**暗黙変換を一切使わずに**（`new Ops[Box, Int](b)`）ラムダの引数型が第 1 型引数の `Box` になっていたずれを固定します。私有ランタイムと `--scala-library` の両方で走ります。`csyn_ops_bad.scala` は、ラムダに宣言どおりの引数型を与えても witness の無い呼び出しは通らないこと（`could not find implicit value of type FlatMap[Bag]`）を固定します。`a_simulacrum_style_syntax_layer_crosses_a_jar` は **実 scalac** で小さな cats（`Ops[F, A] { type TypeClassType = FlatMap[F] }` という refinement 結果型、パッケージオブジェクトの入れ子 `object all`、その `all` を `InnerClasses` に載せるだけの無関係なクラス）をコンパイルして jar に詰め、`ScalaSignature` だけを通して `b.flatMap(…)` と `b >> …` が解決し、`java -Xverify:all` で走ることを見ます。自前の pickle ライタは `REFINEDtpe` を出さないので、この fixture は scalac が書いたものでなければ意味がありません（scalac が無い環境では skip します）。同じテストで、witness の無い `Crate` には変換が挿さらないこと（`value flatMap is not a member of Crate[Int]`）も見ます。

`agent/companionkind` スライス（コンパニオンとクラスが 1 つのシンボルを兼ねていた件）のフィクスチャは接頭辞 `ckind`（`ckind_future` / `ckind_future_bad`）で、同じ理由から `crates/cli/tests/companionkind.rs` に置いています。`ckind_future.scala` は `scala.concurrent.Future`——prelude が持たず、メンバがすべて jar から来るクラス——の**コンパニオンの名前渡しメンバ** `Future.apply` を呼びます。JVM の generic signature は名前渡しを書けないので `Function0[T]` になり、`Future(21)` が `no matching overload for (Function0[T], ExecutionContext)Future[T]` になっていました。`--scala-library` dual-run と **real scalac 2.13.16** との実行結果 diff（`real_scalac_dual_run_ckind_future`）の両方で見ます（`scala.concurrent` は私有ランタイムに無いので `--no-scala-library` では走らせません）。`ckind_future_bad.scala` は、シグネチャが本物になったことで**その implicit 節も本物**になること——`ExecutionContext` がスコープに無ければ scalac と同じく拒む——を固定します。`a_companion_and_its_class_are_separate_symbols` は **実 scalac** で cats を縮めた jar（高階トレイト `Ref[F[_], A]`、そのコンパニオン、`val Ref = tinyeff.Ref` と`type Ref[F[_], A] = tinyeff.Ref[F, A]` を持つパッケージオブジェクト）を作り、`r.update(_ + 1)` の結果型が `F[Unit]`（classfile 由来の素の `F` ではない）になること、コンパニオンの `Ref.const` がトレイト側に紛れ込まずに引けること、そして無い名前 `bogus` はきちんと拒まれることを見ます。
`agent/ambigmap` スライス（同じ pickle 宣言のコピーが 2 つ入って `ambiguous overload for map` になっていた件）のフィクスチャは接頭辞 `am`（`am_pickledup` / `am_pickledup_bad`）で、同じ理由から `crates/cli/tests/ambigmap.rs` に置いています。`am_pickledup.scala` は **3 つのブロックの順番そのものが再現条件**です: 先に `scala.Seq` のレシーバが `map` を聞き、次に `scala.collection.IndexedSeq` のレシーバが聞き、最後に両方を親に持つ `scala.IndexedSeq` が聞きます。`map` だけでなく `flatMap` / `filter` / `partition` / `foldLeft` も同じ 3 レシーバに通すので、直っているのが「`map` の特別扱い」でないことが分かります。`--scala-library` dual-run と **real scalac 2.13.16** との実行結果 diff（`real_scalac_dual_run_am_pickledup`）の両方で `java -Xverify:all` の下に走らせます（載せ替えたシンボルは呼び先の owner とディスクリプタを変えるので、検証器を通すこと自体が確認です）。私有ランタイムには `scala.collection` が無く pickle も無い（＝束ねるコピーが存在しない）ので、`am_pickledup_without_the_library_is_diagnosed` が `--no-scala-library` で**黙って通さずに診断が出る**ことを固定します。`am_pickledup_bad.scala` は、束ねているのが名前ではなく**宣言**であること——本物のオーバーロード 2 本は 2 本のまま残り、決着が付かなければ scalac と同じく拒む——を固定します。

`agent/buildfrom` スライス（変換メソッドの**結果型**が受け手のコレクションに絞られない件）のフィクスチャは接頭辞 `bf`（`bf_curried` / `bf_coll` / `bf_coll_bad`）で、同じ理由から `crates/cli/tests/buildfrom.rs` に置いています。`bf_curried.scala` は**私有ランタイムと `--scala-library` の両方**で `java -Xverify:all` の下に走ります（`scala.Function2` が私有ランタイムに無いので単項関数だけで書いてあります）。`bf_coll.scala` は結果型がすべて実物の `scala.collection` クラスなので jar 限定で、出力は **real scalac 2.13.16 の出力そのもの**（`expected/bf_coll.txt`）と一致します。`bf_coll_without_library_is_error` は私有ランタイムに `MapOps` / `Factory` / `TreeMap` が無いことを**黙って通さずに診断する**ことを固定します。`bf_coll_bad.scala` は narrowing が**通してはいけない 3 つ**——ペアを返さないラムダの `Map.map` は `Iterable`、`to(ArrayBuffer)` は `List` ではない、`groupMapReduce` の値型は第 2 節が返すもの——を、scalac と同じ理由で拒むことを固定します。単体寄りのケースは 9 本あり、特に `bf_plus_minus_on_non_collections_is_untouched`（`+` / `-` は全レシーバがこの経路を通るので、算術と文字列結合が無傷であること）と `bf_user_subclass_does_not_rebuild`（`scala.collection` のクラスでなければ組み直さない）が、直しているのが「症状ごとの特別扱い」でないことの担保です。

`agent/hkinfer` スライス（引数の基底型からの型引数推論と、オーバーロードされた呼び先の自動タプル化）のフィクスチャは接頭辞 `hk`（`hk_base` / `hk_base_lib` / `hk_base_bad` / `hk_tuple` / `hk_tuple_lib` / `hk_tuple_bad`）で、同じ理由から `crates/cli/tests/hkinfer.rs` に置いています。`hk_base.scala` と `hk_tuple.scala` は**私有ランタイムと `--scala-library` の両方**で `java -Xverify:all` の下に走ります。`hk_base_lib.scala`（`Option` / `List`）と `hk_tuple_lib.scala`（`println(1, "a")`）は jar 限定です。異常系は 2 本で、どちらも**両モードで**エラー件数まで固定します: `hk_base_bad` は基底型の型引数が合わないもの、`hk_tuple_bad` はタプル化が通してはいけない 4 つの形（特に逆向きの `g((1, 2))` と、同じ引数個数の候補があるときの `c(1, "x")`）です。詳しくは下の「引数の基底型と自動タプル化」を見てください。

`agent/genrep` スライス（slick が `.fm` テンプレートから生成する 7 本を通すための穴: import を見ないクラス型パラメータ境界、型パラメータ付き `implicit class`、`TupleN extends Product`、継承したオーバーロードの受け手での型、引数リストのタプル化、`Tuple` で始まるだけのクラス名、可変長引数コンストラクタ、ワイルドカード型引数と反変、`package p { … }` の後ろのトップレベル定義）のフィクスチャは接頭辞 `genrep`（`genrep` / `genrep_bound_bad` / `genrep_tuple_bad` / `genrep_product_bad`）で、同じ理由から `crates/cli/tests/genrep.rs` に置いています。`genrep.scala` は `--scala-library` dual-run に加えて real scalac 2.13.16 との実行結果 diff（`real_scalac_dual_run_genrep`）でも見ます。異常系は 3 本: `genrep_bound_bad` は namer が黙るようにした境界でも**存在しない型はきちんと診断される**こと、`genrep_tuple_bad` はタプル化が**間違った呼び出しを通さない**こと、`genrep_product_bad` は `--no-scala-library` で `Product` の辺を張らない（私有ランタイムに裏付けが無い）ことを固定します。

`agent/ctoraccessor` スライス（コンストラクタ引数のアクセサ、`FunctionN.tupled` / `curried` / `Function.untupled`、`Builder` の `+=` / `++=`）のフィクスチャは接頭辞 `ctacc`（`ctacc` / `ctacc_fn` / `ctacc_builder` / `ctacc_plain_bad`）で、同じ理由から `crates/cli/tests/ctoraccessor.rs` に置いています。`ctacc.scala` は**私有ランタイムと `--scala-library` の両方**で `java -Xverify:all` の下に走らせ、`real_scalac_dual_run_ctacc` で real scalac 2.13.16 の出力とも比較します（`expected/ctacc.txt` は scalac の出力そのもの）。`ctacc_case_class_params_get_public_accessors` は `javap -p -s` でアクセサのディスクリプタ（`ConstRep.value()Object` / `NumRep.n()I` / `IntBox.unwrap` の `()I` ＋ `()Object` ブリッジ / `StringBox.label` の `()String` ＋ `()Object` ブリッジ）と、**第 2 引数リストがアクセサにならない**こと（`Multi.extra`）を固定します。`ctacc_fn.scala` と `ctacc_builder.scala` は library ABI 限定（`scala/FunctionN` の default メソッド、`scala/Function$`、`Growable`）なので library dual-run と real scalac dual-run のみで、`fixtures_ctacc_fn_without_library_is_error` / `fixtures_ctacc_builder_without_library_is_error` が `--no-scala-library` で**きちんと診断される**ことを見ます。`ctacc_plain_bad.scala` は `val` の無いコンストラクタ引数が外から読めないままであること（case class の第 1 引数リストだけがアクセサになる）を固定します。
オーバーロード集合が別のクラスの読み込みで消える回帰のフィクスチャは接頭辞 `oshadow`（`oshadow` / `oshadow_java_first` / `oshadow_java_last` / `oshadow_bad`）で、同じ理由から `crates/cli/tests/overloadshadow.rs` に置いています。`oshadow.scala` は `--scala-library` dual-run に加えて real scalac 2.13.16 の実行結果とも直接比較します（`oshadow_matches_scalac`）。`oshadow_java_first.scala` と `oshadow_java_last.scala` は `java.math.BigDecimal` の位置だけを入れ替えた同じプログラムで、`oshadow_order_independent` が両方通ることと stdout が一致することを固定します。`oshadow_bad.scala` は `BigDecimal(Some(1))`（real scalac も拒否）が `no matching overload` になり、しかも**候補一覧が丸ごと**出る（`(String)BigDecimal` を含む）ことを見ます。`oshadow_without_library_is_error` は `--no-scala-library` で `not found: value BigDecimal` の診断が残ることを見ます。
`agent/parentimpl` スライス（親コンストラクタの implicit 節・デフォルト引数の補完）のフィクスチャは接頭辞 `pimpl`（`pimpl` / `pimpl_bad`）で、同じ理由から `crates/cli/tests/parentimpl.rs` に置いています。`pimpl.scala` は slick の `ConstColumn` 形（`class ConstColumn[T : TT] extends TypedRep[T]`）、明示節＋2 引数の implicit 節、全部デフォルト／末尾だけデフォルト、デフォルト節＋implicit 節、匿名クラスの親、引数無しの `new` を 1 本にまとめ、**私有ランタイムと `--scala-library` の両方**で `java -Xverify:all` の下に走らせます。`real_scalac_dual_run_pimpl` は real scalac 2.13.16 でも同じソースを走らせて stdout が一致することを見ます（`expected/pimpl.txt` は scalac の出力そのもの）。`pimpl_late_a.scala` / `pimpl_late_z.scala` は**子を親より先にコンパイル**して、親の context bound の evidence がシグネチャパス時点で未生成でも埋まる（＝ファイル順に依存しない）ことを見ます。`pimpl_bad.scala` は witness の無い親 implicit 節が**黙って通らない**ことを固定し、`pimpl_bad_reports_the_extends_clause_once` で診断が `extends` の行に 1 件だけ出る（3 パス分に増えない）ことも見ています。

`agent/integral` スライス（`Integral` / `Fractional` を `Numeric` の型クラス階層に入れる）のフィクスチャは接頭辞 `ig`（`ig_hier` / `ig_hier_bad`）で、同じ理由から `crates/cli/tests/integral.rs` に置いています。`ig_hier.scala` は `List.range` / `Vector.range` / `Seq.range`、`implicitly[…]` 13 件の**選ばれたインスタンスのクラス名**、`quot` / `rem` / `div`、`Numeric[T]` を implicit に取るユーザーコード、`sum` / `product` / `sorted` / `max` / `min` / `sortBy`、`Integral[Int]` → `Numeric[Int]` / `Ordering[Int]` の widening、`Ordering[Option[Int]]` を 1 本にまとめてあり、library dual-run と **real scalac 2.13.16** との実行結果 diff（`ig_hier_matches_real_scalac`）の両方で `java -Xverify:all` の下に走らせます。クラス名を出力しているので「一意になった」ではなく「**実 scalac と同じインスタンスを選んでいる**」ことが見えます。`ambiguity_did_not_increase` は `Ordering[Int/Double/Long/Byte/Short/Char/Float]` と `sum` / `product` / `sorted` / `max` / `min` / タプルの `sorted` に `ambiguous` が 1 件も出ないことを固定します（`Numeric[T] extends Ordering[T]` なので、ここが今回いちばん壊れやすい所でした）。`ig_hier_bad.scala` は階層がゴム印にならないこと——`Numeric[Int]` → `Integral[Int]` と `Ordering[Int]` → `Numeric[Int]` の逆流、実在しない `Integral[Double]` / `Fractional[Int]` / `Integral[String]`——を固定します（real scalac も同じ 6 行で 6 件出します）。私有ランタイムには `scala/math/Integral` が無いので、`range_is_diagnosed_without_the_jar` が `--no-scala-library` で `not found: type Integral` / `range is not a member of List$` と**きちんと診断される**ことを見ます。

`agent/ordsummon` スライス（`Ordering` コンパニオンの項位置と summon `Ordering[T]`）のフィクスチャは接頭辞 `os2`（`os2_summon` / `os2_summon_bad`）で、同じ理由から `crates/cli/tests/ordsummon.rs` に置いています。`os2_summon.scala` は `Ordering.Int.reverse` / `Ordering[String]` / `Ordering[Int].reverse` / `Ordering.String.reverse` / `implicitly[Ordering[Int]].reverse` / `List(…).sorted(Ordering[String].reverse)` / `Ordering.by[(String, Int), Int]` / `Numeric[Int]` / `Numeric.IntIsIntegral` / `Integral[Int]` / `Fractional[Double]` / `BigInt` の乗算／選ばれたインスタンスのクラス名（`scala.math.Ordering$Int$`）／`List(Some(2), None, Some(1)).sorted` を 1 本にまとめ、library dual-run と **real scalac 2.13.16** との実行結果 diff（`os2_summon_matches_real_scalac`）の両方で `java -Xverify:all` の下に走らせます。`ClassCastException` は**型検査を通ったあとに**出ていたので、コンパイルが通ることだけでは足りません。`the_three_reported_forms_run` が報告された 3 形をそのまま実行し、`integral_and_fractional_summon` は `val i: Integral[Int] = Integral[Int]` が黙って通って実行時に落ちていた形（`59d967a` では型エラー）を固定します。`option_ordering_is_still_derived_but_is_not_a_view` は `Ordering.Option` が導出規則としては効き続け、view としては効かない（`val o: Ordering[Option[Int]] = Ordering.Int` は `type mismatch`）ことを両方見ます。`module_apply_redirect_still_works` は `List[Int](1, 2)` / `Vector[String]` / `Option[Int]` / `Map[String, Int]` の既存のファクトリが `ambiguous overload` にならないことを固定します（pickle からの `apply` 供給をここで足したので、いちばん壊れやすい所でした）。`alias_module_keeps_the_pickled_overloads` は `BigDecimal(3L)` / `BigDecimal(BigInt(6))` / `BigInt("7")`——**このスライスが一度 revert された回帰**（別名が module に解決されると `widen_with_companion` の経路を通らず、prelude 手書きの 3 本しか候補に残らない）——を固定します（`oshadow` が同じプログラムを端から端まで見ますが、こちらは別名の経路そのものを見ます）。`os2_summon_bad.scala` はコンパニオンを項に出せるようにしたことが「なんでも通る」ことにならない 5 行——`val a: Ordering[Int] = Ordering` / `val b: Ordering[Option[Int]] = Ordering.Int` / `Ordering.Foo` / `Numeric.Int` / `Ordering[Object]`——で、real scalac も同じ 5 行で 5 件出します。`summon_is_diagnosed_without_the_jar` は `--no-scala-library` で `not found: value Ordering` の診断が残ることを見ます。

`agent/traitextends` スライス（trait がクラスを継承する、`abstract override` / stackable trait）の
フィクスチャは接頭辞 `trex`（`trex_stack` / `trex_inherit` / `trex_mixin_bad` /
`trex_ungrounded_bad` / `trex_object_bad` / `trex_ctorargs_bad` / `trex_absover_class_bad` /
`trex_ownimpl_bad`）で、
同じ理由から `crates/cli/tests/traitextends.rs` に置いています。`trex_stack.scala` は
コンストラクタ引数を取るクラスを継承した trait、`abstract override` の連鎖、線形化順で結果が
変わる 2 通り（`LOUD-please-woof` / `please-LOUD-woof`）、trait 本体からの継承メンバ参照を 1 本にまとめ、
**私有ランタイムと `--scala-library` の両方**で `java -Xverify:all` の下に走らせます。
`expected/trex_stack.txt` と `expected/trex_inherit.txt` は **real scalac 2.13.16 の出力そのもの**です。
バイトコード側の不変条件も 3 本で固定しています。`trex_super_accessor_shape` は匿名クラスの
`Loud$$super$speak` が `invokespecial Main$Dog.speak` になること（scalac の `Main$$anon$1` と同じ形）、
`trex_inherited_superclass_reaches_the_class_file` は `class X extends Loud` が classfile でも
`Main$Animal` を継承すること、`trex_trait_interface_does_not_extend_its_superclass` は trait の
interface がスーパークラスを継承せず、`T$class` 本体が継承メンバを読む前に `checkcast` を出すことを見ます。
異常系 6 本はすべて**両モードで**（`--no-scala-library` と `--scala-library`）診断されることを
確認しており、文面は real scalac 2.13.16 のものです。`trex_mixin_bad` は名前付きクラスと匿名クラスの
両方で拒否され、しかも**テンプレート 1 つにつき 1 件**（ヘッダパスとの二重報告なし）であることも見ます。

`agent/localconv` スライス（メソッド本体 / ブロック / ラムダ本体に書いたローカルの
`implicit val` / `implicit def` / `implicit class` が view 探索から見えていなかった件。
「実装している言語サブセット」の「ローカルスコープの implicit 変換（view）」節を参照）の
フィクスチャは接頭辞 `lc`（`lc_param` / `lc_class` / `lc_conv` / `lc_shadow` / `lc_capture` /
`lc_outofscope_bad` / `lc_ambiguous_bad`）で、同じ理由から `crates/cli/tests/localconv.rs` に
置いています。`lc_param.scala` はローカルの `implicit val` がネストした `def` の implicit
パラメータを埋める、直していない対照群（修正前から動いていた経路）です。`lc_class.scala` は
ローカルの `implicit class` がメソッド本体・入れ子の `def`・ラムダ本体の 3 か所すべてから
拡張メソッドとして見つかること、`lc_conv.scala` はローカルの `implicit def` が代入の型強制と、
別に宣言したローカルクラスへの拡張メソッド源の両方に効くことを見ます。`lc_shadow.scala` は
外側の `implicit def i2s` と同名のローカル `implicit def i2s` が**シャドー**すること（scalac
と同じ `inner5`。曖昧にならないこと）、`lc_capture.scala` はローカルの `implicit class` が
別のローカル（`factor`）を捕捉する形で、合成された変換メソッドという別のネストしたローカル
`def` を経由して `new` される、という `lambda_lift` の自由変数解析の独立したバグを踏むケース
です。すべて**私有ランタイムと `--scala-library` の両方**で `java -Xverify:all` の下に走らせ、
`expected/*.txt` は real scalac 2.13.16 の stdout そのものです。`lc_outofscope_bad.scala` は
兄弟メソッドに書いた `implicit class` が見えないこと（`value dbl is not a member of 3`）、
`lc_ambiguous_bad.scala` は同じ特定度のローカル `implicit def` が 2 つあると scalac と同じ
`ambiguous implicit` になることを、両方とも両モードで固定します。
`agent/parentcheck` スライス（解決できない親クラス／トレイト・自分型・`new` を診断する）の
フィクスチャは接頭辞 `pc`（`pc_parents` / `pc_extends_bad` / `pc_selfnew_bad` /
`pc_qualified_bad`）で、同じ理由から `crates/cli/tests/parentcheck.rs` に置いています。
`pc_parents.scala` は引数付きの親・ジェネリックな親・`with` 混入・自分型・匿名クラス・
修飾付きの親・型エイリアス経由の親を 1 本にまとめた**正常系**で、**私有ランタイムと
`--scala-library` の両方**で `java -Xverify:all` の下に走らせます（`expected/pc_parents.txt`
は real scalac 2.13.16 の stdout そのもの）。規則が広すぎればここが落ちる、という受け皿です。
異常系 3 本は**両モードで**拒否されることに加え、拒否したときに classfile を 1 つも
書かないことも見ます。文面は実 scalac 2.13.16 のもので、`pc_extends_bad` は
`extends` の頭・`with` の項・適用された親の頭・その型引数の 4 形（scalac と同じ 6 件）、
`pc_selfnew_bad` は自分型・`new Missing` / `new Missing {}` / `new Obj`、
`pc_qualified_bad` は修飾付きの 6 形（`is not a member of object …` /
`… of package …` / `not found: value …` / `object … is not a member of package …`）です。
`pc_new_of_a_missing_type_is_not_a_missing_value` は `new Missing` が
`not found: value`（間違った名前空間）に戻らないことを固定します。

`agent/setapply` スライス（コンパニオンの `apply` が、手書きの prelude と jar 由来の pickle とで二重に載っていた件）のフィクスチャは接頭辞 `sa`（`sa_setapply` / `sa_setapply_bad`）で、同じ理由から `crates/cli/tests/setapply.rs` に置いています。`sa_setapply.scala` は `Repo` trait の `xs(tag)`（`SetOps.apply(String): Boolean` をメンバ経由で強制的に完了させる、元の報告と同じ形）→ `Set(...)` の順、逆順、`Map` / `List` / `Seq` の同型ケースを 1 本にまとめ、**`--scala-library` dual-run** と **real scalac 2.13.16** との実行結果 diff（`real_scalac_dual_run_sa_setapply`）の両方で `java -Xverify:all` の下に走らせます（載せ替えたシンボルはリンク先を変えるので、検証器を通すこと自体が確認です）。私有ランタイムには `scala.collection` の pickle が無い（＝二重に載る余地が無い）ので、`sa_setapply_without_the_library_is_diagnosed` が `--no-scala-library` で `Set` が**黙って通らず** `not found: type Set` と診断されることを固定します。`sa_setapply_bad.scala` は、直したのが「名前」ではなく「erased パラメータの形」であること——共通の親を持たない 2 つの実在するオーバーロード（`Ax` / `Bx` を実装する `Cx` への `Pick.apply`）は 2 つのまま残り、決着が付かなければ scalac と同じく拒む——を固定します。1 回目の版（見つからなかった候補を `None` で握りつぶす形）はマージ後の全体検証で `agent/oshadow`（`BigDecimal(2)` が `ambiguous overload`）と `agent/uniteq`（`scala.Enumeration` のメンバ欠落）を壊し、2 回目の版（同じ検査だが、握りつぶす代わりに既存の prelude シンボルをそのまま返す）で両方直っています。詳しくは下の該当節を参照してください。

### オーバーライドの適合検査（`agent/override`）

SLS 5.1.4 "Overriding"（nsc の `RefChecks.checkOverride`）と SLS 5.2.6
（"needs to be abstract"）が**丸ごと無く**、次の 2 つが黙って通っていました。

```scala
trait It[A] { def next(): A }
val i = new It[Int] { def next(): String = "x" }
println(i.next())          // 型検査を通り、呼ぶ側の unbox で ClassCastException

abstract class B { def f: Int }
class D extends B          // 型検査を通り、呼んだ瞬間 AbstractMethodError
```

規則そのものは `crates/typer/src/override_check.rs` に**シンボル表の純関数**として
置き（`traitparent.rs` と同じ形）、`check.rs` は `type_class` / `type_module` の
末尾に呼び出しを 2 行足すだけです。診断文面はすべて**実 scalac 2.13.16 を同じ
ソースに掛けて採取**しました（`javap` ではありません）。オーバーライドされた側は
**オーバーライド地点から見た形**でエコーします（`trait It[A]` の `def next(): A` は
`new It[Int]` の下では `def next(): Int`）。

| 規則 | scalac の文面 | 実装前 | 実装後 |
| --- | --- | --- | --- |
| 1. 結果型は共変 | `incompatible type in overriding` | 無 | 有 |
| 2. パラメータ型は不変（違えばオーバーロード） | `method f overrides nothing.` + `Note: …` | 無 | 有 |
| 3. `override` 修飾子の要否 | `` `override` modifier required to override concrete member: `` / `method h overrides nothing` | 無 | 有 |
| 4. deferred な再宣言は下の具象実装を打ち消す | `class C needs to be abstract.` + `No implementation found in a subclass for deferred declaration` | 無 | 有 |
| 5. `final` はオーバーライド不可 | `cannot override final member:` | 無 | 有（ソース由来のみ。下の残件） |
| 6. 可視性は広げる方向のみ | `weaker access privileges in overriding` | 無 | 有 |
| 7. `val` は `def` を覆える／逆は不可、具象 `var` は覆えない | `stable, immutable value required to override:` / `mutable variable cannot be overridden:` | 無 | 有 |
| 8. 型パラメータの個数と境界 | `overrides nothing.` / `incompatible type in overriding` | 無 | 有 |
| 9. 抽象メンバの未実装 | `class X needs to be abstract.` / `object creation impossible.` | 無 | 有（ソース／Java classfile 由来のみ。下の残件） |

**受け入れすぎを直すスライスなので、誤診断こそが最大のリスク**です。slick 184 ファイルは
**346 エラー / 64 ファイル**（着手前）→ **346 / 64**（実装後）で、**診断の多重集合が
1 件も変わりません**。途中の 502 まで増えた版から潰した誤診断の原因は 5 つで、
どれも「規則」ではなく**シンボル表の近似**が原因でした。

1. **prelude のメンバのフラグは信用できない**。`prelude::method` は**全メンバに
   `Flags::FINAL`** を立てるので `override def toString` が「final を覆っている」に
   なり、逆に「deferred」を表す手段が無いので本当は抽象な `Product.productArity` が
   具象に見えて、手書きの `Product` すべてに `override` を要求していました。pickle
   から来たメンバも `Flags::EMPTY` で作られるので同じです。`modifiers_are_known`
   （`prelude_end` より後 かつ `pickled_origin` が空）でゲートしました。
2. **信用できない型の比較はしない**。`ClassTag[C[Any]]` と `ClassTag[_]`、
   `Builder[E, C[E]]`、`BasicBackend.Session` — 型パラメータ・抽象型メンバ・未簡約の
   適用・ワイルドカードが絡む比較は scala-rs の近似なので、`robust` でない型は
   「一致する」として黙ります（150 件の誤診断がこれでした）。
3. **結果型を書いていないメンバは検査しない**。nsc の namer は結果型が無いメンバを
   **オーバーライド先の結果型を期待型として**型付けるので、推論された結果型が適合し
   ないことは原理的にありません。こちらの推論は近似で、slick の
   `override def toString = { … }` は `Any` になります。
4. **素のコンストラクタ引数はメンバではない**（nsc の
   `OverridingPairs.Cursor.exclude`）。slick の
   `class JdbcFunction(name: String) extends FunctionSymbol(name)` は `private[this]`
   なので何も覆いません。書かれた `private[this] val` の方は scalac 同様に診断します。
5. **フィールド初期化子のラムダ引数がクラスの member リストに入る**。
   `protected lazy val pkNames = pkSyms.map { fs => … }` の `fs` は所有者がクラスに
   なるので、基底クラスの `fs` と衝突して `override` を要求していました。
   `ctor_fields` に居る `PARAM` だけをメンバとして数えます。

フィクスチャは接頭辞 `ov` で、コンフリクト回避のため `crates/cli/tests/override.rs` に
置いています。正常系 `ov_ok.scala` は**合法なオーバーライドの形を全部**（共変な結果型、
オーバーロード、deferred の実装、`final` の兄弟、可視性の拡大、`val` による `def` の
オーバーライド、`val` コンストラクタ引数による抽象 `val` の実装、素の引数による遮蔽、
境界の緩和、匿名クラスでの具体化、`def f` と `def f()` の相互一致、`toString` の
オーバーライド）1 本にまとめ、**私有ランタイムと `--scala-library` の両方**で
`java -Xverify:all` の下に走らせます。`expected/ov_ok.txt` は **real scalac 2.13.16 の
出力そのもの**です。異常系 14 本は 1〜9 を 1 本ずつ潰しており、`rejected_once` が
**両モードで拒否されること**に加えて**エラーが実 scalac と同じくちょうど 1 件**である
ことを固定します（カスケードも、片方のモードだけの診断も、検査の抜けと同じく不合格です）。

あわせて `agent/anonbridge` の残件をもう 1 つ直しました。**消去で `Object` を返すように
なったメンバの結果を捨てるとき `pop` が出ない**問題です。`trait Box[A] { def get: A }` の
`get` は**引数リストを持たない `def`** なので呼び出しが `Apply` を持たない裸の `Select`
になり、`unit_stat_leaves_ref` の `Apply` の腕に当たりませんでした。直線コードでは
verifier が黙っているので気づかれず、あとに stackmap frame を要する分岐（`try` / `while`）が
来た瞬間 `VerifyError: Inconsistent stackmap frames` になります。`ov_unitpop.scala` が
`Apply` 形・裸の `Select` 形・型パラメータのまま呼ぶ形・`while` の後端の 4 つを
両モードで実行し、`ov_nilary_unit_select_is_popped` が `javap -c` で `pop` が
本当に出ていることを見ます。

既存フィクスチャ 2 本（`java_override.scala` / `implicit_specific.scala`）に `override`
修飾子を足しました。どちらも**実 scalac 2.13.16 がそのままでは拒否する**ソースで
（`java_override` は Java の `@Override` **アノテーション**が Scala の `override`
**修飾子**の代わりにならないため、`implicit_specific` は `pick()` が typer で止まって
RefChecks まで届かないため scalac が言わずに済んでいただけ）、検査が無かったから
通っていたものです。テストの意図（`@Override` の受理／implicit の specificity）は
変えていません。

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

`agent/localtrait` スライス（メソッド本体の中の `trait` / `class` / `object`）の
フィクスチャは接頭辞 `lt`（`lt1` / `lt2` / `lt3` / `lt4` / `lt1_bad`）で、
コンフリクト回避のため `crates/cli/tests/localtrait.rs` に置いています。正常系は
私有ランタイムと `--scala-library` の両方で走らせ、`expected/lt*.txt` は**実 scalac
2.13.16 の出力そのもの**です。これも**型検査は通って実行時に落ちていた**（あるいは
黙って別のクラスを上書きしていた）バグなので、`javap` でバイトコードの形も 4 本の
テストで固定します。そのうち `implementing_class_members_match_scalac` は
`/tmp/scala-2.13.16/bin/scalac` があるとき実 scalac を走らせ、実装クラスの public
メソッド集合が nsc のそれを包含することを見ます（**誰も呼ばないフォワーダが欠けて
いても stdout は一致してしまう**ため）。詳しくは「メソッド本体の中の宣言
（ローカル trait / class / object）」の節を参照。
`agent/conspat` スライス（`::` の中に入れ子の抽出子がある形、`unapply` の呼び出し口、
尽きた `match` の `MatchError`）のフィクスチャは接頭辞 `cp`
（`cp_cons` / `cp_infix` / `cp_seq` / `cp_err` / `cp_cons_bad`）で、コンフリクト回避のため
`crates/cli/tests/conspat.rs` に置いています。これも**型検査は通って実行時に落ちていた**
バグなので、テストは 3 通り（私有ランタイム / `--scala-library` / 実 scalac 2.13.16）走らせて
stdout が全部一致することを見ます。`expected/cp_*.txt` は実 scalac の出力そのものです。
`cp_cons.scala` は `case P(v) :: t`、深さ 2（`case P(a) :: P(b) :: _`）、`::` の右の抽出子
（`case _ :: P(v) :: _`）、タプル（`case (a, b) :: _`）、ガード付き、`@` 束縛
（`case (p @ P(v)) :: _`）、型パターン（`case (p: P) :: _`）、`case Some(P(v))`、
`Option[Any]` に対する定数パターン、`case P(v) :: Nil` / `case Some(Nil)` の安定識別子、
それに従来から動いていた `case h :: t` を 1 本にまとめてあります。`cp_infix.scala` は
ユーザー定義の中置抽出子（`object ~`）と、抽出子の引数型がスクルーティニより狭い形
（`Option[Any]` に対する `case Some(Two(a, b))`）です。この 2 本は**私有ランタイムでも**
走ります。`cp_seq.scala` は `case List(P(a), Q)` / `case Seq(P(a), _*)` と `Tuple3` を返す
`unapply` で、`Seq` / `List` の抽出子パターンと `Tuple3` が jar にしか無いので library
dual-run 専用です。`cp_err.scala` は尽きた `match` が `scala.MatchError` になること
（クラス名とメッセージの両方）を両モードで見ます。`cp_cons_bad.scala` は
`case P(a, b)`（アリティ違い）と `case Nope(a) :: _`（無い抽出子）が診断されることを固定します。

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
| `ov_ok.scala`（`crates/cli/tests/override.rs`、私有ランタイム・library dual-run） | `agent/override` スライス: 合法なオーバーライドの形すべて（共変な結果型 / オーバーロード / deferred の実装 / `final` の兄弟 / 可視性の拡大 / `val` による `def` のオーバーライド / `val` 引数による抽象 `val` の実装 / 素の引数による遮蔽 / 境界の緩和 / 匿名クラスでの具体化 / `def f` と `def f()` / `toString`） | `narrow` `int 1/str x` `implemented` `derived` `grounded` `final` `public` `2` `given` `bare` `boundless` `42` `7` `talks` `talks2` |
| `ov_unitpop.scala`（同上） | 消去で `Object` を返すメンバの結果を捨てるときの `pop`（`Apply` 形 / 裸の `Select` 形 / 型パラメータのまま / `while` の後端） | `a` `t1` `b` `t2` `p` `t3` `l` `l` `t4` |
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
| `dr_duration.scala`（`crates/cli/tests/durrange.rs`、library dual-run のみ） | `agent/durrange` スライス: `scala.concurrent.duration` の後置単位。`DurationInt` / `DurationLong` / `DurationDouble` の 20 本の単位メソッド（`nanoseconds` / `nanos` / `nanosecond` / `nano` … `days` / `day`）、`FiniteDuration` と `Duration` の相互運用、`+` / `-`、`Duration(5, SECONDS)` / `Duration.Inf` | `1 nanosecond …` `1500 milliseconds` `5 seconds Duration.Inf` |
| `dr_range.scala`（`crates/cli/tests/durrange.rs`、library dual-run のみ） | `Range` コンパニオン: `Range(0, 5)` / `Range(0, 10, 2)` / `Range.inclusive(1, 3)` / `Range.inclusive(1, 9, 3)` / `Range.count`（`javap` 上 `Range$` にあるのは `Int` 版だけ）と、既に動いていた `1 until 10 by 3` の回帰 | `List(0, 1, 2, 3, 4)` … `4 2,3,4,5` |
| `dr_view.scala`（`crates/cli/tests/durrange.rs`、library dual-run のみ） | 関数型の implicit パラメータ `implicit ev: A => Ordered[A]` を `Ordered.orderingToOrdered` の eta 展開で埋める。view bound `A <% Ordered[A]`、入れ子の implicit パラメータ、素の `val o: Ordered[Int] = 3` も同じ探索 | `5` `b` `2.5` … `true` `false` |
| `dr_viewuser.scala`（`crates/cli/tests/durrange.rs`、私有ランタイム・library dual-run） | 同じ view 経路を利用者の `implicit def` だけで。単相な変換、自分の implicit 節を持つ多相な変換、view bound、implicit パラメータを内側の呼び出しへ渡し直す形、**未決定型パラメータを view の結果型から解く**形（`def unwrap[A, B](a: A)(implicit view: A => Wrap[B]): B`） | `<i7>` `<shi>` `7` `hi` `<szz>` `<i1>\|<i2>` `<sa>\|<sb>` `2 w9` |
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
| `am_pickledup.scala`（`crates/cli/tests/ambigmap.rs`、library dual-run・real scalac dual-run） | `agent/ambigmap` スライス: `IterableOps.map` のコピーが `immutable.Seq` と `collection.IndexedSeq` の両方に載り、両方を親に持つ `scala.IndexedSeq` で `ambiguous overload for map` になっていた（`flatMap` / `filter` / `partition` / `foldLeft` も同型） | `2,3,4` `1,1,2,2,3,3` `2,3` `6` `8,10` `4,-4,5,-5` `5` `9` `16,17` `6\|7` `6,600,7,700` `7` `113` `6 / 7` |
| `am_pickledup_bad.scala`（`crates/cli/tests/ambigmap.rs`、異常系） | 束ねるのは名前ではなく pickle の**宣言**: 本物のオーバーロード 2 本は 2 本のまま残り、決着が付かなければ scalac と同じく拒む | （コンパイルエラー） |
| `bf_curried.scala`（`crates/cli/tests/buildfrom.rs`、私有ランタイム・library dual-run） | `agent/buildfrom` スライス: 3 つのパラメータリストを持つメソッドで、各節は**その節の宣言型**に対して型を解く（`groupMapReduce` の第 3 節が `Any` になっていた根） | `20` `1!` `yes\|1\|4` `20` |
| `bf_coll.scala`（`crates/cli/tests/buildfrom.rs`、library dual-run） | `agent/buildfrom` スライス: `Map.map` / `flatMap` / `collect` / `filterNot` / `++` / `take` / `partition` / `groupBy`、`groupMapReduce` / `groupMap`、`TreeMap` の `-` / `+` / `updated`、`Set ++ List`、`IndexedSeq` の `flatMap` / `zip` / `partition` / `groupMap`、`to(ArrayBuffer)` / `to(List)` / `to(Map)`、`implicitly[Factory[…]]`、`mutable.Map - k`。出力は real scalac 2.13.16 のものと一致 | `expected/bf_coll.txt` |
| `bf_coll_bad.scala`（`crates/cli/tests/buildfrom.rs`、異常系） | narrowing が通してはいけない 3 つ: ペアを返さない `Map.map` は `Iterable`、`to(ArrayBuffer)` は `List` でない、`groupMapReduce` の値型は第 2 節が返すもの。実 scalac も同じ 3 件 | （コンパイルエラー 3 件） |
| `ckind_future.scala`（`crates/cli/tests/companionkind.rs`、library dual-run・real scalac dual-run） | `agent/companionkind` スライス: prelude が持たない `scala.*` のメンバが classfile からしか来ず、コンパニオンの名前渡し引数（`Future.apply` の `=> T`）が `Function0[T]` になっていた | `21` `20` |
| `ckind_future_bad.scala`（`crates/cli/tests/companionkind.rs`、異常系） | pickle から読んだシグネチャは implicit 節も本物: `ExecutionContext` が無ければ `Future(21)` は通らない | （コンパイルエラー） |
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
| `so8.scala` | `StringOps` を pickle から補完する経路（`agent/stringops8`）：`zipWithIndex` / `zip` / `scanLeft`（`wrapString` 経由）、`sliding` / `groupBy` / `sortBy` / `sortWith` / `distinctBy` / `collect`×2 / `partition` / `span` / `splitAt` / `tails` / `inits` / `permutations` / `combinations` / `indexWhere` / `lastIndexWhere` / `fold` / `prepended` / `appended` / `:++` / `++:` / `linesWithSeparators` / `view` / `apply` / `s(i)` / `withFilter` / `addString`×3（library dual-run のみ。期待出力は実 scalac 2.13.16 の stdout そのまま） | `Vector((a,0), …)` … `[a-b-c-d-e-f]` |
| `so8_bad.scala` | 戻り型だけのオーバーロードが**区別**できること：`Int` を返す case ブロックの `collect` は `IndexedSeq[B]` を選ぶので `String` に束縛できない（異常系。scalac も拒否する） | `type mismatch` |
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

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、複合型に無いメンバは `compound_bad.scala`、テンプレートへの二つ目のクラスは `mism7_mixin_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。`Either` に無いメンバーは `either_ops_bad.scala`、`Option` に無いメンバーは `option_x1_bad.scala`、`Option.toRight` の結果の `Either` に無いメンバーは `option_x2_bad.scala`、`Try` に無いメンバーは `try_ops_bad.scala`、`Throwable` に無いメンバーは `try_exceptions_bad.scala`。`Try.recover` に `PartialFunction` でない全域関数リテラルを渡すのは `try_recover_bad.scala`（nsc どおり `required: PartialFunction`）。`either_ops.scala` / `option_x2.scala` / `try_ops.scala` は `--no-scala-library` では診断になることも見ています（私有ランタイムに `Either` / `Try` は無い）。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` / `@noinline` は実 scalac 2.13.16 と同じくどの定義（val / var / class / type ...）にも、両方同時にも、警告なしで付けられる（`crates/cli/tests/smallgaps.rs` の `fixtures_sgap`。`inline_bad.scala` は削除）。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。パッケージ境界の外からの `private[p]` と、継承元の無いコンストラクタ引数は `mism8_access_bad.scala`。依存メソッド型が読み替えても型検査は残ることは `mism8_dep_bad.scala`。`-Xsource:3` 無しの `f(xs*)` は `mism8_star_bad.scala`。 高階の適用を期待型から解いても型検査が消えないこと（`F.pure(i): F[String]`、`String => F[Int]` を `Int => F[Int]` に、`copy(name = 3)`）は `mism9_bad.scala` で固定しています。実 scalac 2.13.16 も 3 件すべて拒否します。シグネチャパスの診断を捨てても親の実引数の型検査が残ること、およびクラスの型パラメータを束縛しても既定引数の型検査が残ることは `mism10_bad.scala` で固定しています（実 scalac 2.13.16 も 2 件とも拒否します）。
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

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、複合型に無いメンバは `compound_bad.scala`、テンプレートへの二つ目のクラスは `mism7_mixin_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。`List` に無いメンバーは `list_core1_bad.scala`。私有ランタイムに裏付けの無い `List.sorted` を `--no-scala-library` で使うのは `list_core2_bad.scala`（`value sorted is not a member of List[Int]`）。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` / `@noinline` は実 scalac 2.13.16 と同じくどの定義（val / var / class / type ...）にも、両方同時にも、警告なしで付けられる（`crates/cli/tests/smallgaps.rs` の `fixtures_sgap`。`inline_bad.scala` は削除）。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。
| `text_string1.scala` | `java.lang.String` の素のメソッド `trim` / `substring` / `lastIndexOf` / `replace` / `contains` / `equalsIgnoreCase` / `matches` / `concat` / `strip` / `repeat` / `compareTo`（library dual-run のみ。`--no-scala-library` でも動く） | `Hello World` `cdef` `bc` `1` `4` `zbc` `hello there` `true` `false` `true` `true` `abcdef` `x` `ababab` `-1` |
| `text_stringbuilder1.scala` | bare `StringBuilder`（`scala.StringBuilder` エイリアス）の `append` 各オーバーロード / `+=` / `++=` / `insert` / `deleteCharAt` / `setLength` / `reverse` / `clear` / `isEmpty` / `nonEmpty` / `result` / `charAt`（library dual-run のみ） | `hello 42!` `9` `false` `>>hello 42!` `>hello 42!` `>he` `eh>` `true` `abc` `true` `b` |
| `text_range1.scala` | `Range` の `withFilter`（for 内包表記の guard）/ `foldLeft` / `foldRight` / `sum` / `product` / `min` / `max` / `toList` / `toVector` / `filter` / `filterNot` / `map` / `flatMap` / `reverse` / `contains` / `exists` / `forall` / `count` / `take` / `drop` / `takeWhile` / `dropWhile` / `zipWithIndex` / `by`（library dual-run のみ） | `3` `4` `5` `Vector(20, 40)` `15` `15` `15` `120` `5` `1` `List(1, 2, 3, 4, 5)` … |
| `text_math1.scala` | `RichInt`/`RichLong`/`RichDouble`/`RichChar` の `toBinaryString` / `toHexString` / `toOctalString` / `sign` / `isNaN` / `round` / `floor` / `ceil` と `scala.math.{abs,max,min,pow,sqrt,floor,ceil,round,signum}`（library dual-run のみ） | `101` `ff` `377` `-1` `-1` `-1.0` `false` `true` `3` `2.0` `3.0` … |

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、複合型に無いメンバは `compound_bad.scala`、テンプレートへの二つ目のクラスは `mism7_mixin_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` / `@noinline` は実 scalac 2.13.16 と同じくどの定義（val / var / class / type ...）にも、両方同時にも、警告なしで付けられる（`crates/cli/tests/smallgaps.rs` の `fixtures_sgap`。`inline_bad.scala` は削除）。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。`java.lang.String` の未対応メソッドは `text_string1_bad.scala`。`StringBuilder` の未対応メソッドは `text_stringbuilder1_bad.scala`。`Range` の未対応メソッドは `text_range1_bad.scala`。`scala.math` の未対応関数は `text_math1_bad.scala`。
implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、複合型に無いメンバは `compound_bad.scala`、テンプレートへの二つ目のクラスは `mism7_mixin_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。`mutable.ArrayBuffer` の新規メンバーに無いものは `coll_arraybuffer1_bad.scala`。`mutable.ListBuffer` は `coll_listbuffer1_bad.scala`。新規 `mutable.Map` は `coll_mutablemap1_bad.scala`。新規 `mutable.Set` は `coll_mutableset1_bad.scala`。`immutable.Map` の新規メンバーは `coll_immutablemap1_bad.scala`。`immutable.Set` は `coll_immutableset1_bad.scala`。`Vector` の新規メンバーは `coll_vector1_bad.scala`。`Tuple2` の新規メンバーは `coll_tuple2_extra1_bad.scala`。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` / `@noinline` は実 scalac 2.13.16 と同じくどの定義（val / var / class / type ...）にも、両方同時にも、警告なしで付けられる（`crates/cli/tests/smallgaps.rs` の `fixtures_sgap`。`inline_bad.scala` は削除）。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。
| `anoncap1.scala` | 匿名クラスの基本キャプチャ：パラメータ 1 つ、パラメータ 2 つ + ブロックローカル `val`、親コンストラクタ引数と `super` オーバーライドでの使用、匿名クラス自身の `val` 初期化子からの参照（両 ABI で実行） | `mk 7` `13` `b:t9/9` `13` |
| `anoncap2.scala` | キャプチャ + `$outer`（囲みクラスのメンバと同時参照）、匿名クラス内のラムダによる二重キャプチャ、入れ子匿名クラス、ラムダの中の `new`、lambda-lift されるネスト `def` の中の `new`、trait のメソッドの中の匿名クラス（`$outer` はインタフェース型。レシーバは class と object の両方）（両 ABI で実行） | `holder 15` `14` `inner 42` `16` `12` `106` `206` |
| `anoncap3.scala` | キャプチャした `var` への書き込み、コンストラクタ引数を持つローカル `class` のキャプチャ、`var` と `val` の同時キャプチャ、ループをまたいだ `var` の書き戻し、by-name パラメータのキャプチャ（両 ABI で実行） | `3` `7` `acc=20` `6` `byName 6` `6` |

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、複合型に無いメンバは `compound_bad.scala`、テンプレートへの二つ目のクラスは `mism7_mixin_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` / `@noinline` は実 scalac 2.13.16 と同じくどの定義（val / var / class / type ...）にも、両方同時にも、警告なしで付けられる（`crates/cli/tests/smallgaps.rs` の `fixtures_sgap`。`inline_bad.scala` は削除）。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。匿名クラスが囲みメソッドに無い名前を参照するのは `anoncap1_bad.scala`（`not found: value missingLocal`）、匿名クラスより後ろで定義した `val` を参照するのは `anoncap2_bad.scala`（`not found: value later`）。匿名クラス / ローカルクラスのキャプチャは `crates/cli/tests/anoncap.rs` にあり、各 fixture を `--no-scala-library` と `--scala-library` の両方で `java -Xverify:all` 実行して同じ出力になることを見ています。
型パラメータの境界は `lowbound.scala`（`::` の `[B >: A]`、ユーザー定義 `Box.widen`、`[A <: Shape]`。私有ランタイムと `--scala-library` の両方で dual-run。`java -Xverify:all`）と `lowbound_lib.scala`（`List(...)` 可変長の lub。library リンク時のみ）で見ています。境界違反は `lowbound_bad.scala`（推論した上限境界違反）/ `lowbound_bad2.scala`（明示した上限境界違反）/ `lowbound_bad3.scala`（明示した下限境界違反）でコンパイルエラーになることを見ています。これらは `crates/cli/tests/lowbound.rs` から回します。

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、複合型に無いメンバは `compound_bad.scala`、テンプレートへの二つ目のクラスは `mism7_mixin_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` / `@noinline` は実 scalac 2.13.16 と同じくどの定義（val / var / class / type ...）にも、両方同時にも、警告なしで付けられる（`crates/cli/tests/smallgaps.rs` の `fixtures_sgap`。`inline_bad.scala` は削除）。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。ArrayOps の変換系（`toList` 等）に無いメンバーは `arrconv1_bad.scala`、ArrayOps の集約系（`sum` 等）に無いメンバーは `arrconv2_bad.scala`、`MapView` に無いメンバーは `mapview1_bad.scala` です。
| `xsource3_wildcard.scala` | `?` ワイルドカード型（`? <: T` / `? >: Lo <: Hi` / backtick 付き `` `?` ``） | `shape` ×4 `7` |
| `xsource3_intersection.scala` | `-Xsource:3` の `&` 交差型（`with` 混在・型メンバー・上限境界） | `bounded` `ada` `36` `36` `72` |
| `xsource3_block_lambda.scala` | ブロック位置の関数リテラル（`{ x => val n = 1; n }` / `{ x: Int => … }` / `case` 本体・入れ子） | `8` `12` `11` `9` `21` `100` `8` `11` |

型パラメータを取る型メンバー / 型エイリアスと高階 context bound は `crates/cli/tests/tmember.rs` の専用スイート（9 本）で回します。`tmember1.scala`（別トレイトで宣言した `type C[T] <: TypedType[T]` を `type C[T] = JdbcType[T]` で実装、self type 経由の `type C[T] = self.C[T]`、型メンバーを境界に取る context bound `def base[U: BaseColumnType]`）、`tmember2.scala`（`def f[F[_]: Async]()` / `class C[F[_]: Async]`、型パラメータ `F` と同名の `val F` による名前空間の分離）、`tmember3.scala`（境界内のワイルドカード `R <: Rep[?]`、高階パラメータを型引数に渡す `Query[?, U, C]`、型引数付き `#` 射影 `Profile#AbstractTable[?]`）を、**scala-rs で実行した出力**と**実 scalac 2.13.16 で実行した出力**の両方に対して突き合わせます（`same_as_scalac`）。負例は `tmember_bad.scala`（高階 view bound → `type F takes type parameters`）、`tmember_bad2.scala`（未解決の適用型 → `not found: type Missing`）、`tmember_bad3.scala`（`type C[T] = Int` が `<: Bound[T]` に反する → `incompatible type in overriding type C`）で、いずれも実 scalac と同じ診断です。

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、複合型に無いメンバは `compound_bad.scala`、テンプレートへの二つ目のクラスは `mism7_mixin_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` / `@noinline` は実 scalac 2.13.16 と同じくどの定義（val / var / class / type ...）にも、両方同時にも、警告なしで付けられる（`crates/cli/tests/smallgaps.rs` の `fixtures_sgap`。`inline_bad.scala` は削除）。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。`?` ワイルドカードと `&` 交差型は `crates/cli/tests/xsource3.rs` の専用スイート（12 本）で、`xsource3_wildcard.scala`（フラグ無し / `-Xsource:3` / `-Xsource:3-cross` の 3 通りで実行）と `xsource3_intersection.scala`（`-Xsource:3` 系のみ）を回します。負例は `xsource3_intersection_bad.scala`（フラグ無し・`-Xsource:2.13` では `&` を診断し、`-Xsource:3` を付けると同じソースが通ることも見る）と `xsource3_question_bad.scala`（`` 型名 `?` には backtick が要る ``）。`-Xsource:2.12` は nsc と同じくオプションエラーです。パーサ側の単体テストは `crates/parser/src/lib.rs` にあり、`?` の木が `_` と一致すること、`&` の木が `with` と一致すること、フラグ無しでは `&` が中置型のままであることを見ています。ブロック位置の関数リテラルは `xsource3_block_lambda.scala`（フィクスチャ名の衝突を避けるため同じ接頭辞にしています）で、`{ x => val n = 1; n }` / 複数行ブロック本体 / 本体中の `def` / 括弧なし `{ x: Int => … }` / `{ () => … }` / `case` 本体の中のラムダ / 入れ子ラムダを実行します。パーサ単体テストでは、本体が `Local` 位置では従来どおり関数型注釈（`(f: Int => Int)`）になること、本体ブロックが `case` で止まることも見ています。

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

`InnerClasses` / `EnclosingMethod` 属性は `crates/cli/tests/innerclasses.rs`（fixture 接頭辞
`inner`）の専用スイート（10 本）です。`inner.scala` は報告されたバグそのもの
（`object Main { trait Shape; class Circle extends Shape }`）で、`getClass.getSimpleName` /
`isMemberClass` / `getEnclosingClass` / `getDeclaringClass` を Scala コードから直接呼んで
結果を出力し、私有ランタイムと `--scala-library` の両方でコンパイル・実行して実 scalac
2.13.16 の stdout（`expected/inner.txt`）と突き合わせます。`inner_local.scala` は無名クラス
（`isAnonymousClass` / 空の `getSimpleName`）とローカルクラス（`isLocalClass`）、
`inner_nested.scala` は `object`（module）ではなく **`class` の直下**にネストしたクラス /
`private` クラス / object（`$outer` を持つので `ACC_STATIC` が付かない）、`inner_case.scala`
は case class のコンパニオンと value class を回します（`AnyRef == null` が私有ランタイムだけ
クラッシュする既存の別バグを避けるため、null 比較そのものは `isMemberClass` などの真偽値経由
で間接的に確認します）。加えて `javap -v` の `InnerClasses:` セクションをパースして実 scalac
2.13.16 の出力と突き合わせる 6 本（`inner_circle_lists_self_and_shape` ほか）があり、
プールインデックスと（scala-rs 独自の `Main`/`Main$` 分割に由来する）外側クラス名の綴りだけを
正規化して比較します。`javap` は同じディレクトリに接頭辞を共有する classfile（`Foo.class` と
`Foo$class.class` など）があると誤ったファイルを解決することがあるため、対象ファイルを空の
一時ディレクトリへコピーしてから呼び出します（`run_javap`）。
クラス / trait のメンバである `object` は `crates/cli/tests/nestedobj.rs`（fixture 接頭辞
`nestedobj`）の専用スイート（7 本）です。`nestedobj.scala` は外側の `val` / `Outer.this`、
二つのメンバ `object` の相互参照、メンバ trait を継承した `object`、非 static な `object`
の中の `object`、2 段ネストしたクラスの中の `object`、そして**同一性**（`o.P eq o.P` が
`true`、別インスタンスの `P` とは `false`）を回します。`nestedobj_trait.scala` は trait の
メンバ `object` を実装クラスと匿名クラスの両方から使い、クラスにネストした `case class`
も見ます。どちらも**私有ランタイムと `--scala-library` の両方**でコンパイルして
`java -Xverify:all` で実行し、期待出力は**実 scalac 2.13.16 の stdout そのまま**です。
`member_object_takes_its_enclosing_instance` は `Main$Outer$P$` の `<init>` が
`(LMain$Outer;)V` で `MODULE$` が無いこと、`enclosing_class_holds_the_module_field` は
`Main$Outer` が `P$module` と `P()` を持つこと、`trait_member_object_is_mixed_in` は
interface 側が `Opt()` を abstract で宣言し実装クラスがフィールドを持つこと、および
`Main$Outer$T$$$outer` が interface と実装の両方にあることを classfile のバイト列で固定
します。異常系は `nestedobj_bad.scala` で、外側インスタンスを読むローカル `object` と、
value class の中の `object`（scalac と同じ `implementation restriction: nested object is
not allowed in value class`。以前は `VerifyError` になっていました）の 2 つを見ます。

メソッドローカルの `lazy val` は `crates/cli/tests/lazyref.rs`（fixture 接頭辞 `lr`）の
専用スイート（15 本）です。`lr_local.scala` は「一度も読まない」「1 回」「複数回（初期化は
1 回だけ）」、ローカル `val` / `var` / メソッド引数の捕捉、`lazy val` 同士の依存（前方・後方
の両方向）、全セルクラス（`LazyBoolean` … `LazyDouble` / `LazyRef` / `LazyUnit`）、
`while` の中（反復ごとに別セル）、ラムダの中とラムダからの捕捉、例外を投げる初期化子の
再試行、ネストした `def` の中、結果型を書かない形（セルクラスは推論した型で決まる）、
メソッド型パラメータ `A`（`Object` へ消去されるので `LazyRef`）を 1 本で回します。`lr_edge.scala` はローカル class から読む形、
初期化子からの**囲みメソッドへの `return`**（`return` がアクセサへ移るので、メソッド側は
それでも `NonLocalReturnControl` のハンドラを持たないといけない）、**value class** の結果
（erasure でアクセサの戻りが `int` になる一方セルは `LazyRef` のまま）、`match` の case と
`try` ブロック、外側ブロックのセルを読む初期化子、trait のメソッド、コンストラクタ本体、
`this` の捕捉、同名の `lazy val` が兄弟スコープにある形を回します。`lr_nestdef.scala` は
ラムダリフトの**推移的な捕捉**（持ち上げた `def` が別の持ち上げた `def` を呼ぶとき、
その捕捉も渡せないといけない。素のネスト `def` でも壊れていた既存バグ）です。
`lr_member.scala` は既存の
`bitmap$0` 方式（クラス・trait・object のメンバ、および同じテンプレートにメンバと
ローカルが同居する形）が壊れていないことの回帰です。どちらも**私有ランタイムと
`--scala-library` の両方**でコンパイルして `java -Xverify:all` で実行し、期待出力は
**実 scalac 2.13.16 の stdout そのまま**（`real_scalac_dual_run_lr_*` が毎回その場で
突き合わせます）。laziness は値ではなく `println` の**順序**にしか現れないので、
自分自身との比較では検出できないためです。形の固定は
`local_lazy_val_compiles_to_a_lazy_cell`（`Main$` が `scala/runtime/Lazy*` と
`initialized` / `initialize` を参照し、ローカル用に `bitmap$0` を使わないこと）、
`member_lazy_val_still_uses_the_bitmap`、
`cells_come_from_the_private_runtime_only_when_it_is_used`（jar モードでは
`scala/runtime/Lazy*.class` を出さない）、`private_cells_match_the_library_signatures`
（私有セルの 3 メソッドの記述子が scala-library と一致）。異常系は
`lr_forward_bad.scala`（前方参照できるのは `lazy val` だけで、素の `val` は今までどおり
`not found: value b`）。

`agent/reify2` スライス（宣言クラスでの呼び出しと quasiquote の reification）のフィクスチャは接頭辞 `reify`（`reify` / `reify_bad` / `reify_qq` / `reify_qq_bad`）で、コンフリクト回避のため `crates/cli/tests/reify.rs` に置いています。`reify.scala` は 1 コンパイル単位で trait-extends-class のディスパッチ（宣言クラスへの `checkcast` + `invokevirtual` と、トレイト自身の `invokeinterface`）を private ランタイム・library ABI の両方で見るもので、期待出力は実 scalac 2.13.16 の出力です。`reify_qq.scala` は **scala-reflect.jar を `-cp` に置いて**quasiquote を実行し、実 scalac の出力と毎回その場で比較します（`reify_qq_quasiquotes_build_the_same_trees_as_scalac`）。`reify_runtime_universe_builds_a_tree` は `scala.reflect.runtime.universe` 上で `SyntacticTermIdent` / `SyntacticSelectTerm` / `Literal(Constant(42))` を組み立てて**実行**します（以前は `NoSuchMethodError`）。`reify_classpath_trait_is_an_interface_and_inherits` は `-cp` 越しのトレイトのメンバと継承メンバ（以前は `IncompatibleClassChangeError` と `is not a member`）。異常系は `reify_bad.scala`（トレイトにもクラスにも無い名前）と `reify_qq_bad.scala`（reification が落とせない形が、どれも形の名前つきで診断されること）。

quasiquote と、その受け皿である reflect ABI の下地は `crates/cli/tests/quasi.rs` にまとめています。正常系 `tests/fixtures/quasi.scala` は `scala_library_dual_run_quasi`（jar リンクで実行し `expected/quasi.txt` と一致）と `real_scalac_dual_run_quasi`（**実 scalac 2.13.16** の stdout・期待値・scala-rs の出力の三者一致）の 2 通りで回し、package object のメンバ（`scala.math.Pi` / `abs` / `max`）、`import <値>._`、引数なし `def` の結果に対する `apply` 挿入（`Literal(1)` = `Literal.apply(1)`）、そして**ユーザ定義の `q` 補間子が quasiquote に横取りされないこと**を実行結果まで固定します。異常系 `quasi_bad.scala` は `fixtures_quasi_bad_is_error` が `q` / `tq` / `pq` / `cq` の 4 種すべてに診断が出ること、`q""` は `unimplemented syntax: quasiquote q"..." (empty quasiquote)` になることを見ます。`quasiquote_is_not_reported_as_a_stringcontext_member` は、以前の**誤った**診断 `value q is not a member of StringContext` が戻らないことを固定します。

同じファイルの後半（接頭辞 `qq`）は **scala-reflect.jar が `-cp` にあるとき**の reflect universe とマクロ `Context` です。`tests/fixtures/qq_universe.scala` は `qq_universe_wildcard_import_reaches_inherited_members`（`java -Xverify:all` で実行し `expected/qq_universe.txt` と一致）と `qq_universe_matches_real_scalac`（**実 scalac 2.13.16** と三者一致）の 2 通りで回し、`import <universe>._` が継承メンバ（`TermName` / `TypeName` / `Constant` / `Literal` / `EmptyTree` / `termNames` / `NoSymbol`）を値としても型としても持ち込むこと、メソッドローカルの `import u._` の prefix がそのメソッドの外に漏れないこと、そして `showRaw` まで含めて**同じ木**ができることを固定します。`tests/fixtures/qq_ctx.scala` は**マクロ実装そのもの**で、`qq_ctx_macro_implementation_compiles` が scala-rs と実 scalac の両方でコンパイルできること、吐いた classfile が JVM にロード・検証されること（`java -Xverify:all` + `Class.forName`）を見ます（展開には engine が要るので実行はしません）。異常系 `qq_ctx_bad.scala` は `qq_ctx_bad_names_every_form_it_cannot_build` が、reify できない形（`else` の無い `if`・by-name 型）を**その形の名前つきで**診断すること、`Tree` でない穴が型エラーになることを見ます。`qq_ctx_without_scala_reflect_is_diagnosed` は scala-reflect.jar が無いときに空の `Context`（`crates/typer/src/prelude_reflect.rs`）のまま `value universe is not a member of Context` と言うことを固定します。

接頭辞 `qr` は **reification の残りの形**（`docs/macros.md` §7.7）です。`tests/fixtures/qr_forms.scala` は `tq"..."` / `pq"..."` / `cq"..."` と `q"..."` の型注釈・eta 展開（`f _`）・型適用・ブロックと `val` 定義・`new`（カリー化コンストラクタ含む）・`match`・部分関数 `{ case … }`・関数リテラル・`this`・代入・`if`-`else`・演算子名の符号化・名前の位置の穴を **56 行ぶん `showRaw` で印字**し、`qr_forms_reifies_the_remaining_shapes`（`java -Xverify:all` で実行して `expected/qr_forms.txt` と一致）と `qr_forms_matches_real_scalac`（**実 scalac 2.13.16** と三者一致）の 2 通りで回します。同じファイルの末尾は、オーバーロード集合になっている木のファクトリ（`Ident(TermName("x"))` / `Bind` / `This` / `New`。slick の `TableQuery` のマクロ実装はこれだけで書かれている）が通り、JVM の検証も通ることを見ます。異常系 `qr_forms_bad.scala` は `qr_forms_bad_names_every_form_it_cannot_build` が、パーサが nsc の保つ区別ごと正規化してしまう形（`else` の無い `if`・by-name 型・`..$` と普通の引数の混在・`type` 定義）を**その形の名前つきで**診断することを固定します。

接頭辞 `dq` は **定義の reification**（`docs/macros.md` §7.8）です。`tests/fixtures/dq_defs.scala` は `class` / `case class` / `trait` / `object` / `def` / 修飾つきの `val`・`var`、クラス・パラメータのアクセサ・フラグ（`PARAMACCESSOR` / `CASEACCESSOR` / `PRIVATE | LOCAL`）、implicit 節（`ImplicitParams`）、型パラメータと変位・境界、nsc が補う親（`AnyRef` と `case` の `Product with Serializable`）、定義を含むブロック、匿名クラスの本体（`new C(1) { … }`）、そして名前・パラメータリスト・親・本体の位置の穴を **93 行ぶん `showRaw` で印字**し、`dq_defs_reifies_definitions`（`java -Xverify:all` で実行して `expected/dq_defs.txt` と一致）と `dq_defs_matches_real_scalac`（**実 scalac 2.13.16** と三者一致）の 2 通りで回します。異常系 `dq_defs_bad.scala` は `dq_defs_bad_names_every_form_it_cannot_build` が、12 の落とせない形（自分型・early definition・`private[X]`・by-name パラメータ・可変長パラメータ・手続き構文・型も本体も無い `def`・パターン定義・高階型パラメータ・context bound・`case` クラスの親の `..$`・末尾でない implicit 節・`macro` 定義）を**その形の名前つきで**診断することを固定します。

接頭辞 `lf2` は **`Liftable`**（`docs/macros.md` §7.8）です。`tests/fixtures/lf2_lift.scala` は `Tree` でない穴——リテラル 7 種・`Constant`・`TermName` / `TypeName`（項・型・パターン・名前枠の 4 つの位置）・`Type`・`Symbol`・`..$` 越しの要素——を **29 行ぶん `showRaw`（`TypeTree` は中身の型が隠れるので `show` も）で印字**し、`lf2_lift_builds_the_standard_liftable_trees`（`java -Xverify:all` で実行して `expected/lf2_lift.txt` と一致）と `lf2_lift_matches_real_scalac`（**実 scalac 2.13.16** と三者一致）の 2 通りで回します。実 scalac 側は nsc 自身が implicit `Liftable` を推論するので、これで scala-rs が組む木が**標準インスタンスの作る木と同じ**であることが固定されます。`tests/fixtures/lf2_ctx.scala` は materialiser 無しには実行時に作れない 2 つ（`WeakTypeTag` と `Expr`。slick の `mapToImpl` が使うもの）を**マクロ実装として**書いたもので、`lf2_ctx_lifts_tags_and_exprs_in_a_macro_implementation` が scala-rs と実 scalac の両方でコンパイルできること、classfile が JVM にロード・検証されることを見ます。`symbolOf[T]` / `weakTypeOf[T]`（型パラメータを implicit 節にしか書かないメンバ）もここに入っています。異常系 `lf2_lift_bad.scala` は `lf2_lift_bad_names_every_hole_it_cannot_lift` が、標準インスタンスの無い型（`File`）・rank 0 のコレクション（`List[Int]`）・`..$` 越しの `Symbol`（nsc も断る）を**型を名指しして**診断し、`reify { … }` が `cannot expand reify { ... }` になること（以前の**誤った**診断 `value reify is not a member of JavaUniverse` が戻らないこと）を固定します。

接頭辞 `fn2` は **fresh 名を要する 3 形**（[`docs/macros.md`](docs/macros.md) §7.10）です。`tests/fixtures/fn2_fresh.scala` は `_` プレースホルダ関数リテラル（`q"_.get"` / `q"_.foo(_)"` / `q"(_: Int).get"` / 入れ子）、`_` 型引数＝存在型（`tq"P[_, _]"` / 境界つき / 入れ子 / `asInstanceOf` / `new` の型引数）、右結合演算子（`q"a :: b"` / `q"a :: b :: c"` / 穴つき / ドット呼び `q"b.::(a)"` は素の適用のまま / パターンの `case a :: b`）、そしてパターンの中の `_` 型引数（裸は型変数パターン、境界つきは存在型）を **32 行ぶん `showRaw` で印字**し、`fn2_fresh_reifies_the_fresh_name_forms`（`java -Xverify:all` で実行して `expected/fn2_fresh.txt` と一致）と `fn2_fresh_matches_real_scalac`（**実 scalac 2.13.16** と三者一致）の 2 通りで回します。最後の行は slick の `ShapedValue.mapToImpl` の巨大な `q"""…"""` の形そのもの（`ProductResultConverter[_, _, _, _]` と `TypeMappingResultConverter[…, _]`）です。fresh 名の**番号**は universe のグローバルなカウンタから来るうえ、nsc は右から左に配るので、比較の前に **1 行ごとに初出順で 1 から採番し直します**（`renumber_fresh_names`）。落ちるのはカウンタの状態と採番の向きだけで、**どの出現がどの束縛を指すか**は落ちません。その正規化自体は `renumber_fresh_names_keeps_binder_identity` が固定します。異常系 `fn2_fresh_bad.scala` は `fn2_fresh_bad_refuses_an_unbound_wildcard` が、束縛するものの無い `q"_"` / `tq"_"`（実 scalac も拒否する）を診断することを固定します。
接頭辞 `tt` は **`TypeTag` / `WeakTypeTag` の materialization**（`docs/macros.md` §7.10）です。`tests/fixtures/tt_tags.scala` はタグを 1 つも書かずに `typeOf` / `weakTypeOf` / `typeTag` / `weakTypeTag` を呼び、作られたタグの `tpe` を **30 行ぶん印字**して `tt_tags_materialises_type_tags`（`java -Xverify:all` で実行して `expected/tt_tags.txt` と一致）と `tt_tags_matches_real_scalac`（**実 scalac 2.13.16** と三者一致）の 2 通りで回します。scala-rs が組む木は nsc の木と同じではない（`$u` / `$m` の束縛を省き、mirror を cast し、creator の結果型を書き下す）ので、固定しているのは**答え**——`toString` / `=:=` / `<:<` / `typeSymbol.fullName`——です。`tests/fixtures/tt_ctx.scala` はマクロ実装の中の `c.typeOf[T]`（slick の `ShapedValue.mapToImpl` が `uTag.tpe <:< c.typeOf[HList]` と書く形）で、`tt_ctx_materialises_in_a_macro_implementation` が両コンパイラでコンパイルできること・classfile が JVM にロード・検証されることを見ます。異常系 `tt_tags_bad.scala` は `tt_tags_bad_names_every_tag_it_cannot_build` が、組めない 7 形（型引数のある型・入れ子クラス・`AnyRef`・型パラメータ・singleton 型）を**その形の名前つきで**診断し、`no implicit: could not find implicit value of type TypeTags$TypeTag[...]` に戻らないことを固定します。

def マクロは `crates/cli/tests/macros.rs` にまとめています。呼ばれない macro def のコンパイルと、`Sugar$.class` にメソッドが出ていないことは `macro_def.scala`。マクロ呼び出しの診断は `macro_call_bad.scala`（`macro expansion is not implemented`）。戻り値型の無いマクロ def は `macro_no_result_type_bad.scala`。`Context` を第 1 引数に取らない実装は `macro_impl_shape_bad.scala`。解決できない実装参照は `macro_impl_missing_bad.scala`。whitebox は `macro_whitebox_bad.scala`。設計は [`docs/macros.md`](docs/macros.md)。

def マクロの**展開**（JVM ブリッジ）は接頭辞 `eg`、`crates/cli/tests/engine.rs` です。nsc と同じく**2 回コンパイル**します: `tests/fixtures/eg_impl.scala`（マクロ実装 5 つ——引数なし / `c.Expr[Int]` / 生の `c.Tree` / `c.WeakTypeTag[T]` / static シンボルを名指す木）を先にコンパイルし、その出力を `-cp` に載せて `tests/fixtures/eg_use.scala`（マクロ def と呼び出し地点）をコンパイルします。`eg_macros_expand_and_run` が `java -Xverify:all` で走らせて `expected/eg_use.txt` の 8 行と一致することを見て、`eg_macros_match_real_scalac` が**同じ 2 ファイルを実 scalac 2.13.16 で 2 段コンパイル・実行した stdout** と三者一致することを見ます。マクロが「違う木」に展開されてもコンパイルは通ってしまうので、**出力の比較だけが間違った展開を捕まえられます**。異常系は `eg_samerun_bad.scala`（実装が同じ run にある。nsc も同じ理由で断る）と `eg_gaps_bad.scala`（渡せない引数の形＝ブロック・関数リテラル、作れないタグ＝`List[Int]`）で、いずれも**その形を名指しした理由つき**で診断されることを固定します。`java` / `javac` / scala-reflect.jar が無い環境ではスキップします。

名前付き引数とデフォルト引数は `tests/fixtures/namedargs.scala` にまとめ、`crates/cli/tests/e2e.rs` から 2 通りで回します: `scala_library_dual_run_namedargs`（jar リンクで実行し `expected/namedargs.txt` と一致）と `real_scalac_dual_run_namedargs`（**実 scalac 2.13.16 でコンパイル・実行した stdout** と、期待値および scala-rs の出力の三者が一致することを見る）。中身は並べ替え（`Api.area(height = 3, width = 4)`）、自分の位置にある名前付き引数のあとの位置引数（`Api.area(width = 4, 3)`）、デフォルトとの組み合わせ、コンパニオン `apply`、後続の引数リストのデフォルト（`Api.curried(1)(2)` / `Api.dep(4)()`）、可変長引数（`Api.tagged(first = 1)` / `Api.tagged(first = 1, 2, 3)`）、case class の `apply` / `copy` / `super.info.copy(port = 2)`、コンストラクタの名前付き引数とデフォルト（`new Server(threads = 8)` / `new Server()`）、パラメータ名で絞るオーバーロードです。負例は `namedargs_unknown_bad.scala`（`unknown parameter name: q`。メソッドとコンストラクタの両方）、`namedargs_dup_bad.scala`（`parameter 'c' is already specified at parameter position 2`）、`namedargs_order_bad.scala`（`positional after named argument.`）で、いずれも文面を実 scalac 2.13.16 に合わせています。
| `lazysig.scala` | 型注釈のないメンバを前方参照（`Store.base` / `prefix` / `lazy val`） | `60` `log:store` `[store]log:store` `40` `5` `c7:7` |
| `impl2.scala` | 多相 implicit の再帰導出（`Show[List[List[Int]]]` / `Show[(A, B)]` / `Ord[List[List[Int]]]`）、specificity（`Tag[Int]` は `tagInt`）、`<:<`（`upcast[Int, Any]`）、`List`/`Iterator` の `toMap`（library dual-run のみ） | `1` `hi` `[1,2,3]` `[[1],[2,3]]` `(1,x)` `[(1,a),(2,b)]` `Some(7)` … `ab` `cd` |
| `impl2_poly.scala` | 同じ導出をユーザー定義型だけで（私有ランタイムでも走る） | `1` `Box(2)` `Box(Box(3))` `<4,four>` `Box(<5,five>)` `<Box(6),Box(six)>` `int` `any` |

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、複合型に無いメンバは `compound_bad.scala`、テンプレートへの二つ目のクラスは `mism7_mixin_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` を val に付けたのは `inline_bad.scala`。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。型注釈のないメンバどうしの相互再帰は `lazysig_cyclic_bad.scala`（scalac 2.13.16 と同じ `recursive value y needs type`）。多相 implicit の導出が底を打たないのは `impl2_missing_bad.scala`（`no implicit`。型パラメータを黙って `Any` で埋めない）、同じ形の多相 implicit が二つあるのは `impl2_ambiguous_bad.scala`（`ambiguous implicit: boxA, boxB`）、発散する導出は `impl2_diverging_bad.scala`（`implicit def loop[A](implicit a: A): A` を必ず打ち切り、scalac 2.13.16 と同じ `diverging implicit expansion for type Show[Int] starting with method loop`）。
implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、複合型に無いメンバは `compound_bad.scala`、テンプレートへの二つ目のクラスは `mism7_mixin_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` / `@noinline` は実 scalac 2.13.16 と同じくどの定義（val / var / class / type ...）にも、両方同時にも、警告なしで付けられる（`crates/cli/tests/smallgaps.rs` の `fixtures_sgap`。`inline_bad.scala` は削除）。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。型注釈のないメンバどうしの相互再帰は `lazysig_cyclic_bad.scala`（scalac 2.13.16 と同じ `recursive value y needs type`）。
| `exptype.scala` | 期待型からのメソッド型パラメータ推論（nsc `instantiateExpecting`）：`val a: Array[AnyRef] = Array("x", "y")` / `val b: Array[Any] = Array(1, 2)` / 期待型だけが `T` を決める implicit 付き `column[T]` / 不変位置は期待型が勝ち共変位置は引数が勝つ（library dual-run のみ） | `2` `x` `[Ljava.lang.Object;` `2/x` `2` `2` `[Ljava.lang.Object;` `[I` `4` `id:int` `nm:str` `any` `int` `cov str` `List()` |
| `dead.scala` | 到達不能コード（`def f(): Int = throw e` / 片側だけ `throw` の `if` / 両側 `throw` / `throw` する `match` の case / 非局所 return / 常に投げる `try/finally` / catch が投げる `try/catch/finally`）と、finalizer / `monitorexit` を飛ばさない `return`。期待出力は実 scalac 2.13.16 の stdout そのまま | `eboom` `7` `ehalf` `et` `zero` `one` `bad pick 2` `1` `3` `0` `6` `-1` `fin3` `40` `fin3` `1` `105` `2` `fin` `outer inner` `fin2` `caught b` |
| `dead_targs.scala` | 明示的型適用が implicit 節に届くこと（オーバーロードあり／なし、implicit 変換経由の拡張メソッド、可変長引数）、継承 implicit の as-seen-from、クラス型パラメータを含む引数からの推論、同型候補の specificity、親コンストラクタ引数の型引数代入（library dual-run のみ。期待出力は実 scalac 2.13.16 の stdout そのまま） | `abs/int` `abs/bool` `== raw t` `== typed bool` `bool:3` `int` `abs/int` `int\|r` `c!/bool` `int` |
| `ovl.scala` | エイリアス型メンバ / 普通のクラスのコンパニオン `apply`（デフォルト引数・可変長引数＋implicit 節）/ 値の位置で勝つ `val ==` と抽出子 / 型注釈のない `unapply` の前方参照 | `7` `1` `cfg` `t/2/true` `t/2/false` `int/5/false` `string/s/false` `int:3` `string:0` `=` `eq 42` `not 7` `not@7` |
| `numt.scala` | 7×7 の数値変換（NaN / ±Inf / MIN・MAX 込み）、`Byte`/`Short` のパラメータ・戻り値・フィールド・配列・オーバーフロー、演算子の昇格、弱適合、`Short` スクルティニーの `Int` 定数パターン（両 ABI で実行し real scalac の stdout と一致） | `B 0 0 0 0 0 0.0 0.0` … `100\|30000\|a` |

implicit の失敗（`no implicit` / `ambiguous implicit`）は typer のユニットテストと、`implicit_ambiguous.scala` / `implicit_ambiguous_parents.scala` / `implicit_inherit_local_ambiguous.scala` のコンパイル失敗で見ています。`@implicitNotFound("no show for ${A}")` のカスタム文面は `implicit_not_found.scala` です。`private[this]` / `protected[C]` の違法アクセスは `private_this_bad.scala` / `protected_qual_bad.scala` です。オーバーロードの失敗は `overload_ambiguous.scala` / `overload_none.scala`。`f` interpolator の未対応フォーマットは `f_interp_bad.scala` です。境界付き存在型の値量化のうちパックできない形は `existential_val.scala`（`T forSome { val x: Int }`）で診断します。`p.Inner forSome { val p: Outer }` は `existential_val_ok.scala` で実行します。クラスコンストラクタからの `return` は `return_ctor.scala`、誤った `@Override` は `override_bad.scala` です。クラス型パラメータの view bounds で evidence が無いのは `view_bounds_class_bad.scala` です。kind 不一致は `hk_bad.scala` です。高階型メンバーの kind 不一致と `m.F` の proper 使用は `type_member_hk_bad.scala`。refinement HK の proper 使用は `refine_hk_bad.scala`。境界付き型メンバーの incompatible override は `type_member_bounds_bad.scala`。`{ type A <: Int }` に合わない具象は `refine_bound_bad.scala`。HK 境界の incompatible override は `hk_bounded_bad.scala`。入れ子射影の失敗は `nested_proj_bad.scala`（`Int#X`）/ `nested_proj_abs_bad.scala`（`B#U#T`）。`val` に `+=` メンバーが無いのは `assign_op_bad.scala`。`asScala` が無い（import なし）のは `collection_converters_bad.scala`。高階 view bound `F[_] <% Ordered[_]` は `hk_view_bounds.scala`（scalac 2.13.16 と同じ `takes type parameters`）。トレイトの context bound は `trait_context_bounds.scala`。不安定なパス依存型は `type_proj_bad.scala`（`stable identifier required`）、不安定な singleton は `this_type_bad.scala`、複合型に無いメンバは `compound_bad.scala`、テンプレートへの二つ目のクラスは `mism7_mixin_bad.scala`、構造的代入は `structural_bad.scala`（`foo_= is not a member`）。package object の enrichment は import 無しだと `pkg_implicit_class_bad.scala`、トップレベル `implicit class` は `pkg_implicit_toplevel_bad.scala`。`IndexedSeq` に無いメンバーは `indexedseq_queue_bad.scala`。`stripMargin` / `lines` に無いメンバーは `string_ops4_bad.scala`。`Range` に無いメンバーは `numeric_range_bad.scala`。`ListBuffer` に無いメンバーは `listbuffer_bad.scala`。`takeRight` / `dropRight` / `contains` に無いメンバーは `string_ops6_bad.scala`。`NumericRange[Long]` に無いメンバーは `long_range_bad.scala`。`HashMap` に無いメンバーは `hashmap_bad.scala`。`startsWith` / `endsWith` / `indexOf` に無いメンバーは `string_ops7_bad.scala`。`NumericRange[Char]` に無いメンバーは `char_range_bad.scala`。`HashSet` に無いメンバーは `hashset_bad.scala`。`head` / `last` / `stripLineEnd` / `replaceAllLiterally` に無いメンバーは `string_ops8_bad.scala`。`ArrayOps` に無いメンバーは `array_ops2_bad.scala`。`LinkedHashMap` に無いメンバーは `linkedhashmap_bad.scala`。`tail` / `init` / `distinct` / `mkString` に無いメンバーは `string_ops9_bad.scala`。`ArrayOps.foreach` に無いメンバーは `array_ops3_bad.scala`。`LinkedHashSet` に無いメンバーは `linkedhashset_bad.scala`。`filter` / `reverseIterator` に無いメンバーは `string_ops10_bad.scala`。`ArrayOps.map` に無いメンバーは `array_ops4_bad.scala`。`ArrayDeque` に無いメンバーは `arraydeque_bad.scala`。bare `_` は `placeholder_bad.scala`（`unbound placeholder parameter`）。`byteArrayOps` に無いメンバーは `array_ops5_bad.scala`。`diff` に無いメンバーは `string_ops11_bad.scala`。Function2 の arity 不一致 `_ + _` は `placeholder2_bad.scala`（`missing parameter type for expanded function`）。`charArrayOps` に無いメンバーは `array_ops6_bad.scala`。`updated` に無いメンバーは `string_ops12_bad.scala`。typed `(_: Int)` の bare は `placeholder3_bad.scala`（`unbound placeholder parameter`）。`doubleArrayOps` に無いメンバーは `array_ops7_bad.scala`。`partition` に無いメンバーは `string_ops13_bad.scala`。`genericArrayOps` に無いメンバーは `array_ops8_bad.scala`。`unitArrayOps` に無いメンバーは `array_ops9_bad.scala`。`SortedSet` に無いメンバーは `sortedset_bad.scala`。ArrayOps `filter` に無いメンバーは `array_ops10_bad.scala`。`sorted` に無いメンバーは `string_ops14_bad.scala`。`SortedMap` に無いメンバーは `sortedmap_bad.scala`。ArrayOps 4 引数 `flatMap` に無いメンバーは `array_ops11_bad.scala`。`indices` に無いメンバーは `string_ops15_bad.scala`。`BitSet` に無いメンバーは `bitset_bad.scala`。ArrayOps `take` に無いメンバーは `array_ops12_bad.scala`。`dropWhile` に無いメンバーは `string_ops16_bad.scala`。`Breaks` に無いメンバーは `breaks_bad.scala`。ArrayOps `drop` に無いメンバーは `array_ops13_bad.scala`。`find` に無いメンバーは `string_ops17_bad.scala`。`tryBreakable` に無いメンバーは `breaks2_bad.scala`。ArrayOps `foldLeft` に無いメンバーは `array_ops14_bad.scala`。`toByte` に無いメンバーは `string_ops18_bad.scala`。`BigInt` に無いメンバーは `bigint_bad.scala`。ArrayOps `scanLeft` に無いメンバーは `array_ops15_bad.scala`。`grouped` に無いメンバーは `string_ops19_bad.scala`。`pipe` に無いメンバーは `chaining_bad.scala`。ArrayOps `last` に無いメンバーは `array_ops16_bad.scala`。`:+` に無いメンバーは `string_ops20_bad.scala`。ArrayOps `find` に無いメンバーは `array_ops17_bad.scala`。`compare` に無いメンバーは `string_ops21_bad.scala`。`Using` に無いメンバーは `using_bad.scala`。ArrayOps `filterNot` に無いメンバーは `array_ops18_bad.scala`。`>` 系に無いメンバーは `string_ops22_bad.scala`。`Using.Manager` に無いメンバーは `using2_bad.scala`。ArrayOps `zipWithIndex` に無いメンバーは `array_ops19_bad.scala`。`iterator` に無いメンバーは `string_ops23_bad.scala`。`Using.resources` に無いメンバーは `using3_bad.scala`。ArrayOps `lengthIs` に無いメンバーは `array_ops20_bad.scala`。`flatMap` に無いメンバーは `string_ops24_bad.scala`。`View.fill` に無いメンバーは `view_bad.scala`。キャプチャした `val` への `+=` は `capture_var_bad.scala`（`not assignable`）。self type の不正 mixin は `self_type_bad.scala`、共変パラメータの `var` は `variance_bad.scala`。循環 type alias は `type_alias_bad.scala`。`apply` の無い `c(1)` は `update_apply_bad.scala`（`update` には落とさない）。非末尾再帰の `@tailrec` は `tailrec_bad.scala`、未対応アノテーションは `annot_bad.scala`（`@specialized`）。`@inline` を val に付けたのは `inline_bad.scala`。SAM でない型へのラムダは `sam_bad.scala`。`@switch` にできない match は `switch_bad.scala`（nsc どおり warning）。early defs の違法 `def` は `early_defs_bad.scala`。定数型の不一致は `const_types_bad.scala`、`language.dynamics` なしの Dynamic 選択は `dynamic_bad.scala`、postfix / implicitConversions なしは `postfix_ops_bad.scala` / `implicit_conv_bad.scala`（nsc どおり warning。`-Xfatal-warnings` でエラー）です。別コンパイルは `separate_lib.scala` を classfile にしてから `separate_main.scala` を `-cp` でコンパイルします（vals / パラメータ付き defs / 型パラメータ / case class `Point` / `val one: 1` / `def lit(x: 1)` を pickle から読む）。package object の `implicit class` は `pkg_implicit_lib.scala` を classfile にしてから `pkg_implicit_main.scala` を `-cp` でコンパイルします（pickle の IMPLICIT）。`scalac` 2.13 は PATH、`/tmp/scala-2.13.16`、または公式 tarball（約 20MB）で取れれば、同じ classfile に対して `Lib.greet` / `Lib.magic` / `Lib.id(42)` / `new Box("hi").get` / `Point(3, 4)` / `p.x` / `p match { case Point(a, b) => … }` / `Lib.add` / `Lib.f(List(1, 2))`（`List[_]`） / `Lib.g`（`@deprecated("msg", "2.13.0")`） / `new Holder().me.n`（`this.type`） / `Lib.fAnyRef(List("a"))`（`List[_ <: AnyRef]`） / `Lib.h(1)`（`Int @unchecked`） / `Lib.one`（`val one: 1`） / `Lib.lit(1)`（`def lit(x: 1)`） / `Lib.gone`（Java `@Deprecated`。`-deprecation` で warning） / `Lib.nest(List(List(1)))`（`List[_ <: List[_]]`） / `Lib.idRef(new MixD())` の `y.a + y.b + y.f`（refinement pickle） / `Lib.marked`（`@Ann(foo)` TREE Ident） / `Lib.markedSel`（`@Ann(c.x)` TREE Select） / `Lib.markedLit`（`@Ann(3)` リテラル） / `new Holder().markedThis`（`@Ann(this)` THIStree） / `new Holder().markedClass`（`@Ann(classOf[Int])` LITERALclass） / `Lib.markedApply`（`@Ann(ident(1))` APPLYtree） / `new Holder().markedThisSel`（`@Ann(this.x)` Select(This)） / `new Holder().markedSuper`（`@Ann(super.foo)` SUPERtree） / `Lib.markedNest`（`@Ann(ident(ident(1)))` ネスト APPLYtree） / `Lib.markedNamed`（`@Ann(foo = 1)` named → 位置 LITERALint） / `new Holder().markedNamedTree`（`@Ann(foo = this.x)` named TREE → 位置 Select） / `Lib.markedNamedIdent`（`@Ann(foo = bar)` named TREE → 位置 Ident） / `Lib.markedReorder`（`@Ann2(b = 2, a = "ok")` を位置ソース順 Constant のまま。ctor 順並べ替えなしでも scalac 2.13.16 が typecheck） / `Lib.join("a","b")`（VARARGS） / `new OrdBox(1).compare(new OrdBox(2))`（BRIDGE） / `Lib.usesAlias(1)` / `val x: Lib.T`（ALIASsym）を typecheck します。読めない pickle 形は成功扱いにしません。取れなければスキップします。未知の XML エンティティは `xml_attr_bad.scala` で診断します。本文付き `@native` は `native_bad.scala`。`-cp` 上の壊れた Java `.class` は `unsupported classfile` で診断します。Java `protected` の違法アクセスは `java_prot_bad.scala`。Java 非 enum の合成 `values` は `java_enum_bad.scala`。コンテキストバウンドの欠ける evidence は `context_bounds_bad.scala` / `context_bounds_class_bad.scala`。違法な補助コンストラクタ（`this` なし / 文のあとの `this` / `super`）は `aux_ctor_bad.scala` / `aux_ctor_stmt_bad.scala` / `aux_ctor_super_bad.scala`。型注釈のないメンバどうしの相互再帰は `lazysig_cyclic_bad.scala`（scalac 2.13.16 と同じ `recursive value y needs type`）。オーバーロードで甲乙つけがたい候補は `ovl_ambiguous_bad.scala`（scalac は `ambiguous reference to overloaded definition`）、コンパニオン `apply` のパラメータ型に合わない呼び出しは `ovl_none_bad.scala`（末尾のデフォルト引数は先行パラメータを省略可能にしない）。期待型からの型パラメータ推論の負例は `exptype_unsolved_bad.scala`（引数でも期待型でも `T` が決まらず、nsc と同じ `could not find implicit value …`）と`exptype_arrayvar_bad.scala`（`Array` は非変なので `Array[Int]` は `Array[Any]` に渡せない）です。

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
| `reify_qq.scala` / `reify_qq_bad.scala`（`crates/cli/tests/reify.rs`、scala-reflect.jar が要る） | quasiquote の reification（実 scalac 2.13.16 と dual-run）: リテラル / 名前 / 選択 / 適用（カリー化含む）/ `$x` 穴 / `..$xs` 穴 / 引数ゼロ。異常系は落とせない 5 形（右結合演算子 / `else` の無い `if` / `_` プレースホルダ / `..$` の混在 / rank 不一致）が形の名前つきで診断されること | `1` `greet` `true` `"hi"` `a.b.c` `f(1)` `a.b(1)(2)` `g(x)` `h(x, 2)` `x.size` `k(p, q)` `k()` |
| `quasi.scala` | quasiquote の下地（実 scalac 2.13.16 と dual-run）：jar の package object のメンバ（`scala.math.Pi` / `abs` / `max`）、`import <値>._` とその書き戻し、引数なし `def` の結果に対する `apply` 挿入（`Literal(1)` = `Literal.apply(1)`）、ユーザ定義 `q` 補間子が横取りされないこと | `3.141592653589793` `7` `9` `<1>` `<x>` `small` `<via-path>` `a$1b$2c` `user-q:a\|b` `user-tq:c` |
| `qq_universe.scala`（`crates/cli/tests/quasi.rs`、scala-reflect.jar が要る） | `import <universe>._` が継承メンバを値・型の両方で持ち込むこと、メソッドローカル import の prefix がスコープを越えないこと、`showRaw` まで実 scalac 2.13.16 と一致すること | `hi` `T` `Constant(42)` `42` `<empty>` `<init>` `<none>` `Literal(Constant("s"))` `local/Local` `n N Constant(7) 7` ほか `showRaw` 4 行 |
| `qq_ctx.scala` / `qq_ctx_bad.scala`（同上、コンパイルのみ） | マクロ実装のシグネチャ `c.Tree` / `c.Expr[T]` / `c.WeakTypeTag[T]`（精製 `Context` からも）と本体の `import c.universe._` + `q"..."`。scala-rs と実 scalac の両方が通し、classfile は `java -Xverify:all` でロード・検証される。異常系は reify できない形を名指しで診断 | （コンパイル結果のみ） |
| `qr_forms.scala`（`crates/cli/tests/quasi.rs`、scala-reflect.jar が要る） | reification の残りの形（実 scalac 2.13.16 と三者一致、`showRaw` まで）: `tq` / `pq` / `cq` 全体と `q` の型注釈 / eta / 型適用 / ブロック / `val` / `new` / `match` / 部分関数 / 関数リテラル / `this` / 代入 / `if` / 演算子名の符号化。末尾はオーバーロード集合の木ファクトリ（`Ident(TermName("x"))` ほか） | `expected/qr_forms.txt`（56 行） |
| `qr_forms_bad.scala`（同上） | パーサが区別ごと正規化してしまう形が、必ず名指しで診断されること（右結合演算子 / `else` の無い `if` / `_` プレースホルダ / by-name 型 / `..$` の混在 / `class` 定義 / 修飾つき `val`） | （診断のみ） |
| `lf2_lift.scala`（同上） | `Liftable`（実 scalac 2.13.16 と三者一致、`showRaw` まで）: `Tree` でない穴——リテラル / `Constant` / `Name`（項・型・パターン・名前枠）/ `Type` / `Symbol` / `..$` の要素——が標準インスタンスと同じ木になること | `expected/lf2_lift.txt`（29 行） |
| `lf2_ctx.scala`（同上、コンパイルのみ） | マクロ実装の中の `WeakTypeTag` / `Expr` の持ち上げ（slick の `mapToImpl` の形）と `symbolOf[T]` / `weakTypeOf[T]`。scala-rs と実 scalac の両方が通し、classfile は `java -Xverify:all` でロード・検証される | （コンパイル結果のみ） |
| `lf2_lift_bad.scala`（同上） | 持ち上げられない穴が型を名指しで診断されること（`File` / rank 0 の `List[Int]` / `..$` 越しの `Symbol`）と、`reify { … }` が `cannot expand reify { ... }` になること | （診断のみ） |
| `qr_forms_bad.scala`（同上） | パーサが区別ごと正規化してしまう形が、必ず名指しで診断されること（右結合演算子 / `else` の無い `if` / `_` プレースホルダ / by-name 型 / `..$` の混在 / `type` 定義） | （診断のみ） |
| `dq_defs.scala`（`crates/cli/tests/quasi.rs`、scala-reflect.jar が要る） | 定義の reification（実 scalac 2.13.16 と三者一致、`showRaw` まで）: `class` / `case class` / `trait` / `object` / `def` / 修飾つき `val`・`var`、`Modifiers` のフラグ、クラス・パラメータのアクセサ・フラグ、implicit 節、型パラメータと変位・境界、nsc が補う親、定義を含むブロック、匿名クラスの本体、名前・パラメータ・親・本体の位置の穴 | `expected/dq_defs.txt`（93 行） |
| `dq_defs_bad.scala`（同上） | 定義のうち落とせない 13 形が、必ず名指しで診断されること（自分型 / early definition / `private[X]` / by-name・可変長パラメータ / 手続き構文 / 型も本体も無い `def` / パターン定義 / 高階型パラメータ / context bound / `case` クラスの親の `..$` / 末尾でない implicit 節 / `macro` 定義） | （診断のみ） |
| `tt_tags.scala`（`crates/cli/tests/quasi.rs`、scala-reflect.jar が要る） | `TypeTag` / `WeakTypeTag` の materialization。`typeOf` / `weakTypeOf` / `typeTag` / `weakTypeTag` が作ったタグの `tpe` を、クラス・トレイト・9 つの基本型・`Unit` / `String` / `Any` / `AnyVal` / `Nothing` / `Null`・ライブラリのクラスについて印字し、`=:=` / `<:<` / `typeSymbol.fullName` まで**実 scalac 2.13.16 と三者一致**（`java -Xverify:all`） | `expected/tt_tags.txt`（30 行） |
| `tt_ctx.scala`（同上） | マクロ実装の中の `c.typeOf[T]` / `c.weakTypeOf[T]`（slick の `ShapedValue.mapToImpl` の形）。scala-rs と実 scalac の**両方**がコンパイルでき、classfile が JVM にロード・検証されること | （ロード確認のみ。展開には engine が要る） |
| `tt_tags_bad.scala`（同上） | 組めないタグの 7 形が、必ず名指しで診断されること（`List[Int]` / `Option[Foo]` / 入れ子クラス / `AnyRef` / 型パラメータ（`TypeTag` と `WeakTypeTag`）/ singleton 型）と、`no implicit: ... TypeTags$TypeTag` に戻らないこと | （診断のみ） |

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

| `mism7.scala` | 匿名クラス越しに as-seen-from されないメソッドのパラメータ、複合*型*の 2 つのクラス親、eta 展開の型パラメータ解決（`xs.map(identity)` と `identity _`）、抽象型の下界、モジュール → `apply` 付け替え時のシグネチャ完了、implicit 節だけが残った引数、不変な型引数の lub（**私有ランタイムと library の両モード**、`java -Xverify:all`） | `n1!` `n2!` `7` `true` `false` `a` `1` `1` `false` `true` `true` |

`mism7.scala` は `crates/cli/tests/mismatch7.rs` の
`mism7_fixture_runs_in_both_modes` から**両モードで**回し、どちらの stdout も
**実 scalac 2.13.16 の出力そのもの**（`tests/fixtures/expected/mism7.txt`）と
突き合わせます。同ファイルには最小形の受理テスト
（`a_captured_parameter_is_not_seen_through_the_anonymous_class` /
`a_compound_type_may_name_two_classes` /
`eta_expansion_solves_its_type_parameters_from_the_parameters` /
`an_abstract_types_lower_bound_is_a_subtype_of_it` /
`both_arities_of_index_where_are_supplied` /
`the_module_apply_redirect_completes_the_signature` /
`an_implicit_only_argument_is_filled_before_it_constrains_the_call` /
`the_lub_of_an_invariant_argument_is_an_existential`）も置いてあります。
`toMap` / `indexWhere` / `Vector` / `Seq(a, b)` は私有ランタイムに無いので、
そちらはこの受理テスト側だけです。逆に、緩めた規則が診断を飲み込まないことは
`mism7_mixin_bad.scala`（`class C extends A with B`）/
`mism7_lobound_bad.scala`（`def wrong[E, O >: E](x: O): E = x`）/
`mism7_capture_bad.scala`（匿名クラス自身の `this.next()` を外側の `T` として
渡す）で固定しています。実 scalac 2.13.16 も 3 件すべて拒否します。

| `mism8.scala` | オブジェクトの型エイリアスを期待型として解く多相呼び出し、期待型がタプルの成分へ届く（`protoTypeArgs`）、依存メソッド型（`def get[P <: Phase](p: P): Option[p.State]`）、`private[p]` を定義側から解決、`private[this]` なコンストラクタ引数の下から継承メンバを読む（**私有ランタイムと library の両モード**、`java -Xverify:all`） | `Box` `true` `true` `false` `l/r` `true` `false` `f` |
| `mism9_hk.scala` | 高階の適用（`F[B]`）の型パラメータを期待型から解く（`FlatMap[F[_]]` の入れ子 `flatMap` / `map`）、クラスの中に書いた `copy(…)` が共変型パラメータを推論し直す（`Cell[+F <: Option[Int]]`）（**私有ランタイムと library の両モード**、`java -Xverify:all`、期待出力は実 scalac 2.13.16 の stdout そのまま） | `42` `7` `n=5` `2` `z` `1` `-1` |
| `mism9_coll.scala` | ソート済みコレクションの `map` / `flatMap` / `collect`（`TreeSet` / `TreeMap`。静的型を `TreeSet[Int]` に絞っても実行時に `TreeSet` が返る）、`foreach[U](f: A => U)` に**関数値**を渡す（library モードのみ。期待出力は実 scalac 2.13.16 の stdout そのまま） | `TreeSet(2, 3, 4)` … `123` |
| `mism10_ctor.scala` | 親コンストラクタの実引数が**後ろで宣言された**メンバを名指す（`extends Base(Chain.of((column.toNode, ord)))`）、プライマリコンストラクタの既定引数が**クラス自身の型パラメータ**を名指す（`class Box[A](one: Chain[A] = Chain.empty[A])` / `class HkBox[F[_]](cell: Cell[F] = Cell.empty[F])`）（**私有ランタイムと library の両モード**、`java -Xverify:all`、期待出力は実 scalac 2.13.16 の stdout そのまま） | `n:asc` `7` `8` `2` `z` `empty` `given` |
| `mism10_coll.scala` | ソート済みマップの `collect`（リテラルの `{ case (k, v) => … }` から `K2` を解く／型注釈付きの `PartialFunction` 値でも `TreeMap` が返る。前に `Map.collect` があっても変わらない）（library モードのみ、期待出力は実 scalac 2.13.16 の stdout そのまま） | `Map(10 -> 1, 20 -> 2)` … `TreeMap(101 -> a, 102 -> bb)` |
| `mism11_hkopen.scala` | 未決定の**型構築子**が引数の期待型に届く形（`flatMap[F, T, D[_]](f: E => Qry[F, T, D])` の本体が `Qry[G, T, Box]` を返す。slick `Query.map` と同じ書き方）（**私有ランタイムと library の両モード**、`java -Xverify:all`、期待出力は実 scalac 2.13.16 の stdout そのまま） | `2` `n1` `4` |
| `mism11_coll.scala` | `it.grouped(2).map { case Seq(i, t) => … }`（要素は `Seq[B]`）と `mutable.ArrayBuilder.make[E]` を `Builder[E, Array[E]]` として返す（library モードのみ、期待出力は実 scalac 2.13.16 の stdout そのまま） | `List((1,2), (3,4), (5,6))` … `x-y` |
| `ab.scala`（`crates/cli/tests/anonbridge.rs`） | 消去後の `Block` / `If` / `Match` / `Try` の値をちょうど 1 回だけ箱詰めする（`agent/anonbridge`）：8 つのプリミティブのブロック本体、`abstract class` と名前付きクラスの実装、プリミティブ引数、型パラメータ 2 つ、`It[Cell[Int]]`、`val` 実装、SAM ラムダ、`while` / `if` / `match` / `try` 本体、捕捉した `var`、`val x: Any = { … }` / `id({ … })`、逆向きの `val n: Int = { val z: Any = …; … }`（**私有ランタイムと library の両モード**、`java -Xverify:all`、期待出力は実 scalac 2.13.16 の stdout そのまま） | `1` `2` `1.5` … `28` `29` |

`mism9_hk.scala` / `mism9_coll.scala` は `crates/cli/tests/mismatch9.rs` から回します
（`mism9_hk` は**両モード**、`mism9_coll` は library モードのみ。私有ランタイムには
`TreeSet` / `TreeMap` が無いので、`mism9_coll_without_library_is_error` で
**黙って通さない**ことも見ています）。同ファイルには最小形の受理テスト
（`mism9_hk_result_comes_from_the_expected_type` /
`mism9_hk_result_lines_up_with_a_class` /
`mism9_sorted_set_map_is_a_tree_set` /
`mism9_foreach_result_is_polymorphic` /
`mism9_bare_copy_reinfers_type_parameters` /
`mism9_user_copy_is_not_rewritten` /
`mism9_notype_is_not_reported_twice`）と、拒否テスト
（`mism9_hk_wrong_result_is_rejected` / `mism9_bad_is_still_rejected`）も置いてあります。

`mism10_ctor.scala` / `mism10_coll.scala` は `crates/cli/tests/mismatch10.rs` から
回します（`mism10_ctor` は**両モード**、`mism10_coll` は library モードのみ。
私有ランタイムには `TreeMap` / `TreeSet` が無いので、
`mism10_coll_without_library_is_error` で**黙って通さない**ことも見ています）。
同ファイルには最小形の受理テスト
（`mism10_sorted_map_collect_infers_its_key` /
`mism10_sorted_map_collect_after_a_plain_map` /
`mism10_ctor_default_names_the_class_type_parameters` /
`mism10_method_default_still_works` /
`mism10_parent_argument_sees_a_later_member`）と、拒否テスト
（`mism10_wrong_parent_argument_is_rejected` /
`mism10_wrong_ctor_default_is_rejected` / `mism10_bad_is_still_rejected`、
フィクスチャは `mism10_bad.scala`）も置いてあります。

`mism11_hkopen.scala` / `mism11_coll.scala` は `crates/cli/tests/mismatch11.rs` から
回します（`mism11_hkopen` は**両モード**、`mism11_coll` は library モードのみ。
私有ランタイムには `ArrayBuilder` / `ClassTag` が無いので、
`mism11_coll_without_library_is_error` で**黙って通さない**ことも見ています）。
同ファイルには最小形の受理テスト
（`mism11_grouped_element_is_a_seq` /
`mism11_two_argument_lambda_keeps_its_parameters` /
`mism11_array_builder_is_a_builder` /
`mism11_ordinary_element_types_are_unchanged`）と、拒否テスト
（`mism11_bad_is_still_rejected`、フィクスチャは `mism11_bad.scala`。
実 scalac 2.13.16 も同じ 3 件を拒否します）も置いてあります。

`mism12_lang.scala` / `mism12_lib.scala` は `crates/cli/tests/mismatch12.rs` から
回します（`mism12_lang` は**両モード**、`mism12_lib` は library モードのみ。
私有ランタイムには `IterableOnce` / `Factory` / `scala.math.BigDecimal` の
コンパニオンが無いので、`mism12_lib_without_library_is_error` で**黙って
通さない**ことも見ています）。多ファイルの原因は `tests/multi/mism12_basic.scala`
/ `mism12_memory.scala` / `mism12_relational.scala` / `mism12_use.scala` の 4 本
（`mism12_late_parent_type_alias_resolves`）で、単一ファイルでは再現しません。
同ファイルには最小形の受理テスト
（`mism12_constructor_bound_is_applied` /
`mism12_case_apply_from_inside_the_class` /
`mism12_big_decimal_overloads` /
`mism12_companion_inherits_its_implicits` /
`mism12_wildcard_and_contravariant_witness`）と、拒否テスト
（`mism12_bad_is_still_rejected`、フィクスチャは `mism12_bad.scala`。
実 scalac 2.13.16 も同じ 4 件を拒否します）も置いてあります。

`t2_lang.scala` / `t2_lib.scala` は `crates/cli/tests/tail2.rs` から回します
（`t2_lang` は**両モード**、`t2_lib` は library モードのみ。私有ランタイムには
`scala.math.Integral` が無いので、`t2_lib_without_library_is_error` で**黙って
通さない**ことも見ています）。同ファイルには最小形の受理テスト
（`t2_wildcard_import_brings_a_jar_class_implicits_into_scope` /
`t2_overridden_conversion_is_one_candidate` /
`t2_companion_implicits_are_supplied_from_the_pickle` /
`t2_primitive_companions_stay_out_of_the_view_search` /
`t2_nested_class_members_read_at_the_outer_parameters`）と、拒否テスト
（`t2_bad_is_still_rejected`、フィクスチャは `t2_bad.scala`。実 scalac 2.13.16 も
同じ 3 件を拒否します）も置いてあります。

`bf2_lazyzip.scala` / `bf2_lazyzip_bad.scala` は
`crates/cli/tests/buildfrom2.rs` から回します（`BuildFrom` / `LazyZip2` /
`IterableOps` は jar 側にしか無いので library モードのみ。私有ランタイムで
**黙って通さない**ことは `bf2_lazyzip_without_library_is_error` で見ています）。
`bf2_lazyzip.scala` の stdout は **real scalac 2.13.16 の出力そのもの**
（`expected/bf2_lazyzip.txt`）と一致し、`bf2_lazyzip_bad.scala` は実 scalac が
`Cannot construct a collection of type … based on a collection of type …` と
言う 3 行を、こちらも 3 件のエラーとして拒みます。同ファイルの単体寄りテストは、
**どの witness が答えるか**を実行結果で固定するためにあります:
`bf2_sorted_witness_does_not_answer_for_a_list` /
`bf2_sorted_witness_answers_for_a_treeset` /
`bf2_string_receiver_builds_a_string` /
`bf2_lazyzip_on_a_map_rebuilds_a_map` /
`bf2_lazyzip_at_an_abstract_element_type`（slick の
`values.lazyZip(…).map(mux).toSeq` の形）。

`mism8.scala` は `crates/cli/tests/mismatch8.rs` の
`mism8_fixture_runs_in_both_modes` から**両モードで**回し、どちらの stdout も
**実 scalac 2.13.16 の出力そのもの**（`tests/fixtures/expected/mism8.txt`）と
突き合わせます。同ファイルには最小形の受理テスト
（`an_expected_type_alias_is_seen_through_before_it_solves` /
`an_empty_repeated_parameter_is_unconstrained_not_unsolved` /
`a_splatted_argument_is_the_element_type` /
`xsource3_spells_the_splat_without_the_ascription` /
`the_expected_type_is_the_prototype_of_a_tuple_component` /
`a_prototype_that_does_not_fit_is_dropped` /
`a_dependent_method_type_reads_its_member_off_the_argument` /
`a_qualified_private_is_resolved_from_the_definition`）も置いてあります。
`Map` / `Vector` / `Set` / `Array` と可変長引数の本体は私有ランタイムに無いので、
そちらはこの受理テスト側だけです。緩めた規則が診断を飲み込まないことは
`mism8_access_bad.scala`（パッケージ境界の外からの `private[p]` と、継承元の無い
コンストラクタ引数）/ `mism8_dep_bad.scala`（`Option[Int]` を `Option[String]` に）/
`mism8_star_bad.scala`（`-Xsource:3` 無しの `f(xs*)`）で固定しています。
実 scalac 2.13.16 も 3 件すべて拒否します。

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

`agent/stmtval` スライス（最後の文が定義のブロック、op-assignment の優先順位、入れ子配列の要素型、`Array.ofDim` の型引数と `t(i) op= x`）のフィクスチャは接頭辞 `sv`（`sv_block` / `sv_opassign` / `sv_array` / `sv_update` / `sv_ofdim` / `sv_lib` / `sv_bad`）で、同じ理由から `crates/cli/tests/stmtval.rs` に置いています。`sv_block` / `sv_opassign` / `sv_array` / `sv_update` は**私有ランタイムと `--scala-library` の両方**で `java -Xverify:all` の下に走ります（1 と 3 は、以前の出力が verify を通らなかったことがそのものなので）。`sv_ofdim` と `sv_lib` は `Array.ofDim` / `List` / `Int.max` が実ライブラリにしか無いので library dual-run 専用で、`fixtures_sv_ofdim_without_library_is_error` / `fixtures_sv_lib_without_library_is_error` が`--no-scala-library` で**きちんと診断される**ことを見ます。`expected/*.txt` は 7 本とも real scalac 2.13.16 の stdout そのものです。`method_body_that_is_only_a_val_verifies` は報告そのままの 1 行（`object Main { def main(a: Array[String]): Unit = { val v = 1 } }`）をその場でコンパイルして走らせます。`sv_bad` は不変な受け手への op-assign がnsc と同じ 2 件の診断のままであること（`any2stringadd` のエラーに化けないこと）を固定します。

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
| `mc_update.scala`（`crates/cli/tests/mutcoll.rs`、library + 私有ランタイム dual-run） | `f(args) = v` → `f.update(args, v)`（SLS 6.15）: 配列、ユーザークラスの `update`、2 引数 `update`、選択された受け手（`h.b(1) = 41`）、ジェネリックな `update`、`Unit` 以外を返す `update`、`apply` の結果を受け手にする形 | `7,0,8` `15` `1:2:hi` `42` `3=x` `10` `5` |
| `mc_maps.scala`（`crates/cli/tests/mutcoll.rs`、library dual-run） | `mutable.Map` / `HashMap` / `LinkedHashMap` / `Set` / `HashSet` / `LinkedHashSet` / `ArrayBuffer` / `ListBuffer` / `Buffer` のコンパニオン `apply`（0 引数と varargs）と `empty`、`m(k) = v`、`update` / `getOrElseUpdate` / `remove` / `contains`、`+=` / `-=` / `++=` / `--=`、入れ子の `Map` への `nested("outer")("inner") = 42` | `List((d,4), (e,5))` ほか 16 行 |
| `mc_queue.scala`（`crates/cli/tests/mutcoll.rs`、library dual-run） | `mutable.Queue` / `Stack` / `ArrayDeque` / `PriorityQueue` / `TreeSet` / `TreeMap` / `ArraySeq` / `StringBuilder`: コンパニオン `apply`（0 引数含む）と `empty`、`new X[T]()`、`enqueue` / `dequeue` / `head` / `push` / `pop` / `top` / `append` / `prepend`、`Growable` / `Shrinkable` 演算子、`StringBuilder.newBuilder` | `1` `2` `2` `List(2, 3)` ほか 33 行 |
| `mc_maps_bad.scala`（`crates/cli/tests/mutcoll.rs`） | `m("a") = "wrong type"` / `m(1) = 2` は desugar 後の `update(String, Int)` で拒否、`update` を持たないクラスへの `n(0) = 7` は `value update is not a member of NoUpdate`、`q.enqueue("not an Int")` は要素型で拒否 | 4 errors |
| `mc_queue_bad.scala`（`crates/cli/tests/mutcoll.rs`） | `op=` が受け手のメンバーでないとき、nsc と同じく**1 つ**のエラー（2 行目が `Expression does not convert to assignment because receiver is not assignable.`） | 1 error |

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
これは次の `agent/companionkind` で直りました。
差し引きが 8 件にしかならないのは、拡張メソッドが解決するようになったことで
**その先で止まっていたカスケードが表に出た**ためです（`found: F required: F[Unit]`、
`no matching overload for (Function0[A])F` など。どれも同じ「素の `F`」が原因）。

### コンパニオンとクラスは別のシンボル（`agent/companionkind`）

`agent/catsyntax` が原因まで特定して戻した件（「jar のメンバの結果型が
素の `F` になる」）を**根から直しました**。
テストは `crates/cli/tests/companionkind.rs`、fixture の接頭辞は `ckind` です。

計測は `files=184 errors=518 files_with_errors=80` →
**`errors=443 files_with_errors=75`**（−75 件 / −5 ファイル）。
このスライスが狙った 3 種類は**全部消えています**:
`no matching overload for (Function0[A])F` は **8 → 0**、
`value flatMap is not a member of F` は **4 → 0**、
`value >> is not a member of F` は **4 → 0** です。

**1. `find_or_stub_java_class` が `X$` から `SymKind::Class` を作っていた。**

`find_or_stub_java_class` は、親リスト・ディスクリプタ・`InnerClasses` が
名指した JVM 名すべての入り口です。`cats/effect/kernel/Ref$` を渡されると
`java_simple_name` が末尾の `$` を落とし、`Ref` という名前の **`SymKind::Class`**
を作って `jvm_name` には**コンパニオンの**名前（`…/Ref$`）を入れていました。
1 つのシンボルが 2 つのものを兼ねるので、両方が壊れます。

- **トレイト `Ref` が自分のシンボルを持てない。** `ensure_class("cats.effect.kernel.Ref")`
  は「その名前のシンボルはあるが `jvm_name` が key と違う」で `None` を返すので、
  `Ref#update` の型は pickle ではなく **classfile の generic signature** から
  来ます。JVM のシグネチャは `F[Unit]` を書けず `TF;` としか書けないので、
  結果型が**素の `F`** になります。これが slick の
  `value >> is not a member of F` / `value flatMap is not a member of F` /
  `no matching overload for (Function0[A])F` の正体でした。
- **オブジェクトのメンバがトレイトに載る。** `Ref.of` / `Ref.const` が
  トレイト側のメンバとして入ります。

`$` 付きの名前は `install_java_module` と同じ形——`ModuleClass`（名前 `Ref$`）と
その `Module`（名前 `Ref`）——を作るようにしました。既存シンボルの探索も
`$` 付きなら `Module`、無しなら `Class` だけを見ます。

この経路が最初に踏まれるのは、cats-effect のパッケージオブジェクトにある
`val Ref = cats.effect.kernel.Ref` です。その getter のディスクリプタが
`Lcats/effect/kernel/Ref$;` なので、`import cats.effect.{Async, Ref, Resource}`
と書いた瞬間（slick の `BasicBackend.scala` の 5 行目）に
コンパニオン名でトレイト名のシンボルが入っていました。

`agent/catsyntax` のスクラッチではここで `Async[F]` から `FlatMap[F]` が
引けなくなり差し引き悪化しましたが、**現在の main では再現しません**
（同スライスが入れた `InnerClasses` の扱い・refinement の変換・
`give_stub_its_kinds` が前提になっています）。この変更単独で
`errors=518 → 494` です。

**2. prelude が持たない `scala.*` を pickle から読む。**

同じ「コンパニオン経由のメンバが classfile から読まれる」問題は、
**cats を一切使わなくても**出ます:

```scala
import scala.concurrent.Future
import scala.concurrent.ExecutionContext.Implicits.global
object Main { def main(a: Array[String]): Unit = println(Future(21)) }
```

`Future.apply` は本体を**名前渡し**（`=> T`）で取りますが、JVM の
generic signature には名前渡しが無く `Function0[T]` としか書けません。
`no matching overload for (Function0[T], ExecutionContext)Future[T] with
arguments (21)` になります。名前渡し自体は壊れていません
（`Option.getOrElse` / `scala.util.Try` / `Using.resource` はすべて通る）。
これらは**prelude が手で書いているクラス**で、`Future` は違うからです。

`adopt_binary_class` は `scala/` で始まる JVM 名を**すべて**拒んでいました。
理由は正しくて、prelude が組み立てたクラスを jar の形で作り直すと
（`ensure_class` が拒むのと同じ理由で）動いていたメンバが壊れます。
ただしその線引きは「`scala.*` かどうか」ではなく
「**prelude が作ったかどうか**」です。`install_prelude` 直後の
`st.symbols.len()` を `SymbolTable::prelude_end` に控え、それより後の
シンボルだけ pickle から読むようにしました。
実際に adopt されるのは `scala.concurrent.Future` / `Promise` /
`scala.collection.mutable.Growable` / `Builder` / `SeqOps` など
**prelude が名前を出していない 50 クラスほど**です。

**3. pickle の `this.type` は「載せたクラスの `this.type`」。**

2 の副作用で `scala.collection.mutable.Growable` が adopt されるようになり、
`b ++= xs` が `Growable[Int]` を返すようになりました（`ctacc_builder` が落ちた）。
`PickleSupply::conv` は `SigType::This` を **`self_ty`＝メンバを載せる
クラスを自分の型パラメータに適用したもの**に潰していました。メンバを
常にレシーバ自身のクラスに載せていた頃は正しく、**基底クラスをそれ自身として
完了させた瞬間に誤り**になります。`Type::ThisType(class_sym)` を返すように
して、レシーバへの読み替えは既にある `subst_as_seen_from` に任せます。
`Builder[Int, List[Int]]` に対する `Growable#++=` は `Builder` を返します。

**残っている隣接した穴**（このスライスでは直していません。
`agent/tail1` が本文を参照）:

- **jar のコンパニオンの入れ子クラス**が、コンパニオンを
  パッケージオブジェクトの `val` 経由で掴んだときに引けません。
  `object Box { final case class Const[A](get: A) }` を `import tiny2.alias.Box`
  （`val Box = tiny2.Box`）で使うと `Box.of` は通り
  `Box.Const` は `value Const is not a member of Box$` になります。
  main では `Box.of` すら通らないので**悪化ではありません**。
  slick の `Outcome.Succeeded(_)` / `Resource.ExitCase.Errored(e)`（6 件）が
  これです。`Outcome$Succeeded` という JVM 名は
  「クラス `Outcome` の入れ子」と「オブジェクト `Outcome` の入れ子」を
  区別しないので、直すには `InnerClasses` の `outer_class_info_index`
  （`parse_inner_classes` は今これを捨てています）か pickle が要ります。
  → 「`value X is not a member of Y$`（`agent/tail1`）」で、
  `outer_class_info_index` ではなく**別の根**（`qual.sym` が val 自体を指し、
  空の `jvm_name` から候補を組み立てていたこと）だと判明し、直りました。

### 同じ pickle 宣言のコピーが 2 つ（`agent/ambigmap`）

`agent/companionkind` が入れた退行の後始末です。テストは
`crates/cli/tests/ambigmap.rs`、fixture の接頭辞は `am` です。

計測は `files=184 errors=411 files_with_errors=72` →
**`errors=387 files_with_errors=70`**（−24 件 / −2 ファイル）。
`ambiguous overload` は **32 → 7**、うち `ambiguous overload for map` は
**25 → 0** です。

**症状**。`pkSyms.map { fs => quoteIdentifier(fs.name) }` のような
ごく普通の `map` が `ambiguous overload for map` になっていました。

**原因は「同じ宣言のコピーが 2 つある」ことで、`map` 固有の話ではありません。**

`map` は prelude が書き出していません。宣言しているのは
`scala.collection.IterableOps` で、`Seq` も `IndexedSeq` も `Set` も
そこから継承します。`PickleSupply::complete_named` は
**聞かれたクラスにメンバを載せます**（そこが typer が次に引く場所だからです）。
つまり `IterableOps.map` のコピーが**どのクラスに載るかは、どのレシーバが
最初に聞いたか**＝コンパイル対象のプログラム次第です。

`scala.Seq` のレシーバが先に聞くと `scala.collection.immutable.Seq` に
1 つ載り、その後 `scala.collection.IndexedSeq` のレシーバが聞くと
（`immutable.Seq` はその親ではないので何も見つからず）そちらにも 1 つ載ります。
`scala.IndexedSeq`（＝`immutable.IndexedSeq`）は**その両方を親に持ち、
かつ 2 つは互いに親子ではありません**。したがって

- `drop_overridden` は「サブクラス側が親のメンバを override している」
  形に当てはめられず、
- 2 つは書き換えられた語彙が違うだけ（`Seq[B]` と `IndexedSeq[B]`）なので
  specificity でも決着せず、

`xs.map(f)` はすべて `ambiguous overload` になります。`map` が最も目立った
だけで、`flatMap` / `filter` / `partition` / `foldLeft` も同じ形でした
（fixture `am_pickledup.scala` は 5 個とも踏みます）。

`agent/companionkind` 以前は、たまたま `scala.collection.Iterable` が最初に
聞かれていて、コピーが 1 つしか出来ていませんでした。pickle 由来のクラスが
50 個ほど増えて**聞かれる順番が変わった**のが引き金です。バグ自体は前から
そこにありました。

**直し方**。nsc から見れば `IterableOps.map` は 1 つです。そこで
`Symbol::pickled_origin` に「このコピーがどの pickle 宣言を指しているか」＝
**宣言元クラス＋メソッド名＋erased パラメータディスクリプタ**を記録します
（**載せたクラスは入れません**。それが違うからこそ重複するので）。
`Check::drop_overridden` は候補集合の先頭で
`collapse_pickled_copies` を通し、同じ `pickled_origin` のコピーは
最初の 1 つだけ残します。`lookup_member` は親を後ろから辿るので、
先頭に来るのはレシーバに一番近いコピー（`immutable.IndexedSeq` なら
`collection.IndexedSeq` 側、つまり結果型が `IndexedSeq[B]` の方）です。

`pickled_origin` が空のシンボル（prelude・ソース・classfile 由来）は
一切触りません。**名前ではなく宣言で束ねている**ので、本物のオーバーロードは
2 つのまま残り、決着が付かなければ従来どおり `ambiguous overload` を出します
（`am_pickledup_bad.scala`）。

### `StringOps` を jar から読む（`agent/stringops8`）

`"abcdef".zipWithIndex` / `.sliding(2)` / `.groupBy(identity)` / `.sortBy(…)` /
`.collect { … }` などが軒並み `is not a member of String` になっていました。
原因は `StringOps` が **prelude に手書きされていた**ことで、1 メソッド足りない
たびに手で足す形になっており、穴が延々出る構造でした。

**結論: jar から読む形に寄せられます。そしてそれが本筋でした。**

読む仕組み自体は既にありました。`crates/pickle`（`ScalaSignature` リーダー）と
`crates/typer/src/pickle_supply.rs`（「prelude に無いメンバだけ、必要になった
ときに pickle から補完する」）が揃っており、`List` などはこの経路で穴が
埋まります。欠けていたのは**接続**だけでした:

- `Check::supply_from_pickle` は**レシーバ**の型にしか聞いていませんでした。
  `"abc".groupBy(f)` のレシーバは `java.lang.String` で、これは
  `ScalaSignature` を持たないので、必ず空手で帰ってきます
  (`[pickle] #groupBy: asking String (java/lang/String)`)。
- 暗黙変換の候補探索 `Check::search_extension` は、変換の**結果**
  （`StringOps`）に対して `lookup_member` を引くだけで、pickle には一切
  聞いていませんでした。

そこで `search_extension` に「prelude が何も持っていないときだけ、変換結果の
pickle に聞く」1 か所を足しました。`pickle_supply` の 3 原則（prelude が常に
勝つ／表現できないものは供給しない／先読みしない）はそのままです。

これが効くのは、prelude が `StringOps` の**クラスの殻**（`parents = [AnyVal]` と
`ctor_fields = [repr: String]`）を持ち続けるからです。`SymbolTable::is_value_class`
がこの 2 つで決まり、backend の `invoke_value_extension` →
`value_extension_desc` が「シンボルの型から descriptor を作り、レシーバの
`Ljava/lang/String;` を先頭に足して `<name>$extension` を invokestatic する」
という 2.13 の規約そのままなので、pickle が入れたメンバも**そのまま正しく
リンクします**（pickle 由来のシンボルは classfile から読んだ erased
descriptor を `jvm_name` に持ち、`method_desc_from_sym` がそれを優先します）。
`ensure_class` が既存シンボルを作り直さない制約とも衝突しません。

もう 1 つ、`Predef.wrapString` に `low_priority` が立っていませんでした。javap
で確認すると `wrapString` は `scala.LowPriorityImplicits`、`augmentString` は
`Predef$` の宣言なので、両方がメンバを持つときは `StringOps` が勝つのが nsc の
規則です。`search_extension` のコメントはその規則を書いていたのに、フラグが
立っていないので実行できていませんでした（立っていたのは `intWrapper` 系だけ）。
これを直すまで `groupBy` は「`StringOps` と `WrappedString` の両方が供給して
曖昧」で落ちていました。

**pickle で表現できない分だけ**を `crates/typer/src/prelude_stringops8.rs` に
手書きしました。2.13 の `StringOps` は**戻り型だけが違うオーバーロード**を
持ち、`erased_desc` は*引数*の erasure でメンバを引くので、2 本見つけて
区別できず供給を断ります（これは `pickle_supply` として正しい判断です）。
ところが断られたメンバは下位の `wrapString` に流れて `WrappedString` として
返るので、`"abcdef".collect { case c if c > 'c' => c }` が scalac の `"def"`
ではなく `Vector(d, e, f)` になっていました。**間違った型は無いより悪い**ので、
`prelude_strmap.rs` の `map` と同じく**2 シンボル**で宣言しています:

| 手書きに残したもの | 理由 |
|---|---|
| `collect` × 2 | 戻り型だけのオーバーロード（`String` / `IndexedSeq[B]`） |
| `withFilter` と `StringOps$WithFilter` | 戻り値が普通のクラスで、その `map` も同じ二重 erasure |
| `addString` × 3 | `mutable.StringBuilder` の pickle 形状が合わない |
| `apply(Int): Char` | classfile 側に対応する instance メソッドが無い |

`collect` のオーバーロード解決には、`map` が使っている `Infer.pretypeArgs`
相当の事前型付けを `PartialFunction` にも広げました（`agreed_pf_param`）。
PF のパラメータは `Type::Function` ではなく**クラス**なので
`agreed_lambda_params` が降りてしまい、case ブロックの本体が何を返しても
より specific な `Char` 版が勝っていました。

`"abcdef"(1)` の添字構文も直しました。`s.apply(1)` は通るのに `s(1)` が
`value apply is not a member of String` になっていたのは、`Apply` の経路が
「レシーバが `Type::Class` なら `apply` を探す」だけで、暗黙変換を試して
いなかったためです（`retry_apply_extension`）。

`withFilter` は `is_with_filter_ty` に `StringOps$WithFilter` を足すまで、
結果型が**レシーバ**（`StringOps` = erasure は `String`）に上書きされ、
続く `.map` が実物の `StringOps$WithFilter` に `checkcast java/lang/String`
を出して `ClassCastException` になっていました。

dual-run: `so8`（期待値は**実 scalac 2.13.16 の出力そのもの**、`java -Xverify:all`
で一致）。異常系は `so8_bad`（戻り型だけのオーバーロードが「解決はする」だけ
では不十分で、`Int` を返す case ブロックは `IndexedSeq[B]` を選ぶので `String`
に束縛できないこと）。**私有ランタイム（`--no-scala-library`）には `StringOps`
自体が無い**ので、`so8.scala` は 40 件の診断になります（黙って通しません）。
slick: `errors=518 → 516`。
### 引数の基底型と自動タプル化（`agent/hkinfer`）

実 scalac との差分で見つかった、引数の適合まわりの独立した 2 件です。
テストは `crates/cli/tests/hkinfer.rs`、fixture の接頭辞は `hk` です。

**1. 引数の基底型から型引数が推論できていなかった。これは高階固有ではありません。**
報告は `def use[F[_]](c: C[F])` に `object OC extends C[Option]` を渡す形でしたが、
**1 階でも同じように落ちます**：

```scala
trait D[A]; object OD extends D[Int]
def u[A](d: D[A]): A = ???
u(OD)   // error: no matching overload for (D[A])A with arguments (OD$)
```

分けているのは kind の階数ではなく、**引数が単一型かどうか**でした。`new LC`
（クラスのインスタンス）は元から通り、`OC` / `OD`（オブジェクト）だけが落ちます。
`unify_tparam_all` は引数を `align_to_param_class` でパラメータのクラスに
直してから単一化しますが、その `align_to_param_class` と `base_type_instance` が
**`Type::Class` しか受け付けていなかった**ためです。オブジェクト参照の型は
`Type::ModuleRef` なので、そこで素通りしていました。

nsc の `Types.baseType` は単一型を**それが広がる先**を通して読みます。同じように
`base_type_instance` に `ModuleRef` / `ThisType` / `SingleType` / `Annotated` を
足し、`align_to_param_class` もその 3 つの単一型を通すようにしました。
`this.type` を返すメソッドの結果（`SelfInt.me`）や、`val sv: SelfInt.type = SelfInt`
のようなパス型も同じ経路で通ります。

基底型が**在る**だけでは足りず、その型引数が合うことは今までどおり要求します
（`hk_base_bad`: `object OD extends D[Int]` は `D[String]` ではなく、`A` を `Int` に
固定するので `two(OD, "s")` も通りません。実 scalac も同じ 2 件を出します）。

**2. 自動タプル化（SLS 6.6）がオーバーロードされた呼び先で効かなかった。**
`retry_tupled_args` そのものは前からありましたが、
「nsc はオーバーロードにタプル化しない」として呼び先が多重定義なら降りていました。
`println` はまさに多重定義なので `println(1, "a")` が通りません。

実 scalac に当てて確かめた順序は次のとおりです。

- **書いた引数個数を取る候補が 1 本でもあれば、タプル化しない。**
  `def c(x: String, y: String)` / `def c(t: (Int, String))` に `c(1, "x")` は
  scalac でも `type mismatch; found: Int(1) required: String` です（`hk_tuple_bad`）。
- どの候補もその個数を取らないときだけ、引数を 1 個のタプルに詰めて**もう一度だけ**
  型付けし直します（`println(1, "a")` → `println((1, "a")): Any`）。
- 詰め直した後は普通のオーバーロード解決が走ります。`def b(x: Any)` /
  `def b(t: (Int, String))` に `b(1, "x")` は、より特化した `b((Int, String))` が
  勝ちます（scalac も `bTup`）。
- 通常の解決が先に成功するならそちらが勝つのは元のままです。`def h(a: Int, b: Int)` /
  `def h(t: (Int, Int))` に `h(1, 2)` は `two-args` です。

判定は `some_alt_takes_arity`（`check.rs`）で、可変長パラメータと省略可能な
末尾デフォルトも数えます。**逆向きの展開はしません**: `def g(a: Int, b: Int)` に
`g((1, 2))` はエラーのままです（`hk_tuple_bad`）。

2 要素に限りません。`Tuple3` … `Tuple22` まで同じ経路で、要素は普通の式です
（`hk_tuple_lib` で `println(Red == Red, Red.toString, Custom("a") == Custom("a"))`、
`println(Set(1,2) & Set(2,3), Set(1,2) | Set(3), Set(1,2) diff Set(1))`、
`println(f.isDefinedAt(1), f.applyOrElse(-1, (_: Int) => "neg"))`、
4 要素・6 要素・22 要素を実 scalac と突き合わせています）。23 要素以上は
`Tuple23` が無いので scalac と同じくエラーです。

**警告は出しません。** nsc は自動タプル化のときに警告を出しますが、2.13.16 では
`-deprecation` ではなく **`-Xlint:adapted-args`** です
（`adapted the argument list to the expected 2-tuple: add additional parens instead`）。
scala-rs はこの lint を持っていないので、**警告なしで受理します**。

| fixture | 何を固定するか | 期待出力 |
| --- | --- | --- |
| `hk_base.scala`（`crates/cli/tests/hkinfer.rs`、私有ランタイム・library dual-run） | 引数の基底型から型引数を解く: オブジェクト（1 階 `Box[Int]` / 高階 `Ctor[IdBox]`）、クラスのインスタンス、`this.type` を返すメソッドの結果、`val sv: SelfInt.type` のパス型 | `7` `s` `3` `5` `6` `8` |
| `hk_base_lib.scala`（`crates/cli/tests/hkinfer.rs`、library dual-run） | 報告そのままの形: `object OC extends C[Option]` / `class LC extends C[List]` を `def use[F[_]](c: C[F])` に。明示指定 `use[Option](OC)` と 1 階の `firstOrder(OD, 42)` も | `Some(1)` `List(1)` `Some(1)` `42` |
| `hk_base_bad.scala`（`crates/cli/tests/hkinfer.rs`、異常系・両モード） | 基底型の型引数は合っていなければならない（`need(OD)` / `two(OD, "s")`）。実 scalac も 2 件 | （コンパイルエラー 2 件） |
| `hk_tuple.scala`（`crates/cli/tests/hkinfer.rs`、私有ランタイム・library dual-run） | 自動タプル化の順序: 単一メソッド（`f` / `s`）、同じ個数の候補が勝つ（`h`）、多重定義でタプル化してから最特化で選ぶ（`a` / `b`） | `1` `3z` `two-args` `aAny` `bTup` |
| `hk_tuple_lib.scala`（`crates/cli/tests/hkinfer.rs`、library dual-run のみ） | `println(1, "a")` が `(1,a)` を出す。`Tuple3` / `Tuple4` / `Tuple6` も同じ（`==`・拡張メソッド・`PartialFunction` のメンバを要素に持つ形も）。私有ランタイムの `Tuple2` には自前の `toString` が無く、`println((1, "a"))` と括弧を書いても同じように差が出るので jar 限定 | `(1,a)` `1` `(true,Red,true)` `(3,4)` `(Set(2),Set(1, 2, 3),Set(2))` `(true,neg)` `(1,2,3,4)` `(1,b,3.0,true,c,6)` |
| `hk_tuple_bad.scala`（`crates/cli/tests/hkinfer.rs`、異常系・両モード） | タプル化が**通してはいけないもの**: `g((1, 2))` の逆展開、タプルでないパラメータ（`one(1, 2)`）、引数なしメソッド（`zero(1, 2)`）、同じ個数の候補があるとき（`c(1, "x")`）。実 scalac も同じ 4 件 | （コンパイルエラー 4 件） |

計測は `files=184 errors=518 files_with_errors=80` → **`errors=517
files_with_errors=80`**。エラーの多重集合の差は**ちょうど 1 件**で、
消えたのは `type mismatch; found: DBIOAction[R, S, Effect with E with E2]
required: DBIOAction[R, S, E]`、**増えたものはありません**。slick には
`object` を型クラスの証人として渡す形も、引数個数の合わないオーバーロード呼び出しも
ほとんど出てこないので、この 2 件は slick の数字ではあまり動きません
（どちらも実 scalac との差分から出てきたものです）。

### メソッド本体の中の宣言（ローカル trait / class / object、`agent/localtrait`）

`trait` / `class` / `object` は**メソッド本体（やブロック、`if` の枝、ラムダの中）**
にも書けます。トップレベルの宣言では正しく動いていた 2 つの仕組みが、
ここでは丸ごと抜けていました。**どちらも型検査は通り、実行時に落ちる／黙って
間違ったコードになる**種類の穴です。

**1. 具象メンバの収集がテンプレートの中しか歩いていなかった。**
`collect_trait_impls` は `PackageDef` / `ClassDef` / `ModuleDef` の直下だけを
辿っていたので、メソッド本体の中の `trait` は登録されず、`T$class` 実装クラスも
mixin フォワーダも**1 本も出ませんでした**。

```scala
def main(a: Array[String]): Unit = {
  trait L { val v: String; lazy val w = v + "!"; def plain = v + "?" }
  class LC extends L { val v = "x" }
  println(new LC().plain)   // AbstractMethodError
}
```

`javap -p` で見ると `Main$LC` は `v()` しか持っていませんでした（`plain()` も
`w()` も interface の abstract 宣言のまま）。`lazy val` だけでなく素の `def` も
落ちていたのはこのためです。収集を汎用の子ノード走査（`for_each_term_child`）に
替えて、宣言がどこにあっても拾うようにしました。トップレベルと同じ経路に乗るので、
線形化・`super` アクセサ・`abstract override`・trait `val` の mixin setter・
`lazy val` の複製もそのまま効きます。

**2. ローカル宣言に索引が付いていなかった。** ローカルな名前は 1 つのメソッドの
中でしか一意ではありません。nsc は `Main$Same$1` / `Main$Same$2` と索引を振りますが、
こちらは両方 `Main$Same` という classfile を出していて、**後から出た方が先の方を
黙って上書き**していました（`dupA()` が `dupB` を印字する）。`jvm_for_current` で
「クラスに届くまでに項（メソッドや `val` の初期化子）を跨いだか」を見て、跨いだ場合
だけ `$N` を付けます。`case class` のコンパニオンは、クラスが引いた索引を
そのまま使います（別に引くと `Main$P$1` と `Main$P$2$` がずれる）。

**ローカル trait が外側のローカルを捕捉する場合。** trait にはコンストラクタが
無いので、ローカル `class` のように捕捉値をコンストラクタ引数にはできません。
nsc は捕捉ごとに trait のアクセサ（`outerVal$1()`）を立てて実装クラスに持たせます。
こちらも同じ形で、

- `anon_capture` が trait の捕捉を**それを mixin する全クラスへ伝播**させ
  （既存の「ローカル class の捕捉はコンストラクタ引数＋フィールド」の仕組みに乗る）、
- interface に捕捉ごとの abstract アクセサを宣言し、
- 実装クラスはそのアクセサを自分の捕捉フィールドから実装し、
- `T$class` のメソッド本体・`$init$` は入口で `$this` 経由に `invokeinterface`
  して普通のローカルスロットに落とす（`emit_trait_capture_prologue`）。

アクセサ名は捕捉されたシンボル ID で作ります（`n$4492`）。位置での採番だと、
同名の別ローカルを捕捉する 2 つの trait を 1 つのクラスに mixin したときに
衝突するためです。捕捉した `var` は既存の `scala.runtime.*Ref` boxing に乗ります。

**ローカル class が*トップレベルの* trait を実装する形は元から動いており**、
`lt1.scala` で回帰させないようにしています。逆（トップレベルのクラスがローカル
trait を実装する）は、ローカル trait がスコープの外から見えないのでそもそも
書けません（ただし**こちらは今のところ `Main.Local` を拒否できていません** —
Remaining の「ローカル宣言がスコープの外から見えてしまう」を参照。
ローカル宣言の索引とは別の、名前解決側の既存の穴です）。

| fixture | 何を固定するか | 期待出力 |
| --- | --- | --- |
| `lt1.scala`（`crates/cli/tests/localtrait.rs`、私有ランタイム・library dual-run） | ローカル trait の `val` / `lazy val` / `def`、interface 経由の呼び出し、ローカル class がトップレベル trait を実装、`new T {}` と `new C with T`、ブロック内・`if` の枝・ラムダ本体・`match` の case・`while` 本体・`try` ブロックでの宣言 | `x?` `x!` `F` `x?` `x!` `top:lc` `q` `q` `blockT` `ifU` `lam3` `mm` `w0` `w1` `y` |
| `lt2.scala`（同上） | ローカル trait のスタッキングと線形化（`B with C` / `C with B`）、`abstract override`、ローカル trait がローカル trait を継承、`override` と `super`、ローカル trait がトップレベル trait を継承、型パラメータを取るローカル trait、自分型 | `C(B(A))` `B(C(A))` `mid(late)` `ab` `a` `Over.m/T.m` `T.label` `top/L` `top` `box:7` `7` `hi g` |
| `lt3.scala`（同上） | ローカル trait による捕捉: `val` / メソッド引数 / `var`、trait `val` の右辺での捕捉、継承した trait の捕捉 | `cap42s` `cap42s` `p7` `1` `2` `13` `base!/base` `base!` `hio` |
| `lt4.scala`（同上） | 2 つのメソッドが同名の `trait` / `class` / `object` を宣言、`if` の枝で外側のローカル class 名を隠す | `Aaoa` `Bbob` `P1` `Q2` |
| `lt1_bad.scala`（同上、異常系） | ローカルな mixin にもトップレベルと同じ検査が効く（`illegal inheritance; superclass Other is not a subclass of the superclass Sup`）。実 scalac も同じ 1 件 | （コンパイルエラー 1 件） |

`javap` の比較テストも入れています（`local_trait_gets_mixin_forwarders_and_impl_class`
／`same_named_local_declarations_get_separate_classfiles`
／`local_trait_captures_go_through_an_accessor`
／`implementing_class_members_match_scalac`）。**メソッドの過不足は実行出力だけでは
見逃す**（誰も呼ばないフォワーダが欠けていても stdout は一致する）ので、最後の 1 本は
`/tmp/scala-2.13.16/bin/scalac` があるときに実 scalac を走らせ、実装クラスの
public メソッド集合が nsc のそれを**包含する**ことを見ます。比較の前に、こちらと nsc で
表記だけが違う 2 つを正規化します（ローカル索引を落として
`Main$L$1$_setter_$fixed_$eq` = `Main$L$_setter_$fixed_$eq`、
`super` アクセサの所有者エンコードを落として `B$$super$name` = `Main$B$$super$name`）。

slick の計測は `files=184 errors=411 files_with_errors=72` で**前後とも同じ**です
（型検査は通ってしまうバグなので、エラー数は元から動きません）。
### `Unit` の引数と `scala.runtime.BoxedUnit`（`agent/unitbox`）

`Unit` が `V` になるのは**メソッドの戻り値だけ**です。値が実際に置かれる場所
——**パラメータ・フィールド・配列要素・型引数**——では nsc は
`scala/runtime/BoxedUnit` に erase し、唯一の値 `()` は `BoxedUnit.UNIT`
シングルトンです。そこに `V` を書くのは「nsc と違う」どころか
**ディスクリプタとして不正**で、クラス全体がロードできません。

```
java.lang.ClassFormatError: Method "f" in class Main has illegal signature
  "(V)Ljava/lang/String;"
```

`def f(x: Unit)`、`class C(val u: Unit)`、`var w: Unit`、
`case class K(k: Unit, …)`、`Array[Unit]` がすべてこれで落ちていました。

**`javap -v -p` で実 scalac 2.13.16 から読み取ったこと**（`Main.scala` を
そのままコンパイルして確認）:

- `def f(x: Unit): String` は `(Lscala/runtime/BoxedUnit;)Ljava/lang/String;`。
- `f(())` は `getstatic scala/runtime/BoxedUnit.UNIT` を積む。
- `f(g())`（`def g(): Unit`）は `invokevirtual g:()V` の**あとに**
  `getstatic UNIT`。`V` の呼び出しは何も残さないので、引数はここで作る。
- `val u: Unit = ()` は**スロットを取る**（`LocalVariableTable` に
  `u Lscala/runtime/BoxedUnit;`）。
- `class C(val u: Unit)` のコンストラクタは `(Lscala/runtime/BoxedUnit;)V`。
  `var w: Unit` はフィールドが `Lscala/runtime/BoxedUnit;` で、getter は
  `getfield; pop; return`（戻り値は `V`）、setter は `w_$eq(BoxedUnit)`。
- `case class K(k: Unit)` は `apply(BoxedUnit)` / `copy(BoxedUnit)` /
  `unapply` が `Option<BoxedUnit>`。
- `List((), ())` は `anewarray scala/runtime/BoxedUnit` を作って
  `ScalaRunTime.wrapUnitArray` に渡す。`Array[Unit]` は
  `[Lscala/runtime/BoxedUnit;`（`Array[Nothing]` だけは例外で
  `[Ljava/lang/Object;`）。`Nothing` のパラメータは `Lscala/runtime/Nothing$;`。
- `val any: Any = ()` は `getstatic UNIT; astore`。`println` が `()` と出るのは
  `BoxedUnit.toString` です。
- ふつう `Unit` の式はスタックに何も残しませんが、`def id[A](a: A): A` を
  `id(())` と使うと `(Object)Object` の呼び出しが**参照を残す**ので、
  捨てる位置では nsc も `pop` を出します（`invokevirtual id; pop`）。
- `x.asInstanceOf[Unit]` は結果が `Unit` の式なので何も残しません（nsc は
  キャスト自体を落として、使う位置で `UNIT` を作ります）。
  `x.isInstanceOf[Unit]` は `instanceof scala/runtime/BoxedUnit` です。

実装は 3 つに分かれます。

1. **ディスクリプタ**: `jvm_desc_val`（値の位置）を `jvm_desc`（結果の位置）と
   分けました（`crates/backend/src/gen.rs`）。メソッドのパラメータ、フィールド、
   配列要素はすべて前者です。
2. **スロット**: `Unit` のパラメータは JVM 上では本当に渡ってくるので 1 枠を
   占めます（`Frame::alloc_param`）。取らないと**その後ろのパラメータが全部
   ずれます**。シンボル自体は void ソートのままなので、読んでも何も積まれず、
   ほかの `Unit` 式と同じ扱いのままです。値は `BoxedUnit.UNIT` しかあり得ない
   ので、必要な位置で作り直します（`fill_boxed_unit_slot` / `adapt_unit_arg`）。
   転送だけする合成メソッド（forwarder・bridge・setter・`case class` の
   `apply`/`copy`）は逆に「渡されたものをそのまま流す」ので `jvm_slot_sort`
   （＝ `Unit` は `Ref`）を使います。捨てる位置で参照が残るのは
   **このコンパイル単位で定義したメソッド**のときだけ数えます
   （`unit_stat_leaves_ref`）: `Using.resource` / `Breaks.catchBreak` /
   `ArrayOps` はそれぞれ専用の emitter が既に値を落としているので、
   もう一度 `pop` するとスタックが枯れます。
3. **私有ランタイム**: `crates/backend/src/runtime.rs` が
   `scala/runtime/BoxedUnit`（`UNIT` / `TYPE` / `equals` は同一性 /
   `hashCode` は 0 / `toString` は `"()"`）と `scala/runtime/Nothing$`
   （`Throwable` を継承する abstract クラス。呼べないメソッドでも
   パラメータのクラスは verifier が解決するので要ります）を出すように
   なりました。これで
   `emit_box(Unit)` は**両モードとも** `getstatic UNIT` になり、
   `--no-scala-library` でも `println(x: Any)` が `()` を出し、
   `case () =>` が `null` に当たらなくなりました（`agent/patbind` の残件）。
   `library_abi` で分岐していた `Unit` のボックス表現は全部消えています。

| fixture | 何を固定するか | 期待出力 |
| --- | --- | --- |
| `ub_param.scala`（`crates/cli/tests/unitbox.rs`、両モード dual-run） | `Unit` のパラメータ: `f(())`、`f(g())`、`middle(Int, Unit, String)`（`Unit` の後ろの引数がずれない）、2 個続く `Unit`、コンストラクタ `val u: Unit`、クラスのメソッド、`Nothing` のパラメータ（`never(scala.runtime.Nothing$)`） | `got` `got` `s1` `two` `()` `()` `42` `x7` |
| `ub_field.scala`（`crates/cli/tests/unitbox.rs`、両モード dual-run） | `Unit` のフィールド: `val`/`var`/`lazy val` をクラス・`object`・trait で、getter/setter・ローカル `var`・`Any` 代入 | `()` ×12 |
| `ub_case.scala`（`crates/cli/tests/unitbox.rs`、両モード dual-run） | `case class K(k: Unit, n: Int)`: `toString` / `equals` / `hashCode` / `copy` / `productElement` / コンパニオンの `apply` と erased `apply(Object,Object)` ブリッジ / パターン抽出 | `K((),3)` `()` `3` `K((),4)` `true` `false` `2` `()` `3` `true` `U(())` `matched` `()` `3` |
| `ub_mixin.scala`（`crates/cli/tests/unitbox.rs`、両モード dual-run） | trait / 抽象クラス / 値クラス越しの `Unit` メンバ: インタフェースのメソッド、mixin forwarder、`T$class` の静的実装、erasure ブリッジ、抽象 `var` の setter、`Int => Unit` のラムダ | `()` ×4 `m` `d` `m` `d` `()` `sub` `3` `()` |
| `ub_call.scala`（`crates/cli/tests/unitbox.rs`、両モード dual-run） | 普通の呼び出し経路を通らない引数: `this(…)` の委譲、trait の `$init$`、デフォルト引数、名前付き引数、2 つ目のパラメータリスト、by-name の `Unit`、`Unit` を 2 個続けて取るメソッド、`try`/`catch`・`match` の本体、再帰 | `9` `()` `()` `0` `7` `()` `()` `iv` `d1` `d3` `d4` `n5` `c6` `by` |
| `ub_super.scala`（`crates/cli/tests/unitbox.rs`、両モード dual-run） | **スーパーコンストラクタ**の `Unit` 引数（`class D extends B((), 5)`、`case object Asc extends Dir(())`）、trait の抽象メンバ、メソッド内の `def` | `D5` `()` `5` `()` `E` `l2` |
| `ub_boxed.scala`（`crates/cli/tests/unitbox.rs`、両モード dual-run） | `()` は `null` ではなく `BoxedUnit.UNIT`: `id(())`、`String.valueOf(())`、`== ()`、`case () =>` が `null` に当たらない、`toString` / `hashCode`、捨てる位置の `id(())` を `pop` する（ループの後方辺でスタック高が合う）、`asInstanceOf[Unit]` / `isInstanceOf[Unit]` | `()` `()` `true` `false` `unit` `null` `other` `()` `()` `0` `2` `()` `2` `true` `false` |
| `ub_typearg.scala`（`crates/cli/tests/unitbox.rs`、library dual-run のみ） | 型引数位置の `Unit`: `List[Unit]` / `Array[Unit]`（`[Lscala/runtime/BoxedUnit;`）/ `Option[Unit]` / `Seq[Unit]` / `Tuple2` / `Map[String, Unit]` / `Set[Unit]` / `PartialFunction[Int, Unit]` / `Unit*` の可変長 / 結果が `Unit` のラムダ / `(Unit, Int) => String`。私有ランタイムには可変長の `List.apply` / `Array.apply` も `Map` / `Set` / `Function2` も無いので jar 限定 | `3` `()` `true` `2` `List((), ())` `2` `()` `Some(())` `()` `()` `((),1)` `List((), ())` `2` `()` `Map(a -> ())` `Set(())` `Some(())` `()` `()` `f1` |
| `ub_sepdef.scala` + `ub_sepuse.scala`（`crates/cli/tests/unitbox.rs`、`-cp` 越しの分割コンパイル） | 別コンパイル単位から `Unit` メンバを使う: classfile の `Lscala/runtime/BoxedUnit;` を `Unit` に戻して読めること（`case class LK(k: Unit, n: Int)` の `apply` / パターン抽出、`class LC(val u: Unit)`）。クラス名をわざと `L` で始めてある（下記） | `libgot` `s1` `LK((),2)` `()` `()` `()` `m` `()` `2` |
| `ub_param_bad.scala`（`crates/cli/tests/unitbox.rs`、異常系） | erase の都合で typer が緩まないこと: `def g(s: String)` に `g(())` はエラー（実 scalac も `type mismatch; found: Unit required: String`） | （コンパイルエラー） |

ディスクリプタそのものも `javap -p` で見ています
（`ub_param_descriptors_use_boxed_unit` / `ub_typearg_array_descriptor`）。
実行だけでは足りません——`(V)` はクラスがロードできないので、
「たまたま動いた」と区別が付かないからです。私有ランタイムが
`scala/runtime/BoxedUnit` と `scala/runtime/Nothing$` を実際に出している
ことも見ます（`private_runtime_emits_boxed_unit` /
`private_runtime_emits_nothing_class`）。

分割コンパイルで 2 つ、`Unit` とは無関係の穴も踏んだので直しました。

- `StackMapTable` のフレームが名指すクラスを**ディスクリプタから作るとき、
  `trim_start_matches('L')` が先頭の `L` を全部食っていました**
  （`crates/backend/src/code.rs` の `vtype_from_desc`）。既定パッケージの
  `LK` は `LLK;` なので `K` になり、`NoClassDefFoundError: K` で落ちます。
  `strip_prefix` に直しました。`ub_sepdef.scala` のクラス名が `L` で
  始まるのはこの回帰を踏ませるためです。
- 別コンパイル単位の `var` は `-cp` から読むと `val` に見えます
  （`reassignment to val w`）。フィールド型に依らないので、こちらは
  Remaining に置いてあります。

計測は `files=184 errors=411 files_with_errors=72` → **変わらず
`errors=411 files_with_errors=72`**。slick は型検査で止まっていて classfile を
1 つも出していない（`classes=0`）ので、バックエンドだけを直したこのスライスでは
数字が動かないのが正しい姿です。動かしたのは**出したコードが JVM にロード
できるか**であって、通る本数ではありません。

このスライスで漏れていた 2 つの値の位置——**`==` / `!=` のオペランド**と、
`Unit` の値に対して選んだメンバの**レシーバ**——は
「[`Unit` の比較オペランドと `scala.Enumeration`](#unit-の比較オペランドと-scalaenumerationagentuniteq)」
（`agent/uniteq`）で塞ぎました。`ub_boxed.scala` の `== ()` はレシーバが
`Any` の形だけだったので、`() == ()` はここでは踏めていません。

### コレクションの変換メソッドの結果型（`BuildFrom`、`agent/buildfrom`）

2.13 のコレクションは `map` などの結果型を `BuildFrom` / `IterableFactory` /
`MapFactory` と `CC[_]` で決めます。scala-rs はこれが効いておらず、結果が
上位のコレクションに落ちていました。

```scala
val m: Map[String, List[Int]] = Map("x" -> List(1,2))
m.map { case (d, g) => d -> g.sum }   // scalac: Map(x -> 3)
// scala-rs: found: Iterable[Tuple2[String, Int]] required: Map[String, Int]
```

主要コレクション × 主要メソッドの表を作り（`List` / `Vector` / `Seq` /
`IndexedSeq` / `Set` / `Map` / `SortedMap` / `TreeMap` / `TreeSet` /
`ArrayBuffer` / `ListBuffer` / `LazyList` / `Array` / `String` × `map` /
`flatMap` / `collect` / `filter` / `filterNot` / `++` / `zip` / `groupBy` /
`groupMap` / `groupMapReduce` / `partition` / `to` / `sorted` / `reverse` /
`distinct` / `take` / `drop` / `updated` / `-` / `+`、308 通り）、実 scalac
2.13.16 と突き合わせたところ **99 件**が食い違っていました。原因は 5 つです。

1. **カリー化された呼び出しが、どの節も宣言の「第 1 節」に対して型を解いて
   いた**。`Typer::instantiate_from_call` は `self.st.get(sym).ty` の
   `paramss.first()` を無条件に読んでいたので、
   `def f[K, B](k: A => K)(g: A => B)(r: (B, B) => B)` は `K` を 2 回解いて
   `B` を一度も解きませんでした。`groupMapReduce(key)(f)(reduce)` の
   `reduce` が `(Any, Any) => Any` になり、`_ + _` が
   `no matching overload for (String)String with arguments (Any)` として
   **無関係な行のエラーに見えていた**のはこれです。消費済みの節数
   （`s_paramss.len() - paramss_ids.len()`）を渡して、その節の宣言型に
   対して解きます。

2. **`BuildFrom` そのもの**。`Typer::rebuild_from_receiver` を 1 つ置き、
   宣言された結果型 `D[…]` を受け手の根クラス `R` で組み直します。`R` が
   `D` の真の部分クラスで、かつ `scala.collection` のクラスであるときだけ
   です（`maps_to_own_class`）。`R` と `D` の型引数の本数が同じならそのまま
   差し替え、`R` が 2 つ取り `D` が 1 つのペアを渡しているなら
   ペアをほどきます——これが `javap -p -s scala.collection.MapOps` の
   `public default <K2, V2> CC map(Function1<Tuple2<K, V>, Tuple2<K2, V2>>)`
   と `IterableOps` の `<B> CC map(Function1<A, B>)` の違いそのものです。
   ペアを返さないラムダは `Iterable[B]` のままで、nsc もそう推論します。
   `partition` は `(C, C)`、`groupBy` / `groupMap` は `Map[K, C]` なので、
   結果の**内側**も組み直します（`rebuild_inside`）。カリー化された
   `groupMap(k)(f)` はレシーバが `Select` の向こうにあるので
   `curried_receiver_ty` で辿ります。

3. **`erases_to_object` のゲートを外した**。`filter` / `take` / `++` …の
   narrowing は「ディスクリプタが `Object` を返すときだけ」に絞られて
   いました。`TreeMap - key` は
   `(Object)Lscala/collection/immutable/Map;` を返すので対象外で、README
   にも「Apply の結果型が erasure を生き延びる必要がある」と書いてありました。
   `maybe_unbox_erased_result` は既に**宣言より狭い結果型には checkcast を
   出す**ようになっていたので、ゲートは古くなっていました。外したうえで、
   ディスクリプタを直書きしている stdlib ディスパッチ
   （`is_stdlib_map` / `is_stdlib_set` の `+` / `-` / `++` / `filter` /
   `map` / `updated`）が固定の `checkcast` を出していたところを
   `cast_collection_result` に置き換え、typer が決めた型が宣言クラスの
   部分クラスならそちらへキャストします。これをやらないと
   `s.copy(waiting = s.waiting - key)` が
   `VerifyError: Bad type on operand stack` になります。
   `-` / `+` / `--` / `removed` / `incl` / `excl` / `concat` を
   `returns_receiver_collection` に足しました（`1 + 2` や `"a" + b` も
   この経路を通りますが、受け手が `scala.collection` のクラスでなければ
   組み直さないので無傷です）。

4. **`Map.map` の**オーバーロード**をコード生成でも選ぶ**。`MapOps.map` は
   *マップを作る*ので関数がペアを返す必要があり、2.13 はそうでなければ
   `IterableOps.map` を選びます。scala-rs はペア側のシンボルを 1 つしか
   持たないので、結果型が対（あるいはマップ）でなければ
   `IterableOps.map:(Lscala/Function1;)Ljava/lang/Object;` を呼びます。
   これが無いと `m.map { case (_, v) => v }` が
   `ClassCastException: Integer cannot be cast to Tuple2` で落ちます。

5. **`xs.to(ArrayBuffer)` の `Factory`**（`agent/ambigmap` の残件）。
   `to[C1](factory: Factory[A, C1]): C1` の引数はコンパニオン
   *オブジェクト*で、`Factory` ではありません。橋渡しは
   `object IterableFactory { implicit def toFactory[A, CC[_]](factory:
   IterableFactory[CC]): Factory[A, CC[A]] }` という **view** で、必要な
   ものが 3 つありました。
   - コンパニオンに `IterableFactory[CC]` / `MapFactory[CC]` の親辺
     （`prelude_buildfrom.rs`）。これは**適合のためだけの辺**なので、
     factory トレイト側のメンバは落とします——class file 由来の
     `apply` / `empty` はトレイト自身の抽象 `CC` を返すので、継承すると
     `mutable.ArrayBuffer[Int]()` が `ArrayBuffer[A]` になってしまいます。
   - `toFactory` を prelude で宣言し直す。Java の generic signature は
     `CC[A]` を書けないので class file 側は `<A, CC> Factory<A, CC>` で、
     `C1 = ArrayBuffer`（素の型構成子）と解かれます。`implicit` である
     ことも pickle にしか無く、`PickleSupply::supply_implicit_members` は
     `scala/` を意図的に飛ばします。
   - **期待型に未確定の型パラメータが残ったままの view 探索**
     （`Typer::search_conversion_open` / `apply_open_views`）。nsc の
     `inferView` は `Context.undetparams` を持ったまま走ります。宣言型
     同士を比べる従来の `conversion_provides` では `C1` が束縛されず、
     どの conversion も適用できませんでした。conversion 自身の型引数は
     まず引数から（引数は**パラメータのクラスで読んでから**——
     `align_to_param_class`）、残りと呼び出し側の未確定を両側 `Unify` で
     解きます。`implicitly[Factory[Int, Vector[Int]]]` の側は view では
     なく**値**なので、コンパニオンの
     `implicit def iterableFactory[A]: Factory[A, CC[A]]`（javap:
     `List$.<A> Factory<A, List<A>> iterableFactory()`）を宣言して塞ぎます。

ついでに落ちた小さい穴が 3 つあります。`[B >: A]` の下限を**受け手の
「素の型引数」で置換していた**（`Map[K, V]` は `IterableOps[A, …]` から
`++` を継承していて `A = (K, V)` なのに `A := K` になり、
`Map("a" -> 1) ++ Map("b" -> 2)` が `Iterable[Serializable]` になっていた——
しかも `IterableOps.++` を別のレシーバで先に完了させたファイルでだけ
起きるので、**無関係な行のバグに見える**類でした）ので、owner での基底型を
通して読むようにしました（`owner_args_as_seen_from`、`check_tparam_bounds`
も同じ）。`immutable.Set.++` は `SetOps.concat(IterableOnce[A])` なのに
`Set[A]` を取る宣言になっていました。`mutable.Map` に `-` がありません
でした（javap: `mutable.MapOps.$minus(K)`、
`(Ljava/lang/Object;)Lscala/collection/mutable/MapOps;`）。

表の食い違いは **99 → 12** になりました。slick は
`errors 354 → 339`、`files_with_errors` は 65 のまま。

**原因まで分かって直していない**もの:

- **ソート済みマップの `map` / `flatMap` / `collect`**。`TreeMap.map` は
  `SortedMapOps.map[K2, V2](f)(implicit ord: Ordering[K2]): CC[K2, V2]`
  （javap: `(Lscala/Function1;Lscala/math/Ordering;)Lscala/collection/Map;`）
  で、witness を渡さない限り `MapOps.map` に落ちて素の `Map` ができます。
  静的型だけ `TreeMap` に絞ると代入で `ClassCastException` になるので、
  `rebuild_widened` はソート済みの受け手では組み直しません。`filter` /
  `take` / `-` / `+` / `updated` は witness が要らないので今まで通りです。
- **`TreeSet.map` / `flatMap` / `collect` / `zip`** は
  `ambiguous overload`。`IterableOps.map[B]` と
  `SortedSetOps.map[B](f)(implicit ord: Ordering[B])` の 2 本が両方
  適用可能で、nsc は「宣言しているクラスが部分クラスの方」を選びます。
  オーバーロードの特定性に owner の部分クラス関係が入っていません。
- **`SortedMap.keySet` / `TreeMap.keySet`** は `SortedSet` /
  `TreeSet` ではなく `Set` を返します（`SortedMapOps.keySet` の
  対応が別に要ります）。
- **`Array.to(…)` / `Array.groupMapReduce`** は `ArrayOps` に無い
  メンバで、2.13 では `ArraySeq` 経由です。`"abc".zip(…)` も同様に
  `WrappedString` 経由なので `IndexedSeq` ではなく `Iterable` になります。
- **`Map.groupBy(f)` の `K$`** は、ラムダがキー型以外を返すと `Any` に
  なります（`m.groupBy(_._2 > 1)` が `Map[Any, Map[Any, Int]]`）。
  main の時点からある推論の穴で、このスライスでは動かしていません。
- slick で 2 件だけ新しく出た
  `found: TypedType[Option[Option[Any]]] required: TypedType[Option[Any]]`
  （`lifted/OptionMapper.scala`）。最小化できておらず、原因は未特定です。


### Remaining

- **`Nothing` を返すものを値の位置で使うと `VerifyError`**（`agent/lazyref` で確認、
  main でも同じ。ローカル `lazy val` とは無関係の既存バグ）。
  `def boom: Nothing = throw new RuntimeException("x")` を
  `if (n > 0) 1 else boom` のように使うと、`Nothing` の結果が `V` に消去されて
  片方の枝だけスタックが空になり `Inconsistent stackmap frames` になります。
  `lazy val boom: Nothing = throw …` も（アクセサへ持ち上がった結果）同じ経路に
  乗るので、同じ形で落ちます。直すには `Nothing` を返す呼び出しのあとに
  「到達しない」印を置いて、期待型ぶんのダミーを積む必要があります。

- ~~**`Seq`／`IndexedSeq` の `lazyZip`**（`agent/ambigmap` で確認）~~。
  `lazyZip` 自体はその後 pickle から入り、`LazyZip2.map` の `BuildFrom` も
  「[`BuildFrom` の高階 implicit 照合](#buildfrom-の高階-implicit-照合lazyzipagentbuildfrom2)」
  （`agent/buildfrom2`）で解けるようになりました。

- **`xs.to(ArrayBuffer)`**（`agent/ambigmap` で確認）。
  コンパニオンから `Factory[A, C]` の implicit を取れないので
  `no matching overload for (Factory[Any, C1])C1 with arguments (ArrayBuffer$)`
  になります（`memory/HeapBackend.scala`）。これも `map` が通ってから
  見えるようになった箇所で、直前は同じ行が
  `ambiguous overload for map` で止まっていました。
### 定義で終わるブロック・op-assign の優先順位・入れ子配列（`agent/stmtval`）

基本的な形が 4 つ壊れていました。互いに独立で、根も別々です。

**1. 本体の最後が定義のブロックが `VerifyError`。**

```scala
object Main { def main(a: Array[String]): Unit = { val v = 1 } }
// java.lang.VerifyError: Operand stack underflow
//   Location: Main$.main([Ljava/lang/String;)V @2: pop
```

nsc の `TreeBuilder.makeBlock` は、最後の文が**項ではなく定義**のとき
`Block(stats, ())` を作ります（`scalac -Xprint:parser` で見えます）。

```
def main(a: Array[String]): Unit = {
  val v = 1;
  ()
}
```

こちらの `block_from_stats` は文が 1 つならそれをそのまま返し、複数なら
最後の文をブロックの値にしていたので、`{ val v = 1 }` は**そのまま `ValDef`**
でした。ブロックの型が定義の型（ここでは `Int`）になり、
`emit_body_return` の `pop_if_value` が**積まれていない値を pop** します。
これは `val` / `var` / `def` / `class` / `object` / `import` / `type` の
どれで終わっても起きるので、**あらゆるメソッドに効きます**。修正は
`crates/parser/src/parse.rs` の `block_from_stats` に nsc と同じ分岐を
足しただけです（`stat_is_definition`）。空ブロック `{}` は元から
`Literal(())` でした。

**2. `n += i + x` が文字列連結に解決される。**

```
error: no matching overload for (String)String with arguments (Int)
```

nsc の `precedence` は `isOpAssignmentName`（`=` で終わり、`=` で始まらず、
`!=` / `<=` / `>=` でなく、演算子文字で始まる名前）に**0＝最下位**を返します。
こちらは先頭文字だけを見ていたので `+=` は `+` と同じ 8 で、`n += i + x` は
**`(n += i) + x`** と解釈されていました。左辺 `n += i` は `Unit` なので、
そこから `any2stringadd` → `String.+` が選ばれてあのエラーになります。
`n += 1` のように後続の演算子が無ければ壊れないので、複合式のときだけ
再現していました。`crates/parser/src/ast.rs` の `op_precedence` に
`is_op_assignment_name` を足しただけです。`var s = "a"; s += 1`（`String + Any`）
は今も通ります。

**3. `new Array[Array[Int]](n)` が `anewarray java/lang/Object`。**

`anewarray` のオペランドは**内部名**で、要素が配列型のときはその
ディスクリプタそのもの（`[I`）です。scalac は
`anewarray "[I"` を出します。`emit_newarray` は `String` / `Class` /
`ModuleRef` 以外を全部 `java/lang/Object` に落としていたので、
`Array[Array[Int]]` が `[Ljava/lang/Object;` になり、最初の `arr(i)(j)` が
`VerifyError: Bad type on operand stack in iaload` で落ちていました。
`jvm_desc` から内部名を作るようにしたので、`Array[(Int, Int)]` の
`[Lscala/Tuple2;` と `Array[Int => Int]` の `[Lscala/Function1;` も
scalac と同じになります（`Unit` / `Nothing` 要素の扱いは従来どおり）。

**4. `Array.ofDim[T](n1, …)` の型引数が具体化されない ＋ `arr(i) += x`。**

```
error: type mismatch; found: 5  required: T
error: value += is not a member of T
```

`scala.Array$.ofDim` は 1〜5 次元の 5 本のオーバーロードで、**どれも
型パラメータを 1 つ取ります**。`TypeApply` は「その個数の型パラメータを
取る候補が 1 つだけ」のときにしか参照を絞れないので、`ofDim` では絞れず、
明示した `[Double]` は**どこにも届いていませんでした**（結果は
`Array[Array[T]]` のまま）。オーバーロードが値引数で決まった時点で、
書かれた型引数を選ばれた候補に適用するようにしました
（SLS 6.26.3、`pending_targs`）。

同じ呼び出しには続きが 2 つあります。コード生成の `peel_fun` は
`TypeApply` を突き抜けて**下の `Select` の**シンボルを読むので、
そこにも解決結果を伝えないと 1 次元の `ofDim(I, ClassTag)Object` を
呼んでしまいます。そして 2 次元の `ofDim` の JVM 戻り値は
`[Ljava/lang/Object;` なので、`Ljava/lang/Object;` と同じように
**narrowing の `checkcast "[[D"`** が要ります（scalac も出します）。
`maybe_unbox_erased_result` に `erased_array_return` を足しました。

`arr(0) += 1` は別の穴でした。nsc の `convertToAssignment` は受け手が
`t.apply(i)` のとき `mkUpdate` に入り、`t.update(i, t.apply(i) op x)` を
作ります。表と添字は `gen.evalOnce` で**一度だけ**評価されます
（純粋な参照は複製、それ以外は `val ev$…` に束縛）。こちらにはこの分岐が
無く、「receiver is not assignable」で落としていました。`bar` のような
**普通のメソッド呼び出しの受け手は対象外**です（nsc も
`UnexpectedTreeAssignmentConversionError` にします）。`t(i)` を
`t.apply(i)` に書き換えるのはこちらの typer も同じなので、`index_table` で
その形だけを拾います。

| fixture | 何を固定するか | 期待出力 |
| --- | --- | --- |
| `sv_block.scala`（`crates/cli/tests/stmtval.rs`、私有ランタイム・library dual-run） | 最後の文が定義のブロック: `val` / `var` / `def` / `import` / `class` / `object` / `type`、空ブロック `{}`、`if` の両枝、`while` の本体、`try` / `match` の本体、パターン `val`、ネストしたブロック、ラムダ本体。項で終わるブロックは値を保つ | `valLast` `nested` `42` `done` |
| `sv_opassign.scala`（同上） | `+=` `-=` `*=` `/=` `%=` `<<=` `\|=` `&=` `^=` の右辺が複合式（`i + x`、`f(x) + g(y)`、`(a + b) * c`、`if`、英字演算子）。`var s = "a"; s += 1` は `String + Any` として通る | `3` `0` `12` `4` `1` `13` `9` `20` `6` `18` `8` `11` `3` `1` `4.5` `a1` `a1bc` `3` |
| `sv_array.scala`（同上） | `new Array[Array[Int]]` / `Array[Array[String]]` / `Array[Array[Array[Int]]]` の要素型と `getClass.getName`。1 次元（`Int` / `String` / `Object`）、`Array[(Int, Int)]`、`Array[Int => Int]` | `2` `10` `[[I` `y` `[[Ljava.lang.String;` `7` `[[[I` `9` `[I` `s` `[Ljava.lang.String;` `[Ljava.lang.Object;` `1,2` `[Lscala.Tuple2;` `2` `[Lscala.Function1;` |
| `sv_update.scala`（同上） | `t(i) op= x`: 配列（`Int` / `Double` / `String`）、入れ子 `nested(0)(1) += 3`、自前の `apply`/`update` を持つクラス、右辺が複合式、`evalOnce`（表と添字が 1 回ずつしか走らない） | `6` `12` `3` `2.0` `ab` `7` `15` `1` `2` |
| `sv_ofdim.scala`（`crates/cli/tests/stmtval.rs`、library dual-run のみ。私有ランタイムには `ofDim` が無いので診断を出すことも見る） | `Array.ofDim[T]` の 1〜5 次元 × `Int` / `Double` / `String` / ユーザークラス。既に動いていた `val g: Array[Array[Int]] = Array.ofDim[Int](2, 3)` と `Array.fill(3)(0)`、`Array(1, 2, 3)` も | `7` `[I` `7` `[[I` `7` `[[[I` `7` `[[[[I` `7` `[[[[[I` `2.0` `[D` `6.0` `[[D` `2.5` `[[[D` `ab` `[Ljava.lang.String;` `z` `[[Ljava.lang.String;` `Cell(3)` `[LCell;` `Cell(4)` `[[LCell;` `0,0,0;0,0,9` `2` `[I` `1,2,3` |
| `sv_lib.scala`（`crates/cli/tests/stmtval.rs`、library dual-run のみ） | 実 scala-library でしか裏付けられない形での同じ 4 件: `Array[List[Int]]` の要素型、`n += i max x`、`var lst ++= List(…)`、`foreach` のラムダ本体が定義で終わる形 | `List(1, 2)` `[Lscala.collection.immutable.List;` `2` `3` `List(1, 2, 3)` `6` `done` |
| `sv_bad.scala`（`crates/cli/tests/stmtval.rs`、異常系） | 不変な受け手への op-assign は nsc の `convertToAssignment` の診断のまま（`value += is not a member of Int` ＋ `Expression does not convert to assignment because receiver is not assignable.`）。優先順位を直すまでは `any2stringadd` のエラーに化けていた | （コンパイルエラー 2 件） |
| `lf_frame.scala`（`crates/cli/tests/loopframe.rs`） | 最小形の `var c: Option[Int] = Some(1); while (c.isDefined) { c = None }`。実行に加えて **`javap -v` の `StackMapTable` を実 scalac のものと突き合わせる**（scalac は `class scala/Option` 1 つだけ。`java/lang/Object` / `scala/Some` / `scala/None$` に逃げていないことも見る）。両モード | `done` |
| `lf_loopvar.scala`（同上、library dual-run のみ。私有ランタイムに可変長 `List.apply` が無い） | ループを跨ぐローカルの各種: `while` / `do while` / 入れ子ループ / `List` → `Nil` / 1 周で何度も別クラスになる / ループ内の `if`・`match`・`try`・`finally` の分岐 / ハンドラの中での参照 / ループを抜けたあとの参照 / `for` の desugar / ラムダの中のループ / `while` を含む `Unit` メソッド / ループ内のパターン束縛 / 宣言型が trait（フレーム型が interface）/ ラムダに捕まった `var` / もう一方の腕が `Nothing`。フレームが `scala/Option` と `scala/collection/immutable/List` を保つことも見る | `None`×3 `List()`×2 `None`×2 `List(0)` `Some(1)` `true` `Some(3)` `true` `1` `6` `9` `List(2, 1, 0)` `12` `true` `None` `List()` |
| `lf_loopany.scala`（同上） | 宣言クラスが `java/lang/Object` になる形: `var a: Any` がループ内でプリミティブと参照のあいだを動く（フレームが `java/lang/Integer` に固定されていないことも見る）、配列ローカルの再代入、プリミティブだけのループ、`null` 初期値。両モード | `2` `2` `6` `z1` |
| `lf_trystack.scala`（同上） | オペランドスタックが空でない位置の `try`: `println(try …)`、2 番目の引数、`new Box(try …)`（未初期化参照）、プリミティブを積んだ状態、実際に投げる形、ループ内の引数位置、`finally` 付き。両モード | `w0` `w1` `y` `pq` `a` `n=3!` `boom` `ktrue`×2 `kfalse` `true` `fin f` |
| `lf_ctorframe.scala`（同上） | 親コンストラクタ呼び出しのあとの `this` の型: 分岐・ループ・`try` を本体に持つサブクラスのコンストラクタと、親コンストラクタの引数が `try` の形（未初期化の `this` がスタックに載る）。`C.<init>` のフレームが `C` であって `B` でないことも見る。両モード | `b` `pos` `neg` `zero` `3` `g1` `d2` |
| `lf_loopvar_bad.scala`（同上、異常系） | ループ本体で `var c: Option[Int]` に `String` を入れる形。フレームの合流は宣言型なので、これは黙って `Any` に広がらず `type mismatch` になる | （コンパイルエラー） |


### Remaining

- **`t(i) op= x` の受け手が普通のメソッド呼び出しのとき**（`agent/stmtval`）。
  `foo.bar(0) += 1`（`bar` はメソッド）は nsc も
  `UnexpectedTreeAssignmentConversionError` でエラーにしますが、こちらの
  文言は `value += is not a member of …` ＋
  `Expression does not convert to assignment because receiver is not
  assignable.` のままです。どちらもエラーにはなるので受理はしませんが、
  診断は一致していません。

- **`Int` の `max` / `min` が私有ランタイムに無い**（`agent/stmtval` で確認）。
  `n += i max x` は jar モードでは通りますが、`--no-scala-library` では
  `value max is not a member of Int` になります（`RichInt` のメンバなので、
  診断としては正しい形です）。
- **`+:` / `:+` のパターン**（`agent/conspat` で確認）。`case P(v) +: _` /
  `case _ :+ P(v)` は `not found: value +:` / `not found: extractor +:` です。
  `scala.collection.+:` / `:+` という抽出子オブジェクト自体が prelude にも
  pickle 経路にも無く、`::` のような特別扱いもありません。入れ子パターンの穴
  （本節の `case P(v) :: t`）はこの 2 つには**無い**ことを確認済みですが、
  それは動く前に型検査で落ちるからです。

- **私有ランタイムの `Tuple3` 以上と `List.apply` / `Seq` 抽出子**
  （`agent/conspat` で確認）。`--no-scala-library` では `not found: value Tuple3` /
  `value apply is not a member of List$` / `not found: extractor Seq` と診断が
  出ます（黙って通ってはいません）。`cp_seq.scala` を library dual-run 専用に
  してあるのはこのためです。

- **私有ランタイムの `List` に Scala 版 `toString` が無い**（`agent/conspat` で
  確認）。jar モードの `List(Q)` に対して `scala.collection.immutable.$colon$colon@…`
  になるので、`MatchError` のメッセージがリストのときだけ 2 モードで違います
  （`cp_err.scala` はそこだけクラス名で比べています）。
- ~~**`Unit` のメンバが消去後 `Object` を返すとき、捨てた値がスタックに残る**~~
  （`agent/anonbridge` で確認）→ **`agent/override` で直した**（`ov_unitpop.scala`）。
  引数リストを持たない `def`（`trait Box[A] { def get: A }` の `get`）は呼び出しが
  `Apply` を持たない裸の `Select` になるので、`unit_stat_leaves_ref` の `Apply` の
  腕に当たらず `pop` が出ていませんでした。`Select` / `Ident` の腕を足しています。

  残っているのは**ライブラリのメンバ**の同じ形です。

  ```scala
  def f(o: Option[Unit]): Unit = {
    o.get                       // invokevirtual get()Ljava/lang/Object; -- pop が無い
    try { … } catch { … }       // VerifyError: Inconsistent stackmap frames
  }
  ```

  `unit_stat_leaves_ref` は `owner_defined_in_source` で**このコンパイル単位が
  定義したメンバ**に限っています。ライブラリ側は `Using.resource` /
  `Breaks.catchBreak` / `ArrayOps` のように emit 側で既に値を捨てている経路が
  あり、そこで二重に `pop` するとスタックが underflow するためです。本筋の直し方は
  判定を木の形から推測するのをやめて、`Assembler` が持っているスタック高
  （`asm.stack`）を statement の前後で比べて残りを捨てることですが、
  `gen_stat` の全経路に影響するのでこのスライスでは触っていません。

- ~~**匿名クラス／サブクラスのメンバの結果型が親と食い違っても通る**~~
  （`agent/anonbridge` で確認）→ **`agent/override` で直した**。
  `new It[Int] { def next(): String = "x" }` は real scalac と同じ
  `incompatible type in overriding` になります（`ov_result_bad.scala`）。

- **`"abc".appended(1)` のような `B >: Char` の下限推論**（`agent/stringops8`
  で確認）。scalac は `B := AnyVal` と lub を取って `IndexedSeq[AnyVal]` を
  返しますが、こちらは `B := Int` と推論して
  `inferred type arguments [Int] do not conform to method appended's type
  parameter bounds [B >: Char <: Any]` を出します。`Char` 引数の
  `appended('x'): String` は通るので、欠けているのは下限つき型パラメータの
  lub 推論そのものです。`prepended` / `:+` / `+:` / `concat` も同じ形です。

- ~~**`LazyZip2` のメンバ**（`agent/stringops8` で確認）~~。
  `"abc".lazyZip(List(1,2,3)).map(…)` は
  「[`BuildFrom` の高階 implicit 照合](#buildfrom-の高階-implicit-照合lazyzipagentbuildfrom2)」
  （`agent/buildfrom2`）で通るようになりました（`String` レシーバは
  `buildFromString` が答えるので結果も `String` です）。

- **`StringOps.partitionMap`**（`agent/stringops8` で確認）。
  `s.partitionMap(c => if (…) Right(c) else Left(c))` が
  `(Char) => AnyRef` になり、`Either` への lub が取れずに落ちます。
  上の下限推論と同じ根です。
- **jar の trait の `abstract override`**。`Symbol::abstract_override` は自分で
  namer に通したソースにしか立ちません。pickle / classfile から読んだ trait の
  stackable メンバは「接地しているか」の判定対象外なので、そこだけは診断せずに
  通します（同じ理由で、その super 連鎖のコード生成も従来どおりです）。

- **trait のスーパークラスを継承するときの型引数はヘッダパスでは埋まらない**。
  `class X extends Loud` の暗黙のスーパークラス補完（SLS 5.1）は typer 本体の
  パスでだけ行います。ヘッダ（`sigs_only`）パスでは trait の親がまだ
  `Type::Class { args: [] }` なので、そこで補完すると
  `StatementInvoker takes type parameters` になります（slick の
  `class QueryInvokerImpl[R] extends QueryInvoker[R]` で実際に踏みました）。
  したがって**別コンパイル単位のヘッダだけを見る経路では、この補完は効きません**。
- **`-cp` で読み戻した case class の `Product` / `Serializable`**（`agent/product`）。
  別コンパイルした `case class Pt(x: Int, y: String)` を `-cp` 経由で使うと、
  `Pt.tupled`（コンパニオンの `AbstractFunction2` は**スーパークラス**なので
  classfile から読める）は通りますが、`val q: Product = p` と
  `val s: java.io.Serializable = p` は通りません。**インタフェース**側の親が
  `-cp` 読みで落ちるためで、同じ `-cp` にあるユーザ定義 trait
  （`class Plain extends Marker`）は落ちません。実 scalac が出した classfile を
  読ませても同じなので、こちらが**出している**ものの問題ではなく、
  classpath / pickle 読み側（`classpath.rs` の `find_or_stub_java_class` と
  `pickle_supply.rs` の `attach_parents` あたり）の既存の穴です。1 コンパイル単位
  なら jar モード・私有ランタイムモードとも real scalac と一致します。

- **自分で書いたコンパニオンの `tupled` / `curried`**（`agent/product`）。
  nsc は `object P` を自分で書いた case class のコンパニオンには
  `AbstractFunctionN` を**継承させない**（classfile で確認済み）ので、
  こちらもそうしています。それでも scalac が `P.tupled` を通すのは、
  モジュールを `apply` 経由で eta 展開してから `tupled` を引くからです
  （2.13.13 以降 deprecated）。その eta 展開がこちらには無いので、
  `value tupled is not a member of P$` を出します。合成コンパニオン
  （＝`object P` を書いていない普通の case class）は継承で動きます。
- **`Unit` を引数に取る関数型 `Unit => T` が `Function0[T]` になる**
  （`agent/unitbox` で見つけた別件）。`crates/parser/src/parse.rs` の
  `is_unit_tuple` が型位置の `Ident("Unit")` を空パラメータリスト扱いにするので、
  `def h(f: Unit => Int)` が `() => Int` として型付けされ、`f(())` が
  `no matching overload` になります。nsc では `Unit => T` は
  `Function1[Unit, T]` で、`() => T` だけが `Function0[T]` です。
  パーサの 1 行ですが、関数型の解釈が変わるので別のスライスにしました。

- **私有ランタイムに `scala.runtime.BoxedUnit` が無い**（`agent/patbind`）。
  `--no-scala-library` では `Unit` を `Any` に入れると `null` になるので、
  `(x: Any) match { case () => … }` は `null` にも当たります。jar モードは
  nsc と一致します（`pb_nullseq.scala`）。私有ランタイムに `BoxedUnit` を
  足すのが本筋ですが、`Unit` の box 表現全体を変える話になります。
- **メソッドの中の `object`（ローカル `object`）が外を読む形**（`agent/nestedobj`）。
  nsc は呼び出しごとに 1 つのインスタンスを `scala.runtime.LazyRef` に持ち、
  `$outer` とキャプチャした局所を `<init>` に渡します（`javap -v -p -c` で確認）。
  こちらはまだ静的シングルトンしか出せないので、外側インスタンスや囲みメソッドの
  局所を読むローカル `object` は
  `not implemented: a local `object` that reads …` を出します
  （`tests/fixtures/nestedobj_bad.scala`）。外に何も読まないローカル `object` は
  通ります。直すには `LazyRef` のローカル + キャプチャ引数の codegen が要ります
  （`agent/lazyref` でローカル `lazy val` 用に `scala.runtime.Lazy*` のセルと、
  そのセルを取る持ち上げ済みアクセサの codegen が入ったので、下地はあります。
  残っているのは `ModuleDef` を `$outer` とキャプチャを取るクラスへ組み替える
  ところ）。
  なお **value class の中の `object`** は scalac 自身が
  `implementation restriction: nested object is not allowed in value class` で
  断るので、こちらも同じ文面で断ります（以前は通してしまい `VerifyError` でした）。

- **パス依存のコンパニオン `apply` / `copy`**（`agent/nestedobj` で確認、main でも
  同じ）。`class Box(val k: Int) { case class Pair(a: Int) }` に対して
  `bx.Pair(6)` と `p.copy(9)` が `not found: value Pair` になります。
  `new bx.Pair(6)` は通り、生成される classfile 側（`$outer` を先頭に取る
  `<init>`、`copy` が自分の `$outer` を渡す形）は実装済みなので、残るのは
  typer 側のコンパニオン解決だけです。同じ理由で、クラス本体に `object` が
  先にあると後続の `case class` のコンパニオンも見つからなくなります
  （`case class Holder(k: Int) { object Inner; case class Pair(a: Int) }`）。
- **`Unit` パラメータへの value discarding**（SLS 6.26.1、`agent/unitbox`）。
  scalac は `def f(x: Unit)` に `f("s")` を**警告付きで受理**し、値を捨てて
  `()` を渡します。こちらは `no matching overload` を出します。オーバーロード
  解決に手を入れる話なので、そちらのスライスに任せています。

- **`def a: Array[T]` に続く `a(0)`**（`agent/unitbox` で見つけた、`Unit` とは
  無関係の別件）。引数なしメソッドの結果への `apply` 挿入が無いので
  `no matching overload for Array[String] with arguments (0)` になります。
  `val` に受けてから `a(0)` すれば通ります。要素型に依りません。

- **`-cp` から読んだ `var` が `val` に見える**（`agent/unitbox` で見つけた、
  `Unit` とは無関係の別件）。別コンパイル単位の `class C { var w: Int }` に
  `c.w = 5` すると `reassignment to val w` になります。フィールドの型に
  依りません（`ub_sepuse.scala` のコメント参照）。

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

- **`toMap` の implicit 節が埋まらない形が slick に 5 件残る**
  （`agent/mismatch7` で範囲を絞った、未修正。main でも同じ）。
  引数位置（`one(kvs.toMap)`、`(1, kvs.toMap)`）、期待型が直接 `Map[K, V]` の
  形、`lazy val m: Map[String, C] = cs.map(c => c.name -> c).toMap` は
  `agent/mismatch7` で通るようになりました。**最小再現はもう作れません** ——
  残る 5 件はどれも slick の中でしか出ず、診断の形が 2 通りに割れています:
  `(<:<[…])Map[K$, V$]`（メソッド型のまま）と
  `(<:<[…]) => Map[K$, V$]`（**矢印付き** ＝ eta 展開された）です。後者は
  `adapt` が期待型 `Map[…]` を関数として見た（`Typer::function_view` が
  `Map` を `K => V` として読む）ことを示唆しますが、そこだけを取り出した
  形は通るので、レシーバ側のカスケードとの合わせ技だと思われます。
  `JdbcModelBuilder.scala` の 1 件は `A` が `Char` として届いており
  （`<:<[Char, Tuple2[K$, V$]]`）、受け手の `mTables.map(…).zip(…)` が
  先に壊れています。

- **依存メソッド型 `def get[P <: Phase](p: P): Option[p.State]`**
  （`agent/mismatch7` で原因まで特定、未修正）。`Type::TypeMember(id)` は
  接頭辞を持たないので、シグネチャを組む段階で `p.State` は
  `Phase` の抽象メンバ `State` そのものになり、呼び出し時に
  `p := Phase.assignUniqueSymbols` を代入して `UsedFeatures` に
  dealias する道がありません。slick では
  `state.get(Phase.assignUniqueSymbols).map(_.aggregate).getOrElse(true)` が
  `found: Any required: Boolean` になります（4 件＋
  `value aggregate is not a member of Phase.State` などのカスケード）。
  直すには `TypeMember` に接頭辞を持たせる（nsc の
  `TypeRef(SingleType(NoPrefix, p), sym, Nil)`）変更が要ります。

- **jar のメンバの implicit 節が JVM ディスクリプタから来ると implicit で
  なくなる**（`agent/mismatch7` で確認、main でも同じ）。
  `mutable.ArrayBuilder.make[E]` は `(ClassTag[E])ArrayBuilder[E]` という
  メソッド型のまま残ります。`pickle_supply` は implicit フラグを読めますが、
  この経路には入らず（そのメンバは classfile 読みで既にあるので
  `supply_from_pickle` は「見つからなかったとき」しか走らない）、
  JVM のディスクリプタには節が implicit だと書く場所がありません。
  `Array.empty` のように pickle 経由で供給されるものは通ります。

- **匿名クラスがプリミティブの型引数で親のメソッドを実装すると二重に
  箱詰めする**（`agent/mismatch7` で気づいた、未修正。main でも同じ）。
  `new It[Int] { def next(): Int = … }` は `next()Ljava/lang/Object;` の
  本体で `boxToInteger` を 2 回出し、`java -Xverify:all` が
  `Type 'java/lang/Integer' … is not assignable to integer` で落とします。
  参照の型引数（`new It[String]`）なら通ります。

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
- **jar のメンバの結果型が素の `F` になる**件は `agent/companionkind` で
  **直りました**（上の「コンパニオンとクラスは別のシンボル」）。
  そこで残った隣接の穴 —— jar のコンパニオンの入れ子クラス
  （`Outcome.Succeeded(_)` / `Resource.ExitCase.Errored(e)`、6 件）—— は同節の
  末尾に書いてあります。
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

- ~~**override 検査が無い**~~ → **`agent/override` で入った**（「オーバーライドの適合検査」節）。
  SLS 5.1.4 の 1〜9 と SLS 5.2.6（`needs to be abstract`）を検査する。残るのは
  **ライブラリ側のメンバに対する `final` と deferred**: `PickleSupply` は pickle の
  `FINAL` / `DEFERRED` ビットを運ばないので（メンバは `Flags::EMPTY` で作られる）、
  jar の `final` メソッドを覆っても、jar の trait の抽象メンバを実装し忘れても
  診断できない。ソース由来と Java classfile 由来（`classpath.rs` は `ACC_ABSTRACT` を
  読む）は診断する。塞ぐには `Shape` にフラグを足すのが筋。
  もう 1 つの残件は **`class C extends A with T` の「accidental override」**
  （scalac: `class C inherits conflicting members`）。無関係な `A.f` と `T.f` が
  ぶつかったときに `override` を要求する規則で、1〜9 とは別の規則なので入れていない。
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
- ~~**`override` 修飾子と override 適合性の検査**~~ → **`agent/override` で入った**。
  `val` も `var` も `def` と同じく検査する（`ov_valdef_bad` / `ov_var_bad` /
  `ov_modreq_bad`）。残件は上のライブラリ側フラグの件。
- **implicit 探索の残り**: 多相 implicit のユニフィケーションと再帰導出、発散の打ち切り、nsc 相当の specificity は入った（「Implicit 解決」節）。残るのは (a) `xs.toMap` を `scala.collection.Iterable` にも載せること — pickle 供給が具象コレクション（`HashMap` / `ConstArray` …）に自前の `toMap` を付けるので、継承した 2 本目がオーバーロード衝突になる。いまは `List` / `Iterator` だけに宣言している、(b) 期待型からのメソッド型パラメータ推論が要る implicit（slick の `TypedType[T]` / `TypedType[P1]` はこちらで、implicit 探索ではなく `T` の推論が先に必要）、(c) 診断文面は nsc の複数行（`both … and … match expected type …`）ではなく 1 行のまま
- **def マクロの展開の残り**。JVM ブリッジ（`docs/macros.md` §2 / §7.11）は入り、
  マクロ実装を**本当にロードして呼ぶ**。`java` と scala-reflect.jar があれば
  `def f(): Int = macro Impl.m` は展開され、展開後のプログラムは実 scalac の
  出力と一致する（`crates/cli/tests/engine.rs`）。残件は、
  マクロバインディングの pickle（nsc の `MACRO` フラグ + `@macroImpl`。§5。
  だからマクロ def を*別 run*から展開することはできない）、
  `c.Expr[T](tree)` を返す実装、推論された型引数のタグ、
  `c.prefix` / `c.enclosingPosition` / `c.typecheck` / `c.inferImplicitValue`、
  引数に渡せる木の形（ブロック・関数リテラル・`new` は不可）、
  型引数のある型のタグ、whitebox と macro bundle。
  外れる形は**すべて理由つきで診断する**。
  テストは `crates/cli/tests/macros.rs` と `crates/cli/tests/engine.rs`
- **quasiquote の reification の残り**。`q"..."` はリテラル / 名前 / 選択 /
  適用（カリー化含む）/ `$x` 穴 / 引数リスト 1 節ぶんの `..$xs` を
  `internal.reificationSupport.Syntactic*` に落として実行できる
  （`crates/typer/src/reify.rs`、実 scalac と dual-run 済み）。宣言クラスでの呼び出しも
  済んでいて、`scala.reflect.runtime.universe` 上の Tree 構築は実際に走る。
  `tq` / `pq` / `cq` 全体と `q` の残りの形（ブロック / `new` / 関数リテラル /
  `if`-`else` / `match` / 型注釈 / `val` 定義 / `this` / 代入 / 型適用）、
  `c.Expr[T]` のようなパス依存型、そして **`Liftable`**（`Tree` でない穴を
  標準インスタンスと同じ木に持ち上げる）も済んでいる。
  残るのは `docs/macros.md` §7.8 の 4 つ:
  (a) パーサが nsc の保つ区別ごと正規化してしまう形（右結合演算子 `a :: b` /
  `else` の無い `if` / `_` プレースホルダ / by-name 型）、
  (b) `..$` と普通の引数の混在（`q"f(a, ..$xs)"`）と、期待型からの
  メソッド型パラメータ推論、
  (c) `class` / `def` 定義の quasiquote（`SyntacticClassDef` / `Modifiers` の
  フラグ変換）。`ShapedValue` の `q"""…"""` 全体はこれが要る、
  (d) `reify { … }` 本体（式を `TreeCreator` の無名クラスに落とす
  コンパイラ内蔵マクロ）。対になる `TypeTag` / `WeakTypeTag` の materialization は
  §7.10 で入り、単相型については実 scalac と実行結果が一致する。
  落とせない形は**すべて名指しで診断する**
  （`unimplemented syntax: quasiquote ...` / `a hole of type X is not lifted (…)` /
  `cannot expand reify { ... }`）
  `tq` / `pq` / `cq` 全体と `q` の残りの形（§7.7）、そして**定義**
  （`class` / `case class` / `trait` / `object` / `def` / 修飾つき `val`・`var`。
  §7.8、`crates/typer/src/reify_defs.rs`）も入った。残るのは:
  (a) `..$` と普通の引数の混在（`q"f(a, ..$xs)"`）、
  (b) `Liftable`（`$x` の `x` が `Tree` でないとき nsc は implicit で持ち上げる。
  `mapToImpl` は `$rTag` / `${c.prefix}` でこれを使う）、
  (c) `_` プレースホルダ関数リテラル・右結合演算子・`else` の無い `if`・
  by-name / 可変長パラメータ・手続き構文・パターン定義・自分型・early definition・
  `type` 定義（いずれもパーサが nsc の保つ区別ごと正規化してしまう形）、
  (d) 展開器（engine）そのものは入った（§7.11、上の「def マクロの展開の残り」）。
  落とせない形は**すべて `unimplemented syntax: quasiquote ...` で診断する**
- **ローカルな `case class` のコンパニオン**。メソッド本体で宣言した
  `case class P(a: Int)` は、クラス `Main$P$1` は出るが**コンパニオン
  `Main$P$1$` を出していない**ので、`P(1)`（合成 `apply`）が実行時に
  `NoClassDefFoundError` になる。型検査は通ってしまう既存のバグで、
  `agent/defquasi` が `{ case class X(…); … }` を**パースできる**ように
  したことで新しい綴りからも届くようになった（バグ自体は以前からある。
  `{ … }` の先頭でない `case class` は元から同じ経路）。`case object` と
  ローカルな非 `case` クラスは正しく出ている
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
- **`agent/reify2` 第 2 スライス**（reification の残りの形）: scala-reflect.jar を `-cp` に足した slick 184 ファイルのエラーは **257 → 255**。数字が動かないのは、**同じ行が別の理由で落ちるようになった**からで、quasiquote 系の内訳は `unimplemented syntax: quasiquote …`（形が足りない）が **10 → 4**、`cannot expand quasiquote …` が **1 → 0**、`TableQuery.scala` のエラー合計が **11 → 6**。`ShapedValue.mapToImpl` の 8 つの型注釈は形としては通るようになり、いま落ちている理由は `$uTag` / `$rTag` が `WeakTypeTag` で `Tree` でないこと（＝**`Liftable`** が要ること）に変わった。実装した形は `tq` / `pq` / `cq` 全体と `q` の型注釈 / eta 展開 / 型適用 / ブロックと `val` / `new` / `match` / 部分関数 / 関数リテラル / `this` / 代入 / `if`-`else` / タプル / 演算子名の符号化で、すべて実 scalac 2.13.16 の `-Ymacro-debug-lite` から読み取り `showRaw` まで突き合わせてある。ついでに直した一般の穴は、**オーバーロード集合への `apply` 挿入**（`val Ident: IdentExtractor` と `def Ident(String)`）、**同名の型メンバに項の選択が食われる**（`u.Modifiers(flags)` が `<notype>` になる）、**`invokeinterface` の `count` がスロット数でない**（`long` 引数で `VerifyError`）、**抽象型メンバの引数に erasure 適応の `checkcast` が無い**（`Names$TermNameApi` → `Names$NameApi`）。詳細と残件は `docs/macros.md` §7.7
- **`@specialized` codegen** はこのスライスでは開始しない
- **オーバーロード / メソッド適用のスライスで残っているもの**: slick 177 ファイルのエラーは **2,901 → 2,539**（`tests/slick_measure.sh`。エラーを含むファイルは 116 → 115）。`no matching overload for (Type, Any, Boolean)LiteralNode` / `(#N*)(TypedType[T])Rep[T]` / `not found: extractor ==` / `type arg is not a member of OptionMapperDSL$.arg[B1, P1]` は消えた。残る上位は implicit 探索（`could not find implicit value of type TypedType[BR]` など）と、`.fm` テンプレート由来で存在しない型（`Table` / `Sequence` / `Ref`）のカスケード。`no matching overload for (String)String` は最小再現では通るので、別の穴のカスケード
- 高階 `F[_] <% …` は nsc どおり `takes type parameters`（`F[_]: C` は nsc が受理するので実装済み。README の旧記述は誤りだったので実測に合わせて直した）
- placeholder の残り（より深い入れ子の完全再現。unary / Function2 / typed `_ : T` の必要形はこのスライスまで）
- **implicit の導出**（`implicit def optShow[A](implicit s: Show[A]): Show[Option[A]]` のように、implicit パラメータを取る implicit def を型パラメータの単一化つきで再帰的に解決する形）。`implicit_provides` は今のところパラメータリストが空の implicit しか候補にしないので、`Show[Option[Int]]` は `no implicit` になる
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
- **`agent/mutcoll` スライスで残っているもの**: (a) **`mutable.Buffer` のコンパニオンは参照の順に依存する**（このスライス以前からある既存バグで、`prelude_mutcoll` を外しても同じように再現する）。`mutable.Buffer(1, 2, 3)` / `mutable.Buffer[Int]()` を先に書けば `Buffer.empty[Int]` も通るが、`Buffer.empty[Int]` が `Buffer` の**最初の**言及だと型検査は通って実行時に `RuntimeException: select Buffer` になり、同じコンパイル単位で先に `Buffer` を**型として**使うと `value apply is not a member of Buffer$` になる（`object Buffer extends SeqFactory.Delegate[Buffer]` を pickle から補完する経路の順序依存で、`find_or_stub_java_class` のコンパニオン周り = 別スライスの担当）。`ArrayBuffer` / `ListBuffer` を経由すれば動く。(b) `mutable.PriorityQueue` の `Ordering` は暗黙値がスコープにあるものだけ（`Int` / `Long` / `Double` / `String` / `Boolean` など prelude が持つもの）。(c) `ArraySeq` は `apply` / `update` / `length` / `size` / `toList` / `mkString` までで、`ofInt` などの特殊化サブクラスは宣言していない。(d) `Queue.dequeueFirst` / `dequeueAll` / `Stack.popAll` / `popWhile` / `ArrayDeque.removeHead` などの残りのメンバーは pickle 供給頼みで、供給されないものは診断になる

- **私有ランタイムの `Tuple2` に `toString` が無い**（`agent/hkinfer` で気づいた、
  自動タプル化とは無関係の別件。main でも同じ）。`--no-scala-library` では
  `println((1, "a"))` が `(1,a)` ではなく `scala.Tuple2@…` になります。括弧を
  省いた `println(1, "a")` でも当然同じなので、`hk_tuple_lib` は jar 限定に
  してあります。`runtime.rs` が生成する `TupleN` に `toString` / `equals` /
  `hashCode` を持たせるのが筋です。

- **自動タプル化の `-Xlint:adapted-args` 警告**（`agent/hkinfer`、未実装）。
  nsc は自動タプル化のときに
  `adapted the argument list to the expected 2-tuple: add additional parens instead`
  を出します（2.13.16 では `-deprecation` ではなく `-Xlint:adapted-args`）。
  scala-rs にはこの lint の枠組みが無いので、**警告なしで受理**します。

- **ブロックの中の `case class` / `case object`**（`agent/localtrait` で見つけた、
  ローカル trait とは別件のパーサの穴）。ブロック文の先頭の `case` は必ず
  `case` 節として読まれるので、

  ```scala
  def f(): Unit = {
    case class P(x: Int)   // error: expected pattern, found class
    println(P(1))
  }
  ```

  が通りません。ブロック文の位置では `case` の次のトークンが `class` / `object`
  なら定義として読む、というのが nsc の形です。**診断は出る**ので黙って
  間違うことはありません。普通の（`case` でない）ローカル `class` / `object` /
  `trait` は動きます。

- **abstract メンバを実装しないクラスに `needs to be abstract` を出さない**
  （`agent/localtrait` で気づいた既存の穴。ローカルに限らずトップレベルでも同じ）。

  ```scala
  trait L { def v: String; def plain = v + "?" }
  class LC extends L        // scalac: class LC needs to be abstract.
  ```

  実 scalac はエラーにしますが、こちらは通してしまい、実行時に
  `AbstractMethodError` になります。`agent/localtrait` の修正で mixin
  フォワーダは正しく出るようになりましたが、**この検査そのものは未実装**です
  （`lt1_bad.scala` は代わりに、こちらも実装済みの
  `illegal inheritance; superclass … is not a subclass of …` を固定しています）。

- **クラスの中の trait から外側インスタンスのメンバを読む codegen**
  （`agent/localtrait` で確認。**ローカルかどうかとは無関係の既存の穴**で、
  `$outer` を触る作業（`agent/nestedobj`）の領域なのでこのスライスでは直していません）。

  ```scala
  class Holder(val base: String) {
    trait Tag { def t = base + "!" }   // 外側 Holder のメンバを読む
    class TC extends Tag
    def make(): String = new TC().t    // NoSuchFieldError: $outer
  }
  ```

  `Holder$Tag$class.t` が `$this`（interface 型）に `getfield $outer` を出すので、
  実行時に `NoSuchFieldError` になります。nsc は捕捉と同じやり方で、interface に
  abstract アクセサ（`Holder$Tag$$$outer()`）を立てて実装クラスに自分の `$outer`
  フィールドから実装させます。`agent/localtrait` で入れた
  `trait_capture_accessors` はまさにその形なので、`$outer` にも同じ仕組みを
  被せるのが筋です。ローカル trait でも同じ症状になります
  （このスライスの前は同じコードが `AbstractMethodError` で落ちていたので、
  悪化はしていません）。

- **ローカルクラスの `InnerClasses` の `inner_name`**（`agent/localtrait`）。
  binary name は nsc と同じ `Main$LocalC$1` になりましたが、`InnerClasses` の
  `inner_name` は nsc が索引つきの `LocalC$1`、こちらは元の `LocalC` です。
  そのため**ローカルクラスの `getSimpleName()` だけが nsc と食い違います**
  （`Main$LocalC$1` の `EnclosingMethod` / `isLocalClass` / `isMemberClass` は
  一致します）。`crates/cli/tests/innerclasses.rs` の
  `inner_local_class_has_no_outer` に書いてあります。

- **ローカル宣言がスコープの外から見えてしまう**（`agent/localtrait` で確認。
  main でも同じ既存の穴）。

  ```scala
  object Main { def mk(): Unit = { trait Local { def l = "l" } } }
  class TopUser extends Main.Local   // scalac: type Local is not a member of object Main
  ```

  実 scalac は拒否しますが、こちらは通してしまい、親を持たない `TopUser` を
  出します。ローカル宣言のシンボルの所有者はメソッドなのに、`Main.Local` の
  名前解決がそこまで届いてしまうためです。**逆向き**（トップレベルのクラスが
  ローカル trait を実装する）はそもそも Scala では書けない形なので、
  `agent/localtrait` の fixture でも扱っていません。

- ~~**引数位置の `try` と `while` の StackMapTable**~~ →
  `agent/loopframe` で直しました。**同じ根ではなく、別々の 2 件**でした
  （「ループ先頭のフレームとオペランドスタックの上の `try`」節）。

### ループ先頭のフレームとオペランドスタックの上の `try`（`agent/loopframe`）

型検査は通るのに**クラスロード時に落ちる** 3 件です。同じ根だと思われていた
最初の 2 件は**別々の原因**でした。3 件目はそれを追ううちに見つけた既存の穴です。
いずれも `crates/cli/tests/loopframe.rs` と `lf_*` fixture で押さえています。

#### 1. ループを跨ぐローカルのフレームは「宣言型」

```scala
var c: Option[Int] = Some(1)
while (c.isDefined) { c = None }
```

が `VerifyError: Bad type on operand stack` になっていました。スロットは
入口で `scala/Some`、後ろ向き分岐で `scala/None$` を持つので、無関係な 2 クラス
の合流はこのアセンブラでは `java/lang/Object` になります。フレームとしては
正しいのですが、そのスロットを読む
`invokevirtual scala/Option.isDefined` には**緩すぎ**ます。

実 scalac 2.13.16 に `javap -v -c` をかけると答えが書いてあります。

```text
  StackMapTable: number_of_entries = 2
    frame_type = 252 /* append */
      offset_delta = 12
      locals = [ class scala/Option ]
  LocalVariableTable:
     Start  Length  Slot  Name   Signature
        12      23     2     c   Lscala/Option;
```

`class scala/Option`——`LocalVariableTable` と同じ、スロットの**宣言型の消去**です。
scalac は `Some` と `None$` の最小上界など計算していません。ローカルの型は
生存期間を通じて 1 つで、どのフレームもそれを繰り返すだけです。宣言型は
ソースがそこに書き込みうるものすべての上界なので、クラス階層を持たなくても
求まり、必要以上に広がることもありません。同じ規則を採りました
（`declare_local_ty` → `Assembler::set_local_class`）。

**合流のときだけでは足りません**。このアセンブラはフレームを 1 回の前向き
パスで書くので、後ろ向き分岐を見る前に書き終えたフレームは入口の型のままです。

```scala
var a: Any = 1
while (i < 2) { a = if (i == 0) "s" else 2; i += 1 }
```

はループ先頭こそ正しく `java/lang/Object` に合流したのに、条件式の中で先に
出していたフレームが `java/lang/Integer` のままで
`VerifyError: Inconsistent stackmap frames` でした。**書き込みのたびに**
宣言クラスに揃えるのが要点で、`java/lang/Object` も（`var a: Any` の実際の
宣言型として）他と同じく宣言クラスとして扱います。

#### 2. オペランドスタックが空でない位置の `try`

```scala
println(try { "y" } catch { case _: Throwable => "no" })
two("p", try { "q" } catch { case _: Throwable => "no" })
new Box(try { "a" } catch { case _: Throwable => "b" })
```

JVM は例外ハンドラに入るときオペランドスタックを捨てます（JVMS 4.10.1.6）。
`try` の前に積んであったもの——`Predef$` のレシーバ、先に評価した引数、
`new` が残した**未初期化**参照——は catch 側では消えているので、`try` の後の
合流点でスタック段数が片側 n・片側 0 になり
`VerifyError: Inconsistent stackmap frames` でした。`println` が jar モード
だけ通っていたのは、そこだけ引数を先に評価して `swap` する形だったからで、
`two("p", try …)` は両モードで落ちていました。

`javap -c` で見ると scalac は `LiftTry` フェーズで `try` を合成メソッド
`private static final java.lang.String liftedTree1$1()` に持ち上げ、引数位置
からはそれを呼びます。こちらは保護区間のあいだ**積んであった値をローカルへ
退避**する形にしました（`spill_operand_stack` / `restore_operand_stack`）。
メソッドを増やさずに済み、`new` の未初期化参照も——検証器は
`uninitialized(Address)` をローカルに置くことを許すので——そのまま扱えます。

#### 3. 親コンストラクタ呼び出しのあとの `this`（上の 2 件を追ううちに見つけた既存の穴）

```scala
class B(val s: String)
class C(n: Int) extends B("b") {
  val sign: String = if (n > 0) "pos" else "neg"
}
```

が `VerifyError: Bad type on operand stack in putfield` /
`Type 'B' … is not assignable to 'C'` でした。JVMS 4.10.1.9 では
`invokespecial <init>` は `uninitializedThis` を**検証中のクラス**の型に
置き換えますが、こちらは**呼んだ側のクラス**（＝親の `B`）に置き換えていました。
親コンストラクタ呼び出しのあとにフレームが必要なコンストラクタ——分岐・ループ・
`try` を本体に持つもの——はすべてこれで落ちていました
（`Assembler::initialize`。fixture は `lf_ctorframe.scala`）。

計測は `files=184 errors=346 files_with_errors=64` → **変わらず**（診断の中身も
一字一句同じ）。slick は型検査で止まっていて classfile を 1 つも出していない
（`classes=0`）ので、バックエンドだけを直したこのスライスで数字が動かないのが
正しい姿です（`agent/unitbox` と同じ）。動かしたのは**出したコードが JVM に
ロードできるか**です。

#### Remaining

- **フレームは今も `full_frame` だけ**です。scalac は `append` / `same` /
  `same_locals_1_stack_item` に圧縮するので、同じ内容でも classfile は
  こちらの方が大きくなります。検証器はどちらも受け付けるので正しさの問題では
  ありませんが、`javap -v` の出力を 1 行ずつ比較することはできません
  （`crates/cli/tests/loopframe.rs` が「フレームに現れるクラス」の集合で
  突き合わせているのはこのためです）。
- **`lf_loopvar.scala` は jar 限定**です。私有ランタイムに可変長の
  `List.apply` が無いためで（`value apply is not a member of List$`）、
  フレームの話とは無関係の既存の穴です。`Option` の `toString` が
  case class のものでないのも同じ（`lf_trystack.scala` はそこを避けて
  両モードで走ります）。

### ファクトリメソッドの戻り型と `…Ops` への erasure（`agent/fillconcat`）

型検査は通るのに実行時に落ちる 1 件から始まりました。

```scala
object Main { def main(a: Array[String]): Unit = println(List.fill(2)(5) ::: List(9)) }
```

```
java.lang.VerifyError: Bad type on operand stack
  Reason: Type 'scala/collection/SeqOps' (current frame, stack[1]) is not
          assignable to 'scala/collection/immutable/List'
```

**`List.fill` の戻り型は間違っていません**。`javap -p` で jar を見ると
`List$` には

```
public scala.collection.SeqOps fill(int, scala.Function0);
public java.lang.Object       fill(int, scala.Function0);
```

の 2 本があり、実 scalac 2.13.16 も**前者**を呼びます
（`StrictOptimizedSeqFactory[+CC[_] <: SeqOps[…]]` なので `CC[A]` は上限の
`SeqOps` に erase される）。違いは**その次の 1 命令**だけでした。

```
# scalac
invokevirtual List$.fill:(ILscala/Function0;)Lscala/collection/SeqOps;
checkcast     scala/collection/immutable/List      ← これが無かった
```

**根本原因**は `crates/backend/src/gen.rs` の `maybe_unbox_erased_result` の
判定です。ディスクリプタの戻り型が結果型の erasure より広いときに
`checkcast` を足す規則はもともとありましたが、条件が「**結果型が、宣言された
戻り型に prelude の継承関係で到達できるか**」でした。`crates/typer/src/prelude_hier.rs`
は `SeqOps` / `IterableOps` などの `…Ops` トレイトを**意図的に**階層から
外しているので（メンバを持たないので長さが増えるだけ、と書いてある）、
`List <: SeqOps` は永遠に示せず、`checkcast` は一度も出ませんでした。

そこで**決定可能な逆向きの問い**に変えました。「宣言された戻り型が、欲しい型に
**適合すると示せるか**」——示せなければ `checkcast` を出します。片側だけの判定
で、`false` は「適合しない」ではなく「示せない」を意味します。余計な
`checkcast` は 3 バイトの無駄で済みますが、足りない `checkcast` は
メソッド全体が `VerifyError` になるからです。

これは `List.fill` の話でも `:::` の話でもありませんでした。`:::` は
**右結合**なので `List.fill(2)(5) ::: List(9)` は `List(9).:::(List.fill(2)(5))`
であり、ファクトリの結果はレシーバではなく**引数**です。実際に壊れていたのは:

| 形 | 直す前 | 直したあと |
|---|---|---|
| `List.fill(2)(5) ::: List(9)` | VerifyError | OK |
| `List.tabulate(3)(i => i) ::: List(9)` | VerifyError | OK |
| `List.concat(List(1), List(2)) ::: List(9)` | VerifyError | OK |
| `List.fill(2)(5).head` / `.reverse` | VerifyError | OK |
| `Vector.fill(2)(5).length` | VerifyError | OK |
| `val xs = Vector.tabulate(5)(i => i * i); xs.updated(0, 99)` | VerifyError（ローカルのフレーム型が `SeqOps`） | OK |
| `ArrayBuffer.fill(2)(5).size` / `ListBuffer.fill(2)(5).size` | VerifyError | OK |
| `List.iterate` / `List.empty` / `List.unfold` | もともと OK | OK |
| `List.fill(2)(5) ++ List(9)` / `.length` / `.map` / `match` | もともと OK | OK |
| `Seq.fill` / `Set.fill` / `IndexedSeq.fill` / `Iterator.fill` / `Array.fill` | もともと OK | OK |
| `TreeMap(…) - key`（宣言は `Map`、結果型は `TreeMap`） | もともと OK（既存の規則） | OK |

`++` や `.length` が通っていたのは `gen.rs` が別経路で明示的な
ディスクリプタを書いていたからで、`fill` が特別だったわけではありません。

**fixture**（すべて library dual-run のみ。私有ランタイムには `IterableFactory`
が無いので、`--no-scala-library` では診断が出ることも
`crates/cli/tests/fillconcat.rs` で見ています）:

| fixture | 見ているもの | 期待 |
|---|---|---|
| `fc_factory.scala` | `List` / `Vector` / `Seq` / `Set` / `ArrayBuffer` / `ListBuffer` の `fill` / `tabulate` / `concat` / `iterate` / `empty` / `unfold` を `:::` の引数・レシーバ・`val` 経由・型注釈付き・`match` のスクルーティニで使う | `List(5, 5, 9)` ほか 22 行 |
| `fc_ops.scala` | `TreeMap - key` / `TreeSet` / `SortedSet` / `SortedMap`、可変バッファ、`LazyList` / `Queue` / `Iterator`、`sum` / `sorted` / `zip` / `toArray` | `1` `2` `List(1, 2, 3)` ほか 25 行 |
| `fc_local.scala` | ファクトリの結果を `val` に束縛してから複数のメソッドを呼ぶ形（落ちるのはこの形だけで、1 回使って捨てる分には落ちなかった）。`Seq[Int]` への widening と `def` の戻り値経由も | `Vector(0, 1, 4, 9, 16)` ほか 18 行 |
| `fc_factory_bad.scala`（異常系） | 足した `checkcast` が型エラーを飲み込まないこと（`Vector[Int]` に `List[Int]` を入れる、`List[String]` に `List[Int]`、`::: Vector(9)`） | コンパイルエラー 3 件 |

計測は `files=184 errors=346 files_with_errors=64` → **変わらず**。slick は
型検査で止まっていて classfile を 1 つも出していない（`classes=0`）ので、
バックエンドの codegen を直したこのスライスでは数字が動かないのが正しい姿です。
動かしたのは**出したコードが JVM を通るか**であって、通る本数ではありません。

#### Remaining

- ~~**`List.range` / `Vector.range` / `Seq.range` に `Integral[Int]` が無い**~~
  → 次節 `agent/integral` で解消しました。`fillconcat.rs` のテストは
  `range_resolves_the_integral`（通るようになった形）に書き換えてあります。
- `Array.range(0, 3)` は `Integral` を取らない別オーバーロードなので通ります。

### `Integral` / `Fractional` を型クラス階層に入れる（`agent/integral`）

前節が残件にした 1 件です。

```scala
println(List.range(0, 5))   // error: no implicit: could not find implicit value of type Integral[Int]
println(Vector.range(0, 3)) // 同上
println(Seq.range(0, 3))    // 同上
```

`IterableFactory#range[A](start: A, end: A)(implicit ord: Integral[A])` が
実シグネチャで（`javap -p scala.collection.IterableFactory`）、その下に
**2 つ**穴がありました。

1. `Integral` / `Fractional` が prelude の時点で symbol table にいない。
   ソースが名前を出すと `pickle_supply` がスタブを起こしますが、pickle 由来の
   親（`Numeric`）を付けるのは `attach_parents`＝**メンバ解決に失敗したとき
   だけ**です。`SCALA_RS_PICKLE_DEBUG=1` で見ると
   `#quot: asking Integral` → `attaching pickled parent Numeric` の順で、
   subtyping の判定にはまるで間に合っていませんでした。だから
   `def f(x: Integral[Int]): Numeric[Int] = x` が `type mismatch` でした。
2. `object Numeric` の implicit インスタンスに `Numeric[Int]` を付けていた。
   実 ABI はもう 1 段下です。

`javap -p -s /tmp/scala-rs-lib/scala-library-2.13.16.jar` で確かめた形:

```
interface scala.math.Numeric<T>    extends scala.math.Ordering<T>
interface scala.math.Integral<T>   extends scala.math.Numeric<T>
interface scala.math.Fractional<T> extends scala.math.Numeric<T>
```

| implicit object（`Numeric$…$`） | implements | その trait の親 | 与えた型 |
|---|---|---|---|
| `IntIsIntegral$` | `Numeric$IntIsIntegral`, `Ordering$IntOrdering` | `Integral<Object>` | `Integral[Int]` |
| `LongIsIntegral$` | `Numeric$LongIsIntegral`, `Ordering$LongOrdering` | `Integral<Object>` | `Integral[Long]` |
| `ByteIsIntegral$` | `Numeric$ByteIsIntegral`, `Ordering$ByteOrdering` | `Integral<Object>` | `Integral[Byte]` |
| `ShortIsIntegral$` | `Numeric$ShortIsIntegral`, `Ordering$ShortOrdering` | `Integral<Object>` | `Integral[Short]` |
| `CharIsIntegral$` | `Numeric$CharIsIntegral`, `Ordering$CharOrdering` | `Integral<Object>` | `Integral[Char]`（新規） |
| `BigIntIsIntegral$` | `Numeric$BigIntIsIntegral`, `Ordering$BigIntOrdering` | `Integral<BigInt>` | `Integral[BigInt]`（新規） |
| `DoubleIsFractional$` | `Numeric$DoubleIsFractional`, `Ordering$Double$IeeeOrdering` | `Fractional<Object>` | `Fractional[Double]` |
| `FloatIsFractional$` | `Numeric$FloatIsFractional`, `Ordering$Float$IeeeOrdering` | `Fractional<Object>` | `Fractional[Float]`（新規） |
| `BigDecimalIsFractional$` | `Numeric$BigDecimalIsFractional`, `Ordering$BigDecimalOrdering` | `Numeric$BigDecimalIsConflicted`, `Fractional<BigDecimal>` | `Fractional[BigDecimal]`（新規） |

「どれが implicit として実際に選ばれるか」は jar の形だけでは決まらないので
（`BigDecimalAsIfIntegral` / `FloatAsIfIntegral` のように implicit でない
兄弟がいる）、実 scalac に `implicitly[…].getClass.getName` を出力させて
1 件ずつ確かめました。

実装は `crates/typer/src/prelude_numhier.rs` に閉じています
（`Integral` / `Fractional` を prelude に用意して `<: Numeric[T]` の辺を張り、
`add_numeric` が付けた型を上書きし、足りないインスタンスを足す）。
`prelude.rs` 側の変更は呼び出しに `library_abi` を渡す 1 行だけです。
`quot` / `rem` / `div` は `pickle_supply` が jar から供給するので手書きしていません
（`Integral` の型パラメータ名を実ライブラリと同じ `T` にしておく必要があります。
`pickle_supply` は名前でスコープを作るので、違う名前だと `quot(T, T): T` を写せません）。

#### なぜ曖昧にならないか

`Numeric[T] extends Ordering[T]` なので、`Integral[Int]` を導入すると
`Ordering[Int]` に適合する値が 1 つ増えます。**それでも候補は増えません**。
`Ordering[Int]` の implicit scope（SLS 7.2、`implicits.rs` の
`collect_type_parts` / `companion_implicits`）は `Ordering` とその基底クラス、
および `Int` の companion であって、**`Numeric` の companion は入らない**
からです。実 scalac も `implicitly[Ordering[Int]]` に `Ordering$Int$` を返し、
`Numeric$IntIsIntegral$` を返しません。fixture `ig_hier.scala` は
`implicitly[…].getClass.getName` を 13 件出力して real scalac と
バイト単位で比較しているので、「一意だと主張する」ではなく
「**実 scalac と同じものを選んでいる**」ことを見ています。
`crates/cli/tests/integral.rs` の `ambiguity_did_not_increase` が
`Ordering[Int/Double/Long/Byte/Short/Char/Float]` と `sum` / `product` /
`sorted` / `max` / `min` / タプルの `sorted` に `ambiguous` が出ないことを固定します。
slick でも `ambiguous` 8 件は**行単位で完全に同一**でした。

#### 併せて塞いだ prelude の穴

- `Numeric[Float]` / `Numeric[BigDecimal]`（`agent/mismatch8` が
  `no implicit` 27 件の一部として報告していたもの）。
- `Ordering.Option`（`implicit def Option[T](implicit ord: Ordering[T]):
  Ordering[Option[T]]`、jar では
  `Ordering$.Option:(Lscala/math/Ordering;)Lscala/math/Ordering;`）。
  `List(Some(2), None, Some(1)).sorted` が通るようになりました。
  `Ordering.TupleN`（`prelude_ordtuple.rs`）と同じ形の穴です。slick では
  `Ordering[Option[String]]` と、それを要素に持つ
  `Ordering[Tuple4[String, Option[String], Option[String], String]]` の
  2 件がこれで消えました（`Ordering.Tuple4` は既にあったのに、その
  implicit 引数の `Ordering[Option[String]]` が埋まらず落ちていた）。

#### 私有ランタイム

`--no-scala-library` には `scala/math/Integral` の classfile も
`Numeric$IntIsIntegral$` もありません。読み込めないクラスを参照する
バイトコードを出さないよう、`prelude_numhier::install` は `library_abi`
でない場合に**何もせず戻ります**。`ig_hier.scala` を `--no-scala-library`
でコンパイルすると `not found: type Integral` /
`range is not a member of List$` が出ることを
`range_is_diagnosed_without_the_jar` が固定しています。

#### fixture

| fixture | 見ているもの | 期待 |
|---|---|---|
| `ig_hier.scala` | `List`/`Vector`/`Seq`/`Long` の `range`、`implicitly` 13 件のクラス名、`quot`/`rem`/`div`、`Numeric[T]` を取るユーザーコード、`sum`/`product`/`sorted`/`max`/`min`/`sortBy`、`Integral[Int]` → `Numeric[Int]` / `Ordering[Int]` の widening、`Ordering[Option[Int]]` | 42 行（real scalac 2.13.16 と一致） |
| `ig_hier_bad.scala`（異常系） | `Numeric[Int]` → `Integral[Int]` の逆流、`Ordering[Int]` → `Numeric[Int]` の逆流、実在しない `Integral[Double]` / `Fractional[Int]` / `Integral[String]`、`List.range("a", "z")` | コンパイルエラー 6 件（real scalac も同じ 6 行で 6 件） |

計測は `files=184 errors=346 files_with_errors=64` →
**`files=184 errors=342 files_with_errors=64`**（`no implicit` 26 → 22）。
減った 4 件は `Numeric[Float]` / `Numeric[BigDecimal]` /
`Ordering[Option[String]]` /
`Ordering[Tuple4[String, Option[String], Option[String], String]]` で、
**増えた診断は 1 件もありません**（`grep '^error' | sort | uniq -c` の差分が
この 4 行の削除だけ。`ambiguous` の 8 行は行単位で完全に同一）。

#### Remaining

- slick は `Integral` / `Fractional` を使っていないので、減ったのは
  `Numeric` / `Ordering` の穴 4 件だけです。残る `no implicit` 22 件は
  別の型クラス（`ClassTag` / cats など）です。
- 明示的に書く `Ordering.by(...)` は通ります。`Ordering.Iterable` は
  implicit 探索に入れていませんが、`implicitly[Ordering[List[Int]]]` は
  **real scalac 2.13.16 も拒否する**（`Ordering` は不変で
  `Ordering[Iterable[Int]]` は `Ordering[List[Int]]` にならない）ので、
  今のところ差はありません。
- `import Numeric.Implicits._` で `a + b` を書く形は通りません
  （`+` が `String` の連結として解決され `type mismatch` になる）。
  この修正の前後で挙動は同じで、`n.plus(a, b)` の形は通ります。
- `Numeric.BigDecimalAsIfIntegral` のような **implicit ではない**
  インスタンスを名前で書くと、型検査は通るのに `pickle_supply` が
  `Numeric$` のフィールドとして供給してしまい、実行時に
  `NoSuchFieldError: BigDecimalAsIfIntegral` になります
  （正しくは `Numeric$BigDecimalAsIfIntegral$.MODULE$`）。
  **この修正の前後で同じ**（`agent/integral` 以前のバイナリでも再現）で、
  pickle 由来の `object` メンバの形の問題です。implicit として選ばれる
  9 個は prelude 側で module として持っているので影響を受けません。

### テンプレート本体の式文（`agent/ctorstmt`）

`class A { println("ctorA") }` は型検査を通り、classfile も出て、**何も印字せずに**
走っていました。`val` / `var` / `def` 以外のテンプレート本体の文（裸の式文）が、
主コンストラクタにも trait の `$init$` にもモジュール初期化にも**一切入っていません**
でした。診断は出ないので、気づけるのは実行結果の差だけです。

SLS 5.1 / 5.3 では、テンプレート本体の文はテンプレートの**初期化子**の一部です。

- **class**: 主コンストラクタの中で、`val` / `var` の初期化と**宣言順に交互に**走る
- **trait**: `$init$` の中に入り、mixin 時に線形化順で走る
- **object**: モジュール初期化（`MODULE$` を作るとき）に 1 度だけ走る

原因は backend の 3 か所で、テンプレート本体を `ValDef` だけに絞っていたことです
（`crates/backend/src/gen.rs`）。

| 場所 | 直す前 | 直したあと |
|---|---|---|
| `emit_class_ctor` | `body.filter(ValDef)` | `template_init_stats(body)`（`ValDef` ＋裸の文をソース順に） |
| `emit_module_init` | 同上 | 同上 |
| `emit_trait_init` | `trait_vals`（`val` のみ） | `trait_inits`（`val` ＋裸の文をソース順に） |

`ValDef` は今までどおり `gen_expr` ＋ `putfield`（trait なら mixin setter）で、
裸の文は `gen_stat` で出します。`gen_stat` は値を残す式を捨てる既存の経路なので、
`if` / `match` / `try` を**文の位置**で生成する（`expectedType = UNIT`）扱いも
そのまま乗ります。

`trait_vals` はアクセサ・mixin フォワーダの生成にも使われていて、そちらは `val` だけを
見たいので、`$init$` が実際に走らせる並びは別のマップ `trait_inits` に持ちます。
`T$class` を出すかどうかと、実装クラスが `$init$` を呼ぶかどうかの判定も
`trait_inits` に切り替えました。これが無いと `trait T1 { note("T1") }` のように
**本体が文だけの trait** に `$init$` がそもそも生成されません。

`extends App` / `DelayedInit` の経路は元から `is_delayed_ctor_stat` で裸の文を
拾っていたので、そちらは変わりません。

#### `val x: T` の次の行の文が型に飲まれていた（パーサ）

同じ「文が消える」現象のパーサ側の変種も直しました。

```scala
trait A {
  val p: String
  println("x")     // ← String println "x" という中置型になっていた
}
```

`parse_compound_type` が、`with` と refinement の `{` を探すために改行を**無条件で**
読み飛ばしていたため、文を切る NEWLINE まで消えて、次の行の識別子が中置型
コンストラクタとして食われていました。nsc の `newLineOptWhenFollowedBy(LBRACE)` は
「改行の次が本当に `{`（あるいは `with`）のときだけ読み飛ばす」ので、同じ
`newline_opt_when_followed_by` を入れました（`crates/parser/src/parse.rs`）。
直す前は `not found: type +` という無関係な診断になるか、右辺のある `val` なら
文が黙って消えるかのどちらかでした。

#### 検証

fixture 接頭辞は `cs`、テストは `crates/cli/tests/ctorstmt.rs` です。
どれも**私有ランタイムと jar の両モード**で `java -Xverify:all` して、
実 scalac 2.13.16 の出力と突き合わせます。

| fixture | 中身 | 期待 |
|---|---|---|
| `cs.scala` | class / trait / mixin / object の文、文と `val` の交互配置（class と trait の両方）、本体が文だけの trait、抽象 `val` のあとの文、本体の `var` を後続の文で更新、`O.v` を 2 回触ってもモジュール初期化は 1 回 | `A;T1;T2;B;` ほか。実 scalac と完全一致 |
| `cs_forms.scala` | 早期の `require` / `assert`、`if` / `match` / `try` / `while` / ラムダ、`case class` 本体、ローカルクラス、匿名クラス（`new AnyRef { … }` と抽象メンバを実装する `new Greeter { … }`）、`$outer` 経路のメンバ `object` | 実 scalac と完全一致 |
| `cs_bad.scala`（異常系） | class 本体の文の `notAMethod(1)`、trait 本体の文の `n.noSuchMember` | エラー 2 件（real scalac も同じ 2 行で 2 件） |

`javap -p -c` で読んだ実 scalac の形も固定してあります。`Main$B()` は
`invokespecial Main$A.<init>` → `T1.$init$` → `T2.$init$` → `Main$.note` の順で、
これは私たちの出力と（trait の `$init$` を interface の static ではなく `T$class`
に置くという既存の trait ABI の違いを除いて）同じです。

#### Remaining

- モジュールの初期化を、scalac は静的な `object` について `<clinit>` に畳んで
  static フィールドへ書きます。こちらは `<init>` に置いてインスタンスフィールドへ
  書きます（`agent/ctorstmt` 以前からの差）。どちらもモジュール初期化として
  ちょうど 1 度走るので、観測できる差はありません。
- 私有ランタイムの `require(cond, msg)` は例外メッセージに
  `requirement failed: ` を付けません（jar モードと実 scalac は付ける）。
  この修正とは独立の既存の差で、`cs_forms` はメッセージ本文に依存しない形に
  してあります。
- 早期定義（`new { val x = 1 } with T`）の中に**文**を書けるかどうかは
  触っていません。nsc は early definition block に文を許しませんし、
  こちらも `val` だけを pre-super に出す既存の経路のままです。
### slick の残エラーの小さい塊 3 つ（`agent/tail1`）

独立した 3 件を並行して見た結果です。テストは
`crates/cli/tests/tail1.rs`、fixture の接頭辞は `t1` です。

計測は `files=184 errors=327 files_with_errors=64` →
**`files=184 errors=305 files_with_errors=63`**（−22 件 / −1 ファイル）。
3 つの塊それぞれの内訳:

| 塊 | before | after |
|---|---|---|
| `value ExitCase is not a member of Resource$` / `Outcome.Succeeded` 系 | 6 件 | **1 件**（多ファイル限定の残件、後述） |
| `value getOrElse is not a member of Product` | 4 件 | 4 件（**直せていません**、後述） |
| `not found: value fromInt` | 3 件 | **0 件** |

減った差分（−22）には上記 3 塊の直接分（6→1 で −5、3→0 で −3）に加えて、
`fromInt` が見つからないことの巻き添えで出ていた `no implicit` などの
カスケードした診断も含まれます。

#### 1. `value X is not a member of Y$`（jar のコンパニオン + パッケージオブジェクト `val`）

`agent/companionkind` の README 注記（「残っている隣接した穴」）が原因だと
名指ししていた `InnerClasses` の `outer_class_info_index` は、実は**原因では
ありませんでした**。`parse_inner_classes` を拡張して確かめましたが、
`Resource$ExitCase$Succeeded$` のようなクラスは `InnerClasses` の**自分自身の
エントリ**を見ても outer が常に正しく `Resource$ExitCase$`（引く側の
コンパニオン）を指しており、区別できないケースは実際には踏んでいません。

**本当の原因は `type_select`（`crates/typer/src/check.rs`）の
メンバ探索フォールバックでした**。見つからないとき
`complete_binary_member(qual.sym, name, span)` を呼んでいましたが、
`qual.sym` は `Box.Const` の `Box` が**パッケージオブジェクトの `val`**
（`val Box = tiny2.Box`。`cats.effect` の `package object effect` が
`Resource` / `Outcome` にまさにこの形を使っています）のときその
**val 自身のシンボル**で、`jvm_name` が空です。空の名前から組み立てた
候補（`$Const` 相当）は当然何にも一致しません。`Box.of`（`Box$` の
直接メンバ、jar 読み込み時に埋まる）が通って `Box.Const`（コンパニオンの
入れ子）だけ落ちていたのはこのためで、直入インポート
（`import tiny2.Box`、`qual.sym` がモジュールそのもの）では再現しません。

`recv_ty`（val の**型**。`class_sym_of` で `ModuleRef` から実体の
モジュールクラスへ解決できる）を先に試すよう変えたところ、芋づる式に
4 つの隣接した穴が見つかりました:

1. **`complete_binary_member` の候補ループが最初に見つかった JVM 名で
   `return` していた**。`Const` / `Const$` のように**クラスとその
   コンパニオンの両方**が存在するとき、クラスの方が先にヒットして
   `return` し、コンパニオン（`apply` を持つ方）が永遠にインストール
   されません。`Box.Const(5)` は `value apply is not a member of Const`
   になっていました。全候補を試すように変更。
2. **総称シグネチャの `scala/runtime/Nothing$` を `Type::Nothing` に
   していなかった**。`case object Canceled extends Outcome[Nothing]`
   のクラスファイル `Signature` は `Nothing` を書けず
   `Lscala/runtime/Nothing$;`（実行時のプレースホルダクラス）と書きます。
   `jtype_to_type`（`classpath.rs`）はこれを普通のクラス扱いしていたので、
   `Outcome[Nothing] <: Outcome[Int]` の判定が
   `is_sub_type(Nothing$_stub, Int)` になり **共変** `Outcome[+A]` でも
   落ちていました（`type mismatch; found: Canceled$ required: Outcome[Int]`）。
   `parse_field_ty`（ディスクリプタ用）は既にこの変換をしていたので、
   総称シグネチャ側にも同じマッピングを足しました。
3. **jar から読んだクラスの型パラメータに分散（variance）が付かなかった**。
   JVM の総称シグネチャは分散を書けません（コンパイル時だけの概念）。
   分散は **pickle** にしかないのに、`adopt_tparam_kinds`
   （`pickle_supply.rs`）は arity だけ引き継いで分散を捨てていました。
   2 の Nothing 修正だけでは `Outcome[+A]` が実は不変扱いのままで
   同じ症状が残るので、`TParam::variance` から `Flags::COVARIANT` /
   `CONTRAVARIANT` を立てるようにしました。
4. **パッケージオブジェクトの `val` はクラスファイル上ただの 0 引数
   メソッドで、`def` と見分きが付かない**。`Resource.ExitCase` の
   ような `p.T` 型（SLS 3.2.3、`p` は stable path が必須）は
   `Resource` が安定していないと「stable identifier required」に
   なります。安定性は **pickle の `pflags::STABLE`** にしかないのに、
   `adopt_binary_class` は pickle の `MemberKind::Val` を丸ごと無視
   （`Def` だけ処理）していました。`Val` も処理対象に加え、
   `pflags::STABLE` を立っている宣言に `Flags::ACCESSOR` を付け、
   `ident_is_stable` / `member_is_stable` がそれを stable の根拠として
   読むようにしました。加えて `import_named`（`import p.{Resource}` の
   処理そのもの）が pickle 適用より先にクラスファイル由来の生シンボルを
   scope に固定してしまう順序の穴もあり、import 処理の中で先に
   `adopt_binary_class` を呼ぶようにして塞ぎました。
   `type_select_is_term_prefix` も、型エイリアスと val が同じ名前を
   共有するとき（`type Box[A] = …; val Box = …`）型側があるだけで
   term 読みを**拒否**していたので、`p.T` の `p` は常に term として
   読む（SLS の規定どおり）よう直しました。`new Outer.Inner()`
   （オブジェクトのみでコンパニオンの val が無いケース）の既存の
   優先順位（`qualified_type_owners`）は壊さないよう、
   `SymKind::Module` はこの判定に含めていません。

`project_from_prefix`（`p.T` の型解決）にも `complete_binary_member`
フォールバックを足しましたが、`type_select` 側の同種のフォールバックは
**`Type::ModuleRef` のときだけ**に絞りました。`Type::Class`（例:
`Type::String`）に対して無条件に `complete_binary_member` を呼ぶと、
その `owner.kind == Class` 分岐が `ensure_java_loaded` を呼んで
`java.lang.String` の**生クラスファイル全体**を強制的にロードしてしまい、
JDK 11 の `lines(): Stream[String]` が 2.13 の非推奨
`StringOps.lines: Iterator[String]` を隠してしまいました
（`e2e.rs` の `scala_library_dual_run_string_ops4` が退行として捕まえた
ので、そこで絞り込みに気付きました）。

**残っている隣接した穴**: slick の実ソース（`BasicBackend.scala` の
`closeStreamIteratorAndRelease`）で `Resource.ExitCase` 型注釈が
**1 件**だけまだ落ちます。自作の再現（`tail1.rs`、2 段のネスト・
パッケージオブジェクト経由・共変トレイト付き）は real scalac も
通し、私たちの binary も通ります。単一ファイルにも数ファイルの
組み合わせにも縮小できず、slick の 184 ファイル全体でしか再現しません。
これ以上の追跡はこのスライスの範囲外としました。

#### 2. `value getOrElse is not a member of Product`（**直せていません**）

`slick/jdbc/PositionedResult.scala` の `nextBlobOption() getOrElse(…)`
（`{ … val rr = if (rs.wasNull) None else Some(r); …; rr }` という
戻り型注釈の無いブロック）が原因です。**16 個ある同型の `nextXxxOption()`
のうち `Blob` / `Bytes`（`Array[Byte]`）/ `Clob` / `Object` の 4 個だけ**
落ち、`Boolean` / `Int` / `String` / `Date` / `BigDecimal` など残り 12 個は
通ります。

`abstract class … extends Closeable`、`import PositionedResult.
SqlNullException`（コンパニオンの前方参照）、`java.sql.{Blob, Clob,
ResultSet}` の実クラスファイルまで再現した縮小版を何本も作り、
すべて real scalac でも私たちの binary でも**通ってしまいました**
（`None` / `Some(r)` の lub、`Blob` / `Clob` の on-demand ロード、
`getObject` の総称オーバーロードなど疑った箇所はどれも単独では
再現しません）。SlickException / GetResult / GlobalConfig など
slick 内部の依存を数ファイル足して近づけても、今度は無関係な
未解決エラーのカスケードで埋もれてしまい、`Blob`/`Bytes`/`Clob`/
`Object` だけが特別扱いされる理由には辿り着けませんでした。
slick の 184 ファイル全体という状態に依存するらしい点は 1 の残件と
同じ形ですが、こちらは真因の見当すら付いていません。
**推測でスタブ的な回避はしていません**。次に見る人への手がかりとして
`tail1.rs` の doc comment に縮小の試行錯誤を記録しています。

#### 3. `not found: value fromInt`

`import integral._`（`Integral[T]` の implicit）の後で裸の `zero` /
`one` / `fromInt(n)` を呼ぶ形です。`Numeric[T]` はコンパイル済み
scala-library の**pickle** にしかメンバが無い（クラスファイル自体には
対応する入れ子クラスが無い）標準ライブラリの trait で、
`expose_unqualified`（`check.rs`）のワイルドカードインポート
フォールバックは `complete_binary_member` だけを呼んでいました。
1 で見たとおり、これは「入れ子クラスファイルを探す」ためのもので、
`fromInt` のような**ただのメソッド**は最初から見つけようがありません。
`import_wildcard`（インポート時の即時コピー）は「その時点で既に
`owner.members` にあるもの」しか拾わないので、まだ誰も触れていない
`fromInt` はコピーに乗らず、後で参照されたときの遅延フォールバックに
すべてがかかっていました。

直った理由が奇妙なのは、`zero` / `one` は再現に成功したことです
（`crates/cli/tests/tail1.rs::fixtures_t1_wildcard_inherited` で
`zero` / `one` / `fromInt` を**3 つとも**使う最小再現を作ったところ、
修正前は**3 つとも**「not found」でした。slick のソースでは
`zero` / `one` はたまたま同じメソッド本体の中で先に別の形で触れられて
いたため通っていた、と見られます）。修正は
`expose_unqualified` のワイルドカードフォールバックに、
`complete_binary_member` が失敗したときの次善策として
`PickleSupply::complete`（普通のメンバ選択 `x.zero` が既に使っている
pickle 経路）を足しただけです。`scala/` で始まる jvm 名は
`complete_named` の中で無条件に許可されているので、追加の
adopt は要りません。

#### 触らなかった領域

`agent/mismatch9`（`type mismatch` 一般）と `agent/quasi`
（quasiquote / マクロ）には触れていません。

#### fixture

`crates/cli/tests/tail1.rs`:

- `a_nested_member_through_a_package_object_val`: 実 scalac で
  `t1lib.Box` / `t1lib.Outcome`（コンパニオンの入れ子 `Const`、
  `Outcome[+A]` を継承する `case object Canceled extends Outcome[Nothing]`）
  を jar に固め、`t1lib.alias`（`type` + `val` を同名で持つ
  パッケージオブジェクト）経由でしか触らないユーザーコードをコンパイル・
  実行して `java -Xverify:all` を通す。`bogus` メンバが無いことを
  拒否する異常系も見る。
- `real_scalac_accepts_the_same_program`: 同じ 3 ファイルを real scalac
  だけでコンパイル・実行し、同じ標準出力になることを確認する
  （fixture が「私たちのコンパイラの癖」ではなく正しい Scala だという裏付け）。
- `fixtures_t1_wildcard_inherited` / `real_scalac_accepts_
  t1_wildcard_inherited`: `tests/fixtures/t1_wildcard_inherited.scala`
  （`import integral._` の後で `zero` / `one` / `fromInt` を使う
  ループ）を `--scala-library` と real scalac の両方でコンパイル・実行し、
  `tests/fixtures/expected/t1_wildcard_inherited.txt` と一致することを見る。

### `Seq` は `Int => A`（`agent/seqfn`）

```scala
val s = List(10, 20, 30)
println(List(0, 2).map(s))            // List(10, 30) -- List を Int => A として渡す
val f: Int => Int = List(10, 20, 30)  // 代入も通る
List(1, 2).isDefinedAt(5)             // false
```

が `type mismatch; found: List[Int]  required: (Int) => Int` になっていました。
`Map` を関数として渡す辺（`crates/typer/src/prelude_mism4.rs`、`Map[K, V] <:
Function1[K, V]`）は既にあったのに、`Seq` 側だけ抜けていました。

2.13 の `scala.collection.Seq[A]` は宣言そのものが `PartialFunction[Int, A]`
（したがって `Int => A`）を継承しています（`javap scala.collection.Seq`）:

```text
public interface scala.collection.Seq<A> extends scala.collection.Iterable<A>,
  scala.PartialFunction<java.lang.Object, A>, scala.collection.SeqOps<...>, scala.Equals
```

`Map` は `Function1` を直接の親にしていました（`PartialFunction` を挟むと
`toMap` の型検査が壊れる既知の理由があったため）。`Seq` にはその理由が無いので、
今回は真の階層どおり `Seq <: PartialFunction[Int, A] <: Function1[Int, A]` を
`crates/typer/src/prelude_seqfn.rs`（新規ファイル）で張りました。辺は
`scala/collection/Seq`（`prelude_hier.rs` が組み立てる、`List` / `Vector` /
`ArraySeq` / `Range` / `LazyList` / `Queue` / `mutable.Seq`（`Buffer` /
`ArrayBuffer` / `ListBuffer` を含む）などすべての共通祖先）1 箇所にだけ張り、
`base_type_seq` の推移的な親探索で下位のすべての具象コレクションに伝播します。

`PartialFunction` を親にしたことで、`Seq` は `isDefinedAt` / `applyOrElse` に加えて
`lift` / `orElse`（今まで `add_partial_function` に無かったので同じファイルで追加）
も継承します。`Seq[A]` は `SeqOps.apply(Int): A` と
`PartialFunction[Int, A].apply(Int): A` という「実体化後にしか区別できない」2 つの
`apply` を持つことになりますが、`s(1)` / `s.apply(2)` のような素の添字アクセスは
（`overload_member_types` の既存の仕組みにより）今までどおり `List` 自身の
具象 `apply` に解決され、`invokeinterface scala/collection/SeqOps.apply` を吐きます
（`Function1` として渡した先だけが `invokeinterface scala/Function1.apply` になる）。

`Array` は `Seq` そのものではなく、`Predef.wrapBooleanArray: Array[Boolean] =>
mutable.ArraySeq[Boolean]` という**暗黙変換**を経由して初めて `Seq` にたどり着きます
（`List(0, 2).filter(anArrayOfBoolean)` / `(2 to 30).filter(sieve)`）。この
`wrapBooleanArray` はプレリュードに丸ごと欠けていたので新設し（`prelude_seqfn.rs`）、
戻り値は実 jar の descriptor（`scala/collection/mutable/ArraySeq$ofBoolean`）に
一致させています（`mutable.ArraySeq` トレイト自体を返す版は型検査は通るのに
`NoSuchMethodError` で実行時リンクに失敗したため。`mutable.ArraySeq` は
`prelude_mutcoll.rs` が `AnyRef` だけを親にして手組みしていたので、`Seq` の祖先
`mutable.IndexedSeq` への辺もここで足しています）。`wrapIntArray` と同じ理由で
`IMPLICIT` にはしていません（`xArrayOps` と競合させないため）。ただし
`wrapXArray` は暗黙変換であって部分型付けではないので、`arg_score`（オーバーロード
候補の適用可否）と `adapt`（実際の呼び出し木の構築）の両方に専用のフックが要ります
（新規ファイル `crates/typer/src/seqfn_view.rs`）。

これらはすべて `library_abi` 専用です。私有ランタイム（`--no-scala-library`、
`crates/backend/src/runtime.rs`）の `scala/PartialFunction` は `isDefinedAt` /
`applyOrElse` しか持たない抽象インタフェースで、`lift` / `orElse` の default 実装が
無く、`List` / `Vector` などの private classfile も `scala/PartialFunction` /
`scala/Function1` を implements しません。型だけ通して実装の無い相手に
`invokeinterface` を飛ばす壊れたリンクを避けるため、非 jar モードでは今までどおり
`type mismatch` / `value isDefinedAt is not a member of ...` を診断します。

#### 検証

fixture 接頭辞は `sf`、テストは `crates/cli/tests/seqfn.rs` です。

| fixture | 中身 | 期待 |
|---|---|---|
| `sf.scala` | `List` / `Vector` / `mutable.ArrayBuffer` を `Int => A` として代入・引数の両方の位置で渡す、共変（`List[Dog] <: Int => Animal`）、`isDefinedAt` / `lift` / `orElse`、`wrapString` 経由の `String`、`wrapBooleanArray` 経由の `Array[Boolean]`（代入と `filter` の両方）（library モード、`java -Xverify:all`、期待出力は実 scalac 2.13.16 の stdout そのまま） | `20` `List(10, 30)` `c` `7` `Rex` `true` `false` `Some(2)` `None` `1` `-1` `c` `true` `false` `List(0, 2)` `List(0, 1)` |

`sf.scala` は `seqfn_fixture_dual_run` から回します。同ファイルには最小形の
受理テスト（`a_list_is_usable_as_int_to_a` /
`partial_function_members_reach_list_without_upstaging_its_own_apply` /
`vector_indexed_seq_and_array_buffer_are_all_usable_as_functions` /
`a_string_is_usable_as_int_to_char_via_wrapped_string` /
`a_boolean_array_is_usable_as_int_to_boolean` /
`a_list_of_a_subtype_is_usable_as_int_to_the_supertype`）も置いてあります。
逆に、緩めた規則が診断を飲み込まないことは `sf_bad.scala`（`List[Int]` を
`String => Int` に渡す／`List[Animal]` を共変性の効かない向きで `Int => Dog` に渡す）
で固定しています（`sf_bad_is_still_rejected`）。実 scalac 2.13.16 も両方拒否します。
`--no-scala-library` で今までどおりの診断が出ることは
`without_the_library_the_old_diagnostics_still_fire` で固定しています。

#### Remaining

- `Set[A] <: A => Boolean` も実在します（`SetOps` が `Function1[A, Boolean]` を
  継承）が、今回は張っていません（`prelude_mism4.rs` が `Map` のときに残した
  同じ判断で、オーバーロード解決・implicit 探索への波及を抑えるためです）。
- `Predef.wrapXArray` は `Boolean` 以外（`Int` は既存の `wrapIntArray` があります
  が `Seq` に繋がっていない `ArraySeq$ofInt` を返すだけで、`Byte` / `Short` /
  `Char` / `Long` / `Float` / `Double` / `Unit` / 参照型は丸ごと無し）は
  今回のスライスでは足していません。`Array[Int]` を `Int => Int` として渡す形は
  実 scalac は通りますがこちらはまだ `type mismatch` のままです。
- `Array` を関数として渡す変換は `arg_score` / `adapt` への専用フック
  （`seqfn_view.rs`）で、汎用の「引数位置でも implicit view を試す」機構では
  ありません。`arg_score` は元々 `is_sub_type` だけで判定しており、他の
  暗黙変換一般が引数位置で効かない同種の穴が残っている可能性があります。
- `s(1)` / `s.apply(2)` は今までどおり `invokeinterface
  scala/collection/SeqOps.apply` を吐きます（実 scalac は `invokevirtual
  scala/collection/immutable/List.apply` で、`agent/seqfn` 以前からの既存の差）。
  実行結果は同じで、`java -Xverify:all` も通ります。

`agent/nothingcall` スライス（結果型が `Nothing` の**呼び出し**――`sys.error(...)` /
`Predef.???` / 自前の `def die(): Nothing`――が `match` / `if` / `try` の腕や
ブロック末尾、メソッド本体全体、引数位置、ascription に来ると、型検査は通るのに
クラスロード時に `VerifyError` になっていた件）のフィクスチャは接頭辞 `nc`
（`nc_nothing` / `nc_nothing_sys`）で、同じ理由から `crates/cli/tests/nothingcall.rs`
に置いています。原因は 2 つ重なっていました。ひとつは、`Nothing` 型の式は
`jvm_sort` 上ずっと `Unit` と同じ「値を残さない」扱いなのに、`Nothing` を返す
**呼び出し**は JVM 上は普通に `scala/runtime/Nothing$`（もしくは呼び先が宣言する
プリミティブ記述子）への実参照を1つスタックに積むこと――`throw` 自身はこの型では
ないので影響がなく、`case _ => throw new RuntimeException(...)` は元から動いていました。
その幽霊参照が、`match`/`if` の腕の join、`try` の結果スロット、引数リストへ
そのまま流れ込み、他の腕が積んだ型（`Tuple2` や `Int` 等）と食い違って
`VerifyError: Inconsistent stackmap frames` になっていました。もうひとつは、
`jvm_desc`（メソッドの戻り型記述子を組み立てる関数）が `Nothing` を `Unit` と
同じ `V` に潰していたことで、`def die(): Nothing` のようなユーザー定義メソッドは
記述子上は `()V` になり、呼び出し側は実際には積まれる参照を picking up できず、
逆に `emit_return` は `V` から `vreturn`（無引数 `return`）を選んでしまい、
参照を返すはずの記述子と食い違って `VerifyError: Operand stack underflow` /
`Method expects a return value` になっていました。`javap -c` で実 scalac
2.13.16 の出力（`T1.die()`、`T1.f(Int)` の `tableswitch`、`$anonfun$opt$1` 等）を
確認したところ、nsc は `Nothing` 型の呼び出しの直後に必ず `athrow` を続けて
そこから先を到達不能にしており（`println(sys.error("x"))` は `invokevirtual
println` 自体が出ない）、`Nothing` はメソッドの戻り型としても常に
`Lscala/runtime/Nothing$;` のまま（`V` にはならない）で、その参照を
tail-return する場所（静的フォワーダ、`Function0` の by-name ラムダ本体）だけ
`areturn` を使う――という形でした。直し方は 3 点です。`gen_expr`
（`crates/backend/src/gen.rs`）を薄いラッパーにして、式の型が `Nothing` なら
必ず `athrow` を追加で挿すようにしました。アセンブラには元から「`athrow`/
`return`/`goto` の後に出したバイトは次のラベルまで捨てる」という dead-code
機構（`Assembler::kill` / `drop_dead`、`ab` スライス由来のコメントにある通り
「every emitter about reachability」を教えない設計）があったため、この 1 箇所の
変更だけで `match`/`if`/`try` の腕・ブロック末尾・引数位置・ascription の
すべてに波及します（`Predef.???` 側にあった手書きの `pop` は `athrow` と二重に
なるので削り、`is_unit_like` の場合だけ残しました）。`jvm_desc` の `Nothing`
腕を `V` から `Lscala/runtime/Nothing$;` に直し（`jvm_desc_val` が既にこの
表現を持っていたので合わせた形）、`emit_return` は `Nothing` を渡されたときだけ
`areturn` を選ぶようにしました。`nc_nothing.scala` は `die(): Nothing` と `???`
だけで書いてあるので**私有ランタイムと `--scala-library` の両方**で
`java -Xverify:all` の下に走り、`nc_nothing_sys.scala` は元の再現ケースそのもの
（`sys.error` と `Tuple2` を返す `match`、`Option.getOrElse` への by-name 引数）
なので library dual-run 専用です。`nc_nothing_wholly_diverging_methods_end_at_athrow`
/ `nc_nothing_diverging_arms_still_grow_an_athrow` /
`nc_nothing_user_method_descriptor_is_not_void` は `javap -c` で
バイトコードの形そのもの（本体全体が発散するメソッドは `athrow` で終わる、
`match`/`if`/`try` の腕は `athrow` を含みつつ生きている側の `return` で終わる、
`die()` の記述子が `V` でなく `Nothing$` であること）を固定します。
明示 `throw` の既存経路（`explicitThrowArm`）は退行チェックとして同じ
フィクスチャに含めてあります。

#### Remaining

- `println(sys.error("x"))` のような、`Nothing` 型の実引数からオーバーロードを
  絞る経路には別の穴があります（`ambiguous overload for println with arguments
  (Nothing)`）。今回のバックエンドの修正とは無関係の typer 側のオーバーロード
  解決の話なので、`nc_nothing_sys.scala` では `takeAny(a: Any): Unit` という
  単一シグネチャのメソッドに逃がしています。
- 私有ランタイムの `scala/Tuple2` は `toString` を上書きしていないため、
  `println` すると `scala.Tuple2@<hash>` になります（jar モード・実 scalac は
  `(1,1)`）。この修正とは無関係の既存の差で、`nc_nothing.scala` はタプルを
  そのまま印字せず `._1` 経由で比較しています。

### `Unit` の比較オペランドと `scala.Enumeration`（`agent/uniteq`）

独立な 2 件です。fixture 接頭辞は `ue`、テストは `crates/cli/tests/uniteq.rs`。

#### 1. `() == ()` が `VerifyError: Operand stack underflow`

```scala
println(() == ())                            // VerifyError（診断は出ない）
val u1 = (); val u2 = (); println(u1 == u2)  // 同じ
```

`agent/unitbox` が `Unit` の**値の位置**——パラメータ・フィールド・配列要素・
型引数——に `scala/runtime/BoxedUnit` を入れましたが、**比較のオペランド**と、
`Unit` の値に対して選んだメンバの**レシーバ**が漏れていました。

`Unit` の式はスタックに何も残しません。`() == ()` は erasure が引数側だけを
`$box` していたので、`getstatic BoxedUnit.UNIT` が 1 個だけ積まれて
`BoxesRunTime.equals(Object,Object)` が 2 個 pop する形になります。
classfile は診断なしで書き出され、JVM が検証したときに初めて落ちます。

```
java.lang.VerifyError: Operand stack underflow
  Location: Main$.main([Ljava/lang/String;)V @3: invokestatic
  Reason: Attempt to pop empty stack.
```

`().toString` / `().hashCode` / `().isInstanceOf[T]` / `().asInstanceOf[T]` も
同じ形で、レシーバが積まれないまま invoke していました。

直したのは `crates/backend/src/gen.rs` の以下です。既存の
`adapt_unit_arg`（`unit_leaves_boxed_ref` なら `checkcast`、そうでなければ
`getstatic BoxedUnit.UNIT`）に乗せただけで、新しい仕組みは足していません。

| 場所 | 直した内容 |
|---|---|
| `gen_receiver` | `Apply` のレシーバを `adapt_unit_arg` に通す |
| `gen_select_receiver` | 引数無しの `Select`（`().toString`）のレシーバも同じ |
| `gen_any_eq` / `gen_eq_ne` | 右辺のオペランドも同じ（`x == ()`） |
| `TypeApply` の `asInstanceOf` / `isInstanceOf` | レシーバを積む。`asInstanceOf[Unit]` はそのあと `pop` するので釣り合う |
| `emit_any_hash` | `Unit` を**自分では箱詰めしない**。レシーバは上で箱詰め済みなので二重に積んでしまう |

`getClass` も巻き添えで直しました。引数無しの `.getClass` は intrinsic の
分岐に無く素の `Object.getClass` に落ちていたので、`1.getClass` が nsc の
`int` ではなく `class java.lang.Integer` を返していました（`().getClass` も
`void` ではなく `class scala.runtime.BoxedUnit` になるところでした）。
`Apply` 側は元から正しかったので、両方を `emit_get_class` に寄せています。

scalac は `() == ()` を警告付きで `true` にします
（`comparing values of types Unit and Unit using == will always yield true`）。
こちらは警告を出しませんが、値は一致します。

#### 2. `scala.Enumeration` のメンバが無い

```scala
object Color extends Enumeration {
  val Red, Green, Blue = Value
  val Custom = Value(10, "custom")   // no matching overload
}
Color.values                          // value values is not a member of Color$
Color.withName("Green")               // 同じ
```

原因は **継承メンバの供給が効いていなかった**ことです。
`PickleSupply::complete_named` は「レシーバのクラスが `scala/…`（または
`adopt_binary_class` が引き取ったもの）」でなければ pickle を読みません。
`Color$` はユーザのクラスなので、`object Color extends Enumeration` は
prelude が手書きしていたもの（`Value` と `Value.id`）以外を**何も**
受け取れませんでした。

`PickleSupply::complete` に、ほかで何も見つからなかったときだけ
**ライブラリ側の祖先**を順に（線形化順、近い方から）聞く経路を足しました
（`crates/typer/src/pickle_supply.rs` の `library_ancestors`）。メンバは
それを宣言している祖先の上に入るので、JVM の呼び出しが名指すクラスとも
一致します。これで `values` / `withName` / `apply` / `maxId` と
`ValueSet` の面は全部 `scala/Enumeration.class` の `ScalaSignature` から
読めます。手書きの複製はしていません。

prelude に足したのは `Value` の 3 オーバーロードだけです
（`crates/typer/src/prelude_enum.rs`）。`Enumeration` は**クラス `Value`**と
**4 つのメソッド `Value`**を同じ名前で持っていて、供給はメンバ探索が
「何も見つからなかった」ときにしか走らないので、内側のクラスが名前に
答えてしまうと 4 つのオーバーロードは永久に聞かれません。prelude の
`Value`（引数無し）を消しても駄目で、今度は `Value(10, "custom")` が
素の名前をクラスに解決して `value apply is not a member of Value` になります。

`val Red, Green, Blue = Value` の連番はコンパイラ側の仕掛けではありません。
ライブラリの `Value()` が実行時に `Enumeration.nextId` を読んで増やすので、
右辺を名前ごとに 1 回ずつ評価する既存の多重代入の扱いだけで 0, 1, 2 になります。

#### 検証

| fixture | 何を固定するか | 期待出力 |
| --- | --- | --- |
| `ue_eq.scala`（両モード dual-run） | `Unit` のオペランド: リテラル・ローカル・`Unit` を返す呼び出し・`Unit` パラメータ、`equals` / `hashCode` / `toString` / `isInstanceOf[Unit]` / `asInstanceOf[Unit]` / `getClass`、`Any` 経由、`Unit` と非 `Unit`、型パラメータで erase された `id(())`、条件式と文の位置、`case () =>`、`case class` の `equals`、ユーザ定義 `equals` | `true` `false` … `2` |
| `ue_eqlib.scala`（library dual-run のみ） | `##`（`scala.runtime.Statics`）、`List` / `Set` / `Map` / `Option` の中の `Unit`、`() -> 1`、`(Unit, Unit) => Boolean` のラムダ、`count(_ == ())`。私有ランタイムには `Statics` も可変長 `List.apply` も `Set` / `Map` / `Function2` も無いので jar 限定 | `0` `0` `true` … `List(())` |
| `ue_eq_bad.scala`（異常系） | 箱詰めで typer が緩まないこと: `val s: String = ()`、`() eq ()`、`().length` はどれもエラー（実 scalac も同じ 3 件） | （コンパイルエラー） |
| `ue_enum.scala`（library dual-run のみ） | `val Red, Green, Blue = Value` の連番、`Value(i, name)` / `Value(i)` / `Value(name)`、`values` / `withName` / `apply` / `maxId`、`ValueSet` の `toList` / `filter` / `size` / `contains`、`type Weekday = Value`、`case Color.Red =>` の安定識別子パターン、`Value` が `Ordered`、`withName` の `NoSuchElementException` | `(Red,0,10)` `List(Red, Green, Blue, custom)` `true` `Blue` `Color.ValueSet(Red, Green)` … |
| `ue_enum_bad.scala`（異常系） | `withName(1)` / `Value(1, 2)` / `Color.nosuchMember` / `val n: Int = Color.Red` はどれもエラー（実 scalac も 4 件。`Value(1, 2)` は向こうでは `protected` 違反、こちらはオーバーロード不一致） | （コンパイルエラー） |

`ue_enum` は私有ランタイムに `scala/Enumeration` が無いので、
`--no-scala-library` では**診断が出ること**を固定しています
（`ue_enum_private_runtime_is_diagnosed`）。`ue_eqlib` も同様です。

バイトコードそのものも見ています（`ue_eq_pushes_both_operands`）。
`javap -p -c` で `BoxesRunTime.equals` の直前 2 命令が両方
`BoxedUnit.UNIT` であることを確認します——実行だけでは足りません。
直す前の出力も**コンパイルは通っていて**、気づいたのは検証器だけだからです。

slick の計測は `files=184 errors=327 files_with_errors=64` →
`files=184 errors=322 files_with_errors=64` です。減ったのは主に継承メンバの
供給で、`lazyZip` / `toMap` / `compare` が解決するようになりました
（`lazyZip` が通った結果、その先の `LazyZip.map` が新しく見えています）。

#### Remaining

- **未知の親クラスが黙って通ります**。`object Bogus extends NoSuchThingHere`
  は**両モードとも**診断なしで classfile が出ます（この修正以前からの挙動で、
  `Unit` とも `Enumeration` とも無関係）。そのため
  `object Color extends Enumeration` 自体は `--no-scala-library` でも
  エラーになりません。`ue_enum` が私有ランタイムで落ちるのは、
  中で `Value` を使っているからです。
- `Color.Value` と `Weekday.Value` を**別の型**として区別しません。prelude の
  `Value` は前置（パス依存）を持たない 1 つのクラスなので、nsc なら
  `type mismatch` になる代入が通ります。`ue_enum_bad` はこの形を避けています。
- `Unit` の比較に nsc の警告
  （`comparing values of types Unit and Unit …`）は出しません。
- `Unit => Boolean` を `Function1[Unit, Boolean]` ではなく
  `() => Boolean` に読みます（`missing parameter type for expanded function`）。
  パーサ側の別件で、`ue_eqlib` はこの形を避けています。
- `##` は `scala.runtime.Statics.anyHash` を無条件に呼ぶので、私有ランタイムでは
  `NoClassDefFoundError` になります（`Unit` に限らず `1.##` も同じ）。
  これも既存の穴で、`ue_eqlib` を jar 限定にしてある理由の 1 つです。

### 入れ子の Java インタフェースとインタフェースの static（`agent/javanest`）

Java 相互運用の 2 件と、1 件目にぶら下がっていたカスケード 2 件です。

#### 1. 入れ子の Java ジェネリックインタフェースが型パラメータを失う

```scala
val e: java.util.Map.Entry[String, Int] = it.next()
// error: Entry does not take type parameters
```

`java.util.Map$Entry` は `interface Map<K,V> { interface Entry<K,V> {…} }` で、
**入れ子側にも独自の型パラメータ**があります。それを書いているのは
`java/util/Map$Entry.class` の `Signature` 属性で、classfile リーダはこれを
きちんと読んでいました（`crates/typer/src/javaclass.rs`）。

原因はその手前です。`Map.entrySet()` の generic signature が
`java/util/Map$Entry` を**名指しする**ので、`java/util/Map.class` を読むだけで
`Entry` が親も型パラメータも無い**スタブ**としてシンボル表に入ります。
`complete_binary_member`（`crates/typer/src/check.rs`）は所有者がクラスのとき
「メンバが見つかったら戻る」だけで、見つけたのがそのスタブでも
`java/util/Map$Entry.class` を読みに行きませんでした。見つけたメンバがクラスなら
`ensure_java_loaded` を掛けるようにして直しています。

#### 1a. 線形化が同じクラスを 2 度出していた（SLS 5.1.2）

`class Cache extends java.util.LinkedHashMap[String, Int]` は、入れ子の型が
直ったあとも **`class Cache needs to be abstract.`** のままでした。`HashMap` と
`AbstractMap` が定義している `size` / `isEmpty` / `containsKey` / `put` /
`remove` / `putAll` / `equals` / `hashCode` の 8 本が「未実装」と言われます。

線形化を出してみると `java/util/Map` が **3 回**現れていて、しかも最初の 1 回が
`java/util/HashMap` より**手前**でした。抽象メンバ検査は「より派生した基底だけが
実装しうる」＝ `lin[..bi]` しか見ないので、`HashMap.put` が `Map.put` の実装として
数えられません。

`crates/typer/src/lin.rs` の C3 マージは、2 つの親が矛盾した順序を課したときに
`lists[0][0]` へフォールバックし、そこで同じクラスを二重に出していました。
Java のクラスは自分のスーパークラスが既に `implements` しているインタフェースを
もう一度 `implements` するのが普通で（`class LinkedHashMap<K,V> extends
HashMap<K,V> implements Map<K,V>`）、この形が毎回そこに落ちます。

SLS 5.1.2 の `L(C) = C, L(Cn) +: … +: L(C1)` は、`a +: b` が「`b` に既にあるものを
`a` から落とす」定義なので、**後ろの位置が勝ちます**。そこでマージ結果から
**最後の出現だけを残す**ようにしました。これは `+:` そのもので、重複も消えます。

```
直す前: Cache, LinkedHashMap, Map, HashMap, Serializable, Cloneable, Map, AbstractMap, Map
直したあと: Cache, LinkedHashMap, HashMap, Serializable, Cloneable, AbstractMap, Map
```

#### 1b. `Object` と superclass chain が実装するもの（JLS 9.2）

Java のインタフェースは `equals` / `hashCode` を deferred で**再宣言**しますし
（`java.util.Map`、`java.util.Map.Entry`、…）、スーパーインタフェースのメソッドも
再宣言します（`java.util.List` が `java.util.Collection.containsAll` を）。

`full_lin` は backend の mixin 機構に見せないために `Object` / `AnyRef` / `Any`
を**列の最後**に足すので、`lin[..bi]` にはまず入りません。`Object` は実際には
そのクラスの究極のスーパークラスなので、その具象メンバは常に実装として数えます
（`trait T { def hashCode(): Int }; class D extends T` は scalac も通します）。

同じ理由で、**Java インタフェース**が deferred 宣言したメンバは、線形化の
どこにあっても**非インタフェースの基底**（＝スーパークラス連鎖）の具象メンバが
実装します。Java に `abstract override` は無いので、インタフェースが下の実装を
打ち消すことはありません。インタフェース同士は対象外です（スーパーインタフェースの
default メソッドを abstract で再宣言したら、本当に未実装になります）。

#### 2. インタフェースの static メソッドが `Methodref` で呼ばれる

```scala
val e = java.util.Map.entry("k", 7)
// IncompatibleClassChangeError: Method 'java.util.Map$Entry
//   java.util.Map.entry(...)' must be InterfaceMethodref constant
```

**型検査は通り、実行時に落ちる**サイレント誤コンパイルです。JVMS 4.4.2 では
インタフェースが宣言するメソッド（`static` を含む）は定数プールで
`CONSTANT_InterfaceMethodref` でなければなりません。`invokestatic` 命令自体は
正しく、**定数のタグだけ**が違うので逆アセンブルしても見た目は同じです。

`Assembler::invokestatic_interface` は既にあった（`scala/App.$init$` 用）ので、
`invoke_method`（`crates/backend/src/gen.rs`）の `Flags::STATIC` 分岐で
「所有者がインタフェースなら」そちらを使うようにしました。Java 9+ の
インタフェースファクトリ（`Map.entry` / `List.of` / `Map.of` / `List.copyOf` /
`Comparator.comparing` …）が全部これです。`invokeinterface` と
`invokespecial` は元から `iface_ref` を使っていました。

#### 3. 捨てられる erased な結果を unbox していた

LRU キャッシュのプローブが通るようになってから出てきた、もう 1 つのサイレント
誤コンパイルです。

```scala
val m = new java.util.HashMap[String, Int]()
m.put("a", 1)   // NullPointerException（実 scalac は通る）
```

`java.util.Map[String, Int].put` は JVM 上 `(Object, Object)Object` なので、
typer は結果を `$unbox` で包みます。nsc はこの適合を**期待型**から入れるので、
文の位置（期待型 `Unit`）では `invokevirtual put; pop` で値に触りません。
`put` は**直前の値**を返すため、最初の挿入では `null` を unbox して落ちます。
`gen_stat` で、捨てられる `$unbox` はオペランドをそのまま文として出すように
しました（`map.remove(k)` / `list.set(i, x)` / `buf.remove(0)` も同じ形）。

#### 検証

fixture 接頭辞は `jn`、テストは `crates/cli/tests/javanest.rs` です。正常系は
**私有ランタイムと jar の両モード**で `java -Xverify:all` し、実 scalac 2.13.16
の出力と突き合わせます。

| fixture / テスト | 中身 |
|---|---|
| `jn_nested.scala` | `Map.Entry[K, V]`、Scala 側から `implements` する形、ワイルドカード `Entry[_, _]`、深さ 2 の `AbstractMap.SimpleEntry` |
| `jn_static.scala` | `Map.entry` / `List.of` / `Map.of` / `List.copyOf`（インタフェース static）、`Iterator.next` / `CharSequence.length`（default メソッド ＝ `invokeinterface`）、`Integer.valueOf` / `String.valueOf`（クラスの static は `Methodref` のまま） |
| `jn_lru.scala` | プローブ全体：`LinkedHashMap` の LRU キャッシュ ＋ `Thread` 継承 ＋ 匿名 `Comparator` ＋ `Arrays.sort` |
| `jn_nested_bad.scala`（異常系） | `getValue` / `setValue` を書かない `Map.Entry` の実装。`class Half needs to be abstract.` と、`getValue` / `setValue` **だけ**が並ぶこと（`equals` / `hashCode` / `getKey` は並ばない）。実 scalac 2.13.16 も同じ 2 本を挙げる |
| `jn_interface_static_constant_has_the_interface_tag` | classfile の定数プールを自前で読み、`Map.entry` / `List.of` がタグ 11、`Integer.valueOf` がタグ 10 であることを固定する。実行が通るだけでは間違ったタグを見逃せる |
| `jn_extending_java_collections_is_concrete` | `HashMap` / `ArrayList` / `LinkedHashMap` / `LinkedList` / `Thread` の継承（どれも以前は「needs to be abstract」） |
| `jn_object_members_implement_deferred_declarations` | `trait T { def hashCode(): Int }` などを `Object` が実装すること |
| `jn_nested_arity_is_still_checked` | `Map.Entry[String, Int, Long]` は今でもエラー |
| `jn_discarded_erased_result_is_not_unboxed` | `put` / `remove` / `set` を文の位置で呼んでも落ちないこと |

#### Remaining

- `java.util.Arrays.toString(a: Array[Object])` が
  `no matching overload …with arguments (Array[AnyRef])` になります。
  classfile の `[Ljava/lang/Object;` を `Array[Any]` に写しているためで、
  配列は Scala では不変なので `Array[AnyRef]` が適合しません。
  この修正とは独立の既存の差です。
- `java.util.function.Function.identity[String]()` /
  `java.util.Comparator.naturalOrder[String]()` のように、インタフェースの
  ジェネリックな static を**明示的な型引数つきで**呼ぶと
  `no matching overload` になります。定数プールのタグとは別の、
  型引数適用側の既存の穴です。
- `java.util.Set.of("x")` は `ambiguous overload`（可変長引数を含む
  10 本の `of` の選択）。これも既存の多重定義解決の残件です。
- 抽象メンバ検査の緩和は「Java インタフェースが宣言したメンバ」に限っています。
  Scala の trait が deferred 宣言したものは今までどおり `lin[..bi]` だけを見ます
  （`abstract override` の意味があるため）。

### trait の private メソッドと、ジェネリック親への `extends` 引数（`agent/traitpriv`）

`tests/slick_subset.sh`（実 slick の閉包をコンパイルし、出た全 classfile を
`-Xverify:all` で `Class.forName` する計測。「型検査を通る」だけでなく「実際に
JVM がロードできる」を測る）が見つけた独立な 2 件と、`agent/javanest` が発見・
位置特定していたもう 1 件です。

#### 1. trait の private メソッドが `ACC_PRIVATE | ACC_ABSTRACT` で出ていた

```
BAD slick.util.ReadAheadIterator : java.lang.ClassFormatError: Method update
    in class slick/util/ReadAheadIterator has illegal modifiers: 0x402
```

`slick/util/ReadAheadIterator.scala` は `private[this] def update()` を他の
trait メンバから呼ぶだけの、ごく普通の形です。JVMS 4.6 はどんなメソッドにも
`private` と `abstract` の同時指定を禁じていて、interface のメンバも例外では
ありません。

実 scalac がどう出すかを最小再現で確認しました（`javap -p -v`）。

```scala
trait T { private def h = 1; def g = h + 1 }
```

nsc 2.13.16 は trait を Java 8 default メソッドにコンパイルするので、`h` は
interface に**本体付きの真の `private` メソッド**として直接乗り、`g`（interface
自身に書かれた default メソッド）からは `invokespecial` で呼ばれます。

```
private int h();
  flags: (0x0002) ACC_PRIVATE
public default int g();
  flags: (0x0001) ACC_PUBLIC
    0: aload_0
    1: invokespecial #20   // InterfaceMethod h:()I
```

この backend は default メソッドを使わず、trait の具象メンバを常に
`<Iface>$class` という補助クラスへ `static` メソッドとして出し、interface 側は
抽象シグネチャだけを宣言し、mix-in するクラスにフォワーダを生やす旧来の方式
（Scala 2.11 の trait 実装）を採っています。この方式のまま nsc の形をそのまま
真似ることはできません（`$class` の中身は interface とは**別クラス**なので、
そこから `private` メンバを直接呼べない）。代わりに、nsc の形が守っている不変
条件——`private` メンバを呼ぶコードは常にその trait 自身の中にある——を保つ形に
しました。**genuine `private`（typer が `access_widened` していないもの）は
interface に一切現れず**（抽象宣言もフォワーダもなし）、`$class` 上の実体は
`private static` にし、同じ `$class` 内の他メンバからは `invokestatic` で
（`invokeinterface` ではなく）呼びます。`access_widened`（`private` メンバを
コンパニオンなど別クラスから読むために typer が公開化した場合）はこれまでどおり
`public abstract` の通常経路のままです。

`crates/backend/src/gen.rs` の `is_trait_private_def` が判定を持ち、4 か所から
呼ばれます: interface の抽象メソッド宣言ループ（`emit_class`）、`$class` 側の
アクセスフラグ（`emit_trait_impl_method`）、線形化上の「次の実装」探索
（`next_lin_impl`）、mix-in フォワーダの選定（`emit_mixin_forwarders`）。後の 2
つを直さないと、`private` メンバの名前が別トレイトの同名メンバのフォワーダ選定に
紛れ込んだり、存在しない `private` シグネチャへのフォワーダを生成してしまいます。

#### 2. ジェネリック親への `extends` コンストラクタ引数が box されない

`agent/javanest` が実行中に発見し、修正箇所まで特定していたものです（上の
javanest 節にはまだ載っていません）。ここで直します。

```scala
class A1 extends java.util.concurrent.atomic.AtomicReference[Int](1)
// VerifyError: Type integer ... is not assignable to 'java/lang/Object'
```

式位置の `new AtomicReference[Int](1)` は `gen_new` が正しく box していました
（消去後の実パラメータ型 `Object` とスタック上の値の静的型 `Int` を比べて、
プリミティブなら `emit_box`）。`extends` 節が生成する superclass コンストラクタ
呼び出しは同じ判定を持っていませんでした。`class` の `<init>` を組み立てる
`super_args` ループと、`object … extends …(args)` の `<init>` を組み立てる方の
`super_args` ループ（`crates/backend/src/gen.rs`、どちらも `emit_class` /
モジュール `<init>` ビルダの中）の 2 か所です。

`parent_super_ctor` がコンストラクタの**宣言された**引数型（Java の `<init>` の
`ctor_sym` があればその型、なければクラスの `ctor_fields`）も返すようにし
（`ctor_param_tys`。`gen_new` の同じ計算を共有関数へ切り出したもの）、両方の
`super_args` ループで `gen_new` と同じ判定
（`is_jvm_primitive(&a.ty) && !is_unit_like(&a.ty) && !is_jvm_primitive(pty)`）
を入れました。Java のジェネリック親（`AtomicReference[Int]`）・Scala 自作の
ジェネリック親（`class Box[T](val v: T)`）の両方、`class` と `object … extends`
の両方、8 種のプリミティブ全部で実 scalac の出力と突き合わせています。

#### 検証

fixture 接頭辞は `tp`、テストは新規 `crates/cli/tests/traitpriv.rs` です。
正常系は**私有ランタイムと jar の両モード**で `java -Xverify:all` し、実 scalac
2.13.16 の出力と突き合わせます。1 の 3 本は classfile を直接読んでメソッドの
アクセスフラグも固定します（`javap` は `private abstract` でも一見普通の宣言に
見える逆アセンブルを出すことがあり、シェイプの回帰は出力比較だけでは検出できない
ため）。

| fixture / テスト | 中身 |
|---|---|
| `tp1.scala` | `ReadAheadIterator` そのものの形（`private[this] var` 2 本 ＋ `private[this] def update()` を 2 つの public メンバから呼ぶ）。`tp1_private_method_is_not_abstract_on_the_interface` が interface に `update` が一切無いこと、`$class` の `update` が `private static`（`abstract` でも `public` でもない）であることを固定 |
| `tp2.scala` | 2 つの trait が同名の `private` メソッドを持つ場合の名前衝突。`tp2_private_method_gets_no_mixin_forwarder` が mix-in クラスに `helper` というメンバが一切無いことを固定 |
| `tp3.scala` | `access_widened` される側の回帰ガード：trait のコンパニオンから読む `private def secret` は widen され、`tp3_widened_private_keeps_interface_signature` が interface に `public abstract` のまま残ることを固定（`tp1` の genuine private と対照） |
| `tp4.scala` | `class ... extends java.util.concurrent.atomic.AtomicReference[Int](1)`（報告された再現そのもの） |
| `tp5.scala` | 自作の Scala ジェネリック親 `Box[T]`、`object ... extends`、8 種のプリミティブ全部 |

`./tests/slick_subset.sh` の verify 失敗数（着手時 → 完了時）:

```
着手時: subset_files=38 classes=204 (of 184 sources)  verified=203 failed=1
        BAD slick.util.ReadAheadIterator : ClassFormatError (illegal modifiers 0x402)
完了時: subset_files=38 classes=204 (of 184 sources)  verified=204 failed=0
```

`tests/slick_measure.sh`（型検査エラー数）は着手時・完了時とも
`files=184 errors=257 files_with_errors=63 classes=0` で変化なし
——この 2 件はどちらも型検査を通った後の codegen バグで、slick 184 ファイルの
残りのエラーは無関係な既存の穴です。

#### Remaining

- `agent/javanest` の README 節の「Remaining」に載っている 3 件（`Arrays.toString`
  への `Array[AnyRef]` 不適合、interface ジェネリック static への明示型引数、
  `Set.of` の多重定義曖昧性）はこの修正の対象外です。
- `private` trait メソッドの本体自身が `super.X()` を含む（かつ `X` の名前が
  そのメソッド自身と同名の override チェインでない）ようなごく稀な形は、既存の
  `needs_super_accessor` のヒューリスティックとの相互作用を確認していません
  （実コードでは見つかりませんでした）。
### 存在しない親クラス／トレイトを黙って受理していた（`agent/parentcheck`）

```scala
object Bogus extends NoSuchThingHere   // 両モードとも診断なしで classfile が出ていた
class C extends AlsoMissing            // 同上
```

実 scalac 2.13.16 は `not found: type NoSuchThingHere` です。こちらは**一言も言わず**、
`java/lang/Object` を継承した classfile を書いていました。受け入れすぎの中でも重い部類です。

#### 原因

型位置の名前解決（`resolve_type_name`、`crates/typer/src/check.rs`）は、見つからない名前を
`Type::Named { name }` という**プレースホルダ**にして返します。これは失敗の印**ではありません** —
jar から読んだメンバの型で、pickle が単純名しか書いておらず、そのパッケージをまだ読んでいない
ものも同じ `Type::Named` になります（`crates/typer/src/classpath.rs`）。実行の広い範囲が
これを意図的に許容しているので、「`Type::Named` を見たら即エラー」にはできません。

`extends` 節ではそのプレースホルダが誰にも点検されないまま `Symbol::parents` に入り、
codegen が親を解決できないと `java/lang/Object` に落として黙って書き出していました。
型引数（`extends Seq[MissingArg]`）は `apply_types` が既に見ていましたが、**引数側**は素通り。
自分型は `illegal inheritance: self-type G does not conform to MissingSelf`（存在しない型に
「適合しない」と言う）、`new Missing` は `not found: value Missing`（名前空間が違う）、
`new Missing {}`（匿名クラス）は無言でした。

#### 直し方

`Typer` に `strict_type_names` フラグを 1 本足し、**nsc が解決を終えているとわかっていて、
かつ黙認が「scalac が拒否するプログラムの受理」になる場所でだけ**立てます。

- テンプレートの親（`extends` の頭・`with` の各項・`extends P(args)` の頭）
- 自分型注釈
- `new X` / `new X {}`（匿名クラスは親として同じ経路を通ります）

`tree_to_type` は再帰するので、`extends Seq[Missing]` は scalac と同じく `Missing` を指します。
ヘッダパス（`parents_pass`）の診断は元から捨てられるので、pickle / jar 由来で**遅れて**
解決される正当な親は影響を受けません（`expose_unqualified` が囲いパッケージ・`scala._` /
`java.lang._`・ワイルドカード import・pickle をすべて試したあとの「本当に見つからない」
だけが対象です）。

修飾付きの親（`p.T`）は、**実際に欠けている区間**を名指しします。

| 書いたもの | 診断（実 scalac 2.13.16 と一致） |
|---|---|
| `extends Holder.NoSuch` | `type NoSuch is not a member of object pcq.Holder` |
| `extends pcq.NoSuchInPkg` | `type NoSuchInPkg is not a member of package pcq` |
| `extends java.util.NoSuchJU` | `type NoSuchJU is not a member of package java.util` |
| `extends pkgless.Missing` | `not found: value pkgless`（SLS 3.2.3 — `p.T` の前置は**項**） |
| `extends scala.collection.nosuchpkg.Foo` | `object nosuchpkg is not a member of package collection` |

nsc は欠けたパッケージ区間の持ち主を**単純名**（`package collection`）で、欠けた型の持ち主を
**完全名**（`package java.util`）で書きます。そこも合わせています。

`new Obj`（`Obj` は `object`）も `not found: type Obj` です。構築できる**型**が無いので、
通していたときはどのコンストラクタも答えないモジュールクラスの `new` を出していました。

#### 検証

fixture 接頭辞は `pc`、テストは `crates/cli/tests/parentcheck.rs` です。異常系は
**両モード**（私有ランタイムと jar）で拒否されること、かつ classfile を 1 つも書かないことを
見ます。正常系は両モードで `java -Xverify:all` し、実 scalac 2.13.16 の出力と比較します。

| fixture / テスト | 中身 |
|---|---|
| `pc_parents.scala`（正常系） | 引数付きの親・ジェネリックな親・`with` 混入・自分型・匿名クラス・修飾付きの親・型エイリアス経由の親。どれも未解決名と同じ経路を通るので、規則が広すぎればここが落ちる |
| `pc_extends_bad.scala` | `extends` の頭・`with` の項・適用された親の頭・その型引数（6 件、scalac と同じ 6 件） |
| `pc_selfnew_bad.scala` | 自分型（`A with B` の各項）・`new Missing`・`new Missing {}`・`new Obj` |
| `pc_qualified_bad.scala` | 上の表の 6 件 |
| `pc_new_of_a_missing_type_is_not_a_missing_value` | `new Missing` が `not found: value` に戻らないこと |

slick（`tests/slick_measure.sh`）は **`errors=257 files_with_errors=63` のまま変化なし**で、
新しい誤診断はゼロです。既存の 3 件が `not found: value DumpInfo` / `value Mapper` から
`not found: type …` に**正しい名前空間**へ変わっただけでした。

#### Remaining

- **`Ordering[String].compare(1, 2)` の診断文面が scalac から乖離**(拒否自体は健在)。
  `agent/tail2` の jar implicit 供給で prelude の `compare` の隣に pickle 由来の候補が並び、
  単一候補の `type mismatch; found: 1 required: T`(scalac と同文)が
  `no matching overload` に変わった。同 erasure の重複が供給の門(`agent/setapply2`)を
  すり抜ける新しい継ぎ目と思われる。文面だけの問題。

- ~~`new T`（型パラメータ）/ `new A`（抽象型メンバ）は今も無言で通ります。~~
  `agent/eqtail`（後述）で直しました。
- 修飾付きの名前は、`lookup_qualified_type` が失敗したとき**裸の名前**での再解決に
  フォールバックします（前置を模せない経路のため）。そのため `p.Foo` は、無関係な
  トップレベルの `Foo` が居ると今でもそれに束縛されます。診断が出るのは両方失敗した
  ときだけです。
- 型位置一般（`val x: Missing`、`def f(x: Missing)`、型引数一般）は今回の対象外です。
  `Type::Named` プレースホルダは jar 由来の正当な型でもあるので、そこを閉じるには
  「未解決」と「未読込」を型として分ける必要があります。

### コンパニオンの `apply` が prelude と pickle とで二重に載っていた（`agent/setapply`）

```scala
val u: Set[String] = Set("x")
val b = u("x")          // SetOps.apply(A): Boolean をメンバ経由で完了させる
println(Set("admin"))   // error: ambiguous overload for apply with arguments ("admin")
```

2 行目が無ければ 3 行目は通ります。実 scalac 2.13.16 は両方とも通し、`Set(admin)` を
出力します。

#### 原因

`object Set extends IterableFactory[Set]` の `apply(elems: A*): Set[A]` は
`crates/typer/src/prelude.rs`（`add_set`）に**手書き**してあります。`--no-scala-library`
でも `Set(1, 2, 3)` が動くようにするためで、このシンボルは `pickled_origin` を持ちません
（pickle から供給されたときだけ立つ印だからです）。

`u("x")` は `Set[String]` の**メンバ**としての `apply(A): Boolean`（`SetOps` 宣言）を要求します。
これは prelude に無いので `Check::ensure_apply_supplied` が `PickleSupply::complete` を呼んで
jar から補います。`complete` は「クラス側が見つかっても**必ず**コンパニオンにも聞く」——
`scala.math.BigDecimal` のようにインスタンス側の `apply` だけを持つクラスで、コンパニオンの
7 本を隠してしまわないための仕様です（`agent/companionkind` 由来）。この「必ず聞く」対象に
コンパニオンの module class 自身が入っており、`Set$` に対して**その時点で初めて** `apply` を
完了させます。ところが `Set$` の `apply` は prelude に**既にある**ので、これは
「同じ宣言のコピーが 2 つ」（`agent/ambigmap`）そのものです——ただし今回は 2 つ目のコピーが
別の**クラス**からではなく、別の**由来**（pickle）から同じクラスに載ります。

`agent/ambigmap` の `collapse_pickled_copies` は `pickled_origin` が**両方とも**立っている
コピー同士しか束ねません。手書きの prelude シンボルは意図的に対象外です（`pickled_origin`
が空のシンボルは「一切触らない」——本物のオーバーロードを誤って消さないための境界線でした）。
`drop_overridden` の override 規則も、2 つが**同じオーナー**（`Set$` 自身）を持つ場合は
発動しません（「サブクラスが親を override している」形にしか当てはまらないからです）。
結果、prelude 版と pickle 版の `Set$.apply` は誰にも束ねられずに両方生き残り、`Set(...)` は
**メンバ側の完了が先に走った回数だけ** `ambiguous overload` になります——`u("x")` のような
インスタンス側の呼び出しが 1 度でも先に来ると再現し、無ければ再現しません。順序依存という
症状も `agent/ambigmap` と同型です。

#### 直し方（1 回目、退行あり）

最初の版は `PickleSupply::install`（`crates/typer/src/pickle_supply.rs`）に、pickle から
読んだメンバを実際に載せる直前の検査を足しただけでした: **同じクラスに、同じ名前・同じ
erased パラメータ形のメンバが、すでに prelude の手書きシンボルとして載っていたら、pickle 版は
`None` を返して何も供給しない**。「prelude の手書きシンボル」かどうかは `pickled_origin` が
空であることに加えて、そのシンボル ID が `SymbolTable::prelude_end` より小さいことで判定します
（`pickled_origin` が空なのは prelude シンボルだけでなく `adopt_binary_class` が classfile
リーダから読んだ**仮の**シンボルも同じで、ID だけを見ずに `pickled_origin` の空だけで判定すると
`scala.Equals.canEqual` などが pickle の精密な型に差し替えられなくなり、**すべての case class**
が `needs to be abstract` になる退行を一度作りました。`prelude_end` 未満という条件で防ぎます）。

これはマージ後の全体検証で**別の 2 件**を壊しました（詳しくは次節）。原因はどちらも同じ形:
`None` を返して**何も供給しない**という体裁が、`complete_named` の**戻り値**だけを読む
呼び出し元（`PickleSupply::complete` のコンパニオン合併など）から prelude のメンバを
**見えなくした**ことです。`class_sym.members` 自体には prelude 版がずっと載っていたので、
`lookup_member` で直接引く経路は無傷でしたが、`complete_named` の戻り値だけを積み上げて
候補集合を組み立てる経路は「何も返ってこなかった」としか解釈できませんでした。

#### 直し方（2 回目、現在の版）

`None` の代わりに、**すでに載っている prelude シンボルをそのまま返す**ようにしました
（`Some(blocker)`）。新しいシンボルは作らず `class_sym.members` にも触れませんが、
呼び出し元からは「pickle 自身がこの prelude の宣言を答えとして返してきた」のと**区別が
付かなくなります**——`complete_named` の戻り値を読むどの経路も、prelude のメンバが最初から
そこにあったかのように振る舞います。

名前ではなく形（erased パラメータ）で比較している点は変わりません。**同じ名前でも形が違う**
pickle のメンバは今までどおり新規に供給されます（`Set[A]` のメンバ `apply(A): Boolean` と
コンパニオンの `apply(A*): Set[A]` はオーナーも形も違うので、この検査には一切触れません）。

#### 1 回目の版が壊した 2 件

1. **`agent/oshadow`**（`oshadow_order_independent` / `oshadow_bad_is_rejected`）。
   `scala.math.BigDecimal` は `apply(Int)` / `apply(String)` /
   `apply(java.math.BigDecimal)` の 3 本を prelude に手書きしています
   （`crates/typer/src/prelude_oshadow.rs`。JDBC の結果を Scala 値にするのに使うため）。
   `BigDecimal(2)` を型付けする際、`Check::type_select` は「クラス側（インスタンスの
   `apply(MathContext)`）とコンパニオン側を両方 pickle に聞く」という `PickleSupply::complete`
   の合併結果をそのまま `found`（候補集合）として使い、それを `Check::record_overload_group`
   で `fun_sym` にキャッシュします。1 回目の版では、この合併結果から `apply(Int)` /
   `apply(String)` / `apply(java.math.BigDecimal)` の 3 本が**まるごと欠けました**
   （`None` を返したので `complete_named` の戻り値に現れず、`complete` の合併もそれを
   見つけられない）。結果、`BigDecimal(2)` は `Long` / `Double` / `BigInt` としか比較されず、
   どれも決定的に勝てないため `ambiguous overload` になり——しかもこの誤ったエラーは
   `Check::widen_with_companion`（`OverloadPick::None` のときだけ動く、コンパニオンの
   メンバで候補を広げ直す最後の砦）が一度でも走る**前に**確定・記録されてしまいます。
   `Ambiguous` は `None` と違って widen_with_companion の対象にならないからです。
   2 回目の版（`Some(blocker)`）では、この合併結果に最初から 3 本とも含まれるので、
   `BigDecimal(2)` は 1 回目の解決で `apply(Int)` に一意に決まり、
   `widen_with_companion` を経由する必要すらありません。
2. **`agent/uniteq`**（`ue_enum_scala_library`）。`scala.Enumeration` は `Value`（引数無し）
   を prelude に、残り 3 オーバーロード（`Value(Int)` / `Value(String)` /
   `Value(Int, String)`）を `crates/typer/src/prelude_enum.rs` に手書きしています。
   `values` / `withName` / `apply` / `maxId` は `PickleSupply::complete` の
   `library_ancestors` フォールバック（ユーザクラスがライブラリの祖先を持つときだけ動く）で
   pickle から読みます。1 回目の版はここでも同じ形で、`Enumeration` 自身のクラスに完了させる
   `apply` / `Value` 系の一部が戻り値から欠け落ち、`object Color extends Enumeration` の
   メンバ解決が壊れました。2 回目の版で同時に直っています。

#### 検証

fixture 接頭辞は `sa`、テストは `crates/cli/tests/setapply.rs` です。`sa_setapply.scala` は
`Repo` trait の `xs(tag)`（`SetOps.apply(String): Boolean` をメンバ経由で強制的に完了させる、
元の報告と同じ形）→ `Set(...)` の順、逆順、`Map` / `List` / `Seq` の同型ケースを 1 本にまとめ、
`--scala-library` dual-run と real scalac 2.13.16 の実行結果 diff の両方で
`java -Xverify:all` の下に走らせます。`Set[String]` のメンバ `apply` が今までどおり
`Boolean` を返すこと（`u("x")` / `v(2)` / `m("a")` / `xs(1)` / `ys(0)`）も同じフィクスチャで
固定しています。`Repo` の要素型は `A`（トレイトの型パラメータ）ではなく `String` に固定して
あります——抽象型引数のまま `xs(tag)` を通すと、固定長パラメータと可変長パラメータの
どちらも specificity で決着が付かないという**このバグとは無関係な既存の別バグ**を踏み、
無関係な `ambiguous overload for apply` を追加で出してしまうためです（下記「既知の残件」）。
私有ランタイムには `scala.collection` の pickle が無い（＝二重に載る余地が無い）ので、
`sa_setapply_without_the_library_is_diagnosed` が `--no-scala-library` で `Set` が
**黙って通らず** `not found: type Set` と診断されることを見ます。`sa_setapply_bad.scala` は、
共通の親を持たない 2 つの実在するオーバーロード（`Ax` / `Bx` を実装する `Cx` への
`Pick.apply`）が束ねられずに 2 つのまま残り、決着が付かなければ scalac と同じく
`ambiguous overload` になることを固定します——直したのが「名前」ではなく「形」であることの
担保です。

2 回目の版では、上記に加えて `--test overloadshadow --test uniteq --test ambigmap
--test mutcoll --test conform` をすべて前景で回し、1 回目の版が壊した 2 件を含め全件
グリーンであることを確認しています。

slick（`tests/slick_measure.sh`）は `files=184 errors=257 files_with_errors=63` →
**`errors=241 files_with_errors=61`**（−16 件 / −2 ファイル）。元々の `Set` の順序依存
自体は slick の 184 ファイルには現れていませんでしたが、2 回目の版が同時に直した
`agent/oshadow` / `agent/uniteq` 型の「候補が `complete_named` の戻り値から欠け落ちる」
経路は slick のコードにも当たっていたようです。

#### 既知の残件

- `java.util.Set.of("x")` の `ambiguous overload`（固定 arity 10 本 + 可変長引数の選択）は
  **別根**です。`java.util.Set` は Java の classfile から直接読み込まれ（`javaclass.rs`）、
  `pickle_supply.rs` の completion 経路を一切通らないので、この修正の対象外のままです
  （`agent/javanest` の README 節の Remaining に記載済み）。
- **固定長パラメータと可変長パラメータの specificity が、要素型が抽象型パラメータのときに
  決着しません。** `trait Repo[A] { def hasTag(xs: Set[A], tag: A): Boolean = xs(tag) }` は
  `xs(tag)` が `SetOps.apply(A): Boolean`（固定長）と `IterableFactory.apply(A*): CC[A]`
  （可変長、`Set[A]` が継承）のどちらとも一致して `ambiguous overload for apply` になります。
  要素型が具体型（`String` など）なら固定長側が正しく勝ちます。この修正の前から
  `--scala-library` の素の main にも存在する既存のバグで、`agent/setapply` の対象外です。

`agent/eqtail` スライス（`Equiv[T]` の summon と `Ordering <: PartialOrdering <: Equiv`
の階層辺）のフィクスチャは接頭辞 `eq2`（`eq2_summon` / `eq2_summon_bad`）で、同じ理由から
`crates/cli/tests/eqtail.rs` に置いています。`eq2_summon.scala` は `implicitly[Equiv[T]]`
（`Int` / `String` / `Long` / `Boolean` / `BigInt`）、`Equiv.Int` の直接参照、
`getClass.getName` による instance の同一性確認（`Equiv$Int$` / `Equiv$DeprecatedDoubleEquiv$`）、
`Ordering.Int` を `Equiv[Int]` / `PartialOrdering[Int]` へ渡す劣化代入を 1 本にまとめてあり、
`--scala-library` dual-run と real scalac 2.13.16 の実行結果 diff（`eq2_summon_matches_real_scalac`）
の両方で `java -Xverify:all` の下に走らせます。`eq2_summon_bad.scala` は、階層辺を足しても
`implicitly[PartialOrdering[Int]]` が summon 可能にはならないこと（real scalac にも instance が
無い）、`Equiv[Int]` を `Ordering[Int]` の位置には渡せないこと（劣化は `Equiv` 方向だけ）、
companion object 自身は `Equiv` ではないことを固定します。私有ランタイムには
`scala/math/Equiv` の classfile が無いので、`summon_is_diagnosed_without_the_jar` が
`--no-scala-library` で `Equiv` が**黙って通らず** `not found: type Equiv` と診断されることを
見ます。同じスライスの `Ordering#compare` 修正のフィクスチャは `eq2_compare` /
`eq2_compare_bad` です。`eq2_compare.scala` は `Ordering[String]` / `Ordering[Int]` の
`compare` / `lt` / `gt` / `lteq` / `gteq` / `equiv` / `max` / `min` と、`Ordering[T]`
を受け取るジェネリックな関数（`cmp[T](ord: Ordering[T], x: T, y: T)`）を 1 本にまとめて
あり、`--scala-library` dual-run と real scalac 2.13.16 の実行結果 diff（
`eq2_compare_matches_real_scalac`）の両方で走らせます。`eq2_compare_bad.scala` は、
修正前は黙って通っていた `Ordering[String].compare(1, 2)` / `Ordering[Int].compare("a",
"b")` / `Ordering[String].lt(1, 2)` / `Ordering[String].max(1, 2)` が real scalac と
同じ理由で拒まれることを固定します（`Ordering` 自体が `library_abi` 専用の手書き
シンボルなので `--no-scala-library` のケースはありません）。`new T` / `new A` の
修正（`agent/parentcheck` 残件）のフィクスチャは `eq2_newtype` / `eq2_newtype_bad`
です。`eq2_newtype.scala` は、修正後も壊れていないことを確認するための正常系
（型パラメータへ**適用**した実在のクラス `new Box[T](value)`、型エイリアス経由の
`new Self`（`type Self = ConcreteNamed`）で、jar の機能を使わないので**私有ランタイムと
`--scala-library` の両方**で `java -Xverify:all` の下に走らせ、real scalac
2.13.16 の実行結果とも diff します（`eq2_newtype_matches_real_scalac`）。
`eq2_newtype_bad.scala` は、直す前は両モードで無言で通っていた `new Self`（宣言した
trait 自身の中で、`=` の無い抽象型メンバを裸で参照）と `new T`（メソッド型パラメータ）を、
`class type required but Named.this.Self found` / `class type required but T found`
という real scalac そのままの文面で両モードとも拒否することを固定します
（`eq2_newtype_bad_is_rejected_private_runtime` / `_scala_library`）。slick のソースは
`Equiv` / `PartialOrdering` を参照していないので、`tests/slick_measure.sh` の数字は
このスライスの前後で変わりません。

### ローカル `case class` のコンパニオン classfile が出ていなかった（`agent/localcc`）

```scala
def main(a: Array[String]): Unit = {
  case class P(n: Int)
  println(P(1))       // 型検査は通り、実行時 NoClassDefFoundError: Main$P$1$
}
```

型検査は通りますが（`Typer::ensure_companion` が companion のシンボルをちゃんと
リンクしています）、実行すると `NoClassDefFoundError: Main$P$1$` で落ちます。
**サイレントな誤コンパイル**です。

#### 原因

トップレベル（またはクラス直下）の `case class` は `Backend::walk_stats`
（`crates/backend/src/gen.rs`）が `emit_class` の直後に `emit_case_companion` を
呼び、`apply` を持つコンパニオンの module class を出しています。ところが
**メソッド本体の中**で宣言された `case class` は別の経路（`Backend::emit_anon_classes`
の `Block` 腕）を通り、そちらは `emit_class` だけ呼んで `emit_case_companion` を
一度も呼んでいませんでした。`Main$P$1` は出ますが `apply` を持つ `Main$P$1$` が
出ないので、`P(1)`（コンパニオンの `apply` 呼び出しへデシュガーされる）が
リンクエラーになります。ローカルの `case object`（コンパニオンを持たず `object`
自身がそのまま module）や、既にローカルの `trait` / `class` / `object`
（`agent/localtrait`）を捕まえている周辺の仕組みは無関係で正しく動いていました。

#### 直し方

`emit_anon_classes` の `Block` 腕に、トップレベル用の `walk_stats` と同じ判定
（`case` フラグが立っていて、同じブロックにユーザーが書いた同名のコンパニオン
`object` が無ければ `emit_case_companion` を呼ぶ）を足しました。

#### 検証で見つかった追加の穴（同じ修正の一部）

修正を入れて `lcc1`（このバグの再現そのもの）を通した後、ブリーフが名指しした
「捕捉ありの形」（ローカル `case class` の本体がメソッドの外側の局所変数を読む）を
確認したところ、**もう一つ**サイレント誤コンパイルが見つかりました:

```scala
def main(a: Array[String]): Unit = {
  val base = 10
  case class Q(n: Int) { def total: Int = n + base }
  println(Q(5).total)   // 型検査は通り、実行時 NoSuchMethodError: 'void Main$Q$1.<init>(int)'
}
```

`Q` クラス自身は既存の一般的な仕組み（`crates/typer/src/anon_capture.rs`）で
`base` を捕捉フィールド付きのコンストラクタに正しく変えています（`<init>(int,
int)`）。しかしコンパニオンの `apply`（`emit_case_apply`）は `ctor_fields` だけを
見て `new Main$Q$1(n)` を組み立てており、捕捉引数を一切知りません。実 scalac は
（`javap` で確認: `Cap$Q$2$` は自分自身に `private final int base$1` を持ち、
`MODULE$` 静的シングルトンではなく**呼び出しごとに新しいコンパニオンを構築**する
——通常のローカル `object` が `scala.runtime.LazyRef` 経由で一度得る「捕捉あり
ローカル型は毎回新しいインスタンス」という形そのものです（`crates/typer/src/localobj.rs`
の `check_local_objects` が既にローカル `object` 側でこの形を拒否しています）。

この形はコンパニオンの `MODULE$` 静的シングルトン表現を丸ごと作り直す必要があり
（`LazyRef` 相当）、このスライスの本題（コンパニオンが**出ない**こと）とは別の、
それ自体で 1 スライス分ある実装課題です。`localobj.rs` が既に確立している方針
（未実装の形は診断で断る。動いたことにしない）にそのまま倣い、
`check_local_case_class_captures`（`crates/typer/src/localobj.rs`）を追加しました。
`mark_anon_captures` が `Symbol::captures` を埋めた**直後**（`crates/driver/src/lib.rs`）
に、ローカル `case class` の `captures` が非空なら診断してコンパイルを止めます。
これで「型検査は通るが実行時に落ちる」が「コンパイルが通らない」に変わります。

#### 検証

fixture 接頭辞は `lcc`、テストは新ファイル `crates/cli/tests/localcc.rs` です。
`lcc1.scala` はブリーフの再現そのもの（`P(1)` の構築 + `case P(x) => …` のパターン
マッチ）、`lcc2.scala` はローカル `case object`（元から壊れていないことの回帰
ガード）、`lcc3.scala` は同じメソッド名 `P` を持つ 2 つのメソッド（別々のクラス**と**
別々のコンパニオンが出て互いに漏れないこと）です。3 本とも `--no-scala-library` /
`--scala-library` の両モードで `java -Xverify:all` の下に走らせ、期待値は
実 scalac 2.13.16 の実行結果（`tests/fixtures/expected/lcc{1,2,3}.txt`）です。
修正前の `main`（`emit_case_companion` 呼び出しを外した状態）で `lcc1` を実行すると
`NoClassDefFoundError: Main$P$1$` で落ちることを確認しています。捕捉ありの形は
`lcc4_bad.scala`（コンパイルが `not implemented: a local case class Q that reads a
local of the enclosing method …` という診断で失敗することを固定する
`compile_fails` テスト）です。`local_case_class_companion_has_apply` /
`same_named_local_case_classes_get_separate_companions` の 2 本は `javap` で
実際に出た classfile の形（`Main$P$1$` が存在し `apply(int): Main$P$1` を持つこと、
`lcc3` が `Main$P$1` / `Main$P$2` と 2 つの別コンパニオンを出すこと）を見ます。

`--test localcc --test localtrait --test ctorstmt --test quasi --test product
--test companionkind --test outer --test nestedobj` を前景で回し、全 64 + 6 本
グリーンです（`quasi.rs` はこのスライスの item 2 のテストも含みます）。

#### 既知の残件

- **case class の companion に `unapply` の実体が無い。** `crates/typer/src/check.rs`
  の `namer_class`（コンパニオン合成のところ）は `unapply` のシンボルを作るだけで
  `.ty` を設定せず、`crates/backend/src/gen.rs` には `emit_case_apply` はあっても
  `emit_case_unapply` が存在しません。トップレベルの `case class P(n: Int)` に対して
  `P.unapply(P(1))` を**明示的に**呼ぶと（`p match { case P(x) => … }` というパターン
  マッチ自体は別経路でフィールドを直接読むので無関係に動きます）、型検査は通り
  実行時 `NoSuchMethodError: 'scala.Option P$.unapply(P)'` になります。これは
  ローカルに限らずトップレベルの case class にも共通する既存の別ギャップで、
  今回のスライスの対象外です。
- **ローカル `case class` が外側の局所変数を捕捉する形は診断で拒否したまま**
  （上記「検証で見つかった追加の穴」）。`LazyRef` 相当の実装が要ります。

### `u.Ident(sym: Symbol)` オーバーロードの供給漏れ（`agent/liftable` 残件、`agent/localcc`）

```scala
// slick の TableQueryMacroImpl.apply（scala-2/slick/lifted/TableQuery.scala）
Ident(typeOf[Tag].typeSymbol)
```

`Ident` はツリーファクトリの `val Ident: IdentExtractor`（`apply(name: Name)`）
**だけ**ではなく、`scala.reflect.internal.Trees` トレイトが直接宣言する便利メソッド
`def Ident(sym: Symbol): Ident` も同じ名前で持っています（scala-reflect.jar
2.13.16 を `javap` で確認: `scala/reflect/api/Trees.class` は `abstract
Trees$IdentApi Ident(Symbols$SymbolApi)` を extractor の `apply` のすぐ隣に
宣言しています）。`Ident(sym)` はこの後者に一致するはずですが、
`no matching overload for <overload Trees$IdentExtractor | (String)Trees.Ident>
with arguments (Symbol)` と、候補一覧に **`Symbol` を取る版が最初から入っていない**
状態で拒否されていました。

#### 原因

`PickleSupply::install`（`crates/typer/src/pickle_supply.rs`）は同じ名前・同じ
arity の複数の pickle 由来オーバーロードを、パラメータの**消去後**シグネチャで
区別します（`erased_param_desc`）。抽象 API（`scala.reflect.api.Trees` /
`scala.reflect.macros.Universe`。マクロが実際の展開時にだけ手にする具体的な
`JavaUniverse` ではありません）から見えている段階では、`Symbol` は具象クラスでは
なく**抽象型メンバー**（`type Symbol >: Null <: SymbolApi`）で、`Type::TypeMember`
に変換されます。ところが `erased_param_desc` は `Type::TypeMember` のケースを
持たず `_ => None`（「参照型なら何でもよいワイルドカード」の意味）に落ちていました。
`Ident(String)` も `Ident(Symbol)` もどちらも「参照 1 個」のワイルドカードに
潰れてしまうので、`erased_desc` は候補が 1 つに決まらず
（`no unambiguous erased descriptor`）、`Symbol` を取る版は**そもそも一度も
インストールされていません**でした。

実 scalac 自身、抽象型は自分の上限境界へ消去します（境界が無ければ `Object`）。
実際に `scala.reflect.api.Trees.class` の classfile も
`Ident(LSymbolApi;)LTrees$IdentApi;` という具体的なディスクリプタを持っています
（`javap` で確認済み）。`erased_param_desc` に `Type::TypeMember` のケースを足し、
自分の `bound_hi`（無ければ `Object`）へ**再帰的に**（循環境界に備えて 16 段で
打ち切り）解決するようにしました。

#### 検証

最小のマクロ実装を 2 段コンパイル（`lf2_ctx.scala` と同じ方式）で確認します。
`Ident(c.internal.enclosingOwner)` は修正前 `no matching overload for <overload
Trees$IdentExtractor | (String)Trees.Ident> with arguments (Symbol)` で拒否され、
修正後は候補一覧に `(Symbols.Symbol)Trees.Ident` が加わり通ります。新しい
fixture は `tests/fixtures/lf3_identsym.scala`（接頭辞 `lf`、`agent/liftable` の
既存の番号付けに続けて `lf3`）、テストは `crates/cli/tests/quasi.rs` の
`lf3_identsym_supplies_the_symbol_overload_of_ident`（`lf2_ctx` と同じ形:
コンパイルが通ることと、`javap`/`java -Xverify:all` で実際にロード・検証できる
classfile になることを見て、real scalac 2.13.16 でも同じソースが通ることを
確認します）。slick 本体の該当行（`TableQueryMacroImpl.apply` の
`Ident(typeOf[Tag].typeSymbol)`）でも `no matching overload` が消えたことを
`tests/slick_measure.sh` の生ログで確認しています。

継ぎ目（`pickle_supply.rs`）を触ったので、ブリーフの必須一覧
`--test overloadshadow --test ambigmap --test setapply --test uniteq --test
integral --test ordsummon --test mutcoll --test conform --test e2e`
をすべて前景で回し、グリーンであることを確認しました。

slick（`tests/slick_measure.sh`）は `files=184 errors=223 files_with_errors=60`
→ `files=184 errors=222 files_with_errors=60`。`TableQuery.scala` の
`no matching overload … Ident` 行はログから消えました（同じファイルには
`typeOf` の implicit 未実装など、この修正と無根の別エラーが残っているので
ファイル数自体は変わりません）。

#### 同根の確認（`u.WeakTypeTag[T]` / `u.TypeTag.Int` 残件との関係）

ブリーフが「同根か確認してほしい」としていた `u.WeakTypeTag[T]` /
`u.TypeTag.Int` が `not a member of JavaUniverse` になる件は、**別根**と
判断しました。`import scala.reflect.runtime.universe._` の下で
`WeakTypeTag[Int]` / `TypeTag.Int` は今回の修正後も `not found: type
WeakTypeTag` / `not found: value TypeTag` のまま —— そもそも「型が見つかって
オーバーロードが絞れない」ではなく「ワイルドインポートされた名前として見えて
すらいない」という、もっと手前で失敗する別の症状です。今回の修正
（`erased_param_desc` の消去）はオーバーロード**候補の絞り込み**の話であり、
名前解決そのものの失敗には触れません。残件のままにしています。

#### 見つかった副産物の残件（今回の修正の対象外）

`Ident(sym)` を直そうとする過程で、無関係の**別の**バグも見つけました:
`import c.universe._` の下で `Symbol` という**裸の型注釈**を書くと（
`val sym: Symbol = c.internal.enclosingOwner` のように）、ワイルドインポート
された `c.universe.Symbol`（reflection API の抽象型）ではなく、常にスコープに
ある無関係の `scala.Symbol`（`'foo` のようなシンボルリテラルのクラス）に
解決されてしまい、`type mismatch; found: Symbols.Symbol  required: Symbol` に
なります。ワイルドインポートは暗黙の `scala._` より優先されるべきところが
そうなっていません。slick の実際のコード（`Ident(typeOf[Tag].typeSymbol)`）は
明示的な `Symbol` 注釈を書かないのでこの副産物には当たらず、今回の修正の
検証には影響しません。別チケットとして残しています。

### `super` と自己型、`x @ Extractor(...)` の束縛型、カリー化した `copy`（`agent/tail3`）

割り当ては slick 残エラーの単発・カスケード群（多いもの順）でした。テストは
`crates/cli/tests/tail3.rs`、fixture 接頭辞は `t3` です。

計測は `files=184 errors=203 files_with_errors=60` →
**`files=184 errors=184 files_with_errors=57`**（−19 件 / −3 ファイル）。

| 塊 | before | after |
|---|---|---|
| `value volatileHint is not a member of Node` | 3 件 | **0 件** |
| `recursive method computeCapabilities needs result type` | 3 件 | **0 件** |
| `value apply is not a member of TableNode` | 3 件 | **0 件** |
| `value getDumpInfo is not a member of TypeGenerator` | 2 件（同根の副産物） | **0 件** |
| `value getOrElse is not a member of Product` | 4 件 | 4 件（**直せていません**、`agent/tail1` と同じ理由） |

#### 1. `x @ Extractor(...)` は束縛型を絞らなければならない

`slick/jdbc/{DerbyProfile,JdbcStatementBuilderComponent,SQLServerProfile}
.scala` はいずれも `case c @ LiteralNode(_) if c.volatileHint => …`（あるいは
`:@` を挟んだ同形）を `Node` 型のスクルーティニーに対して書いています。
`volatileHint` は `Node` ではなく `LiteralNode`（`case class` ではなく、
コンパニオンに手書きの `def unapply(n: LiteralNode): Option[Any]` を持つ
普通のクラス）にしかありません。実 scalac は `x @ Extractor(...)` の `x` を
**抽出子自身が宣言する受け取り型**（`case x: T` と同じ暗黙の型テスト）に
束縛しますが、`crates/typer/src/check.rs`（`type_pattern` の `unapply` 枝）は
パターン全体の型（`pat.ty`）を常に**スクルーティニーの型のまま**にしていた
ので、`c` はずっと `Node` で `c.volatileHint` が拒否されていました。

直しは `TreeKind::Bind` の**中だけ**: 内側のパターンを型付けした後、新しい
`unapply_receiver_type`（抽出子の宣言パラメータ型を、`subst_unapply_tparams`
と同じやり方でスクルーティニーに対して単一化する）で束縛変数の型だけ絞ります。
`TreeKind::UnApply` ノード自身の `pat.ty` はわざと**スクルーティニーの型の
まま**にしています —— `crates/backend/src/gen.rs` の `gen_unapply_pattern`
がそれを読んで実行時 `instanceof` テストが冗長かどうか
（`is_sub_type(pat.ty, param_ty)`）を判定しているので、ここも絞ってしまうと
その判定が常に真になって**テスト自体が消えて**しまいます。実際、型検査だけの
版で `describe(new OtherNode)`（どちらの `LiteralNode` ケースにも一致しない
はず）を実行すると `ClassCastException: OtherNode cannot be cast to
LiteralNode` になりました —— `-Xverify:all` の下で実行し、real scalac の
標準出力とも突き合わせてから信用する、というブリーフの手順が実際に効いた例
です。

#### 2. `super` は自己型を歩いてはいけない

`slick/{jdbc/DB2Profile,relational/RelationalProfile,sql/SqlProfile}.scala`
はいずれも `computeCapabilities`（戻り型注釈なし）を
`super.computeCapabilities ++ …Capabilities.all` でオーバーライドしています。
基底（`BasicProfile.computeCapabilities: Set[Capability] = Set.empty`）には
明示的な型があるので本当の循環にはならないはずです —— ブリーフの指示どおり、
まず最小再現を real scalac に通してから調べました（`t3_super_chain.scala` は
scalac も一発で通ります）。

原因は 2 つ重なっていました:

* **型検査**: `RelationalProfile extends BasicProfile with
  RelationalTableComponent with … with RelationalActionComponent` で、
  `RelationalActionComponent { self: RelationalProfile => }` は（`super_target`
  の旧「最後の親を使う」ヒューリスティックでは）`super` が最初に選ぶ親です。
  `SymbolTable::lookup_member`（普通のメンバ探索）は自己型も辿りますが、これは
  自己型付きトレイトの本体**内側**からの `this.foo` / 無限定参照には正しい
  一方、SLS 6.7.3 上 `super` は自己型を絶対に経由しません（実継承の親だけ）。
  `RelationalProfile` の中の `super.computeCapabilities` が
  `RelationalActionComponent` の自己型経由で `RelationalProfile` **自分自身**
  の、まだ完了していないオーバーライドに戻ってしまい、本物の循環参照には
  なるものの nsc が報告するのとは違う理由でした。`SymbolTable::
  lookup_member_real`（自己型を辿らない版）と `Typer::super_select_member`
  （`this_id` の実の親を後方宣言優先で辿り、実継承チェーンに `name` を持つ
  最初の親を探す）を足し、`type_select` の中で qualifier が `Super` のときだけ
  差し替えました。
* **バックエンド**: 上を直すと型検査は通りましたが、`ClassImpl`（普通の
  `class`）と `ObjectImpl`（`object`）で挙動が違いました —— `ObjectImpl.m` は
  `AbstractMethodError: … Mid$$super$m() of interface Mid` を投げました。
  `crates/backend/src/gen.rs` の `emit_class` は `emit_super_accessors`
  （トレイトの `super.m` 呼び出しが必要とする抽象 `Trait$$super$m` アクセサを
  ミックスインされる側の具象クラスに実装する）を呼びますが、`emit_module`
  （`object` 専用の別コード経路）は**一度も**呼んでいませんでした。自分の
  本体で `super` を呼ぶトレイトを継いだ `object Foo extends SomeTrait`
  （まさに slick の各データベース用プロファイルオブジェクト、
  `object H2Profile extends JdbcProfile` 等）は全て影響を受けますが、
  1 の型検査バグが常に先に拒否していたので実際にコンパイルが通ったことが
  一度もありませんでした。`emit_module` に
  `self.emit_super_accessors(&mut b, cls);` を 1 行足すだけです。

#### 3. `p.copy(...)( ...)` はチェーン全体を先に見なければならない

`slick/ast/Node.scala` は `final case class TableNode(schemaName, tableName,
identity, baseIdentity)(val profileTable: Any)` —— 第 2 引数リストが 1 個の
`val` だけの、カリー化した `case class` です。実際の使用側
（`slick/compiler/{AssignUniqueSymbols,EmulateOuterJoins}.scala`）は
`t.copy(identity = x)(t.profileTable)` と、コンストラクタと同じ 2 引数リストで
書きます。

`Typer::try_rewrite_case_copy`（`crates/typer/src/check.rs`）は `p.copy(…)` を
コンストラクタ呼び出しに直接書き換えます（`copy[T]` 独自の型推論を再実装せず
既存のコンストラクタ呼び出し推論に乗せるため）。この関数は `Apply` ノード
**1 個ずつ**に対して呼ばれるので、`t.copy(identity = x)(t.profileTable)` では
まず**内側**の `Apply`（`t.copy(identity = x)`）だけに対して発火し —— 外側の
`Apply`（`(t.profileTable)` を渡す方）がまだ検討もされていないうちに —— 第 2
リストに属する分も含めて**全フィールド**を `t` 自身の値で埋め、完成した
`TableNode` を返してしまいました。外側の `(t.profileTable)` はその
`TableNode` 値への `.apply` 呼び出しとして読まれ、「value apply is not a
member of TableNode」になっていました。（型がバイトコード上も本当にカリー化
されたままか、それとも 1 本の引数リストに潰れているのか —— コンストラクタ
自身も含め、複数引数リストの Scala メソッドは JVM 上では**常に**1 本の
メソッドへ消去されるので、javap だけでは区別できません —— という点は、何も
触る前に real scalac 2.13.16 で `r.copy(a = 2)(r.extra)` が実際に通ることを
確認して裏を取りました。）

`Typer::try_rewrite_case_copy_curried` を新設し、`try_rewrite_case_copy` の
**先頭**で試すようにしました。`Apply` チェーンを `copy` の選択まで剥がし、
2 段以上あれば、コンストラクタの本当の引数リスト形状に合わせて
`ClassName(list1)(list2)…` という呼び出し列を再構築します（`new C(…)(…)`
ではなくコンパニオンの `apply` を使う点に注意 —— カリー化した `new` 呼び出し
自体にも**別の**、より狭いオーバーロード解決の穴があり（`Apply` 層を 1 個
ずつ独立にしか見ない）、そちらに乗せると別のバグと交換するだけでした。剥がす
先が 2 段に満たなければ（`depth < 2`）何もせず既存の単一リスト版に委ねるので、
圧倒的多数を占める非カリー化のケースには触れていません）。

#### 検証

`t3_extractor_bind.scala` / `t3_super_chain.scala` / `t3_curried_copy.scala`
はいずれも `--scala-library` と `--no-scala-library` の両方で
`-Xverify:all` を通し、real scalac 2.13.16 の標準出力とも突き合わせています
（`crates/cli/tests/tail3.rs`）。修正前の `main` では 3 本とも拒否されることを
確認済みです。継ぎ目（`check.rs` / `symbol.rs` / `gen.rs`）に触れたので
`--test tail3 --test conform --test e2e` を前景で回し、`cargo test --workspace`
もグリーンであることを確認しました。

#### 残件

* `value getOrElse is not a member of Product`（4 件）: `agent/tail1` が
  既に縮小を試みて単独再現に失敗したのと同じ症状（`nextBlobOption()
  getOrElse(…)` の `if (rs.wasNull) None else Some(r)` の lub が `Blob` /
  `Array[Byte]` / `Clob` / `Object` の 4 つだけ `Product` に落ちる）。今回も
  スクラッチから何本か縮小版を作りましたが、real scalac でも私たちの binary
  でも**通ってしまい**、slick 184 ファイル全体という状態に依存する点は
  `tail1.rs` の記録と変わりませんでした。追加の手掛かりはありません。
* `no matching overload for (=> F[B])(FlatMap[F])F[B]`（3 件、cats の
  `>>` 拡張メソッド）、`value map is not a member of Any`（3 件）、
  `value flatMap is not a member of Async$` / `value effect is not a member
  of <notype>` / `value database is not a member of BasicBackend.Session` /
  `value reduceLeft is not a member of Option[Node]`（各 2 件）は時間内に
  調査できませんでした。
* **副産物として見つけた別バグ（未修正）**: カリー化した `new C(…)(…)`
  （コンストラクタへの直接呼び出し、`copy` 経由ではない）は `Apply` 層を
  1 個ずつ独立に検査するらしく、`slick/lifted/SimpleFunction.scala:74`
  の `new SimpleLiteral(name)(tpe)` で `ambiguous overload for apply with
  arguments (String)` を出しています（今回の変更の前から存在する症状で、
  今回のどの修正が原因でもありません）。`try_rewrite_case_copy_curried` が
  `new` 経由の再構築を避けた理由がまさにこれで、同じ根を踏むはずです。

## ライセンス

Apache-2.0
