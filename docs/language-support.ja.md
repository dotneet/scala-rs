## 実装している言語サブセット

Scala **2.13** 構文です。Scala 3 の `then`、トップレベル定義、TASTy はありません。エントリポイントは `def main(args: Array[String]): Unit` です。

パースできる（またはデシュガーする）構文:

- packages / imports。**`package` 句が開くのはそれが名指したパッケージだけ**（SLS 9.2）: 修飾付きの `package p.q` は `p.q` だけを開き、`p` のクラスもサブパッケージも見えない。入れ子の `package p { package q { … } }` は両方を開く。ルートは常に最後に見るので、`package p.q` からの修飾参照 `p.X` は解決する。この違いは観測できる —— `package slick.dbio` から見た `cats` は `slick.cats` ではなくトップレベルの `cats`（`agent/proj`）。**開いていないパッケージへの最終フォールバックは削除しました**（`agent/tail6`）—— それが覆っていた穴（デフォルト引数の右辺が呼び出し側で型付けされる）を塞いだためです
- objects / classes / traits / case classes。**補助コンストラクタ** `def this(...) = this(...)`（連鎖の先頭は `this(...)`。`super(...)` や文のあとの `this` は診断）。サブクラスの `extends C(1)` は primary が親 ctor を呼ぶ。内部クラスの `new Inner` は ctor overload 選択後も `$outer` を `<init>` の第一引数に残す。**case class の `copy(...)`**（positional / 一部省略時は自分自身の対応フィールドを default / 名前付き引数。`copy` は namer 時点ではまだ ctor フィールドの型が確定していないため、フィールド型解決後の typer フェーズで `copy` 自身の引数シンボルと `copy$default$N` を作り直す。private ランタイムでも動く）。**コンストラクタの省略可能引数**（`class C(x: Int, y: Int = 5)` の `new C(1)` / `new C(y = 2, x = 1)`）: 末尾を省略した呼び出しへのデフォルト値の充填は、通常の `def` の default getter 経由ではなく（`this` が無い呼び出し元では使えないため）保存した木をその場に差し込む形で実装。**その木は「書かれたスコープ」で型付けします**（`agent/tail6`。`Checker::record_default_scope` / `type_default_rhs_here`）—— 呼び出し側で型付けすると、定義ファイルの import が効かず、クラス自身のメンバまで見えてしまいます。コンストラクタのデフォルトからはクラスのメンバも先行する ctor 引数も見えません（`class Pair(a: Int, b: Int = a)` は nsc も `not found: value a`）。**名前付き引数での並べ替えは `new C(...)` でも動く**（コンストラクタのオーバーロードはパラメータ名で絞ってから型で決める）
- `val` / `var` / `def`（ネストした `def` はパースする）
- **テンプレート本体の式文**（`class A { println("ctorA") }`）。SLS 5.1 / 5.3 どおり、class なら主コンストラクタ、trait なら `$init$`、`object` ならモジュール初期化の一部として、`val` / `var` の初期化と**宣言順に交互に**走る。早期の `require(...)` / `assert(...)`、`if` / `match` / `try` / ループ / ラムダ、`case class` / ローカルクラス / 匿名クラス / メンバ `object` の本体でも同じ。詳細は「テンプレート本体の式文」節
- パラメータ、ラムダ（型付き / 期待型から推論）、ブロック。**placeholder `_`**（nsc `withPlaceholders`）: `_ + 1` / `_.abs` / `f(_)` / `xs.map(_ + 1)` / Function2 `_ + _` / 入れ子 `_.map(_ + 1)` に加え **typed `_ : T`**（`(_: Int) + 1` / `(_: Int) + (_: Int)` / `(_: Int).abs` / `xs.map((_: Int) + 1)`）。レキサが `_:` を `Ident("_")` にするので、式位置では Underscore と同じ placeholder にする。bare `(_: Int)` は `unbound placeholder parameter`。`xs.map(_ : Int)` は nsc どおり wrap せず map に Int が渡り mismatch。unary / Function2 の既存 wrap は触らない。**メソッド適用のセクション** `f(_, x)` / `f(_, _)` は期待型が無くても呼び先のシグネチャからパラメータ型を取る（nsc と同じ条件で、呼び先が単一の非ジェネリックメソッドのときだけ。`poly(_, 3)` や overload された `"abc".substring(_)` は `missing parameter type for expanded function` のまま）。合成パラメータはソース順で並べる（`two(_, _)` は `(a, b) => two(a, b)`）。**リテラルの本体は期待型の結果に対して検査する** ── `xs.foreach((x: Int) => x + 1)` は value discarding、`fl((x: Int) => x)` は `Int => Long` への数値拡大。パラメータ型を書いたリテラルはオーバーロード解決のために期待型より先に型付けられるので、そのぶんは `adapt` 側でやる。関数**値**は対象外で、`val h: Int => Int = …; fu(h)` は nsc どおり `type mismatch`
- `if` / `else`、`while`、`do { ... } while (cond)`
- `try` / `catch` / `finally`（catch は `{ case ... }`。`try/finally` と `try/catch/finally`。finally は正常終了と例外（catch からの throw 含む）の両方で走る。JVM 例外テーブルを出す。パーサは `finally` を落とさない）
- `match`（コンストラクタパターン、リテラル、ワイルドカード、Java enum 定数の安定識別子 `Thread.State.NEW`、`x @ Pat` の束縛、`case null`、入れ子の抽出子 `case P(v) :: t`。どの case にも当たらなければ `scala.MatchError`）
- for-comprehension（`map` / `flatMap` / `foreach` / `withFilter` へデシュガー。私有ランタイムでは `List.withFilter` は eager な `List`。`--scala-library` 時は `scala.collection.WithFilter[+A, +CC[_]]` で、`map[B]` は `CC[B]` を返す。`Option.withFilter` は `Option$WithFilter`）。値定義 `q = e` はラムダ本体の `val` になる ── **生成子ではない**ので、その前の生成子はやはり最内で `map` を取る。値定義の**後ろのガード**は nsc のタプル化が要るので診断する
- apply / select / infix（`:` 終わりの演算子は右結合で、レシーバは右オペランド。`1 :: Nil` → `Nil.::(1)`）。代入 `xs(i) = v` は nsc どおり `xs.update(i, v)`。代入でない `c(1)` で `apply` が無ければ診断する（黙って `update` にしない）
- リテラル、タプル
- 名前付き型・ジェネリック型（`Array[String]`、`def id[T](x: T): T` など）。infix 型 `A Either B` は `Either[A, B]`。`Map[K, V]` の applied 構文はそのまま。**高階型** `trait Functor[F[_]]` / `class Box[F[_], A](val fa: F[A])`。具象は `Id[_]` など。kind 不一致（`F[_]` を proper 位置で使う、proper 型を型コンストラクタとして使う）は診断する（黙って捨てない）。**`Array` は kind `* -> *`**（ソースの `Array[T]` は `Type::Array` になりシンボルに型パラメータが入らないので、`kind_arity` が特別扱いする。`TC[Array]` は nsc と同じく通る。`agent/asttype`）。ワイルドカード型引数がパラメータの kind を引き受けるのは**型パターンの中だけ**（`case o: TC[_]`）で、普通の型位置の `TC[_]` は nsc と同じく拒否する。**高階型メンバー** `trait M { type F[_] }` とパス依存適用 `m.F[Int]`。具象は subclass で `type F[X] = Id[X]`（または `List[X]`）。メンバーの kind 不一致（`type F[_]` を `type F = Int` で束縛、逆も）は診断する。**refinement の高階型メンバー** `M { type F[X] = Id[X] }` と適用。**HK 境界** `type F[_] <: Bound`（proper な境界。`type F[_] <: List` は nsc どおり `takes type parameters`）。**refinement の境界** `{ type A <: Int }`。クラス / トレイトの nullary `type A <: T` は未実装のまま診断する。**入れ子型射影** `Outer#Inner#X` / `Holder#Inner#T`。違法な `Int#X` と抽象 `B#U#T`（メンバー無し）は nsc どおり `is not a member`。**射影のメンバーは前置型から読み直す**: `A#B` の `B` が `A` の祖先の入れ子クラスなら、`B` のメンバーの型に出てくる抽象型メンバーは `A` が与えた定義で読む（`Sub#Ctx` の `def session: S` は `type S = Sess` を持つ `Sub` 越しに `Sess`）。読み直しは as-seen-from であって制約ではないので、`A#B` は部分型としても表示としても素の `B`（`agent/proj`）
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
- implicit val / def（ローカル、import、パッケージオブジェクト、コンパニオン）、implicit パラメータ、スコープ内の implicit conversion。第二パラメータ節の明示渡し `foo(x)(y)` を含む。候補が複数あるときは nsc 風の **more-specific**（結果型の subtype、または定義クラスが subclass である origin）。型と origin が食い違うと（親のより specific な implicit と、子に定義した less-specific な local）`ambiguous implicit`。同じ型が二つなら曖昧。目標型が `A => B` で `A <: B` のときは nsc と同様 identity view を合成する（view bound の呼び出し側）。**implicit class**（object / class 本体。`implicit class Rich(n: Int) { def twice: Int }` の `2.twice`）。**package object の `implicit class`**（同じパッケージの他 compilation unit、または `import pkg._`。pickle の IMPLICIT。トップレベル `implicit class` は nsc どおり `` `implicit` modifier cannot be used for top-level objects ``。import 無しでは enrichment が見えない）。**`Function1` を継承したクラスの値も implicit conversion です**（nsc の「候補の型が `From => To` に適合するか」。`scala.<:<[-From, +To] extends (From => To)` なので `implicit ev: P <:< Q` は `P` を `Q` へ変換し、適用は `Function1.apply`。引数を取らない implicit メソッド（`<:<.refl`）はビューにしません）
- `@tailrec`（末尾再帰でない `def` は nsc 風にエラー。object の末尾再帰は通して実行する。while 変換はしない。**パラメータリストを取らないメソッド**の再帰呼び出しは `Apply` を持たない裸の `Select` なので、`paramss` が空の宣言ではそれも呼び出しとして数える —— slick の `NominalType.sourceNominalType` の形。非末尾位置ならこれまでどおり `a recursive call not in tail position`。`agent/asttype`）/ `@deprecated`（引数付きアノテーションを pickle の SYMANNOT に載せる。コンパイルは壊さない）/ Java `@Override`（本当に override しているメソッドは受理。そうでなければ `overrides nothing`）/ Java `@Deprecated`（メソッドの `RuntimeVisibleAnnotations` に `Ljava/lang/Deprecated;` を出す。pickle は `SYMANNOT` + `java.lang.Deprecated` の TypeRef。`javap -v` と scalac `-deprecation` の両方で見える）/ ユーザー定義の `StaticAnnotation`（`@Ann(foo)` / `@Ann(this)` / `@Ann(classOf[Int])` / `@Ann(ident(1))` / `@Ann(this.x)` / `@Ann(super.foo)` / `@Ann(ident(ident(1)))` / `@Ann(foo = 1)` / `@Ann(foo = this.x)` / `@Ann(foo = bar)` の Ident/Select/This/Super/Apply / リテラル / classOf / named Constant / named TREE 引数を TREE / Constant として pickle。named は nsc と同じく位置引数に直して pickle）/ `@implicitNotFound("…")`（欠ける implicit は nsc と同じくその文面。`${A}` は型引数）/ `@switch`（`(n: @switch) match`。密な Int は `tableswitch`、疎なら `lookupswitch`。switch にできない match は nsc と同じ warning `could not emit switch for @switch annotated match`）。`@inline` / `@noinline` はアノテーションとして格納するだけで、インライン化はしない。実 scalac 2.13.16 は配置を一切検証しない（val / var / class / type などどれに付けても、両方同時に付けても、警告すら出さない — `-opt:inline:...` のバイトコード最適化器だけが読む情報で、typer は無関係）ので、こちらも同様に検証しない。`@volatile` / `@transient` は classfile の `ACC_VOLATILE` / `ACC_TRANSIENT`（javap で見える）。`@native` はメソッドに付けて `ACC_NATIVE` を出し、本文は付けない（`.so` のリンクはしない。本文付きや val への付与は診断する）
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
- **カリー化したコンストラクタ** `class C(a: Int)(b: Int)` の `new C(1)(2)`。`extends A(1)(2)` と同じく、JVM 上は 1 本の `<init>` なので引数リストを平坦化してから解決します。明示された implicit 節（`new K[B]("s")(ev)`）は**探索し直しません**。後続の節の名前付き引数（`new C(1)(c = 3, b = 2)`）も通ります。case class の `copy(…)(…)` はこのコンストラクタ呼び出しに書き換わります（`agent/tail4`）
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
- コレクションの `[B >: A]` な広がり: `Option.getOrElse` / `Option.orElse` / `immutable.Map.getOrElse` / `mutable.Map.getOrElse` は nsc どおり下限付き型パラメータを持ち、引数が結果の型を lub まで広げる（`(o: Option[Sub]).getOrElse(base): Base`）。`List.::` と同じ仕組み（`prelude_lowbound.rs` / `prelude_ovl3.rs`）。`scala.collection.mutable.HashSet` / `HashMap` / `LinkedHashSet` / `LinkedHashMap` は `mutable.Set` / `mutable.Map`（従って `scala.collection.Set` / `Map`）のサブタイプ。`Option` を `IterableOnce` として使う view（`Option.option2Iterable`）は jar の pickle から供給する（`--scala-library` のときだけ。私有ランタイムでは診断）。`new StringBuilder(initCapacity: Int, initValue: String)`（同じく `--scala-library` のときだけ。私有ランタイムの `StringBuilder` は `java.lang.StringBuilder` で、このコンストラクタが無い）
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
  （`Literal` / `Ident` / `Select` / `Apply` / `this`）、作れるタグ、戻せる木の
  種類は**部分集合**で、外れる形はすべて名指しで診断する（黙って違う木に
  展開しない）
  （[`docs/macros.md`](docs/macros.md) §7.11）
- **`c.Expr[T](tree)` を返す実装と `c.prefix`**: `scala.reflect.macros.Aliases` は
  `Expr` を `val`（抽出子）と `def Expr[T: WeakTypeTag](tree: Tree)` の 2 つ
  宣言している。明示型引数はオーバーロードを**値位置の畳み込みより先に**
  絞る（SLS 6.26.3 の順序）ので、`c.Expr[Int](tree)` は生成メソッドに解決する。
  `c.prefix` は呼び出し地点のレシーバ木を engine に渡し、nsc と同じく
  `Expr[Nothing](tree)(TypeTag.Nothing)` として渡す。運べないレシーバ
  （`new`・レシーバなし）は、実装が `prefix` を読んだときにだけ理由つきで
  診断する。あわせて `WeakTypeTag[F[E]]` を `appliedType` とスコープ内の
  タグから合成できるようになった（マクロの外でも同じで、`typeOf[List[Int]]` /
  `weakTypeOf[Option[Foo]]` が通るようになった。ただし `Predef.Map` のような
  **型別名**経由の構築子は、nsc が別名を保つのに対し scala-rs は指す先のクラスを
  名指すので `toString` だけが食い違う。`=:=` と `typeSymbol` は一致する）ので、
  **slick の `TableQueryMacroImpl.apply`
  と同じ形**（`c.Expr[F[E]]` を返し `WeakTypeTag[E]` を取り `New(TypeTree(e.tpe))`
  を書く）のマクロが展開できる。実 scalac 2.13.16 との dual-run で
  プログラム出力が一致する（`tests/fixtures/ex_impl.scala` +
  `tests/fixtures/ex_use.scala`）
  （[`docs/macros.md`](docs/macros.md) §7.12）
- **展開結果の `Function` / `ValDef`**: slick の `TableQueryMacroImpl.apply` が
  組む `Function(List(ValDef(Modifiers(Flag.PARAM), TermName("tag"),
  Ident(typeOf[Tag].typeSymbol), EmptyTree)), …)` が丸ごと往復する。
  `Modifiers` は**フラグの名前**で運ぶ（`universe.Flag` の値を反射で列挙する。
  nsc のビット配置は内部仕様で、しかも 1 ビットに 2 つ名前が乗る）。
  表に無い名前と名前の付かない残りビットは診断で、黙って落とさない
  （`var` を `val` に組み替えても誰も気づかないため）。あわせて
  `import c.universe._` が暗黙の `import scala._` に負けていたのを直し
  （SLS 2 の優先順位。`Function` が `scala.Function` に解決していた）、
  パスとして書いた `scala.Int` が primitive にならなかったのを直し、
  タプル・関数型・配列のタグ（`scala.TupleN` / `scala.FunctionN` /
  `scala.Array`）を組めるようにし、**引数を取らないマクロの結果を適用する形**
  （`M.f(1, 2)` で `f` が引数無し）で `Apply` をマクロ自身の引数節と
  読んでいたのを直した（マクロ def のパラメータ節の数で止める）。
  実 scalac 2.13.16 との dual-run で
  プログラム出力が一致する（`tests/fixtures/sd_impl.scala` +
  `tests/fixtures/sd_use.scala`）
  （[`docs/macros.md`](docs/macros.md) §7.13）
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
- **`reify { … }` の展開**: `reify` は quasiquote と同じコンパイラ内蔵マクロで、
  scala-reflect.jar に実装が無い。scala-rs 自身が
  `Expr.apply[T]($m, new $treecreator1())` を組む
  （`crates/typer/src/reify_expand.rs`）。**衛生性**は nsc と同じで、
  静的 `object` は `mkIdent($m.staticModule("..."))`、`.splice` は
  `x.in[$u.type]($m).tree`、型引数は `mkTypeTree`（単相クラスは
  `staticClass`、型パラメータはスコープの `WeakTypeTag` から
  `tag.in($m).tpe`）。ローカル・パラメータ・ブロック・型注釈・タグの無い型は
  **名指しで断る**（`cannot expand reify { ... }: ...`。黙って裸の名前を組まない）。
  リテラル / 静的 `object` への適用と選択 / `.splice` / 型引数が
  実 scalac 2.13.16 と dual-run で一致する（`tests/fixtures/rb_impl.scala` +
  `rb_use.scala`、[`docs/macros.md`](docs/macros.md) §7.15）。
  slick の `TableQueryMacroImpl` の `reify { TableQuery.apply[E](cons.splice) }`
  はこれで通り、`errors=115 → 113`
  （`else` の無い `if`、by-name 型、by-name / 可変長パラメータ、
  手続き構文 `def f() { … }`、パターン定義、自分型、early definition）と、
  `type` 定義
  （[`docs/macros.md`](docs/macros.md) §7.4 / §7.7 / §7.8 / §7.10）
- **refined `Context`、`MemberScope` のフィールド列挙、混在した `..$`**:
  マクロ実装の第 1 引数は `blackbox.Context { type PrefixType = … }`
  （`c.prefix` に型を付ける nsc のイディオム）でもよい。
  `rTag.tpe.decls.collect { case s: TermSymbol => … }` による case class の
  フィールド列挙が通る（`MemberScope` の pickle 親を 1 段ずつ読み、抽象型メンバの
  上限をたどってメンバの型を置換する）。`..$xs` は普通の要素と**混ぜて**書ける
  ——引数節・パターン引数節・ブロックの文・テンプレート本体のどこでも、
  nsc の `reifyList` と同じ `List(…) ++ xs ++ List(…)` に落ちる。
  rank 2（`...$xss`）は従来どおり名指しで断る。これで slick の
  `lifted/ShapedValue.scala` は **5 件 → 0 件**、`errors=99 → 94`
  （`tests/fixtures/sv_impl.scala` + `sv_use.scala`、
  [`docs/macros.md`](docs/macros.md) §7.16）
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

### ラムダの `invokedynamic` 化（`agent/indy`）

素の `FunctionN` リテラルは、閉包クラスではなく **`invokedynamic`** で出します。nsc 2.13
の `-Ydelambdafy:method` と同じ形です。

```
val f: Int => Int = x => x + 1
```

```
// Main$.<init>
invokedynamic #48, 0   // apply:()Lscala/Function1;
putfield      Main$.f:Lscala/Function1;

// Main$ の中に増える 1 メソッド（classfile は増えない）
public static final synthetic java.lang.Object $anonfun$0(java.lang.Object);
```

クラスファイル側の実装は 3 か所です。

- `crates/backend/src/classfile.rs`: 定数プールに `CONSTANT_MethodType`（JVMS 4.4.9）/
  `CONSTANT_MethodHandle`（4.4.8）/ `CONSTANT_InvokeDynamic`（4.4.10）を書けるようにし、
  `BootstrapMethods` 属性（4.7.23）を出す。ブートストラップの表は `Pool` が持つので、
  メソッドをまたいで同じ表に積まれ、同じ内容は 1 エントリに畳まれる。
- `crates/backend/src/code.rs`: `Assembler::invokedynamic_lambda`。
- `crates/backend/src/gen.rs`: `gen_function_indy`（call site）と `emit_lambda_body`
  （本体メソッド）。

**ブートストラップは `LambdaMetafactory.metafactory`**（3 引数版）で、
`samMethodType` と `instantiatedMethodType` は同じ `(Object…)Object` です。本体メソッドを
**erase した形**（引数も結果も `java/lang/Object`、プリミティブの box / unbox は本体の中）
で書いているので、`LambdaMetafactory` に適応させるものが何も残らず、ブリッジも要りません。

> nsc は代わりに **`altMetafactory`** を使い、`FLAG_SERIALIZABLE`（`1`）を、
> `instantiatedMethodType` が `samMethodType` と違うときは `FLAG_BRIDGES`（`4`）も渡し、
> `scala/runtime/LambdaDeserialize` を bootstrap にした `$deserializeLambda$` を添えます。
> さらにプリミティブ特殊化があるところでは `scala/runtime/java8/JFunction1$mcII$sp` の
> ような特殊化インタフェースを指し、call site 名を `apply$mcII$sp` にします
> （`javap -c -p -v` で確認済み）。scala-rs はどちらもしません。特殊化しないのは、
> 呼び出し側も `apply(Object)Object` で呼んでいて一貫しているからです。
> シリアライズ可能にしないのは、**合成クラスだった頃も `Serializable` ではなかった**ので
> 退化ではなく、`LambdaDeserialize` が私有ランタイムに無いためです。

**本体メソッドの置き場所。** call site を組み立てている時点では囲いの `ClassBuilder` は
`Assembler` に貸し出されていて、そこにメソッドを足せません。そこで本体は
`Gen::lambda_bodies` のキューに積み、クラスのメソッドを出し終えたところで
`Gen::drain_lambdas` が静的メソッドとして書き出します。クラスの emit が入れ子になっても
（無名クラス、trait の `$class`）取り違えないよう、**各 emitter は自分が積み始めた位置
（watermark）より上だけ**を回収します。本体の中にさらにラムダがあれば同じキューに積まれる
ので、キューが watermark に戻るまで回します。

**`$anonfun$N` は `public`** です。`PartialFunction` の合成クラスの中にあるラムダは、
その閉包クラスから **別のクラス**（囲いの実クラス）にある本体を指すためです。nsc の
`$anonfun$` も同じ理由で public static final synthetic です。

**囲いの `this`。** 合成クラスでは `$outer` フィールドでしたが、静的メソッドでは
**第 0 引数**です。`EmitCtx::outer_slot` がそれを持ち、`load_this` が
`getfield $outer` の代わりに `aload 0` を出します。ノンローカル `return`
（`NonLocalReturnControl` の key）も同じ経路なのでそのまま動きます。

**まだ合成クラスのままなもの**（意図的なフォールバック。混在で構いません）:

| 形 | 理由 | nsc は |
|---|---|---|
| `PartialFunction` の `{ case … }` | 抽象メソッドが `isDefinedAt` / `applyOrElse` の 2 本で SAM ではない | 同じく classfile |
| ユーザー定義 SAM 型（`trait Transform { def run(s: String): String }`） | まだ未対応 | `invokedynamic` |
| 引数 23 個以上 | `scala.FunctionN` が 22 までしかない | 同じ |
| interface の classfile の中（trait の抽象側） | JVMS 4.6 が interface メソッドの `ACC_FINAL` を禁じる。そもそもここはコードを出さない | — |

どれに落ちたかは `SCALA_RS_LAMBDA_TRACE=1` で stderr に出ます
（`LAMBDA-FALLBACK partial-function` / `sam:<内部名>` / `arity` / `no-hoist-owner`）。

**効果**（slick 184 ファイル。同じマシン・同じ時間帯で、変更前のバイナリ（`main` の
`crates/backend/src` から作り直したもの）と交互に測った値です）:

| | 変更前 | 変更後 | nsc |
|---|---|---|---|
| classfile 総数 | 4552 | **2127**（−53%） | 1498 |
| 出力サイズ | 22 MB | **13 MB** | — |
| コンパイル時間 | 215.6 秒 | 214.5 秒 | — |
| 全クラスのロード（`Class.forName(initialize=false)`、3 回の最小値） | 267 ms | **155 ms**（−42%） | — |

コンパイル時間は**ほぼ変わりません**（差はノイズの範囲）。閉包クラス 1 個ぶんの
定数プールとメソッド 3 本を書かなくなる代わりに、静的メソッド 1 本と
ブートストラップ 1 エントリを書くので、書き出しの仕事はあまり減らないためです。
減るのは**出力とロード**の側です。

`errors=0 files_with_errors=0` と `tests/slick_subset.sh` の `verified=2127 failed=0` は
変わりません。残る 716 個の閉包クラスのうち **707 個が `PartialFunction`**、9 個が
ユーザー定義 SAM 型です（slick のソースに `{ case` は 728 か所あるので、重複して出して
いるわけではありません）。

fixture は `indy1`（両 ABI）/ `indy2`（library と実 scalac の byte 一致）/ `indy1_bad`、
テストは `crates/cli/tests/indy.rs` です。

### メソッド型パラメータの推論（引数＋期待型）

nsc の `instantiateExpecting` と同じく、メソッドの型パラメータは**引数と期待型の両方**を制約として解きます（`crates/typer/src/check.rs` の `add_expected_constraints`）。

- 結果型の**不変位置**では期待型が引数の解より優先します。`Array` は非変なので `val a: Array[AnyRef] = Array("x", "y")` は `T = AnyRef`（`[Ljava.lang.Object;`）、`val b: Array[Any] = Array(1, 2)` は `T = Any` でボックスされます。
- **共変位置**の期待型は上界にすぎないので引数の解が勝ちます（`cov("q"): List[Any]` は `T = String`）。
- 解いた型引数は**implicit 引数リストの解決より前**に確定します。`def column[T](n: String)(implicit tt: TypedType[T]): Rep[T]` を `Rep[Int]` の位置で呼ぶと `TypedType[Int]` を探しに行きます。
- どちらでも決まらない型パラメータは `Nothing` で埋めず、nsc と同じ診断（`could not find implicit value …`）を出します。
- 引数の期待型（prototype）は、**型パラメータを 1 つも持たない callee** でも
  渡します。ただし「関数型・`FunctionN`・SAM のいずれかで、しかも完全に決まって
  いる」パラメータに限ります（`Typer::proto_arg_type` / `agreed_function_param`）。
  引数**そのもの**が関数リテラルなら `agreed_lambda_params` が面倒を見ますが、
  リテラルが引数の**中**にある `f(if (c) { s => … } else { s => …; … })`
  （slick の `JdbcBackend`）には期待型が届いていませんでした。1 式だけの分岐が
  通っていたのは偶然で、`section_param_types` が本体の呼び出しからパラメータ型を
  拾えていただけです。多重定義（case class のコンパニオン `apply` は
  継承した `AbstractFunctionN.apply` と 2 候補になります）は**全候補が同じ**
  パラメータ型を要求するときだけ、コンストラクタ（`new C(…)` / `C(…)`）は
  クラスに型パラメータが無くアリティが一意のときだけです。
- Java メソッドの型引数に書いた `Any` は、その型パラメータの上限である
  `Object` として読みます（nsc の `ObjectTpeJava`）。
  `java.util.Arrays.copyOf[Any](a: Array[AnyRef], n)`（slick の `ConstArray`）は
  `Array` が不変でも通り、結果は `Array[AnyRef]` です。`Array[String]` を渡すのは
  nsc と同じく拒否します。
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

変換の型引数は、受け手をパラメータのクラスにおける**基底型**に直してから解きます
（`Typer::conv_targs` が `base_type_instance` を通す。`agent/mismatch14`）。
`implicit def mapAsScalaMapConverter[K, V](m: java.util.Map[K, V])` に
`ConfigObject`（`extends java.util.Map[String, ConfigValue]`）を渡すと、
受け手そのものには突き合わせる型引数が 1 つも無いので、`K` も `V` も
`AnyRef` に落ちていました（`config.root.asScala` が `Map[AnyRef, AnyRef]`）。
`implicit class` の型パラメータも同じ経路です
（`class Sub extends Base[String, Int]` に対する `sub.firstOf: String`）。

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

親コンストラクタは**名前付き引数**も取ります（`agent/dbio`）。

```scala
class MultiInsertAction(…)
  extends SimpleJdbcProfileAction[MultiInsertResult](
    _name = "MultiInsertAction",
    statements = rowsPerStatement match { … }
  )
```

`new C(b = 2, a = 1)` と同じく、**オーバーロードを選ぶ前に**パラメータ順へ並べ替えます
（`reorder_named_ctor_args`）。並べ替えなしだと `name = value` が「存在しない変数への代入」
として型付けされ、slick のこの 1 か所から `not found: value _name`、
`not found: value statements`、そして残った 2 個の `Unit` による
`no matching overload for constructor SimpleJdbcProfileAction with arguments (Unit, Unit)`
の 3 件が出ていました。並べ替えに失敗したとき（`unknown parameter name: …`）は木を
**書き換えずに**返します。親位置はシグネチャパスでも歩かれ、そちらの診断は捨てられるので、
そこで名前付き引数を消費してしまうと本体パスには位置引数しか残らず、
`no matching overload` という別の（誤解を招く）診断しか出せなくなるためです。

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

結果型を書かないオーバーライドが親から受け継ぐ型に**抽象型メンバ**が出てくるときは、
そのサブクラス自身の具象エイリアスで読み直します（nsc の as-seen-from。
`Typer::own_type_members`、`agent/mismatch14`）。
`trait Node { type Self <: Node; def rebuild(…): Self }` を
`case class StructNode(…) { type Self = StructNode }` が実装したら、
`rebuild` の結果型は `StructNode` です。

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

### `super.m` は「親」ではなく `this.type` から見る（`agent/lastone`）

slick に残っていた**最後の型エラー 1 件**（`jdbc/SQLiteProfile.scala:183`）と、
それが型検査を通って初めて届くようになった **codegen のバグ 2 件**です。
これで slick の **184 ファイルすべてが 1 回のコンパイルで型検査を通り**、
**4552 個の classfile** が出て、その**全部が `java -Xverify:all` でロードできる**
ようになりました（セッション開始時 537 件 → 直前 1 件 → **0 件**）。

```
# 直前:  subset_files=47  classes=300  verified=300 failed=0
tests/slick_measure.sh   → files=184 errors=0 files_with_errors=0 classes=4552
tests/slick_subset.sh    → verified=4552 failed=0
                           subset_files=184 classes=4552 (of 184 sources)
```

診断はこう出ていました:

```
error: no matching overload for (Iterable[U], JdbcActionComponent.RowsPerStatement)…
       with arguments (Iterable[U], RowsPerStatement)
```

**「オーバーロード」でも「名前付き引数」でもありませんでした。** 候補は 1 つしか
なく、それが引数を拒んでいただけです。根は `super.m` のメンバ型を**親のクラスを
単独で名指しして**読んでいたことでした。正しくは `this.type` から見ます:

```scala
// slick/jdbc/JdbcActionComponent.scala
trait JdbcActionComponent extends BasicActionComponent { self: JdbcProfile =>
  type RowsPerStatement >: slick.jdbc.RowsPerStatement.One.type <: slick.jdbc.RowsPerStatement
  trait InsertActionComposer[U] {
    def insertAll(values: Iterable[U], rowsPerStatement: RowsPerStatement = defaultRowsPerStatement): …
  }
  object MultipleRowsPerStatementSupport extends … {
    override type RowsPerStatement = slick.jdbc.RowsPerStatement   // ← 具体化
  }
}
// slick/jdbc/SQLiteProfile.scala:183
trait SQLiteProfile extends JdbcProfile with JdbcActionComponent.MultipleRowsPerStatementSupport {
  private trait SQLiteInsertAll[U] extends InsertActionComposerImpl[U] {
    override def insertAll(values: Iterable[U], rowsPerStatement: RowsPerStatement = RowsPerStatement.All) =
      super.insertAll(values = values, rowsPerStatement = if (…) RowsPerStatement.One else rowsPerStatement)
  }
}
```

`InsertActionComposerImpl` を単独で見ると `rowsPerStatement` は**抽象型メンバの
まま**（`>: One.type <: RowsPerStatement`）で、そこに適合するのは下限の
`One.type` だけです。`SQLiteProfile` は `MultipleRowsPerStatementSupport` を
mixin しているので、`this.type` から見れば `slick.jdbc.RowsPerStatement` に
なります。`Check::type_select` で `super` の受け手を作るときに `this_id` を
覚えておき、メンバ型を `expand_type_members(this_id, …)` に通すようにしました
（`this.m` と `x.m` は以前から `expand_in_type` で同じことをしています）。

続けて、classfile 側で 2 つ壊れていました。**どちらも main にもとからあった
バグ**で、この形が型検査を通るまで踏めなかっただけです:

1. **抽象型メンバの erasure が `Object` でした。** SLS 3.7 では型パラメータと
   同じく**上限に erase** します。実 scalac 2.13.16 も
   `insertAll(Iterable, Rps)` と書きます。`Object` にしていたため、継承した
   `insertAll(Iterable, Object)` とプロファイル側の
   `insertAll(Iterable, Rps)` が**別の JVM メソッド**になり、trait の
   `$super$` アクセサが `NoSuchMethodError` になっていました
   （`crates/typer/src/erasure.rs::erase_ty`）。**上限が 1 つのクラスを名指し
   している場合だけ**採ります。`type TermName >: Null <: TermNameApi with Name`
   （scala-reflect）のような**合成型の上限**は nsc の `intersectionDominator` が
   要るので、従来どおり `Object` のままにしています。先頭の親を採ると
   `TermNameApi` になり、これは `NameApi` ではないので、`Name` を要求する
   `Select.apply(TreeApi, NameApi)` に `TermName` を渡すときの
   checkcast（`Object` 由来だから入っていた）が消え、マクロブリッジが
   `VerifyError: Bad type on operand stack` になりました。
2. **`T$$super$m` アクセサの記述子が呼び出し側と転送先で食い違っていました。**
   アクセサは trait のメンバなので**オーバーライドした側の** erasure を持ち、
   転送先の親メソッドは**自分の** erasure のままです。
   `override type Rows = One.type`（上限より狭い具体化）だと 2 つは一致せず、
   `invoke_super` は存在しないメソッドを呼び、アクセサ本体は存在しない
   メソッドへ `invokespecial` していました。呼び出しは**現在のメソッドの**
   記述子で、転送は**転送先の**記述子で行い、戻り値が狭まるときだけ
   `checkcast` を挟むようにしました（`crates/backend/src/gen.rs::invoke_super`
   / `emit_super_accessors` / `super_target_desc`）。

**ブリーフの仮説は当たっていませんでした。** 前スライスは
「境界付き抽象型メンバの as-seen-from」と書いていて、領域としては合っていますが、
実際に効いたのは `subst_as_seen_from` でも `self_type_of_class` でもなく
**`super` の受け手だけが `this` の型メンバ表を見ていなかったこと**です。
`self:` 注釈（`self: JdbcProfile =>`）も無関係でした。

**この節では直していない、同じ領域の残件**（実 scalac は通します）:

```scala
trait Profile extends Comp with MultiSupport {   // MultiSupport が type Rows を具体化
  def h(c: ComposerImpl[Int], x: Rps): String = c.single(x)   // ← こちらは今も拒否
}
```

`ComposerImpl` は `Comp` の内部クラスなので、nsc での型は
`Profile.this.ComposerImpl[Int]` です。こちらの `Type::Class` は**前置型を
持たない**ので、`Profile` が `Rows` を具体化していることに到達できません。
`super` と `this` 経由は今回直りましたが、**値を経由した内部クラスの受け手**は
前置型を型に持たせないと直りません（`Type::Class` に prefix を足す変更なので、
このスライスではやっていません）。

### 演算子名の `val` がフィールド名として encode されていなかった

184 ファイル全部の classfile が出て初めて表に出ました。4552 個のうち **2 個**が
`java.lang.ClassFormatError: Illegal field name "/"` でロードできませんでした
（`slick.ast.Library$` と `slick.lifted.NumericColumnExtensionMethods$class`）。

```scala
// slick/ast/Library.scala:31
val / = new SqlOperator("/")
```

**メソッド名は encode していましたが（`crates/pickle/src/names.rs`）、
フィールド名は生のまま**でした。JVMS 4.2.2 の「unqualified name」は
`.` `;` `[` `/` を許さないので、`/` だけがロード時に落ちます
（`+` `-` `*` `%` はたまたま合法なので、名前は変でも動いていました）。
nsc は項の名前をすべて同じ NameTransformer に通します。
`ClassEmit::write_with_pool` のフィールド定義側と、
`getfield` / `putfield` / `getstatic` / `putstatic` の参照側の**両方**を
`encode_method_name` に通すようにしました（`crates/backend/src/code.rs`）。
encode 済みの名前を通しても変わらないので、既存の合成フィールド
（`$outer` / `bitmap$0` / `MODULE$`）には影響しません。

### `slick_subset.sh` が警告でファイルを捨てていた

型エラーが 0 になって初めて表に出ました。`slick_subset.sh` は
`^\s+--> …\.scala` の行からエラーのあったファイルを拾っていましたが、この
`-->` 行は**警告にも付きます**。0 エラー・2 警告の計測ログを種にすると
`JdbcActionComponent.scala` が「悪いファイル」として除かれ、それに依存する
ファイルが次の周で落ち、収束済みの 184 ファイルが 132 まで縮んでいきました。
`grep -A 2 '^error'` を先に噛ませて、**エラーの直後の `-->` 行だけ**を見る
ようにしました。

