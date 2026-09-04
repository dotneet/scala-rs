## 実装していないもの

次は実装していません。スタブで「動いたことにする」こともしていません。言語側の残りとライブラリ側の残りを分けます。

言語:

- **def マクロの展開の残り**。展開そのものは動きます（上の「def マクロの展開
  （JVM ブリッジ）」）。まだできないのは:
  **whitebox マクロ** / **macro bundle**（`class B(val c: Context)`）/
  **マクロバインディングの pickle**（`MACRO` フラグと `@macroImpl`。だから
  マクロ def を*別 run*から展開することはできず、「マクロ def は現在の run、
  実装は前の run」という形だけが通ります）/ **推論された型引数のタグ**
  （明示された `f[T]` だけ）/ **`c.enclosingPosition` /
  `c.typecheck` / `c.inferImplicitValue`**（呼ばれると engine が
  `UnsupportedOperationException` を投げ、その名前が診断に出ます）/
  **ブロック・関数リテラル・`new` などの引数（およびレシーバ）を実装に渡すこと** /
  **レシーバを書かない呼び出しの `c.prefix`**（nsc の `This(<囲むクラス>)`）/
  **同じ run でコンパイルするクラスを型引数に取ること**（タグは
  `staticClass(<完全名>)` で組むので、engine の mirror が解決できるのは
  マクロ classpath＝*前の run* が書いたクラスだけです）/
  **タグを持たない型パラメータのタグ**（nsc は free type symbol を立てますが、
  scala-rs は断ります）。どれも「黙って別の木に展開する」ことはせず、
  `macro expansion is not implemented: cannot expand f (implementation Impl$.m):
  <理由>` と理由つきで診断します
  （**[`docs/macros.md`](docs/macros.md)** §7.11 / §7.12 / §7.13）
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
- `List[Option[A]].flatten`（`List(Some(1), None, Some(3)).flatten`）。witness の `scala.Option.option2Iterable[A](xo: Option[A]): Iterable[A]` は pickle から供給されるようになり、**view としては効きます**（`List(1) ++ anOption` / `val xs: Iterable[String] = anOption` は `agent/ovl3` で通るようになりました）。残っているのは `flatten[B](implicit asIterable: A => IterableOnce[B])` の側で、implicit **値**として `Function1` が要求されたときに implicit 変換*メソッド*を eta 展開しません。**現状はサイレントな誤コンパイルではなく診断**（`value mkString is not a member of ((Option[Int]) => IterableOnce[B])List[B]`）です
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

