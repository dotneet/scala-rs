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
| `c2_thisinterp.scala`（`crates/cli/tests/cats2.rs`、私有ランタイム・library dual-run） | `agent/cats2` スライス: 文字列補間の `$this`。`this` はキーワードなので `Ident` として項に探しに行くと `not found: value this` になる（slick の `s"No type for symbol $sym found in $this"`）。クラス・トレイト・`object`・ラムダの中で見る | `2 of Node(a)` … `5 in MAIN` |
| `c2_thisinterp_bad.scala`（`crates/cli/tests/cats2.rs`、異常系） | `$this` を特別扱いしても、スコープに無い `$name` は依然 `not found: value nosuchvalue` | （コンパイルエラー） |
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
| `o3.scala`（`crates/cli/tests/ovl3.rs`、私有ランタイム・library dual-run・real scalac dual-run） | `agent/ovl3` スライス: `Option.getOrElse[B >: A]` / `orElse[B >: A]` が引数で結果を広げる（`Option[Sub].getOrElse(base): Base`）。`Nothing`（`throw`）は何も広げない | `got Sub` `got Base` `got Base` `got Sub` `fallback` `Sub` |
| `o3_lib.scala`（`crates/cli/tests/ovl3.rs`、library dual-run と real scalac dual-run） | `mutable.HashSet` / `HashMap` が `scala.collection.Set` / `Map` として渡せる、`Map.getOrElse[V1 >: V]`（immutable / mutable 両方）、`Option` を `IterableOnce` として使う（`Seq("a") ++ anOption` / `val it: Iterable[String] = anOption`）、`new StringBuilder(8, "ab")`。私有ランタイムには裏付けが無いので `--no-scala-library` では診断のまま | `2` `1` `Base Sub` `Base` `Sub Base` `a,x` `a` `1` `abc` |
| `o3_bad.scala`（`crates/cli/tests/ovl3.rs`、両モードで拒否） | `Option[Int].getOrElse("no")` は lub の `Any`。`Int` には代入できない（nsc 2.13.16 も拒否） | `type mismatch; found: Any  required: Int` |
| `mism14.scala`（`crates/cli/tests/mismatch14.rs`、私有ランタイム・library dual-run・real scalac dual-run） | `agent/mismatch14` スライス: 単相の callee / SAM パラメータ / case class のコンパニオン `apply` のいずれでも、`if/else` の中の関数リテラルがパラメータ型を受け取る（2 文の本体は `section_param_types` では拾えない）。generic な**基底型**しか持たない受け手の `implicit class`（`Sub extends Holder[String, Int]`）。継承した結果型の抽象型メンバ（`type Self = Leaf`）。`Arrays.copyOf[Any](Array[AnyRef], n)` | `aaabbb` `zz` `si si 7` `leaf1` `mnnn` `3 x null` |
| `mism14_lib.scala`（`crates/cli/tests/mismatch14.rs`、library dual-run と real scalac dual-run） | `java.util.ArrayList[String]` / `HashMap[String, Integer]` を**継承した** Scala クラスに `asScala`（slick の `ConfigObject` と同じ形）。私有ランタイムには `scala.jdk.CollectionConverters` が無いので `--no-scala-library` では診断のまま | `x,y` `7` `1` |
| `mism14_bad.scala`（`crates/cli/tests/mismatch14.rs`、両モードで拒否） | Java の型引数 `Any` を `Object` と読んでも `Array` は不変のまま。`copyOf[Any](Array[String], 3)` は拒否（nsc 2.13.16 も拒否） | `no matching overload for (Array[AnyRef], Int)Array[AnyRef] with arguments (Array[String], 3)` |
| `at.scala`（`crates/cli/tests/asttype.rs`、library dual-run と real scalac dual-run） | `agent/asttype` スライス: `class TC[C[_]]` に `Array` / `List` を渡す（`Array` は kind `* -> *`）、型パターンの `case t: TC[_]`、無引数メソッドの `@tailrec`（`n.last`）、`implicitly[Ordering[Null]]` / `Ordering[java.util.Date]`（`Ordering.ordered` ＋ `Predef.$conforms`）、`ConstArray` 形の `toMap[R, U](implicit ev: T <:< (R, U))` と `immutable.HashMap` の `filter` / `map` / `collect` / `++`（slick `RewriteJoins.hoistFilterFromBind` の形）。`@tailrec` / `Ordering` / `<:<` / `HashMap` は私有ランタイムに裏付けが無いので `--no-scala-library` では診断のまま | `3` `2` `array/list` `true` `false` `3` `2` `true` `-1` `1` `2` `Some(s1)` `1` |
| `at_bad.scala`（`crates/cli/tests/asttype.rs`、拒否） | proper type は型構築子ではない（`TC[Int]`）。ワイルドカードが kind を引き受けるのは型パターンの中だけ（`def anyOf(t: TC[_])` は nsc も `_$1 takes no type parameters` で拒否）。無引数の再帰呼び出しでも非末尾なら `@tailrec` は不可（`def loop: Int = loop + 1`） | `kinds of the type arguments (Int) do not conform …` / `kinds of the type arguments (_) do not conform …` / `a recursive call not in tail position` |
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
`crates/cli/tests/outer.rs`（fixture 接頭辞 `outer`）の専用スイート（9 本）です。
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
`outer1.scala` は「匿名クラスから外側のクラスを触る」形を 1 ファイルにまとめたもので
（§「匿名クラスから外側のクラスを触る 4 つの根」）、親コンストラクタ引数からの外側読み、
`private` / `private[this]` の val・var・def を匿名クラス／ローカルクラス／ラムダ本体／
コンパニオンから、外側の `private[this] var` への代入、ラムダの中で作る匿名クラスを回します。
`outer1_anon_ctor_stores_outer_before_super` は `<init>` の命令順（`$outer` の
`putfield` が super 呼び出しより前で、引数は `<init>` の引数から読む）を、
`outer1_private_members_take_scalacs_expanded_name` は `Main$Outer$$secret` /
`Main$Outer$$bumped_$eq` / `Main$P$$y` / `Main$Holder$$note` という
**実 scalac 2.13.16 と同じ名前**を、`javap -p -c` の出力で固定します。

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

`c.Expr[T](tree)` を返す実装・`c.prefix`・型構築子のタグは接頭辞 `ex`、同じ `crates/cli/tests/engine.rs` に同じ 2 段構成で入れました。`tests/fixtures/ex_impl.scala`（`c.Expr[T]` を返す実装、`c.prefix` を読む実装、そして **slick の `TableQueryMacroImpl.apply` と同じ形**――`c.Expr[ExBox[E]]` を返し `WeakTypeTag[E]` を取り `New(TypeTree(e.tpe))` を書く実装）を先にコンパイルし、`tests/fixtures/ex_use.scala` をその出力に対してコンパイルします。`ex_expr_and_prefix_macros_expand_and_run` が `expected/ex_use.txt` の 10 行と一致することを、`ex_macros_match_real_scalac` が実 scalac 2.13.16 の 2 段コンパイル・実行と一致することを見ます。出力には `weakTypeOf[ExBox[E]].toString`（合成したタグの型）と `c.prefix.staticType.toString` が含まれるので、**タグや prefix の作り方が nsc と違えば行が変わります**。異常系は `ex_notag_bad.scala`（タグの無い型パラメータ）と `ex_gaps_bad.scala`（運べないレシーバ＝`new`、レシーバなしの呼び出し）で、どちらも実 scalac は通るので**scala-rs 側の穴を名指しで固定した fixture** です（[`docs/macros.md`](docs/macros.md) §7.12）。

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
| `lf2_lift_bad.scala`（同上） | 持ち上げられない穴が型を名指しで診断されること（`File` / rank 0 の `List[Int]` / `..$` 越しの `Symbol`）と、`reify { … }` がローカルを名指しで断ること（`cannot expand reify { ... }: \`f\` is a local ...`） | （診断のみ） |
| `rb_impl.scala` + `rb_use.scala`（`crates/cli/tests/engine.rs`、2 段コンパイル） | **`reify { … }` の展開**（実 scalac 2.13.16 と dual-run）: リテラル 4 種、静的 `object` への適用、`.splice`（引数 1 つ / 2 つ / `String` / `Boolean`）、`c.universe.reify` の形、型引数（単相クラス `staticClass` と、タグから解く型パラメータ 1 つ・2 つ）。splice は副作用で「1 回ずつ評価される」ことまで見る | `42` `hello` `true` `9000000000` `42` `42` `42` `head-tail` `false` `7` `3` `5` `s` `1/x` `3` `2` |
| `rb_bad.scala`（同上） | `reify` が断る 5 形が名指しで診断されること（パラメータ / ローカル / 型注釈 / ブロック / タグの無い型引数）。実 scalac は 5 つとも通すので、これは**未実装の告白**である | （診断のみ） |
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

`mism13_lang.scala` / `mism13_lib.scala` は `crates/cli/tests/mismatch13.rs` から
回します（`mism13_lang` は**両モード**、`mism13_lib` は library モードのみ。
私有ランタイムには `scala.<:<` が無いので、
`mism13_lib_without_library_is_error` で `not found: type <:<` を出して**黙って
通さない**ことも見ています）。多ファイルの原因は `tests/multi/mism13_util.scala`
/ `mism13_ast.scala` / `mism13_jdbc.scala` の 3 本
（`mism13_copy_names_no_class_in_the_using_file`）で、`copy` の書き換え先の
クラスを**その名前で解決できないファイル**が要るので単一ファイルでは再現
しません。同ファイルには最小形の受理テスト
（`mism13_self_new_substitutes_once` /
`mism13_conformance_witness_is_a_view` /
`mism13_nested_lambda_result_variable` /
`mism13_lub_sees_the_sequence_head` /
`mism13_inherited_member_at_owner_targs` /
`mism13_explicit_targs_are_the_argument_pt` /
`mism13_branch_join_closes_a_free_variable` /
`mism13_branch_join_keeps_a_parameter_in_scope`）と、拒否テスト
（`mism13_bad_is_still_rejected`、フィクスチャは `mism13_bad.scala`。
実 scalac 2.13.16 も同じ 6 件を拒否します）も置いてあります。13 本のうち
10 本は**修正前の `main` で落ちること**を確認済みで、残る 3 本
（`mism13_lib_without_library_is_error` と 2 つの「広げすぎない」ガード）は
before/after どちらでも通る性質のものです。

`pj_projmember.scala` / `pj_pkgscope.scala` は `crates/cli/tests/proj.rs` から
**両モード**（`--scala-library` / `--no-scala-library`）で `-Xverify:all` の下に
走らせ、real scalac 2.13.16 の標準出力と突き合わせます。`pj_projmember` は
型射影 `A#B` のメンバーを前置型で読み直すこと（選択・引数として渡すこと・
親から継承した射影 `Sub#Deep`・エイリアス経由の同じクラス `Sub#S` と同一で
あること）、`pj_pkgscope` は入れ子の `package p { package q { … } }` が両方を
開くことを見ます。修飾付きの `package p.q` はファイル先頭にしか書けないので
多ファイルが要り、`a_qualified_package_clause_does_not_open_its_parent` が
一時ディレクトリに 5 本書き出して「`package pjq.sub` から見た `inner` は
`pjq.inner` ではなくトップレベル」を確かめます（同じプログラムを実 scalac に
も通す `scalac_agrees_…` 付き）。拒否テストは `pj_projmember_bad.scala`
（`pj_projmember_bad_is_rejected`）で、前置型が何も決めていない `Base#Ctx`、
`database` を持たないクラスに決まる `Alt#Ctx`、そもそも無いメンバーの 3 件。
実 scalac 2.13.16 も同じ 3 件を拒否することを
`scalac_agrees_pj_projmember_bad_is_rejected` で固定しています。8 本のうち
4 本（`fixtures_pj_projmember` / `…_private` / `…_bad_is_rejected` /
`a_qualified_package_clause_does_not_open_its_parent`）は**修正前の `main` で
落ちること**を確認済みです。

`t6_defaults.scala` / `t6_defaults_bad.scala` / `t6_regex.scala` は
`crates/cli/tests/tail6.rs` から回します。`t6_defaults` は**両モード**で
`-Xverify:all` の下に走らせ、実 scalac 2.13.16 の標準出力と突き合わせます
（デフォルト引数の右辺が**定義されたスコープ**で解決されること、default 付き
implicit 引数が探索の空振り時に default に落ちること）。`t6_defaults_bad` は
定義スコープに無い名前（`Hidden`）と、コンストラクタのデフォルトからは見えない
先行 ctor 引数（`a`）の 2 件を拒み、`scalac_agrees_t6_defaults_bad_is_rejected`
が**実 scalac も同じ 2 件を拒む**ことを固定します。`t6_regex` は `Regex` の実
ABI（`CharSequence` パラメータ）が要るので library モードのみで、私有ランタイム
では**黙って通さない**ことを `t6_regex_is_diagnosed_without_the_library` で
見ています。cats-effect の jar が Coursier キャッシュにあるときだけ走る
`an_implicit_from_a_jar_answers_for_its_supertypes` は、`implicit F: Async[F]`
が `Sync[F]` / `GenTemporal[F, Throwable]` にも答えること（jar クラスの親を
読むこと）を見ます。9 本のうち 5 本は**修正前の `main` で落ちること**を確認
済みです。

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
- **名前付き引数の残り**: (a) **prelude / classpath のメソッドはパラメータ名を持たない**ので、`List(1,2,3).mkString(sep = "-")` や jar・`-cp` 上の case class への `copy(name = …)` は `unimplemented syntax: named arguments (method parameters not resolved)` になる（scala-library の pickle からパラメータ名を読む経路も、prelude 手書きシグネチャの名前付けも未実装。同一コンパイル単位のメソッド・クラスなら全部動く）。(b) ~~**複数引数リストのコンストラクタ** `class C(a: Int)(b: Int)` は名前付き引数以前に `new C(1)(2)` 自体が未対応~~ → `agent/tail4` で `new C(1)(2)` も `new C(1)(c = 3, b = 2)` も実装済み（`tests/fixtures/t4_curried_new.scala`）。(c) 名前と型が同一で順序だけ違うオーバーロード（`h(s: String, n: Int)` と `h(n: Int, s: String)`）は nsc なら `ambiguous reference to overloaded definition` だが、こちらは先に宣言された方を黙って選ぶ
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

