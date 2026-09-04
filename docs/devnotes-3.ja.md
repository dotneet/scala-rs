### 期待型は引数のプロトタイプ、オーバーロードの後続の節は受け手の型引数で（`agent/cats3`）

3 スライス（`agent/tail4` / `agent/cats2` / `agent/proj` / `agent/tail6`）が根を
探して残していた cats まわりの 5 件——`no matching overload for (=> F[B])
(FlatMap[F])F[B]` 3 件（`slick/basic/BasicBackend.scala`）と
`could not find implicit value of type GenTemporal[F, _]` 2 件
（`slick/basic/ConcurrencyControl.scala`）——を扱いました。根は**別々の 2 つ**で、
どちらも `>>` そのものとも `Async` / `Deferred` のカスケードとも関係ありませんでした。
ついでに 3 つ目（暗黙変換自身の implicit 節が候補を完成させていなかった件）も
直しています。`tests/slick_measure.sh` は
**`errors=99 → 92`、`files_with_errors=39 → 38`**（新規エラーは 0、消えたのは
上の 5 件と `slick/cats/Database.scala` の `Sync[F]`、`BasicBackend.scala:151` の
`FlatMap[F]`）。codegen（`crates/backend/`）は触っていないので
`tests/slick_subset.sh` は省略しています。

#### 1. by-name の仮引数がプロトタイプになっていなかった

nsc の `Infer.protoTypeArgs` は、引数を 1 つも型付けする前に**期待型から**呼び先の
型パラメータを解いて、それを**仮引数に代入**します。`Checker::proto_arg_type` は
それを「仮引数が**裸の**型パラメータそのものである場合」にしか行っていませんでした。
cats の

```scala
def >>[B](fb: => F[B])(implicit F: FlatMap[F]): F[B]
```

は仮引数が `=> F[B]` なので該当せず、引数は**期待型なし**で型付けされていました。

```scala
a >> commitResult.fold(asyncF.raiseError, _ => asyncF.unit)
```

`fold[C](fa: A => C, fb: B => C): C` の `C` は、期待型が無ければ
`lub(F[A], F[Unit])` ——`AnyRef` ——になります。それが `F[B]` に合うはずもなく、
`no matching overload for (=> F[B])(FlatMap[F])F[B] with arguments (AnyRef)`。
期待型 `F[Unit]` から `B = Unit` を解いて `=> F[Unit]` を渡すと、`C` は `F[Unit]`
に決まり、`asyncF.raiseError` の eta 展開も `A = Unit` で決まります。

代入した結果に呼び先の型パラメータが 1 つでも残る場合は、プロトタイプを出しません
（残った変数はその境界でしか引数を縛れず、それは後段の `open_to_bounds` の仕事です）。
by-name は**外して**渡します: 引数式の期待型は値の型で、`Function0` への包み直しは
`adapt` の仕事だからです（包んだまま渡すと `is_sub_type(F[Unit], => F[Unit])` が偽で、
呼び出し側の「プロトタイプはヒントであって制約ではない」再試行に捨てられていました）。

同じ経路は 3 件のうち 1 件（`BasicBackend.scala:432`）を**カスケードとして**も
消しました。`agent/tail4` の「他の 6 件からのカスケードに見える」という見立ては
向きが逆で、`>>` の 3 件のうち 1 件が残り 2 件の側のカスケードでした。

#### 2. オーバーロードを 1 本に決めた瞬間、受け手の型引数を捨てていた

`type_apply_in` はオーバーロード集合から 1 本選んだあと、

```rust
if matches!(&fun.ty, Type::Overload(_)) {
    fun.ty = self.st.get(sym).ty.clone();   // ← 宣言そのもの
}
```

としていました。`fill_defaults_and_implicits` は**後続の（implicit）節をこの
`fun.ty` から読み直す**ので、implicit パラメータの型は宣言を書いたクラス自身の型
パラメータのまま探索に渡ります。cats-effect の

```scala
final class GenTemporalOps_[F[_], A](val wrapped: F[A]) extends AnyVal {
  def timeoutTo(d: Duration,       fallback: F[A])(implicit F: GenTemporal[F, _]): F[A]
  def timeoutTo(d: FiniteDuration, fallback: F[A])(implicit F: GenTemporal[F, _]): F[A]
}
```

は `Duration` / `FiniteDuration` で**オーバーロードされている**ので、
`wait.timeoutTo(timeout, …)` の implicit 節は `GenTemporalOps_` 自身の `F` を指す
`GenTemporal[F, _]` として探索に届き、スコープの `Async[F]`（呼び出し側の `F`）とは
永久に合いません。オーバーロードされていないメンバは `type_select` が入れた
as-seen-from 済みの型をそのまま持っているので、**オーバーロードされたメンバだけ**が
この穴に落ちていました。

選択が記録した `overload_member_types`（受け手から見た各候補の型）から、選ばれた
候補の型を引いて `fun.ty` に入れるようにしました。

**`agent/tail6` の診断は誤りでした。** `E` が `Type::Wildcard` に潰されているのでは
なく、`GenTemporal[F, _]` の `_` は cats-effect の**ソースにそのまま書かれた存在型**
です（`javap -s` の `GenTemporal<F, ?>` が示すとおり、`timeoutTo` に型パラメータは
ありません）。潰れていたのは `E` ではなく `F` の方で、`cats.effect.syntax` の暗黙変換
とも `Select` の型付けとも無関係でした——`implicitly[GenTemporal[F, Throwable]]` が
通るのに `timeoutTo` が通らなかったのは、前者がオーバーロードされていないからです。

#### 3. 暗黙変換自身の implicit 節は、候補の親を読ませていなかった

`fill_implicit_params_in` は探索が空振りしたら `warm_implicit_candidates` を
呼んで retry します（`agent/tail6`）。**暗黙変換の** implicit 節を埋める
`fill_conv_implicits` にはそれがありませんでした。cats の

```scala
implicit def toFlatMapOps[F[_], A](fa: F[A])(implicit F: FlatMap[F]): FlatMap.Ops[F, A]
```

の `FlatMap[F]` を `implicit val asyncF: Async[F]`（trait の**抽象**メンバ）から
埋めるには `Async` の親を読む必要があり、探索は不変借用の下なので自分では読めません。
同じファイルの他の行がたまたま `Async` を温めていれば通り、単独なら通らない——
`agent/tail6` が直したのと同じ形が、変換の側に残っていました
（`slick/basic/BasicBackend.scala:151` の `connectionArbiter.allocateOrdinal.flatMap { … }`）。
`implicit def` なら通り `implicit val` なら通らない、という差もこれです。

#### fixture とテスト

* `tests/fixtures/c3_infer.scala`（+ `expected/`）—— cats を使わずに上の 2 つを
  並べたもの。両モードで `-Xverify:all` の下に走り、real scalac 2.13.16 の
  stdout とも一致します。**修正前の main では 4 件のエラーで落ちます。**
* `tests/fixtures/c3_infer_bad.scala` —— プロトタイプは推論を導くだけで、
  期待型なしに先に推論された値を通す許可ではないこと（`type mismatch`）、
  別の型構築子のための witness は依然として見つからないこと
  （`could not find implicit value of type TC[Box, _]`）。real scalac も同じ
  2 行で同じ 2 件を出すことを別テストで固定しています。
* Coursier キャッシュに cats があるときだけ走る
  `cats_flat_map_then_and_timeout_to_compile` と
  `cats_syntax_conversion_completes_its_own_witness`（＋ scalac 側の対）。
  後者は**単独のコンパイル単位**であることが再現条件です:
  `Async` に触れる行を 1 行足すだけで、直す前でも通ってしまいます。

テストは新ファイル `crates/cli/tests/cats3.rs` の 9 本です。回したのは
`--release` で `cats3` / `cats2` / `catsyntax` / `catsimpl` / `tail6` /
`overloadshadow` / `ambigmap` / `setapply` / `uniteq` / `integral` /
`ordsummon` / `mutcoll` / `ovl2` / `ovl3` / `hkinfer` / `conform` / `e2e`
（e2e 460 本を含めすべて緑）。

#### 残件

* `BasicBackend.scala` は 5 件 → 1 件になりました。残るのは
  `type ExitCase is not a member of Resource$`（`Resource.ExitCase` は
  cats-effect の package object 経由の入れ子で、`import` の残件 (a) と同じ穴）。
* `ConcurrencyControl.scala` は 3 件 → 1 件で、残りは
  `could not find implicit value of type Make[F]`（`Ref.of[F, State[F]](…)`）。
### `ShapedValue.mapToImpl` — `MemberScope#collect`、refined `Context`、混在した `..$`（`agent/shaped`）

slick の `lifted/ShapedValue.scala` に残っていた **5 件を 0 件**にしました
（[`docs/macros.md`](docs/macros.md) §7.16）。`tests/slick_measure.sh` は
**`errors=99 → 94`、`files_with_errors=39 → 38`**（新規エラー 0）。
codegen（`crates/backend/`）は触っていないので `tests/slick_subset.sh` は
省略しています。

5 件は 3 つの根から出ていました。3 番目まで直すと、残り 2 件
（`<error>` 型の穴という quasiquote 診断）は**手前のカスケード**だったので
一緒に消えました。

#### 1. `MemberScope` が `Iterable[Symbol]` だと読めない（`crates/typer/src/pickle_supply.rs`）

`rTag.tpe.decls.collect { case s: TermSymbol => … }` — `mapToImpl` の 1 行目 —
が `value collect is not a member of Scopes.MemberScope` でした。実
scala-reflect の階層は

```text
type MemberScope >: Null <: AnyRef with Scope with MemberScopeApi
trait MemberScopeApi extends ScopeApi
trait ScopeApi extends Iterable[Symbol]
```

で、`MemberScopeApi` も `ScopeApi` も**自分の pickle を持たない**（`javap` の
`Scopes$MemberScopeApi` は `interfaces: 0`）。`PickleSupply::complete` は
「メンバが見つからなければ**ライブラリ祖先**にも聞く」ようになっていましたが、
その祖先リストは**呼び出し時点の親リストのスナップショット**でした。スタブの
親リストは pickle を読むまで空なので、**2 段以上の登りが 1 段目で止まります**:
`MemberScopeApi` の pickle 親 `ScopeApi` までは届き、`complete_on(ScopeApi)` が
その直後に `Iterable[Symbol]` を付けても、**`Iterable` には誰も聞かない**。

`complete_on_ancestors` に置き換え、**1 段ごとに、その段の pickle 親を読んでから
次に進む**ようにしました。順序（親を後ろから、幅優先）は元のままなので、
どの祖先が答えるかは変わりません。変わったのは「その下に届くようになった」ことだけです。

#### 2. 抽象型メンバ越しに読んだメンバが**置換されない**（`crates/typer/src/symbol.rs`）

1 を直すと `collect` は見つかりますが、`decls.toList` が `List[A]`——`Iterable`
自身の型パラメータのまま——を返しました。`SymbolTable::subst_as_seen_from` の
`walk` に `Type::TypeMember` / `Type::TypeParam` の枝が無く、`_ => ty` に
落ちていたためです。**抽象型メンバから読んだメンバは、その上限が宣言している**
ので、上限をたどって置換するようにしました。これで `decls` の要素型が本当に
`Symbol` になり、`s.isVal` / `s.isCaseAccessor` / `s.typeSignature` が通ります。

#### 3. `blackbox.Context { type PrefixType = … }`（`crates/typer/src/macros.rs`）

`mapTo` の定義は

```
error: macro implementation ShapedValue.mapToImpl must take
       scala.reflect.macros.blackbox.Context (or the whitebox one) as its first parameter
```

でした。slick の実装は `c: blackbox.Context { type PrefixType = ShapedValue[?, U] }`
——`c.prefix` に型を付けるための nsc 自身のイディオム——で、`macro_context_kind`
は `Type::Class` しか見ていませんでした。**refinement の親**を候補にし、さらに
**最後の手段として第 1 引数の erased descriptor** を候補にしました。後者は
scala-rs 自身の classfile から読み戻したときに要ります（我々の pickle は
refinement を落とし、`Any` として読み戻る）。第 1 引数が本当に `Any` なら
descriptor は `java.lang.Object` で、どちらの `Context` でもないので**従来どおり
断ります**。

#### 4. `..$xs` と普通の要素の混在（`crates/typer/src/reify.rs`）

`q"f(a, ..$xs, b)"` は「`..$` splice mixed with ordinary arguments is not
reified yet」でした。nsc の `reifyList` と同じにしました: **連続する普通の要素は
`List(...)` 1 つにまとめ、rank-1 の穴はそのまま、左から `++` でつなぐ**
——`List(<a>) ++ xs ++ List(<b>)`。引数の順序が連結の順序で、どの断片も
すでに `List[Tree]` なので、静的型を推測する場所がありません。引数節・パターン
引数節・ブロックの文・**テンプレート本体**（slick が
`SimpleFastPathResultConverter` を組む形）のすべてに効きます。rank 2
（`...$xss`）は従来どおり名指しで断ります。

#### ついでに直した 2 つ

* **展開の中の空 `TypeTree`**（`crates/typer/src/expand.rs`）。`q"val ff = $f"`
  は nsc の quasiquote が型を書かない `TypeTree()` を作ります
  （`mapToImpl` の冒頭 2 行がこれ）。`ValDef` の型の位置に限って
  `TreeKind::Empty` に落とし、型は推論させます。**この位置に限る**のは、
  他の場所には「推論しろ」を表す木が我々の AST に無いからで、そこでは
  従来どおり断ります。
* **`_root_` が term 位置で解決されない**（`crates/typer/src/check.rs`）。
  import path でしか見ていなかったので、`_root_.scala.collection.immutable.List(…)`
  ——マクロが呼び出し側のスコープに巻き込まれないために書く形で、slick の
  `mapToImpl` は 11 箇所書いています——が `not found: value _root_` でした。
  ルートパッケージに解決します。

#### fixture とテスト

* `tests/fixtures/sv_impl.scala` + `tests/fixtures/sv_use.scala` —
  refined `Context` を取るマクロ実装、`decls.collect` によるフィールド列挙、
  そして `..$` の混在を**引数節・ブロック・テンプレート本体**の 3 箇所で
  使い、2 段コンパイルして 4 行印字します。同じ 2 ファイルを実 scalac
  2.13.16 で 2 段コンパイルして実行しても**同じ 4 行**
  （`tests/fixtures/expected/sv_use.txt`）。テンプレート本体のほうは
  **組んだ木を印字した文字列**を展開に載せているので、splice が別の位置に
  落ちたら（コンパイルも実行も通ったまま）行が変わります。
  列挙する型が**ライブラリの**もの（`Deadline` / `BigDecimal`）なのは下の残件 1 のためで、
  `BigDecimal` は case accessor が 0 個＝連結の空端です。
* `tests/fixtures/sv_gaps_bad.scala` — 断る 3 形。うち 2 つ
  （rank-2 の穴、`Context` でない refinement）は**実 scalac も断る**ので
  一致の固定、1 つ（parents が `..$` の `case` class）はこちらの未実装の告白です。

テストは `crates/cli/tests/engine.rs` の末尾に追記した 3 本
（`sv_refined_context_and_mixed_splices_run` /
`sv_refined_context_and_mixed_splices_match_real_scalac` /
`sv_refused_forms_are_named`）です。

#### 残件

1. **scala-rs 自身の `ScalaSignature` は case accessor を記録しません。**
   マクロは `WeakTypeTag` のメンバを**実行時ミラー**越しに読むので、
   scala-rs がコンパイルした case class は `decls` が**空**に見えます
   （`mapTo[R]` を scala-rs 製の `R` に当てると、黙ってフィールド 0 個の
   展開になります）。fixture がライブラリの型を列挙しているのはこのためです。
2. **抽象型メンバに対する型パターンが `instanceof java/lang/Object` になります。**
   `case s: TermSymbol` の `TermSymbol` は universe の抽象型メンバで、
   `erase_ty` はこれを `Object` に落とします（型パラメータは上限に落とすのに）。
   テストが素通りするので、`decls` に `TermSymbol` でないものが混ざる型
   （`scala.io.Codec` など）で `mapToImpl` を展開すると実行時
   `IncompatibleClassChangeError` になります。直すには型パターンの
   `instanceof` を上限の erasure で出す必要があり、codegen に入ります。
3. **scala-rs の classfile から読み戻した macro def は macro def でなくなります。**
   `macro_impl` が pickle に載らないので、別の run で `mapTo` を呼ぶと
   普通のメソッド呼び出しとしてコンパイルされ、実行時に `NoSuchMethodError` に
   なります（診断が出ません）。実 scalac の classfile 経由なら問題ありません。
4. `_root_.scala.List` / `_root_.scala.Vector` は
   `no matching overload for <overload List$ | List$>` になります。パッケージ
   `scala` のスコープに同じ companion が 2 つ入っており、レキシカルな
   `scala.List` は別経路でそれを避けています。`_root_` 以下の他の名前
   （`_root_.scala.Predef` / `_root_.scala.Some` / `_root_.java.lang.*` /
   `_root_.scala.collection.immutable.List`）は通ります。
5. `ShapedValue.scala` の `mapToImpl` は**コンパイル**できるようになりましたが、
   その**展開**（`mapTo` の呼び出し側）には上の 1〜3 と、展開結果の匿名クラス
   （`ClassDef` を expand.rs が組めない）が要ります。slick 本体のコンパイルには
   不要です。
### `Array` は型構築子、無引数メソッドの自己呼び出しも末尾呼び出し（`agent/asttype`）

slick の `ast/Type.scala`（6 件）と `compiler/RewriteJoins.scala`（4 件）を
担当しました。`tests/slick_measure.sh` は **`errors=99 → 86`、
`files_with_errors=39 → 36`**。2 ファイルは **6 件 → 0 件 / 4 件 → 0 件**、
新規エラーは 0（ほかに `jdbc/JdbcBackend.scala` 2 件と
`lifted/AbstractTable.scala` 1 件が巻き添えで消えました）。
codegen も 1 箇所だけ触りました（下記 5）。

#### 1. `Array` の kind は `* -> *`

`class TypedCollectionTypeConstructor[C[_]]` に `Array` を渡す
（`implicit val forArray: TypedCollectionTypeConstructor[Array]`）のは
実 scalac で通ります。こちらは `kinds of the type arguments (Array) do not
conform …` を出していました。`SymbolTable::kind_arity` はクラスシンボルの
`tparams.len()` を読みますが、**`scala.Array` のシンボルには型パラメータが
1 つも入っていません**——ソースの `Array[T]` は `Type::Array` になるので、
`T` を作る場所が無いからです。`class_tparam_count` が `array_sym` にだけ
1 を返すようにしました。

そうすると substitution（`C := Array` を `C[E]` に代入）が
`Class { array_sym, [E] }` を作ります。これは classfile 由来の綴りとして
既に存在していて、`Type::Array` と**同じ型**です。`is_sub_type` の入口で
`array_class_form` により正規化し、`erasure::erase_ty` でも同じ変換を
掛けます（後者が無いと、擬似名 `[java/lang/Object` がそのまま
クラス名として出て `ClassFormatError: Illegal class name`）。

#### 2. ワイルドカードの kind は**型パターンの中でだけ**引き受ける

slick は `case o: TypedCollectionTypeConstructor[?]` と書きます。nsc は
型パターンではワイルドカードにパラメータの kind を与えますが、**普通の型
位置では**同じ `TC[_]` を「proper type の存在型」と読んで
`_$1 takes no type parameters, expected: 1` を出します（実 scalac 2.13.16 で
確認）。そこで `Checker::pattern_tpt` が立っている間だけ kind 検査を
飛ばします。`def anyOf(t: TC[_])` は今も診断されます
（`tests/fixtures/at_bad.scala`）。

#### 3. 無引数メソッドの再帰呼び出しは `Select` であって `Apply` ではない

```scala
@tailrec
def sourceNominalType: NominalType = structuralView match {
  case n: NominalType => n.sourceNominalType
  case _              => this
}
```

`count_tailrec_calls` は `Apply` / `TypeApply` しか再帰呼び出しと見ません。
パラメータリストを 1 つも取らないメソッドの呼び出しには `Apply` が無いので、
`could not optimize @tailrec annotated method: it contains no recursive calls`
になっていました。宣言に `paramss` が無いときだけ、`sym` の一致する
`Select` / `Ident` も呼び出しとして数えます。非末尾位置なら今までどおり
`a recursive call not in tail position` になります（`def loop: Int = loop + 1`
は以前「再帰呼び出しが無い」と誤診されていました）。

#### 4. `Ordering[Null]` は `Ordering.ordered` と `Predef.$conforms` の合わせ技

実 scalac の `-Xprint:typer` は
`Ordering.ordered[Null](scala.Predef.$conforms[Null])` と出します。こちらは
両方に届いていませんでした。

* `ordered` は `object Ordering` が**継承している**
  `trait LowPriorityOrderingImplicits` の宣言です。`warm_pickled_implicits`
  はコンパニオンの**自分自身の**メンバしか pickle から補っていなかったので、
  implicit スコープに入っていませんでした。親を辿るようにしました
  （SLS 7.2 の「コンパニオン**オブジェクト**のメンバ」には継承分も含まれます）。
  優先順位は既存の `is_as_specific_origin`（owner の派生関係）が見るので、
  `Ordering[String]` は今までどおり `Ordering.String` が勝ちます。
* `$conforms[A]: A => A` は `prelude_conform` が `Predef` に足しますが、
  それは `import_members(st, st.predef)` が基底スコープに取り込んだ**後**
  なので、スコープ経由の候補には決してなりません。ほかの候補が全部落ちた
  ときにだけ、**1 引数の関数型**に対して候補として提示します
  （`Implicits::conforms_witness`）。これで
  `implicitly[Ordering[java.util.Date]]` も通ります。

#### 5. Scala classfile の mixin forwarder は pickle の宣言を隠していた

`RewriteJoins.hoistFilterFromBind` の
`foundRefs.filter(_._2._2.isEmpty).map { … }` が
`value _2 is not a member of Any` でした。`foundRefs` は
`immutable.HashMap` で、その classfile には

```
public java.lang.Object filter(scala.Function1);
public scala.collection.IterableOps map(scala.Function1);
```

という `Signature` 属性を持たない**転送メソッド**が入っています。scalac は
Scala の型が型パラメータに触れるメソッドには必ず `Signature` を書くので、
**型パラメータを持つクラスで `Signature` の無いメソッドは転送か bridge**
です。これを `(Any) => Any` として親の宣言の隣に載せると、親（pickle が
ちゃんと書いている `MapOps.filter`）が隠れます。
`classpath::is_erased_scala_forwarder` で読み飛ばし、通常のメンバ探索と
`PickleSupply::complete` の祖先経路に任せます。これだけで slick の
`no matching overload` / `is not a member` が 13 件（担当外の 3 件を含む）
消えました。

`C = Array` で実装した `def sizeOf(c: C[Int])` の bridge
`sizeOf(Object)I` には `checkcast [I` が要ります。`checkcast_internal` に
配列の腕が無く、`-Xverify:all` が `VerifyError: Type 'java/lang/Object' is
not assignable to '[I'` を出したので足しました。

#### テストと fixture

`crates/cli/tests/asttype.rs`（6 本）。fixture は 1 ファイルに全ケースを
まとめた `tests/fixtures/at.scala`（+ `expected/at.txt`）と、拒否側の
`tests/fixtures/at_bad.scala`。`at.scala` は `@tailrec` / `Ordering` /
`<:<` / `immutable.HashMap` を使うので library ABI 専用で、
`--no-scala-library` では**診断されること**をテストしています。修正前の
`main` では 6 本中 3 本が落ちます。

#### Remaining

* `implicitly[String => String]` のように**関数型そのもの**を implicit
  引数として要求する形は、探索に入る前に `reject_unapplied_implicit_clause`
  が「埋まらない implicit 節」として診断してしまい、上記 4 の
  `$conforms` に届きません（`Ordering.ordered` 経由の間接要求は通ります）。
* `immutable.HashMap("a" -> x, "b" -> y)` のように要素の型が違うタプルを
  並べた `apply` は `no matching overload`（LUB を取っていない）。
* `private[this] val` を同じ外側クラスの匿名クラスから読むと
  `IllegalAccessError`。nsc は `O$$secret` に改名して public にします。
  今回の作業中に見つけた**既存の**codegen バグで、このスライスとは独立です。
* `@tailrec` は検査だけで、末尾呼び出しの**変換はしていません**（従来どおり）。
### slick の `JdbcActionComponent` / `DBIOAction` 13 件の 5 つの根（`agent/dbio`）

`tests/slick_measure.sh` は **`errors=99 → 90`、`files_with_errors=39 → 39`**
（消えた 9 件は全部この 2 ファイル、新規エラーは 0）。担当した 2 ファイルは
**13 件 → 4 件**（`JdbcActionComponent.scala` 7 → 1、`DBIOAction.scala` 6 → 3）。
codegen を触ったので `tests/slick_subset.sh` も回して
`subset_files=38 classes=204 verified=204 failed=0`。
5 つの根はどれも**症状より上流**にあり、1 つの根が 3 件ずつ出していました。

**1. 親コンストラクタの名前付き引数**（3 件）。§「親コンストラクタの implicit /
デフォルト引数」に書いたとおり。`extends SimpleJdbcProfileAction[R](_name = …,
statements = …)` の 1 か所から `not found: value _name` /
`not found: value statements` / `no matching overload for constructor …
with arguments (Unit, Unit)` の 3 件が出ていました。

**2. `private[this]` メンバは継承されない**（1 件を単独で、2 件を 5 と合わせて）。
SLS 5.2 のとおり、
`private[this]` は**そのインスタンス**のものなので、無修飾参照の prefix は
「自分のクラスの `this`」以外にありません。slick は

```scala
trait SynchronousDatabaseAction[+R, +S, C, -E] extends DatabaseAction[R, S, E] { self =>
  private[this] def superZip[R2, E2 <: Effect](a: DBIOAction[R2, NoStream, E2]) = super.zip(a)
  override def zip[R2, E2 <: Effect](a: DBIOAction[R2, NoStream, E2]) = a match {
    case a: SynchronousDatabaseAction[?, ?, ?, ?] => new SynchronousDatabaseAction.Fused[(R, R2), NoStream, C, E with E2] {
      override def nonFusedEquivalentAction: DBIOAction[(R, R2), NoStream, E with E2] = superZip(a)
    }
```

と書きます。匿名クラスは**また別の型引数の** `SynchronousDatabaseAction`
（`R = (R, R2)`）なので、`superZip` を「このクラスを通して」読むと
`DBIOAction[((R, R2), R2), NoStream, E with E2 with E2]` になっていました
（`superAsTry` は `Try[Try[R]]`）。`enter_inherited_members` は
`private[this]` を子のスコープに**入れていない**ので、名前解決は最初から
外側のものに当たっており、間違っていたのは `bind_found` の
`subst_as_seen_from` だけです。`superZip` を `private` でなく public に書くと
**実 scalac もこちらと同じ mismatch を出す**ので、これは `private[this]` に
固有の形です（`tests/fixtures/db.scala`）。

これは codegen 側にも 2 つ帰結があります。

- 呼び出しレシーバも**同一性で**外へ歩かないといけません（`gen_ident` の
  `is_private_this` → `load_self_alias_instance`）。`this` は owner に適合して
  しまうので `load_owner_instance` はその場で止まり、匿名クラス自身の
  `r` を読んでいました（自己型別名の `agent/tail3` と同じ罠）。
- 別クラスから届く以上、JVM から見れば `ACC_PRIVATE` への他クラス呼び出しなので
  `IllegalAccessError` になります。コンパニオン越しの読み出しと同じ
  `access_widened` を立てます。

**3. `Either.getOrElse` / `Try.getOrElse` が `(=> Any): Any` だった**（4 と
合わせて 3 件）。
prelude の `add_either` / `add_try` の署名は widening ではなく**結果の消去**
でした。slick の

```scala
val prit = inv.results(0, …)(ctx.session).getOrElse(throw new NoSuchElementException)
val rows = prit.map(value => new Mutator(value, prit.pr, inv))
```

は使うたびに `… is not a member of Any` を出し、1 つの署名から 3 件になって
いました。nsc は `getOrElse[B1 >: B](or: => B1): B1` /
`getOrElse[U >: T](default: => U): U` です
（`crates/typer/src/prelude_dbio.rs`。`prelude_ovl3` が `Option.getOrElse` に
やったのと同じ形）。消去は変わりません（境界の無い型パラメータは `Object`）が、
**呼び出し側に checkcast が要る**ようになったので、gen.rs の
`Either`/`Try` の `getOrElse` 直書き経路でプリミティブの unbox だけでなく
`lazy_cell_from_object` を通します。

**4. `[B >: A]` の下限が呼び出し側の型パラメータを含むと捨てられていた**
（3 と同じ 3 件のもう半分。3 だけ直すと `is not a member of Any` が
`is not a member of Nothing` に変わっただけでした）。
`tparam_lower_bound` はレシーバを通して読んだあとの下限が
**どれか型パラメータを含んでいれば**捨てていました。捨ててよいのは
**owner 自身**の（＝レシーバから読めていない）ものと、**そのメソッド自身**の
（＝この呼び出しが解こうとしている変数）ものだけです。囲むメソッドの型
パラメータはここでは固定型なので、

```scala
def use[T](e: Either[Int, It[T]]) = e.getOrElse(throw new NoSuchElementException).xs
```

は `B1` が引数の `Nothing` に解けて `value xs is not a member of Nothing` に
なっていました（`It[String]` と書くと通る、という形で出ます）。

**5. 型付きパターンは走査対象の型引数を保つ**（2 件）。nsc の
`inferTypedPattern` です。

```scala
case a: SynchronousDatabaseAction[?, ?, ?, ?] => … superZip(a) …
```

の `a` を裸の `SynchronousDatabaseAction[_, _, _, _]` として束縛すると、
走査対象が既に言っていた `R2` / `NoStream` / `E2` が消えるので
`superZip(a: DBIOAction[R2, NoStream, E2])` に渡せません。パターンのクラスの
**走査対象のクラスにおける基底型**からパラメータを解いて、`_` と書かれた
ところだけ埋めます（`pattern_targs_from_scrutinee`）。結果は交差型ではなく
素のクラス型のままなので、消去も codegen も従来どおりです。走査対象が
決めないパラメータ（slick の `C`。`DBIOAction` は取らない）は `_` のままです。

#### テスト

`crates/cli/tests/dbio.rs` の 7 本、fixture は `db` 接頭辞の 3 本です。
**修正前の main では 7 本中 6 本が落ちる**ことを確認しています（残り 1 本は
`--no-scala-library` で `Either` が診断されることを見る否定テストで、main でも
通ります）。

* `tests/fixtures/db.scala`（+ `expected/`）—— 1・2・4・5 を 1 ファイルに。
  標準ライブラリを使わないので**両モード**で走り、実 scalac との dual-run も
  します。
* `tests/fixtures/db_lib.scala`（+ `expected/`）—— 3。`Either` / `Try` は
  library ABI 専用（`prelude::add_either` が `library_abi` の中）なので jar
  モードのみ。`--no-scala-library` では診断されることも固定しています。
* `tests/fixtures/db_bad.scala`（異常系）—— 親コンストラクタの名前付き引数の
  名前が違う形。実 scalac と同じ `unknown parameter name: stmt` を出すこと。
  並べ替えに失敗したときに木を書き換えないのは、シグネチャパス（診断を捨てる）
  で名前付き引数を消費すると本体パスに `no matching overload` しか残らない
  ためです。

#### 残件

担当 2 ファイルに残る 4 件は、それぞれ別の根で、いずれも最小再現を
**実 scalac 2.13.16 が通す**ことまで確認済みです。

* **`<:<` を `Function1` として渡すときの型引数推論**（`DBIOAction.scala:52`、
  `def flatten[R2, S2, E2](implicit ev: R <:< DBIOAction[R2, S2, E2]) = flatMap(ev)`）。
  適合自体は通ります——`val g: R => Act[R2] = ev; flatMap(g)` と書けば
  コンパイルできます。落ちるのは `flatMap[R2](f: R => Act[R2])` の `R2` を
  **引数から解く**ところで、引数が `<:<[R, Act[R2]]` という*クラス*なので
  `Type::Function` のパラメータと突き合わせる前に `Function1` における基底型を
  読んでいません。
* **固定の型パラメータを引数に取るオーバーロード**（`DBIOAction.scala:367`、
  `String.valueOf(value)` の `value: R`）。`arg_score` に
  `if matches!(param, TypeParam(_)) || matches!(arg, TypeParam(_)) { Some(2) }`
  という腕があり、**引数の型が型パラメータなら全ての候補に適合**します。
  最小再現:

  ```scala
  object Q { def h(x: Any) = "any"; def h(x: Boolean) = "bool"; def h(x: Long) = "long" }
  def c[R](v: R) = Q.h(v)          // ambiguous overload for h with arguments (R)
  object O { def f(x: Any) = "any"; def f(x: Int) = "int" }
  def a[R](v: R) = O.f(v)          // type mismatch; found: R  required: Int
  ```

  `R` は解こうとしている変数ではなく**固定型**なので、適合は
  `is_sub_type(R, param)`（＝上のどちらも `Any` の腕だけ）でなければ
  いけません。`arg` 側の腕を落とすのが直しですが、`arg_score` は
  すべてのオーバーロード解決が通る場所なので、このスライスでは触っていません。
* **引数位置の parameterless polymorphic method**（`JdbcActionComponent.scala:725`、
  `session.withPreparedInsertStatement(sql, keyColumns.toArray)(f)`）。
  `ConstArray` の `def toArray[R >: T : ClassTag]: Array[R]` は期待型が無いと
  `instantiate_parameterless` を通らず（「期待型があるときだけ」という注釈の
  とおり）`Array[R]` のまま残り、`(String, Array[String])` と
  `(String, Array[Int])` の両方に適合して ambiguous になります。nsc は
  `R` を下限 `T` に解いてから解決します。
* **`cats.effect.IO(fa)`**（`DBIOAction.scala:237`）。`IO$` の `apply` が
  見つからない、という形ですが、**単体では再現しません**——同じ式を
  `LiftF[cats.effect.IO, R](cats.effect.IO.fromFuture(cats.effect.IO(fa)))`
  として（兄弟の `from[F[_], R]` オーバーロード込みで）書いても通ります。
  slick 全体を一度に読ませたときだけ出るので、pickle からのメンバ供給の
  順序依存が疑われます。

`SQLiteProfile.scala:183` の
`no matching overload for (Iterable[U], RowsPerStatement)…` は 1 のカスケード
だと思って調べましたが、名前付き引数の修正後も残っています（別の根）。

### 拒否する側の規則が出していた偽陽性 11 件（`agent/reject`）

`tests/slick_measure.sh` は **`errors=65 → 54`、`files_with_errors=34 → 29`**。
消えた 11 件はちょうど担当分（分散検査 7 件、self-type 適合 4 件）で、
**新規エラーは 0**（`grep '^error' ` の集合差が 11 行の削除だけ）。
codegen（`crates/backend/`）は触っていないので `tests/slick_subset.sh` は省略。

分散検査（SLS 4.5）と self-type 適合検査は、どちらも「拒否する側」の規則です。
slick は実 scalac 2.13.16 で完全に通るので 11 件は全部偽陽性でした。
ただし**症状の数え方はブリーフと違い**、`covariant` は 2 件ではなく 4 件
（`BasicProfile.scala` の `head` / `headOption` で 2 件、`SqlProfile.scala` の
`overrideStatements` で `R` と `S` の 2 件）、`contravariant` 3 件と合わせて
**分散は 7 件**、self-type は 4 件（`JdbcBackend.scala` に 2 件：名前付きクラスと
`new JdbcDatabaseDef[F](…){}` の匿名クラス）で、合計 11 件です。

そして根は**症状の数だけありませんでした**。分散 7 件は 1 根、self-type 4 件も
1 根で、都合 2 根です。「同じ症状が 1 根とは限らない」の逆側もあります。

**1. 型引数がどの位置に立つかを、クラスからしか読んでいなかった**（分散 7 件）。

`check_variance_ty` は `Type::Class { sym, args }` のときだけ `sym` の型引数の
分散（`+` / `-`）を見て位置を反転させ、`Type::Applied { ctor, args }`
——つまり**クラスでない型構築子の適用**——では引数を一律に**不変位置**として
扱っていました。nsc は頭が何であれ `sym.typeParams` を読みます。頭が
**抽象型メンバ**でも**高階の型パラメータ**でも、宣言された分散はクラスと同じに
効きます。slick の

```scala
trait BasicAction[+R, +S <: NoStream, -E <: Effect] extends DatabaseAction[R, S, E] {
  type ResultAction[+R, +S <: NoStream, -E <: Effect] <: BasicAction[R, S, E]
}
trait BasicStreamingAction[+R, +T, -E <: Effect] extends BasicAction[R, Streaming[T], E] {
  def head: ResultAction[T, NoStream, E]
}
```

の `ResultAction[T, NoStream, E]` は、第 1 引数が `+` なので共変 `T` は共変位置、
第 3 引数が `-` なので反変 `E` は反変位置で、どちらも合法です。不変扱いすると
`head` 1 本から `covariant type T …` と `contravariant type E …` の 2 件が出て、
`headOption` と `SqlAction.overrideStatements` を合わせて 7 件になります。
`Type::TypeMember` / `Type::TypeParam` / 部分適用された `Type::Class` の
`tparams` から分散を読む `tparam_variances` を足し、`Applied` の枝で使うように
しました（`crates/typer/src/check.rs`）。

**緩めすぎていないこと**は拒否側で確かめてあります。注釈の無い `type M[X]` は
不変のまま（`covariant type A occurs in invariant position`）、`type N[-X]` は
位置を**反転**させる（`… occurs in contravariant position`）、高階の型パラメータ
`F[X]` / `G[-Y]` も同じ、の 4 形は実 scalac と同じ 4 件で落ちます
（`tests/fixtures/rej_bad.scala`）。

**2. self-type を、型引数を捨てた素のクラスと、宣言元の語彙のまま比べていた**
（self-type 4 件）。

`check_self_conformance` は検査対象を `Type::Class { sym, args: vec![] }`
——**型引数を落とした**形——で作り、親の `self_type` を**そのまま**相手にして
いました。self type は宣言したトレイトの語彙で書かれているので、ここで読むには
2 つの読み替えが要ります。

- 親の型パラメータ。`this: Database[F] =>` の `F` は `BasicDatabaseDef` の `F`
  であって、`JdbcDatabaseDef` の `F` ではありません。
- 囲っているケーキが後から別名にした**抽象型メンバ**。`Database` は
  `BasicBackend` の `type Database[F[_]] >: Null <: BasicDatabaseDef[F]` で、
  `JdbcBackend` の中では `type Database[F[_]] = JdbcDatabaseDef[F]` です。

```scala
trait BasicBackend {
  type Database[F[_]] >: Null <: BasicDatabaseDef[F]
  trait BasicDatabaseDef[F[_]] extends AnyDatabaseDef { this: Database[F] => … }
}
trait JdbcBackend extends RelationalBackend {
  type Database[F[_]] = JdbcDatabaseDef[F]
  abstract class JdbcDatabaseDef[F[_]](…) extends BasicDatabaseDef[F] { … }
}
```

読み替えないと、比べているのは「素の `JdbcDatabaseDef`」と
「`BasicBackend.Database[F]`」で、**これに適合できるものは存在しません**。だから
`JdbcBackend` / `HeapBackend` / `DistributedBackend` の 3 クラスと
`new JdbcDatabaseDef[F](…){}` の匿名クラスが揃って落ちていました。
`self_type_of_class` で型引数を入れ、`subst_as_seen_from` で親の型パラメータを、
`expand_type_members` で囲っているクラスの別名を解決します。
`expand_type_members` は `enclosing_classes` を内側から辿るので、匿名クラスも
`object JdbcBackend` 経由で同じ別名に届きます。

こちらも拒否側は生きています。ケーキの別名が `Real[F]` のとき
`class Fake[F[_]] extends DbDef[F]` は落ちますし（実 scalac も同じ）、
`trait P[A] { self: Q[A] => }` に対する `class Miss[A] extends P[A]` も落ちます。
修正前の main はここで `Real[F]` **自身**まで落としていた（7 件）ので、
拒否側のテストは「落ちること」だけでなく**件数**も見ています。

**3. ついでに見つかった 3 つ目**：`subst_as_seen_from` はクラスの**親**は辿るのに
**self type** を辿っていませんでした。self type は `this` がメンバを継ぐ
もう 1 つの経路なので、そこから来たメンバの型は self type の語彙のままでした。

```scala
trait Q[A] { def q: A }
trait P[A] { self: Q[A] => def p: A = q }   // type mismatch; found: A  required: A
```

`Q` の `A` と `P` の `A` は表示が同じで別物です。`walk` のクラス枝で、親を辿った
あとに（クラスの型引数で具体化した）self type も辿るようにしました
（`crates/typer/src/symbol.rs`）。slick の 54 件は 1 件も動きませんが、
`rej_ok.scala` の 5 番目のケースがこれです。

fixture は `tests/fixtures/rej_ok.scala`（受理側 5 ケース・dual-run、期待出力
`expected/rej_ok.txt`）と `tests/fixtures/rej_bad.scala`（拒否側 6 ケース）の
2 本で、テストは `crates/cli/tests/reject.rs`。`rej_bad.scala` は実 scalac に
1 回で全部は出させられません——`illegal inheritance` は typer、分散検査は
refchecks で、nsc は typer でエラーが出ると refchecks に進まないからです。
分散 4 件は同じ 4 トレイトだけのファイルで別に確認しました。
### 差分プローブ（第12ラウンド・`agent/probe12`）——実行して初めて分かった 10 件

slick の計測は**型検査までしか見えません**（`classes=0`）。実行時のサイレント
誤コンパイルは差分プローブでしか出ません。今回は slick / cats が実際に使う形を
**14 本の小さなプログラム**に書き直し、実 scalac 2.13.16 と scala-rs の両方で
コンパイルして `java -Xverify:all` で走らせ、**stdout をバイト一致で比較**
しました。14 本中 **10 本が食い違い**、根は互いに独立でした。

計測は `tests/slick_measure.sh` が分岐元（`2a9db27`）・修正後ともに
**`files=184 errors=65 files_with_errors=34 classes=0`**（型検査の数字は動かない
——直したのは実行時の振る舞いと、slick が踏まない型検査の穴）。codegen
（`crates/backend/`）を触ったので `tests/slick_subset.sh` を
`SLICK_SEED_LOG` 付きで 1 周し、`subset_files=38 classes=204 verified=204
failed=0`（悪化なし）。

14 本は全部 `tests/conform/` に昇格しました（`query_ast` / `group_report` /
`show_typeclass` / `byname_lazy` / `copy_unapply` / `exception_forms` /
`number_mix` / `interp_forms` / `action_monad` / `hk_typeclass` /
`mutable_loops` / `either_validate` / `mixin_profile` / `expr_interp`）。
プローブが覆っていない最小形は `override_val_apply.scala` にまとめてあります。

#### 実行時に壊れていたもの（コンパイルは通っていた）

**1. `override val` / 抽象 `val` をフィールドとして読んでいた。**

```scala
class P { val pre: String = "a"; class T { def q = pre }; def mk = new T }
class A extends P { override val pre = "b" }
abstract class Q { val pre: String; def show = pre + "!" }
class B extends Q { val pre = "c" }
println(new A().mk.q)          // scalac: b     scala-rs: a
println((new A(): P).pre)      // scalac: b     scala-rs: a
println(new B().show)          // scalac: c!    scala-rs: null!
```

scala-rs はソースクラスの `val` をフィールドとして public に出し、読み出しも
**宣言したクラスの `getfield`** でした。`override val` を書いたサブクラスは
自分のスロットを持つので上書きが見えず、抽象 `val` は誰も書かないスロットを
読んで `null` になります。nsc は `private` でないメンバ値をすべて**アクセサ
経由**で読み、仮想ディスパッチが実際に値を持つクラスに着地します。
`gen.rs` の `reads_via_accessor` がその条件（`PARAM` でも `STATIC` でも
`PRIVATE` でもなく、`jvm_name` が空で、**この run でコンパイルしている
クラス**が owner）を判定します。最後の条件が要ります: 私有ランタイムの
`Tuple2._1` はフィールドで、アクセサを持ちません（外すと `fixtures_predef` /
`fixtures_dynamic` が `NoSuchMethodError`）。

**2. 内側クラスから外側クラスの「メソッド」を呼ぶと `this` をキャストしていた。**

```scala
class Outer(val tag: String) {
  def deco(s: String) = "[" + s + "]"
  class Inner(val name: String) { def q(c: String) = tag + name + deco(c) }
}
new Outer("o").make("m").q("c")
// scalac: om[c]
// scala-rs: ClassCastException: Main$Outer$Inner cannot be cast to Main$Outer
```

外側の**フィールド**（`tag`）は既に `$outer` を辿っていましたが、`gen_receiver`
の裸 `Ident` 呼び出しの枝が `load_this` ＋ `checkcast` で済ませていました。
`this` が owner に適合せず、かつ `$outer` の鎖が owner に届くときだけ
`load_owner_instance` を使うようにしています（届かないときは従来どおり）。
trait の内側クラスでも、抽象メソッドでも同じ症状でした。

**3. 自作 `unapplySeq` が `Option[Seq[A]]` を返すと `List` にキャストしていた。**

```scala
object Words { def unapplySeq(s: String): Option[Seq[String]] =
  if (s.isEmpty) None else Some(s.split(" ").toSeq) }
"hello" match { case Words(one) => one; case _ => "" }
// scalac: hello
// scala-rs: ClassCastException: ArraySeq$ofRef cannot be cast to List
```

cons walk は `checkcast scala/collection/immutable/List` で始まります。これが
正しいのは `Option[List[A]]` のときだけで、自然な綴り `Option[Seq[A]]`
（`toSeq` は `ArraySeq$ofRef`）では落ちます。erasure が `Option[Seq[A]]` を
裸の `Option` に潰した後では判定できないので、**型引数が残っているうちに**
typer が `SymbolTable::seq_extractor_payload` に記録し、backend は `List`
以外を scalac と同じ `SeqFactory$UnapplySeqWrapper$`（配列なら
`Array$UnapplySeqWrapper$`）で読みます。

**4. `xs.view.filter(p)` が `SeqView` を名乗っていた。**

```scala
println(List(1, 2, 3, 4).view.filter(_ > 2).map(_ * 10).toList)
// scalac: List(30, 40)
// scala-rs: ClassCastException: View$Filter cannot be cast to SeqView
```

2.13 の宣言は `trait SeqView[+A] extends SeqOps[A, View, View[A]] with
View[A]` で、`C` は**自分自身ではなく `View[A]`** です。`javap
scala.collection.SeqView` に現れる override は `view` / `map` / `appended` /
`prepended` / `reverse` / `take` / `drop` / `takeRight` / `dropRight` /
`tapEach` / `concat` / `appendedAll` / `prependedAll` / `sorted` だけで、
`filter` はありません。`check.rs` の `returns_receiver_collection` が受け手に
作り直していたので静的型が `SeqView[A]` になり、結果に付く `checkcast` が
実物の `scala.collection.View$Filter` で落ちていました。
`prelude_viewc.rs` が `SeqView` に `filter` / `filterNot` / `takeWhile` /
`dropWhile` / `collect` / `flatMap` を **`View[A]` を返す**と宣言し、
その名前に対してだけ作り直しを止めます。ついでに `View.map` の descriptor も
直しました（`IterableOps.map: CC[B]` の消去なので
`(Lscala/Function1;)Ljava/lang/Object;`。`View[A]` のまま呼びに行って
`NoSuchMethodError` になっていましたが、`View` 型の値がこれまで作れなかった
ので誰も踏んでいませんでした）。

**5. `Array(Array(1, 2), Array(3, 4))` が `Object[]` を作っていた。**

`gen_java_class_of` に `Type::Array` の枝が無く `java/lang/Object` に落ちて
いたので、`Array.apply` に `ClassTag[Object]` が渡り、結果の
`checkcast [[I` が落ちます。配列のクラスリテラル定数は内部名ではなく
**descriptor**（`[I` / `[[I` / `[Ljava/lang/String;`）で綴ります。

**6. 文字列補間の `Unit` 引数が評価されていなかった。**

```scala
println(s"unit ${println("side")}")
// scalac: side \n unit ()
// scala-rs: unit ()      ← side が出ない
```

`gen_sb_append` が `Unit` の値を見て `ldc "()"` だけ出し、式そのものを
出していませんでした。`gen_stat` で文として出してから定数を積みます
（`gen_stat` は呼び出しが実際に積むものを捨てる作法を既に持っています）。

**7. by-name 引数をローカル `def` / ローカル `lazy val` に渡すと二重に force。**

```scala
def viaLocal[A](body: => A): A = { def go(): A = body; go() }
def once[A](body: => A): () => A = { lazy val v = { println("forced"); body }; () => v }
// scala-rs: ClassCastException: java.lang.Integer cannot be cast to scala.Function0
```

lambda-lift は捕捉した by-name シンボル**そのもの**を持ち上げた
メソッドのパラメータにします（だから `v$1(Function0, LazyRef)` の中では
正しく force されます）。ところが呼び出し側の引数も同じシンボルの `Ident`
なので、erasure の `erase_ident` が `Flags::BYNAME` を見て問答無用で
`.apply()` を付けていました。**値**を渡された callee がもう一度 force して
落ちます。木の型がまだ `ByName(_)` で、かつ期待型が thunk のスロット
（`=> T` か 0 引数 `Function`）のときは force しません。

#### 実 scalac が受理する形を拒否していたもの

**8. `Either` の for 内包表記。**

```scala
type V[A] = Either[List[String], A]
for { h <- req("host"); ps <- req("port"); p <- int(ps) } yield Cfg(h, p)
// scala-rs: type mismatch; found: Either[List[String], Cfg]
//           required: Either[List[String], String]
```

`prelude_either` の `Either.flatMap` が `(B => Either[A, B]): Either[A, B]` と
**単相**でした。nsc は `def flatMap[A1 >: A, B1](f: B => Either[A1, B1]):
Either[A1, B1]` です。続きが受け手自身の `B` に押し戻されるので、右の型が
段ごとに変わる for 内包表記が全部型エラーになっていました。

**9. implicit パラメータ節を持つ `implicit class`。**

```scala
implicit class ShowOps[A](a: A)(implicit s: Show[A]) { def shown = s.show(a) }
// scala-rs: no implicit: could not find implicit value of type Show[A]
//           （エラー位置は「クラス宣言そのもの」）
```

`implicit_class_conversions` が `vparamss.first()` しか見ておらず、2 節目
以降を捨てていました。すると `new ShowOps[A](a)` が抽象な `A` に対して
`Show[A]` を召喚することになります。nsc の脱糖どおり残りの節も変換メソッド
に持たせ、そのまま `new` に渡します。cats 風の syntax クラス
（`implicit class MonadOps[F[_], A](fa: F[A])(implicit m: Monad[F])`）は
全部これで落ちていました。

**10. `f(x)()`——メソッドが返した `() => A` をその場で適用する。**

```scala
def mk(n: Int): () => Int = () => n
println(mk(3)())   // scala-rs: not enough arguments: expected 1, found 0
```

空の引数節が `mk` の 2 番目のパラメータ節として読まれていました。適用済みの
`Apply` の型が `Function` なら、その節は `Function0.apply` であって callee の
パラメータ節ではありません（erasure の `sym_denotes_callee` と同じ判定）。

#### 直していない差分（次のスライスの入力）

* **`xs.flatten`**。

  ```scala
  val opts: List[Option[Int]] = List(Some(1), None, Some(3))
  println(opts.flatten)      // scalac: List(1, 3)
  // scala-rs: value sum is not a member of ((Option[Int]) => IterableOnce[B])List[B]
  ```

  pickle の `IterableOps.flatten[B](implicit toIterableOnce: A =>
  IterableOnce[B]): CC[B]` の implicit 節が**適用されずに残り**、Method 型が
  そのまま結果になっています（表示されている型がその生の Method 型）。
  実 scalac は `Predef.$conforms` を渡します
  （`invokevirtual List.flatten:(Lscala/Function1;)Ljava/lang/Object;`）。
  埋めるには「`<:<[A, A]` を `A => IterableOnce[B]` に適合させながら
  `B` を解く」——**結果型から逆に型変数を解く implicit 探索**が要ります。
  `List[List[Int]]` でも同じです。

* **型ラムダ `({ type L[X] = Reader[R, X] })#L`**。cats が
  kind-projector 無しで使う形です。

  ```scala
  implicit def readerMonad[R]: Monad[({ type L[X] = Reader[R, X] })#L] = …
  // scala-rs: type mismatch; found: $anon$1  required: Functor[<none>.L]
  //           type mismatch; found: Any  required: R
  ```

  精製型の中の型メンバへの射影が型構築子として解決できず、`<none>.L` に
  なります。型エイリアス `type IntReader[X] = Reader[Int, X]` を経由した
  `Functor[IntReader]` への代入も通りません。

* **`def using(...)` という名前のメソッド**（受理の差）。実 scalac 2.13.16 は
  `using(r)(f)` を `Main.Res does not take parameters` で**拒否**します
  （`using` は引数リストのソフトキーワードで、`(using r)(f)` と読まれる）。
  scala-rs は普通の識別子として受理します。誤コンパイルではなく、
  scala-rs の方が寛容という差です。今回は `tests/conform/exception_forms.scala`
  で名前を `withRes` に変えてあります。

#### 回したテスト

`cargo test --workspace --release`（修正一式の後、conform 追加前）でグリーン。
その後 `--test conform` を単独で回して **77 passed**（従来 62 ＋ 今回 15）。
`--test e2e` は 460 passed。`cargo fmt --all` 済み、`cargo clippy` の新規警告 0。

### slick のオーバーロード解決 26 件のうち 9 件、6 つの根（`agent/ovl4`）

`tests/slick_measure.sh` は **`errors=65 → 55`、`files_with_errors=34 → 31`**
（新規エラー 0）。担当した `no matching overload` 21 件・`ambiguous overload`
5 件の塊は **26 件 → 17 件**。おまけに `value infer is not a member of AnyRef`
（`Comprehension.scala:85`）も 6 番目の根で一緒に消えました。型検査だけを
触ったので `tests/slick_subset.sh` は省略（`crates/backend/` は未変更）。

26 件を 1 件ずつ最小再現した結果は、**「同じ症状は 1 つの根」も「同一ファイルは
1 つの根」も成り立たない**という既存の観察のとおりでした。6 つの根はどれも
無関係で、逆に**別ファイルの 2 件が同じ根**というものが 3 組ありました。

**1. 固定された型パラメータの「引数」は、その上限のものでしかない。**
`arg_score` に

```rust
if matches!(param, Type::TypeParam(_)) || matches!(arg, Type::TypeParam(_)) {
    return Some(2);
}
```

という腕があり、**引数の型が裸の型パラメータだと全候補に適合**していました。
`String.valueOf(value)`（`DBIOAction.scala:367`、`case class SuccessAction[+R](value: R)`）
は `valueOf(Object)` から `valueOf(char)` まで全部が適合して `ambiguous overload`
になります。nsc は `valueOf(Object)` を選びます（`javap -c` で確認。Java の
`Object` 引数には `Any` も適合する——2.13 の `ObjectTpeJava`）。

`param` 側の腕は正しい（`def f[T](x: T)` の `T` は候補自身の変数で、採点中は
`undet_tvars` に入っていない）ので残し、`arg` 側は**上限で採り直す**形に
置き換えました。`is_sub_type` はすでに上限へ広げているので、この腕が効くのは
「パラメータが候補自身の未解決変数を含む」場合だけです——
`Comprehension[+Fetch <: Option[Node]]` の `fetch: Fetch` を
`ConstArrayBuilder.++` の 3 つのオーバーロード（`ConstArray[T]` /
`IterableOnce[T]` / `Option[T]`）に渡す `Comprehension.scala:22` がそれで、
上限 `Option[Node]` を見て初めて `Option[T]` だけが残ります。

ブリーフには「この腕を落とすのが直し」とありましたが、**落とすだけでは
`Comprehension.scala` に 2 件の新規エラーが出ます**。上限で採り直すところまでが
一組です。

**2. 複合型（`A with B`）のパラメータ・引数**（`JdbcTypesComponent.scala:50`、
`MemoryProfile.scala:62`、`MemoryProfile.scala:63`）。slick は

```scala
type BaseColumnType[T] = ScalaType[T] with BaseTypedType[T]
def assertNonNullType[A](t: BaseColumnType[A]): Unit
```

と書き、`assertNonNullType(implicitly[BaseColumnType[U]])` と呼びます。
`class_ctor_matches_typeparam_args`（「パラメータの型引数が型パラメータなら
適合」）も `unify_one` も `Type::Refined` を見ていなかったので、適合が 0、
仮に適合させても `A` が解けませんでした。両方に

* 複合どうしは**要素ごと**に、
* 複合の引数は**どれか 1 つの要素**が適合すればよい（`ScalaType[U] with
  BaseTypedType[U]` を `ColumnType[U'] = ScalaType[U']` のパラメータへ渡す
  `new MappedColumnType(...)`）

の 2 本を足しました。

**3. 単相の呼び先も、引数にパラメータ型を期待型として渡す。**
`proto_arg_type` は、呼び先が型パラメータを持たないときだけ
「関数の形をしたパラメータ」しかプロトタイプに出していませんでした。nsc は
**すべての**引数をパラメータ型に対して型付けします。この差は、
**引数自身の型パラメータが推論で決まる**とき——`RefId[E <: AnyRef]` は不変なので

```scala
val errors = mutable.Set.empty[RefId[Dumpable]]
errors += RefId(n1)            // n1: Node
```

は期待型 `RefId[Dumpable]` があって初めて `E = Dumpable` になります——に
そのまま出ます（`VerifyTypes.scala:38,41`）。プロトタイプは既存の呼び出し側の
規律どおり**ヒント**で、引数がそれに適合しなければ期待型なしで型付けし直します。
`e2e` 460 本・継ぎ目リストを含めて回帰はありませんでした。

**4. 固定された型パラメータは推論でも上限を通る。** 1 の適合が通ったあと、
`mapOrNone[A](o: Option[A])(f: A => A)` に `fetch: Fetch` を渡すと
`A` が解けず `Any` に落ちて `_.infer(scope, …)` が
`value infer is not a member of Any` になっていました
（`Comprehension.scala:85`。これは 26 件の外の別エラー）。`unify_one` は
シンボル表を持たない自由関数なので、`unify_tparam_all` の側で
「何も推論できなかったら上限で採り直す」ようにしました。

**5. コンストラクタは継承されない。** `resolve_overload` は
`Type::Overload` を受け取っても候補表を `overload_alternatives`
（＝最後は `lookup_member`）で**組み直します**。`lookup_member` は親をたどるので、
`java.util.Properties` の `<init>` の候補に `Hashtable` の
`(Int, Float)` と `(Map[_ <: K, _ <: V])` が混ざり、`new Properties(null)` が
`Properties(Properties)` と `Hashtable(Map)` の間で `ambiguous overload`
になっていました（`GlobalConfig.scala:68`）。`pick_ctor_at` は
`owner == class_id` で濾していたのに、この経路がそれを落としていたわけです。

ただし**「owner が一致するものだけ」に濾すと壊れます**。同じ classfile が
2 通りの経路でシンボル表に入ることがあり、`java.io.OutputStreamWriter` が
まさにそれで、片方のコピーの `OutputStream` しか `PrintStream` の親では
ありませんでした。落とすのは**真の上位クラスが owner のもの**だけ
（`owner_is_proper_subclass`）にしてあります。

**6. `-cp` のスタブは何の部分型でもない。** 5 の濾過で
`new OutputStreamWriter(System.out)` が落ちたので調べると、`Writer(Object lock)`
——継承された、nsc なら候補にすらならないコンストラクタ——が拾われて
成功に見えていただけでした。本当の理由は
`find_or_stub_java_class` が記述子から作るスタブが `parents = [AnyRef]` だけを
持つことで、`java/io/PrintStream` の classfile をまだ誰も読んでいない時点では
`OutputStream` に適合しません。**同じ式が、同じファイルの後のほうでは通ります**
（先に他の経路が読むので）。`arg_score` は `&self` なので classfile を読めない
——`Option.option2Iterable` のときと同じ形です——ので、`new` の側でも
「一度失敗したら引数のクラスを読んでからもう一度」（`warm_java_args`）を
入れました。

#### この 6 つで消えたもの

`Comprehension.scala:22,85`、`ExpandSums.scala:27`、`VerifyTypes.scala:38,41`、
`DBIOAction.scala:367`、`JdbcTypesComponent.scala:50`、
`MemoryProfile.scala:62,63`、`GlobalConfig.scala:68` の 10 件。
`ExpandSums.scala:27`（`oldDiscCandidates ++ (tree match { … })`）は
`Set[_ <: AnyRef]` という lub が原因だと見ていましたが、実際には 3 で消えました
——**症状の見立ては当てにならない**という例をもう 1 つ増やしたことになります。

#### テスト

`crates/cli/tests/ovl4.rs`（5 本）と fixture `tests/fixtures/ovl4.scala` /
`ovl4_bad.scala`。6 つの根を**1 ファイルにまとめて**あります（実 scalac 1 回が
1.8 秒なので、fixture は増やさず広くする）。`ovl4.scala` は修正前の `main` では
**両モードとも 7 件のエラー**になります。dual-run は
実 scalac 2.13.16 / `--scala-library` / `--no-scala-library` の 3 通りで
出力一致を確認済み。`ovl4_bad.scala` は 1 の裏側——`def bad[T](x: T) =
takesList(x)`——で、実 scalac も `type mismatch; found: T required: List[Int]`
と拒否します。

回した範囲: `--test ovl4 --test overloadshadow --test ambigmap --test setapply
--test uniteq --test integral --test ordsummon --test mutcoll --test conform
--test ovl2 --test ovl3 --test mismatch14 --test seqfn --test arrconv
--test buildfrom --test dbio --test e2e`（全部グリーン）。

#### 診断の後退（1 件、既知）

1 の前は、候補が 1 つだけの呼び出しで裸の型パラメータを渡すと、適合して
から `adapt` が `type mismatch; found: T required: List[Int]`（nsc と同文）を
出していました。いまは適合の段階で落ちるので
`no matching overload for (List[Int])Int with arguments (T)` になります。
`agent/ovl3` が書いたとおり **`no matching overload` は候補 1 つでも出る**
という既知の粗さで、「候補が 1 つなら引数を `adapt` して本当の不一致を出す」
のが直しですが、既存テストの期待文字列に広く触るのでこのスライスでは
やっていません。

#### 残り 17 件（最小再現と見立て）

* **`Array` は `Seq` の仲間として見られていない**（`ResultConverter.scala:58`
  の `TupleSupport.buildTuple(a)`、`JdbcTypesComponent.scala:526` の
  `Map(...) ++ anArrayOfTuples`）。`def f(x: Seq[Any]) = 1; f(a: Array[Any])`
  はもちろん、`def v(a: Array[Any]): Seq[Any] = a` すら通りません
  （実 scalac は通す）。prelude にあるのは `wrapIntArray` と
  `wrapBooleanArray` だけで、しかも `seqfn_view.rs::array_seq_wrap` は
  `Boolean` にしか答えません。直しは `wrapRefArray[T](Array[T]):
  ArraySeq$ofRef[T]` を足して `array_seq_wrap` を要素型で分岐させ、
  `adapt` と `arg_score` の両方から引くこと。`genericWrapArray` は使えません
  ——実 ABI の記述子が `(Ljava/lang/Object;)…` で、こちらの backend は
  `Array[T]` を `[Ljava/lang/Object;` に erase するためです。
* **`Set() ++ xs`**（`JdbcModelBuilder.scala:280`）。`Set()` が `Set[Nothing]`
  に固まってしまい、`++` の候補が `(IterableOnce[A])Set[A]`（`A = Nothing`）
  しかないので何も渡せません（`Set() ++ List("a")` でも再現）。nsc は
  `SetOps.concat(IterableOnce[A])` と `IterableOps.concat[B >: A]` の 2 本を
  持ち、後者で `B` を解きます。prelude/pickle の継ぎ目なので慎重に。
* **`ConstArray.toArray`**（`JdbcActionComponent.scala:725`）。ブリーフの
  見立てどおりでした。`def toArray[R >: T : ClassTag]: Array[R]` が
  期待型なしでは `Array[R]` のまま残り、`(String, Array[String])` と
  `(String, Array[Int])` の両方に適合します。最小再現は
  `s.withPreparedInsertStatement(sql, ks.toArray)(f)` そのままで取れます。
  nsc は `R` を呼び出し全体の未決変数として扱い、下限 `T = String` で解きます
  ——`undet_tvars` に引数側の未決変数を載せる話で、1 の腕を落としても
  変わりません（引数の型は `Array[R]` であって裸の `R` ではないので）。
* **`FixRowNumberOrdering.scala:19` / `ExpandSums.scala:245`**。
  `fix(ch, Some(c))`（`c` は `case (c: Comprehension[?], _)` で束縛された
  存在型）と `ProductNode(ConstArray(disc, map)).infer()`。どちらも
  素朴に書き直した最小再現は**実 scalac も拒否した**ので、パターンで束縛された
  skolem の変性がそのまま効いています。未解明。
* **カスケード 3 件**: `Node.scala:534`（`:@` エクストラクタが無い）、
  `CreateAggregates.scala:100`（`.toMap` の implicit 引数が入らず結果が
  メソッド型のまま）、`ExpandTables.scala:25`（`collection.Map` に
  `contains` / `apply` が生えていない）。いずれも根は同じファイルの
  1 行上の別診断で、オーバーロードの問題ではありません。
* 残り（`QueryCompiler.scala:220`、`SQLiteProfile.scala:183`、
  `JdbcModelBuilder.scala:93,159`、`DistributedProfile.scala:76`、
  `DBIOAction.scala:52,237`）は、素朴な縮小では再現しませんでした。
  `DBIOAction.scala:237` の `cats.effect.IO(fa)` が slick 全体でしか出ない
  という `agent/dbio` の観察はそのままです。
### 匿名クラスから外側のクラスを触る 4 つの根（`agent/outer`）

匿名クラス／ローカルクラス／ラムダの本体から**外側のクラスのもの**を触る形で、
main に残っていた 4 件。`java -Xverify:all` で落ちる 2 件と、
`IllegalAccessError` になる 1 件と、静かに**別のメソッドを呼んでいた** 1 件です。
どれも実 scalac 2.13.16（`/tmp/scala-2.13.16/bin/scalac`）と `javap -p -c` で
確かめてから直しました。テストは `crates/cli/tests/outer.rs` に追記、fixture は
`tests/fixtures/outer1.scala`（1 ファイルに全ケース）。

**1. `<init>` の中の `$outer` 読み出しは検証を通らない**（`VerifyError`）。

```scala
class Outer(val n: Int) {
  def mk(): Base = new Base("tag" + n) { def describe = tag + "/" + n }
}
```

匿名クラスの**親コンストラクタ引数**が外側インスタンスを読みます。scala-rs は
`$outer` の代入を super 呼び出しの**後**に置き、引数の中では
`aload_0; getfield $outer` を出していました。JVMS §4.10.1.9 の `getfield` は
オペランドが `class(FieldClass)` に適合することを要求するので、
`uninitializedThis` に対する `getfield` は**フィールドを代入済みかどうかに
関係なく**通りません（`putfield` だけが、しかも「現在のクラスが宣言した
フィールド」に限って許されます）。実 scalac の `javap` はこうです。

```
public C$Outer$$anon$1(C$Outer);
   0: aload_1
   1: ifnonnull 6
   4: aconst_null
   5: athrow
   6: aload_0
   7: aload_1
   8: putfield  $outer            ← super 呼び出しより前
  ...
  26: aload_1                     ← 引数は <init> の引数から読む
  27: invokevirtual C$Outer.n:()I
  31: invokespecial C$Base."<init>"
```

つまり nsc は **(a) `$outer` を super 呼び出しの前に代入し、(b) それでも
super 引数の中では `$outer` ではなく `<init>` の引数（local 1）を読む**という
2 つを両方やっています。(b) が必須で、(a) は「親の `<init>` から仮想呼び出しで
戻ってきたメソッドが `$outer` を見られる」ためのものです。両方合わせました
（`EmitCtx::presuper_outer` と `start_outer_walk`。`$outer` の連鎖を歩く 3 か所
——`load_owner_instance` / `load_self_alias_instance` / `load_qualified_this`
——の**最初の 1 ホップ**だけが差し替わります）。

**2. `private` メンバはクラスファイルを跨いだ時点で改名が要る**
（`IllegalAccessError`）。ブリーフの見立てどおりで、ただし**根は「改名して
いない」ことではなく、そもそも「跨いだ」と判定できていなかった**ことでした。

Scala の `private` は**語彙的**で、匿名クラス・ローカルクラス・ラムダ本体・
コンパニオンはどれも所有者のスコープの中にあるので `private[this]` まで名前で
呼べます。JVM の `ACC_PRIVATE` はクラスファイル単位なので、これらは全部
実行時に `IllegalAccessError` です。scala-rs には既に `access_widened`
（`ACC_PRIVATE` を落とす）がありましたが、それを立てているのは check.rs の
**2 か所だけ**——コンパニオン越しの読み出し（`note_companion_access`）と、
`private[this]` を無修飾で読む 1 経路（`agent/dbio` が足したもの）——でした。
そのため次はどれも素通りしていました。

| 形 | main の結果 |
|---|---|
| `C1.this.a`（**修飾された** `this`） | `IllegalAccessError` |
| `private val` / `private def` を匿名クラスから | `IllegalAccessError` |
| `private` メンバをラムダ本体から | `IllegalAccessError` |

3 つめは scala-rs 固有です。nsc はラムダを `invokedynamic` ＋**同じクラスの
static メソッド**（`$anonfun$viaLambda$1`）に落とすので跨ぎませんが、scala-rs は
匿名クラスに落とすので跨ぎます。実 scalac は

```scala
class C { private[this] val a = 1; def viaLambda = List(0).map(_ => a).head }
```

に対して `a` を `private final int a` のまま残します（`javap -p`）。

そして nsc は跨いだメンバを**公開するだけでなく改名**します
（`Symbol.makeNotPrivate` → `nme.expandedName`）。
「所有者の完全名を `$` 区切りにしたもの」＋ `$$` ＋ 名前で、実測は次のとおりです。

| 書いたもの | scalac 2.13.16 が出す名前 |
|---|---|
| `object A { class Outer { private[this] val secret } }` | `public final int A$Outer$$secret` |
| `private val pUsed` | `private final int B$Outer$$pUsed` ＋ `public int B$Outer$$pUsed()` |
| `private var w` | `private int H$C$$w` ＋ `H$C$$w()` / `H$C$$w_$eq()` |
| `object O1 { private[this] val c }` | `public static final int D$O1$$c` |
| `trait T1 { private[this] val b }` | `public abstract int D$T1$$b()` |
| `package pkgj.sub; class R { private[this] val a }` | `public final int pkgj$sub$R$$a` |
| **どこからも跨いで読まれない** `private[this] val ptUnused` | `private final int ptUnused`（**改名なし**） |

改名は飾りではありません。`private[this]` は継承されないので、

```scala
class P { private[this] def y = 2; def mk() = new AnyRef { override def toString = "" + y }.toString }
class Q extends P { def y = 9 }          // 合法
```

は実 scalac だと `new Q().mk()` が **`2`**。ところが「公開するが改名しない」と
`P.y` が public になって `Q.y` がそれを**オーバーライド**し、main の scala-rs は
**`9`** を出していました（`access_widened` が既に効いていた `private[this]` で
起きていた、静かな誤コンパイル）。

そこで nsc の `superaccessors` と同じ位置——**pickler の前**、
scala-rs では `mark_anon_captures` の直後——に
`crates/typer/src/expand_private.rs` を 1 本足しました。ユニットを
「コードが実際に載るクラス」を持って歩き、`private` メンバの参照が所有者以外の
クラスから来ていたらシンボル名とツリー上の名前を一緒に改名して
`access_widened` を立てます（`_$eq` は展開の外に残すので `Outer$$w_$eq`）。
`private[pkg]` は `Flags::PRIVATE` を持ったままなので `private_within` で除外
します（これは public に出るのが正しく、改名したら他ファイルから引けなくなる）。
pickler より前なので**pickle にも改名後の名前が入り**、nsc と同じく
classfile と食い違いません。分離コンパイル（`sep1.scala` を出してから
`-cp` で `sep2.scala`）も実 scalac と同じ出力になることを確認しています。
scala-rs はラムダをクラスに落とす分だけ nsc より**広く**改名しますが、
宣言も参照も同時に改名するので閉じており、`private` メンバは他ファイルから
名指しできないので外に漏れません。

`tp3`（trait の `private def` をコンパニオンが読む）の既存テストは
「`secret` という名前のまま public abstract で残る」を固定していましたが、
実 scalac の `javap -p` は `public default int Widened$$secret()` です。
**テストの期待の方が nsc と違っていた**ので、名前を実 scalac に合わせ、
「ソース名の方は出さない」を追加しました。

**3. 外側の `var` への代入がレシーバを歩いていなかった**（`VerifyError`）。

```scala
class C3 { private[this] var d = 4
  def mk(): Any = new AnyRef { override def toString = { d = d + 1; "" + d } } }
```

読み出し側（`gen_ident`）は `$outer` を歩いていましたが、`gen_assign` の
`Ident` 枝だけが `load_this` のままで、`putfield` のレシーバに匿名クラス自身を
積んでいました（`Type 'D$C3$$anon$4' is not assignable to 'D$C3'`）。読み出しと
同じ `load_owner_instance` / `load_self_alias_instance` に揃えました。

**4. ラムダの中で作る匿名クラスの `$outer`**（`VerifyError`）。

```scala
class C4 { private[this] val e = 5
  def mk(): Any = { val f = () => new AnyRef { override def toString = "" + e }; f() } }
```

ラムダクラスに `$outer` を持たせるかを決める `collect_free` は、`New` の枝で
「そのクラスが捕まえるローカル」は数えていましたが、「**そのクラスの `<init>`
が外側インスタンスを要求すること**」を数えていませんでした。匿名クラスの本体は
`ClassDef` なのでこの walk は降りていかず、ラムダは `this` を使っていないように
見え、`$outer` を持たないまま `load_this` が `aload_0`（＝ラムダ自身）を積んで
いました。`New` の対象が `outer_field_class` を持つならラムダも外側インスタンス
が要る、を足しました。

計測は前後とも同じ数字でした。`tests/slick_measure.sh` は
**`files=184 errors=65 files_with_errors=34 classes=0` → 同じ**、
codegen（`crates/backend/`）を触ったので `SLICK_SEED_LOG` 付きの
`tests/slick_subset.sh` も **`subset_files=38 classes=204 verified=204 failed=0`
→ 同じ**。基準値は README の数字を信じずに、このワークツリーで `crates/*/src`
を main に戻したバイナリで測り直しています。型検査の数字が動かないのは当然で、
`expand_private_names` は `has_errors` が false のときにしか走らず
（`crates/driver/src/lib.rs`）、`classes=0` の間は backend まで届きません。

**残件**（この形で見つけたが直していないもの）:

* nsc は `private[this] val` に**アクセサを作らない**（フィールドだけ）。
  scala-rs は今も `Outer$$secret()` を出します。改名済みなので衝突はしません
  が、余分なメソッドです。
* nsc は `private val` の**フィールドは private のまま**改名し、アクセサだけを
  public にします。scala-rs はフィールドごと public にします。
* `object O { private[this] val c }` を nsc は `static final` フィールドにします
  が、scala-rs はインスタンスフィールドのままです（改名は一致）。
* scala-rs が**実 scalac の出した classfile を `-cp` で読む**方向は、この形とは
  無関係に壊れています。`private` メンバを 1 つも持たないクラスでも
  `VerifyError: Operand stack underflow` になったので、この節の変更の前から
  ある別件です（scala-rs 同士の分離コンパイルは通ります）。

### slick の `TableQuery` / `Compiled` 5 件、3 つの根（`agent/tq`）

slick の `lifted/TableQuery.scala`・`lifted/Compiled.scala`・
`relational/RelationalProfile.scala` に残っていた 5 件。**5 件で 3 根**でした。
どれも診断文の言っている場所が根ではありません。実 scalac 2.13.16
（`/tmp/scala-2.13.16/bin/scalac`）で最小再現が通ることを確かめてから直しています。
テストは `crates/cli/tests/tq.rs`、fixture は `tests/fixtures/tq.scala`
（1 ファイルに全ケース）と `tests/fixtures/tq_bad.scala`。

slick: `errors=44 files_with_errors=26` → **`errors=38 files_with_errors=25`**
（担当 5 件＋巻き添えで直った 1 件、新規エラーなし。`tests/slick_measure.sh`）。

**1. 抽象型構築子の適用がワイルドカードの下に入らない**（診断は「境界不適合」）。

```scala
trait Rep[T]
trait QueryBase[T] extends Rep[T]
trait Query[+E, U, C[_]] extends QueryBase[C[U]]

def t6[BU, C[_]](x: Rep[C[BU]]): Rep[_] = x   // ← ここが落ちる
```

診断は

```
type arguments [Query[B, BU, C],C[BU],BU] do not conform to method apply's
type parameter bounds [T <: Rep[_],TU,EU]
```

で、`StreamingExecutable.apply[T <: Rep[_], TU, EU]` の**境界検査**に見えます。
実際には境界検査は正しく動いていて、`Query[B, BU, C]` を親へ辿った
`Rep[C[BU]]` を `Rep[_]` と比べるところ、`Rep` は不変なので引数どうしの
`C[BU] <: _` に落ち、そこで false になっていました。`C` が `Seq` のような
**具体的な**型構築子なら通る（`t4` は通る）ので、症状は「高階の境界」に見えます。

根は `is_sub_type` の**アーム順**です。`(Type::Applied { ctor, args }, other)`
は右辺を見ずに全部を捕まえるアームで、`Type::Wildcard` のアームより**前**に
あります。その中は `ctor` が `TypeMember` のときだけ `bound_hi` を辿り、
型**パラメータ**（`C[_]`）のときは `false` を返していました。
`(Applied, Wildcard)` と `(Applied, BoundedWildcard)` を Applied のアームの
前に置いて解決しています（`crates/typer/src/symbol.rs`）。

**2. `TypeApply` の被呼び出し側が値位置で型付けされていた**。マクロは無関係。

```scala
class TQ[E](cons: Int => E)
object TQ {
  def apply[E](cons: Int => E): TQ[E] = new TQ[E](cons)
  def apply[E]: TQ[E] = null            // ← 引数なしの側
}
TQ.apply[String](f)   // error: value apply is not a member of TQ[String]
```

ブリーフは「slick が `TableQuery.apply[E]` を**マクロ**として定義しているので、
マクロ定義の解決の問題かもしれない」としていましたが、**マクロは関係ありません**。
上のとおりマクロを 1 つも含まない同じ形で再現します。`§7.13（オーバーロード解決）`
という以前の診断のほうが近く、正確には**オーバーロード集合が畳まれるタイミング**です。

SLS 6.26.3 により、値位置のオーバーロード参照は**引数を取らない候補だけ**を残します。
`Apply` は被呼び出し側を `Type::Method` の期待型で型付けしてこの畳み込みを止めて
いますが、`TypeApply` は自分の `fun` を `Type::NoType`（＝値位置）で型付けして
いました。`TableQuery.apply[E]` は `TypeApply` なので、外側の `Apply` が引数を
見る前に無引数の側へ畳まれ、その結果 `TableQuery[E]` に `(cons)` を適用する形に
なって「value apply is not a member of TableQuery[E]」になっていました。
明示型引数でも絞れません（両候補とも型引数 1 個）。nsc は `typedTypeApply` の
`fun` を FUNmode で型付けするので畳みません。

`TypeApply` が自分自身 `Apply` の被呼び出し側であるとき（＝ `pt` が `Type::Method`）
だけ、その期待型を `fun` へ渡すようにしました。集合が残れば `Apply` 側の
`pending_targs`（既存）が明示型引数を適用します。

ただし `Type::Method` の期待型は**無引数メソッドの自動適用も**止めてしまいます。
fs2 の `Stream.fromIterator[F]` は無引数の多相メソッドで、返り値の側に
引数を取る `apply` があります（部分適用ビルダ）。素直に渡すだけだと
`fromIterator[IO](it, chunkSize = 1)` が無引数メソッドへの適用になり、
`slick/cats/Database.scala` に**新しいエラーが 1 件生えました**。
期待型はオーバーロード集合のためだけのものなので、結果が `Overload` でなければ
値位置と同じ自動適用を掛け直しています（`crates/typer/src/check.rs`）。

**3. 出力型が呼び出し側で未決のとき、implicit 候補**自身**の型パラメータが解けない**。

```scala
def apply[V, C <: Compiled[V]](raw: V)(implicit compilable: Compilable[V, C], …): C
implicit def function1IsCompilable[A, B <: Rep[_], P, U](implicit
  aShape: Shape[ColumnsShapeLevel, A, P, A],
  pShape: Shape[ColumnsShapeLevel, P, P, _],
  bExe: Executable[B, U]): Compilable[A => B, CompiledFunction[A => B, A, P, B, U]]
```

`Compiled { (p: Rep[P]) => … }` の `C` は引数からは決まりません（結果型と
implicit 節にしか現れない）。`C` を未決のまま `Compilable[Rep[P] => Query[T, U, Seq], ?C]`
を探すところまでは既存の `undet_solution` が行きます。候補の結果型と単一化すると
`A`・`B` は決まり、`?C := CompiledFunction[A => B, A, P, B, U]` と束縛されますが、
候補自身の `P` と `U` は**求める型の側に対応するものが無い**ので未決のまま残ります。
`implicit_solve` は結果型だけから完全解を要求するので候補を落とし、slick 自身の
`@implicitNotFound` が

```
Computation of type (Rep[P]) => Query[T, U, Seq] cannot be compiled (as type C)
```

として出ていました。**これは scala-rs のメッセージではありません**（ブリーフの
指摘どおり）。`type mismatch; found: C required: CompiledFunction[…]` は
その後始末で、2 件は 1 根です。

`P` と `U` を言えるのは候補**自身の** implicit 節だけです
（`aShape: Shape[…, A, P, A]` が `P`、`bExe: Executable[B, U]` が `U`）。
nsc は implicit 引数を型付けする間 `Context.undetparams` にこれらを入れて
そこで解きます。`implicit_fit_open`（`crates/typer/src/implicits.rs`）を足し、
通常の解決に失敗した候補についてだけ、残った自分の型パラメータを
`search_implicit_undet` の未決集合として自分の implicit 節から解くようにしました。
**フォールバック**であること、**求める型が候補の型パラメータを 1 つ以上は
決めていること**を条件にしています（全部未決の候補はスコープ中の全 implicit に
当たってしまうため）。

**巻き添えで直った 1 件**: `value apply is not a member of
SqlStreamingAction[Vector[Unit], Unit, Effect]`（根 2 と同じ）。

**私が確かめた範囲**: `--test tq conform buildfrom buildfrom2 asttype hkinfer
overloadshadow ambigmap setapply uniteq integral ordsummon mutcoll ovl2 ovl3 ovl4`
と `cargo test --workspace --release`。`crates/backend/` は触っていないので
`tests/slick_subset.sh` は省略しています。

**残件**:

* 根 1 で直るのは `Rep[C[BU]] <: Rep[_]` の向きだけです。`C[BU]` を**左辺**に
  置いた他の照合（`C[BU] <: Iterable[_]` のような、構築子の境界を辿る必要が
  あるもの）は今も `Type::Applied` のアームで `false` になります。slick には
  現れませんでした。
* 根 3 の完成は候補の implicit 節を**書かれた順**に 1 回ずつ走らせるだけで、
  後の節が前の節の解を狭める形（相互再帰的な解決）には対応していません。
* 同じ 3 ファイルに残っているのは別件です。`TableQuery.scala:16` の
  `cons(new BaseTag { base => … })`（匿名クラスの**自己名** `base` が本体から
  見えない）、`RelationalProfile.scala:72:71` の
  `could not find implicit value of type TypedType[Boolean]`、
  同 `82:61` の `missing parameter type for expanded function`。
  根 3 の 2 件は 72 行目の**同じ行**に出ていましたが、72:71 の
  `TypedType[Boolean]` とは無関係で、そちらを直さないまま消えました。
### 適用されないまま式の型に残る implicit 引数節、4 つの根（`agent/implclause`）

式の型が `(引数)結果` の形で出る——`value isEmpty is not a member of
(<:<[TermSymbol, (K, V)])Map[K, V]` のような——症状を最小再現から追いました。
**同じ症状の裏に 4 つの独立した根**があり、うち 3 つは「implicit 節を埋める
機械」ではなく**その手前**（型引数の解き方、修飾子の型付け、候補の可否判定）に
ありました。slick は `errors=44 files_with_errors=26` → `errors=40
files_with_errors=24`（新規エラーはゼロ）。テストは
`crates/cli/tests/implclause.rs`、fixture は `tests/fixtures/implclause.scala`
（1 ファイルに全ケース）と `tests/fixtures/implclause_bad.scala`。

**1. 関数パラメータの結果を、パラメータ側のクラスに揃えてから型引数を解く。**

```scala
def h(v: Vector[(String, Map[Long, Int])]) = v.iterator.flatMap(_._2).toMap
```

`unify_one` は**クラスのシンボルを見ずに型引数を位置で zip** します。
`flatMap[B](f: A => IterableOnce[B])` の `IterableOnce[B]` にラムダの本体の
`Map[Long, Int]` を突き合わせると `[B]` と `[Long, Int]` を zip して
`B = Long` になり、`flatMap` は `Iterator[Long]` を返していました。だから
続く `toMap[K, V](implicit ev: A <:< (K, V))` は `TermSymbol <:< (K, V)` の
witness を探して見つからず、メソッド型が式の型として残ります。

`unify_tparam_all` は既に `align_to_param_class` で**引数全体**をパラメータの
クラスに揃えていましたが、パラメータが関数型のときは何もしていませんでした。
`align_arg_to_param` を足し、関数パラメータの**結果**も揃えます。パラメータは
反変なので、揃えるのは結果だけです（引数側を基底クラスに読み替えると、
リテラルが実際に書いたことを捨ててしまいます）。

**2. セレクションの修飾子は、呼び出し引数の中にあっても implicit 節を埋める。**

```scala
def f(q: Qy[Int]) = SV(q.pack.to[Seq], "x")   // pack[R](implicit s: Sh[E, R]): Qy[R]
```

`adapt_implicit_apply` には「引数がオーバーロードの決まる前に型付けされて
いる間は節に触らない」という退避（`typing_call_args`）があります。これは
**引数の木そのもの**についての話なのに、その中の**修飾子**にまで効いていました。
`pack` は `to` を引くより前に値でなければならず、nsc も修飾子は EXPRmode で
型付けして adapt します。

ただし `type_select` が修飾子を型付けする**間ずっと**フラグを落とすのは
行きすぎでした。同じフラグは修飾子の中の**タグ要求**の答え方も決めていて、
一律に落とすと `tests/fixtures/ex_impl.scala` の `weakTypeOf[ExBox[E]]` が
`E` のタグを拾い、`ExBox[ExRow]` が `ExRow` と印字されます（`--test engine` が
そこで落ちました）。そこで、修飾子を**通常どおり**型付けしたうえで、
implicit 節が生き残っていたとき（`implicit_only_result`）だけ
`adapt_implicit_apply` をもう一度、フラグを落として当てます。あわせて、
それでも埋まらなかった節はここで `reject_unapplied_implicit_clause` に渡します
——`adapt` の backstop は修飾子を見ません（期待型が無いので）。
`value to is not a member of (Sh[Int, R])Qy[R]` ではなく
`could not find implicit value of type Sh[Int, R]` になります。

**3. `A => B` を継承したクラスは、型引数を解く材料にもなる。**

```scala
abstract class Conv[-A, +B] extends (A => B)
def flatten[R2](implicit ev: R <:< Act[R2]) = flatMap(ev)   // flatMap[R2](f: R => Act[R2])
```

`function_view`（引数を「継承している関数型」として読み直す口）は、親が
`Function1` の**クラス**として記録されている場合しか見ていませんでした。
`extends (A => B)` と書かれた親は `Type::Function` として記録されます
（`<:<` もこの形で入ります）。適合自体は通る（`val g: R => Act[R2] = ev` は
書ける）のに、呼び先の `R2` を引数から解くところだけが空振りして
`no matching overload` になっていました。`Type::Function` の親をそのまま
view として返します。

**4. `ClassTag` を implicit 引数に持つ導出規則は「使えない候補」ではない。**

```scala
implicit def forColl[C[X] <: Iterable[X]](implicit cbf: Factory[Any, C[Any]],
                                          tag: ClassTag[C[Any]]): Coll[C]
implicitly[Coll[Seq]]   // ← 見つからなかった
```

`implicit_fit_at` は導出規則の可否を「自分の implicit 引数が
`search_implicit_at` で**見つかる**か」で判定します。`ClassTag` /
`TypeTag` は nsc と同じく**探すのではなく作る**ものなので、この判定では常に
不合格でした。`implicitly[ClassTag[Seq[Any]]]` を直接書けば通るのに、
規則の中に置くと通らない、という食い違いです。`fill_implicit_params` が持って
いた fallback を判定側にも与え（`built_not_found`）、木を組む
`implicit_tree` の再帰にも同じ fallback を足しました。view 系の fallback
（`identity_view` / `conversion_view` など）は**入れていません**——あれらは
自分で探索を回すので、関数型のパラメータを一律に「埋まる」ことにしてしまいます。
これが slick の `Query.to[Seq]`（`TypedCollectionTypeConstructor[Seq]`）です。

**ブリーフの診断のうち、実測で否定できたもの**:

* `xs.flatten` の implicit 節（`agent/probe12` の指摘）は **main で既に直って
  います**（`cbf207b` のマージ）。`List(Some(1), None).flatten.sum` は今の main
  で通ります。
* `implicitly[String => String]` が `reject_unapplied_implicit_clause` に
  潰される（`agent/dbio` の指摘）も**再現しません**。今は通ります。
* `Predef.$conforms` の消息は現状で正しく、実 jar の
  `javap scala.Predef$` も `public <A> scala.Function1<A, A> $conforms()` です
  （`crates/typer/src/prelude_conform.rs` の記述どおり）。実装が
  `<:<.refl` を返す以上、`A <:< B` の探索が `<:<.refl` に落ちるのは nsc と
  同じ挙動で、スコープ構築の順序の問題ではありませんでした。

**残件**（最小再現あり、直していない）:

* `Array[T]` は**引数位置で `IterableOnce[T]` に変換されません**。
  `Map() ++ arr`（slick `jdbc/JdbcTypesComponent.scala:526`）も
  `def f[B](x: IterableOnce[B]); f(arr)` も `no matching overload` です。
  `arr.toSeq` と書けば通ります。scala-rs は `Array` を `ArrayOps` の
  メンバ供給で支えていて、`Predef.wrapRefArray` / `genericWrapArray` に
  相当する**一般の view が無い**のが根です。埋めるには codegen 側で
  ラップを挟む必要があり、この節の変更（型検査のみ）とは別のスライスです。

#### テストと計測

`cargo test --workspace --release` は 118 バイナリすべてグリーン
（`implclause` が 1 本増えて 117 → 118）。継ぎ目リスト
（`overloadshadow` / `ambigmap` / `setapply` / `uniteq` / `integral` /
`ordsummon` / `mutcoll` / `conform` / `e2e`）と `mismatch14` / `hkinfer` /
`dbio` / `buildfrom` / `buildfrom2` / `arrconv` / `seqfn` / `cats2` / `cats3` /
`catsimpl` / `reject` / `ovl3` / `ovl4` / `proj` / `asttype` / `engine` も
個別に回しています。`crates/backend/` は触っていないので
`tests/slick_subset.sh` は省略。slick は `tests/slick_measure.sh` で
着手時 `errors=44 files_with_errors=26` → 完了時 `errors=40
files_with_errors=24`、消えたのは
`compiler/CreateAggregates.scala:99,100` / `dbio/DBIOAction.scala:52` /
`lifted/Query.scala:191` の 4 件で、**増えたものはありません**。
### 「見つからない／見えない」13 件の 7 つの根（`agent/implfind`）

slick に残っていた **implicit が見つからない 4 件**と**メンバにアクセスできない
2 件**、それに同じ系統の単発 7 件を最小再現したところ、根は 7 つでした。
診断の言葉と根が一致したものは 1 つもありません。全件、実 scalac 2.13.16
（`/tmp/scala-2.13.16/bin/scalac`）が受理することを確かめてから直しています。
テストは `crates/cli/tests/implfind.rs`、fixture は
`tests/fixtures/implfind.scala`（1 ファイルに全ケース）と
`tests/fixtures/implfind_bad.scala`（緩めたアクセス規則の裏側）。

slick: `errors=44 files_with_errors=26` → **`errors=31 files_with_errors=22`**
（13 件減、新規ゼロ）。

**1. 適用済みの抽象型メンバが自分の上限に適合しない。**
「implicit が見つからない」の 3 件（`TypedType[Boolean]`、`JdbcType[U]`、
`JdbcType[U] with BaseTypedType[U]`）の根は implicit 探索ではなく**部分型判定**
でした。

```scala
trait TT[T]
trait C { type CT[T] <: TT[T] }
def d[U](implicit ev: C#CT[U]): TT[U] = ev   // これが型不一致だった
```

`is_sub_type` の `Applied` 対 その他の規則は、抽象型メンバの上限
（`bound_hi`）を**メンバ自身のパラメータのまま**相手と比べていました。
`CT[U]` の上限は `TT[T]` ではなく `TT[U]` です。適用した引数で置換していな
かったので、`CT[U] <: TT[U]` が常に偽になり、文脈境界が入れた evidence が
**自分の境界を満たさない**という状態になっていました。
`crates/typer/src/symbol.rs`。

**2. 文脈境界の evidence の型が self type 越しに展開されない。**

```scala
trait JComp extends Comp { self: JProf =>
  def base[U : BCT](u: U) = implicitly[BCT[U]]   // 候補は evidence だけ、なのに不一致
}
trait JProf extends Prof with JComp { type BCT[T] = JT[T] with BB[T] }
```

`[U : BCT]` は境界を**裸の名前**で書くので、`tree_to_type` の「型構築子に
引数を適用する」経路（`expand_type_members` を最後に呼ぶ）を通りません。
本体の `implicitly[BCT[U]]` は self type 越しに `JT[U] with BB[U]` になるのに、
evidence だけが抽象側の `BCT[U]` のまま残り、唯一の候補が要求に合いませんでした。
`Checker::expand_bound_evidence`（`class_bound_evidence` と `def` 側の両方）。

**3. コンパニオン `object` の `protected` メンバ。**
nsc の `Contexts.isAccessible` は `accessWithin(ab) || accessWithinLinked(ab)`
（`ab = sym.owner`）を先に見ます。**所有者の中か、そのコンパニオンの中**に
いれば、`protected` でもサブクラス規則は要りません。scala-rs は
`protected_subclass_ok` しか見ていなかったので、slick の

```scala
trait ResultConverterCompiler[R, W, U] { … ResultConverterCompiler.logger … }
object ResultConverterCompiler { protected lazy val logger = … }
```

が `value logger cannot be accessed` になっていました。

**4. 入れ子の `private[pkg] object` / `class`。**
`namer_enter_tmpl` は `ClassDef` / `ModuleDef` の `private_within` を**記録して
いませんでした**（`val` / `def` / `type` は記録していた）。修飾付き private が
素の private として扱われ、slick の
`private[jdbc] object GetUpdateValue`（`object GetResult` の中）が
パッケージ内の `SQLActionBuilder` から見えませんでした。
ブリーフの「コンパニオンの private を外から触っている」という見立ては誤りで、
これはコンパニオンとは無関係な**修飾付き private の取りこぼし**です。

**5. 匿名クラスの self alias。** `parse_new` が `new T { base => … }` の
`base` を捨てていました（`self_name: None` 固定）。slick `TableQuery` の
`not found: value base` はこれだけです。

```scala
val baseTable = cons(new BaseTag { base =>
  def taggedAs(path: Node) = cons(new RefTag(path) {
    def taggedAs(path: Node) = base.taggedAs(path)   // ← not found: value base
  })
})
```

**6. 構成子パターンの関数位置では、非 stable な `def` は候補にならない。**
nsc の `Context.lookupSymbol` は `typingConstructorPattern` のとき
`sym.isMethod && !sym.isStable` を候補から外します。slick の `Node` は

```scala
final def :@ (newType: Type): Self = …          // Node のメソッド
import slick.ast.TypeUtil.*                     // object TypeUtil { object :@ { def unapply … } }
val from2 :@ CollectionType(_, el) = from.infer(scope, typeChildren): @unchecked
```

という形で、**継承したメソッド `:@` が import した抽出子 `object :@` を隠して**
`not found: extractor :@` になっていました。`case` の中では起きず、`val` の
パターン定義でだけ出ていたのは、`case (LiteralNode(lv) :@ (lt: TypedType[?]), …)`
の方は `Node` を継承していないクラスで書かれていたからです。
`SymbolTable::lookup_extractor` と `Checker::ctor_pattern_fun`。
これに伴い `Node.scala` の `<notype>` カスケード 2 件も消えました。

**7. Java の `Object` 戻り値は `Any` ではなく `AnyRef`。**
nsc の `objToAny` は ClassfileParser のパラメータのループでしか呼ばれません。
戻り値は `AnyRef` のままなので `eq` / `ne` / `synchronized` が使えます。
scala-rs は `java/lang/Object` を一律 `Type::Any` にしていたので、
typesafe-config の `ConfigValue.unwrapped(): Object` に対する
`if(cv.unwrapped eq null)`（slick `GlobalConfig`）が
`value eq is not a member of Any` でした。

nsc に忠実に「引数だけ `Any`、それ以外（戻り値・フィールド・型引数）は
`AnyRef`」まで広げる版も試しましたが、`Hashtable<Object, Object>` のような
**型引数**まで書き換わり、slick の `HeapBackend` で
`IndexedSeq[Any] <: Int => Any` が落ちる**新規エラーを 1 件出しました**
（差し引きゼロ）。slick で得るものが無い広げ方だったので、
**戻り値の最上位だけ**に留めています（`java_result_obj`）。
残っているのは「型引数の `Object`」の側です。

**8.（副産物）`scala.collection.Map` にメンバが無い。**
`prelude_hier` が作る「リンク用」トレイトはメンバを持たず、
`get` / `contains` / `getOrElse` / `apply` は `immutable.Map` /
`mutable.Map` の側にしかありませんでした。2.13 ではどれも
`scala.collection.MapOps` の宣言なので、抽象側に置くのが正しいです。
slick `ExpandTables` が `collection.Map` で受けた引数に対して

```
value contains is not a member of Map[TableIdentitySymbol, (TermSymbol, Node)]
no matching overload for ((K, V)*)Map[K, V] with arguments (TableIdentitySymbol)
value replace is not a member of B
```

の 3 件を出していたのは 1 根です（`expansions(tsym)` が**コンパニオンの
`Map.apply`** を拾って `exp: B` になり、`exp.replace` が `B` のメンバを
探していた）。`crates/typer/src/prelude_implfind.rs`。

**9.（副産物）pickle 由来の入れ子クラスが型位置でコンパニオンに解決される。**
`object Ref { trait Make[F[_]] }` を classfile から読むと 2 つに割れます:
pickle が `Make` の**モジュールアクセサ**を `Ref$` に載せ、trait の方は
`Ref$Make` という名前だけからは「どちらの `Ref`」の入れ子か決まらないので
`find_or_stub_java_class` が **trait `Ref`** の下に置きます。
`lookup_qualified_type` は最初に当たった owner で打ち切っていたため、
`Ref.Make[F]` が object の方に解決されて
`Make does not take type parameters` になっていました。owner をまたいで
**class を object より優先**するように変更（`fs2.Stream.ToPull[F, O]` も同様）。

**残件**（最小再現つき）:

* `no implicit: could not find implicit value of type Make[F]`
  （slick `basic/ConcurrencyControl.scala:202`、`Ref.of[F, State[F]](…)`）。
  9 で `Ref.Make[F]` は型として通るようになり、**スコープにある
  `implicit mk: Ref.Make[F]` を `implicitly` で見つける**ところまでは直りました。
  残るのは 2 つです。(a) `Ref.of[F, Int](0)` の**暗黙引数の挿入**が起きない
  （明示的に `Ref.of[F, Int](0)(mk)` と書けば通る）。(b) `Ref.Make` の
  コンパニオンが継承する

  ```scala
  implicit def concurrentInstance[F[_]](implicit F: GenConcurrent[F, ?]): Make[F]
  ```

  が implicit スコープから使えない（存在型 `GenConcurrent[F, ?]` に
  `Concurrent[F] = GenConcurrent[F, Throwable]` を合わせる必要がある）。
  最小再現:

  ```scala
  import cats.effect.kernel.{Concurrent, Ref}
  def d[F[_]](implicit mk: Ref.Make[F]): F[Ref[F, Int]] = Ref.of[F, Int](0)   // (a)
  def k[F[_]](implicit F: Concurrent[F]): Ref.Make[F] = implicitly[Ref.Make[F]] // (b)
  ```

* `type ExitCase is not a member of Resource$`
  （slick `basic/BasicBackend.scala:421`）。**単体では再現しません**。

  ```scala
  import cats.effect.{Async, Ref, Resource}
  import cats.effect.kernel.Outcome
  import cats.syntax.all.*
  import cats.effect.syntax.all.*
  object C { def ec(e: Resource.ExitCase): String = e.toString }   // これは通る
  ```

  BasicBackend.scala と同じ import を並べても通るので、slick を丸ごと
  1 回で通したときにだけ壊れます（`Resource$` のメンバ補完の順序を疑って
  いますが未検証）。ブリーフの「パッケージオブジェクトの `val` 越しの
  ネストしたクラス」という仮説は、少なくとも**単体では成り立ちません**
  （`cats.effect.Resource` はまさにその形で、単体では通ります）。

**ブリーフの記述で誤っていたところ**:

* 「アクセス 2 件は companion object の private/protected メンバを companion
  class 以外から触っている形」——`GetResult.GetUpdateValue` の方は
  コンパニオンとは無関係で、`private[jdbc]` という**修飾付き private** を
  scala-rs が記録していなかっただけです。prefix の計算は正しく、
  `SQLActionBuilder` から触るのも正しい形です。
* 「`TypedType[Boolean]` は slick の `TypedType` コンパニオンか profile の
  ケーキの中の implicit が候補」——候補は `api` 越しの
  `booleanColumnType: BaseColumnType[Boolean]` で、スコープには**入って
  いました**。落ちていたのは `BaseColumnType[Boolean] <: TypedType[Boolean]`
  の判定（根 1）です。
* 「`value eq is not a member of <notype>` は型が計算されていない印」——
  正しいですが、上流は `eq` でも `Any` でもなく、同じ行の 2 つ上にある
  `:@` の抽出子解決（根 6）でした。
### `Set`/`Map` の構築・追加と、`Array` が `Seq` として扱われない件（`agent/setmap`）

slick に残っていたコレクション構築系 8 件を最小再現して分けたところ、**7 つの根**
でした。1 件は根に届かず（下の「残件」）、代わりに 1 つ上流で出るようになりました。
slick は `errors=44 files_with_errors=26` → **`errors=37 files_with_errors=22`**
（`tests/slick_measure.sh`。エラーが消えたファイル: `ExpandTables.scala`、
`PruneProjections.scala`、`QueryCompiler.scala`、`ResultConverter.scala`）。
fixture は `tests/fixtures/setmap1.scala` の 1 ファイルに全ケース、テストは
`crates/cli/tests/setmap.rs`。修正前の main（`61023ba`）ではこの 1 ファイルで
13 件のエラーが出ます。

**1. `Array` を `Seq`/`IndexedSeq`/`Iterable` として渡す包み込みが無い。**
`def v(a: Array[Any]): Seq[Any] = a` すら通りませんでした。実 scalac の
`-Xprint:typer` はこう出します。

```
def v(a: Array[Any]): Seq[Any]      = scala.Predef.copyArrayToImmutableIndexedSeq[Any](a)
def y(a: Array[Any]): Iterable[Any] = scala.Predef.genericWrapArray[Any](a)
```

`scala.Seq` / `scala.IndexedSeq` は `immutable` の別名なので、
`genericWrapArray` が返す `scala.collection.mutable.ArraySeq` では届かず、
最下位（`LowPriorityImplicits2`）の `copyArrayToImmutableIndexedSeq` が選ばれます。
`scala.Iterable` は `scala.collection.Iterable` なので `genericWrapArray` で届き、
優先順位どおりそちらになります。両方を `prelude_setmap.rs` に足し、
`seqfn_view.rs` の `array_seq_wrap`（`Array[Boolean]` 専用だった）を
`array_wrap_candidates` に一般化して、優先順位順に最初に適合するものを選びます。

**ブリーフの見立て（「`genericWrapArray` は記述子が合わず使えないので
`wrapRefArray` を足せ」）は誤りでした。** 合わないのは `Array[Any]` と宣言した
ときの `([Ljava/lang/Object;)` で、本物の型パラメータを持たせて `Array[T]` と
書けば `erasure.rs` の `array_elem_is_abstract` が nsc と同じく
`Ljava/lang/Object;` に潰します（javap:
`public <T> scala.collection.mutable.ArraySeq<T> genericWrapArray(java.lang.Object)`）。
`wrapRefArray` は `T <: AnyRef` の制約があり `Array[Any]` には効かないので、
nsc もそこでは選んでいません。

`wrapBooleanArray` と同じ理由で **`implicit` にはしていません**（implicit に
すると普通の `Array` のメンバ選択で `refArrayOps` と競合します）。
オーバーロード解決側は `arg_conforms` の view の列に 1 本足してあります
（`TupleSupport.buildTuple(a)` が `IndexedSeq[Any]` の引数に届くのはこの経路）。

**2. `scala.collection.Map` にメンバが 1 つも無い。** `prelude_hier.rs` の
`LINKS` が作る `scala/collection/Map` は型パラメータだけのつなぎで、
`pickle_supply::adopt_binary_class` は `scala/` 名の prelude クラスを触らない
（`class_sym.0 < st.prelude_end`）ので jar からも補われません。slick の
`expansions contains tsym` が `not a member`、`expansions(tsym)` が
**コンパニオンの可変長 `apply`** に落ちて `no matching overload for ((K, V)*)Map[K, V]`
になっていました。`collection.MapOps` の読み出し 3 つ（`contains` / `apply` /
`get`）を宣言しました。

**3. prelude の近似メンバが jar の本物と 2 択になっていた。** `prelude_coll.rs` は
`Set.map(A => Any): Set[Any]` や `Map.+((K, Any)): Map[K, Any]` を手書きしています。
`immutable.HashSet` / `HashMap` は**両方**に届く——上には pickle の
`IterableOps` / `MapOps`、横には prelude の `Set` / `Map`——ので、どちらの
所有者も他方の部分クラスではなく、`A => B` は `A => Any` に適合し `map[B]` は
`B = Any` で適用できてしまうため、`HashSet.map(f)` も `HashMap + kv` も
`ambiguous overload` でした。nsc が見るメンバは 1 つで、それは jar の方です。
曖昧になったときだけ、`pickled_origin` を持つ側を残します。

**4. `@uncheckedVariance` の付いた要素型でメンバ選択の代入が起きない。**
slick の `ConstArray.toSet: immutable.HashSet[T @uncheckedVariance]` の結果に
`.map(_._1)` すると、`_1` が `Tuple2` の宣言どおりの `T1` のまま返り、
`referenced.map(_._1)` が `HashSet[T1]` になっていました。`Type::Tuple` は
`subst_as_seen_from` が扱わない（`type_select` の `subst_args` がそのための
リスト）ので、注釈を剥がしてから見るようにしました。型注釈はメンバについて
何も言いません。

**5. `Option` が `IterableOnce` でなかった。** 2.13 で
`sealed abstract class Option[+A] extends IterableOnce[A]` になっています
（2.12 の `option2Iterable` ではなく本当の親）。実 scalac は
`Set.apply[String]().++(o)` と、**変換を挟まずに**そのまま渡します。

**6. `++` は 2 つのオーバーロード。** javap:

```
scala.collection.SetOps:      public default C    $plus$plus(scala.collection.IterableOnce<A>);
scala.collection.IterableOps: public default <B> CC $plus$plus(scala.collection.IterableOnce<B>);
```

prelude 側には前者に相当する 1 つしか無く（`prelude_coll` が作り
`prelude_buildfrom::widen_set_concat` が広げたもの）、しかもそれが
`lookup_member` に見つかるので pickle 側は `++` を一度も訊かれません
（`SCALA_RS_PICKLE_DEBUG=1` で確認。`concat` は訊かれるので 2 つ揃っています）。
そのため `s ++ anOptionOfSomethingElse` が `no matching overload` でした。
多相版を `prelude_setmap.rs` で足し、あわせて 2 つの規則を入れました。

* `pickle_supply` の「消去後の引数が同じ宣言は 1 つだけ」の鍵に**自分の型
  パラメータを持つかどうか**を足しました。守りたいのは「結果型だけが違う 2 つ」
  （`IterableOps.map[B]` と `MapOps.map[K2, V2]`）で、それは両方とも多相なので
  今も 1 つに潰れます。片方が単相の組は、引数で区別できる本物のオーバーロードです。
* 単相と多相が**同じくらい specific**になったときは単相を採ります。`Set()` の
  要素型が未確定だと `IterableOnce[?A]` と `IterableOnce[B]` が互いを受けて
  しまいますが、nsc は単相の方を選びます（`-Xprint:typer` が `.++(o)` と、
  型引数なしで出します）。

**7. 空のファクトリの型引数を、後続の引数から解く。** `Set()` は `Set[?A]` の
まま置かれ（`instantiate_leftover_tparams` が意図的にそうしています）、
`++` の引数がそれを解くはずでしたが、`undet_compatible` は**引数側**が持つ
変数しか見ていませんでした。**パラメータ側**が未確定な場合を `arg_score` に
足しました。解いた後の代入は `OverloadPick::Found` の側にすでにあります。
`Map() ++ arrayOfPairs` のように包み込みを挟む場合は、包んだ後の型で
unify する必要があるので、そこも 1 行足してあります。

なお 6 の多相版を足したことで `oldDiscCandidates ++ (tree match { … case _ => Set.empty })`
（slick `ExpandSums.scala`）が**新たに 2 件**壊れました。オーバーロードになった
ことで `proto_arg_type` が引数にプロトタイプを渡さなくなり、`match` の腕が
期待型なしで lub され `Set[_ <: AnyRef]` という存在型になったためです
（**同じ式を実 scalac も期待型なしなら存在型にします**。nsc がここで困らないのは、
`IterableOnce[A]` を期待型として腕を 1 つずつ適合させるからです）。
オーバーロードの中で「引数の位置が具体型なのが 1 つだけ」のときはそれを
プロトタイプに使う、という規則（`only_concrete_param`）で戻しました。

**残件（最小再現つき）**

* `m.Column(name=…, options = Set() ++ … )`（`JdbcModelBuilder.scala:279`）。
  `++` は通るようになりましたが、期待型 `Set[ColumnOption[_]]` が
  `Set()` まで届かないので `Set[ColumnOption[Nothing]]` になり、1 つ上流で
  `no matching overload for Column$` になります。エラーは 280 行目から
  279 行目に移っただけで、ファイル数は増えていません。根は
  `proto_arg_type` の `!type_mentions_wildcard(p)`：ワイルドカードを含む
  パラメータはプロトタイプに使われません。これを外すと下の再現は通りますが、
  **slick の数字は 1 も動かなかった**（`m.Column` はコンパニオンの `apply` で
  `ModuleRef` 経路に入るため）ので、測って利のない広げ方はやめました。

  ```scala
  sealed trait CO[+T]
  case class SqlType(s: String) extends CO[String]
  case object AutoInc extends CO[Nothing]
  object S {
    def take(options: Set[CO[_]]): Int = options.size
    def a(d: Option[String], ai: Boolean): Int =
      take(Set() ++ d.map(s => SqlType(s)) ++ (if (ai) Some(AutoInc) else None))
  }
  ```

* `session.withPreparedInsertStatement(sql, keyColumns.toArray)`
  （`JdbcActionComponent.scala:725`）は**担当外の根**でした。`ConstArray.toArray`
  は `def toArray[R >: T : ClassTag]: Array[R]` で、下限しか持たない `R` が
  未確定のまま `Array[R]` として残るため、`Array[String]` 版と `Array[Int]` 版の
  両方に適合して `ambiguous` になります。nsc は下限に落として `Array[String]` に
  します。最小再現:

  ```scala
  import scala.reflect.ClassTag
  class CA[+T](val xs: Seq[T]) { def toArray[R >: T : ClassTag]: Array[R] = xs.toArray[R] }
  object G {
    def over[T](sql: String, names: Array[String] = new Array[String](0))(f: Int => T): T = f(1)
    def over[T](sql: String, idx: Array[Int])(f: Int => T): T = f(2)
    def call(ca: CA[String]): Int = over("x", ca.toArray)(_ + 1)   // ambiguous overload
  }
  ```

  同じファイルの `xs.toArray[R]`（明示型引数）も `found: Array[T] required: Array[R]`
  になります。これは下の「明示型引数がジェネリック親のメンバに as-seen-from を
  かけない」件と同じ根です。

* `Node.scala:534` の `scope + (sym -> el)` は **`:@` 抽出子が見つからない**
  （533 行目）ことの派生で、この節の担当ではありません。

**ついでに見つかった別件（この節では直していません）**

* 明示型引数を書いたメンバ呼び出しが as-seen-from を通りません。
  `s.map[Int](_.length)`（`s: immutable.HashSet[String]`）が
  `value length is not a member of A` ＋ `found: CC[Int] required: HashSet[Int]`。
  型引数を書かない `s.map(_.length)` は通ります。
* `Array[Any](1, "a")` を含むファイルで、**あとから出てくる**要素型を推論する
  `Array(3, 1, 2)` が壊れた記述子を出します（`Array$.apply(Int, Seq[Int])` を
  選んでおきながら `apply(Seq, ClassTag)` を呼び、`VerifyError`）。
  `Array[Int](3, 1, 2)` と書けば避けられます。main（`61023ba`）からある件です。

  ```scala
  object Main {
    def a(): Unit = println(Array[Any](1, "a").mkString(","))
    def b(): Unit = println(Array(3, 1, 2).sum)      // VerifyError
    def main(args: Array[String]): Unit = { a(); b() }
  }
  ```

* `Array[(Int, String)](1 -> "one")` は `Object[]` を作って `[Lscala/Tuple2;` に
  `checkcast` するので `ClassCastException` になります。これも main からある件で、
  fixture では要素代入で配列を作って避けました。

### `Predef.Function`、シグネチャパスの順序、関数型の lub（`agent/final3`）

slick に残っていた単発 7 件を個別に最小再現したところ、**5 つの根**でした。
6 件が消え、1 件が残っています。slick は `errors=17 files_with_errors=13` →
**`errors=11 files_with_errors=9`**（`tests/slick_measure.sh`。新規エラーはゼロ。
エラーが消えたファイル: `lifted/Shape.scala`、`relational/RelationalProfile.scala`、
`memory/DistributedProfile.scala`、`compiler/FixRowNumberOrdering.scala`）。
fixture は `tests/fixtures/final3.scala`（単一ファイルの全ケース）、
`final3_use.scala` ＋ `final3_def.scala`（**コマンドラインの順序**が再現条件なので
2 ファイル必要）、`final3_bad.scala`。テストは `crates/cli/tests/final3.rs`。
修正前の main（`d7e7767`）では 5 本中 4 本が落ちます。

**診断の言葉は 1 件も根を指していませんでした。** 以下、診断ではなく根で並べます。

**1. `Function` は `Predef` の型エイリアスであって `scala.Function` オブジェクト
ではない。** `Shape.scala:397` の `def genericFastPath(f: Function[Any, Any])` が
`Function does not take type parameters`。ブリーフの見立て（型ラムダ
`({ type L[X] = … })#L`、`agent/probe12` の残件と同じ根かもしれない）は**外れ**で、
型ラムダはまったく関係ありません。`scala.Predef` は

```scala
type Function[-A, +B] = Function1[A, B]
```

を宣言していますが（実 scalac 2.13.16 で確認）、シンボル表にはこのエイリアスが無く、
`prelude_fntuple.rs` が入れている `object Function`（`Function.untupled` の置き場）の
モジュールクラス（kind arity 0）に解決されていました。`tree_to_type` の
`AppliedTypeTree` に `Function` の腕はもともとあって（`java.util.function.Function`
用）、そこで解決先が `scala/Function$` のときだけ 2 引数を関数型に読み替えます。
`java.util.function.Function[A, B]` は arity 2 の interface に解決されるので影響を
受けません。`Predef.Function[A, B]` と明示的に書いた形も、解決を試みる前に受け付けます。

**2. `RelationalProfile.scala:82` の `missing parameter type for expanded function`
は 1 の 1 段下流。** `genericFastPath` のパラメータ型が `<error>` なので、渡している
パターンマッチ匿名関数に期待型が降りてこないだけでした。**3 行で両方同時に再現します**:

```scala
object A1 {
  def genericFastPath(f: Function[Any, Any]): Any = f("x")
  val r: Any = genericFastPath(x => x)
}
```

**3. シグネチャパス中に強制した lazy completion が、まだ型の付いていない
「明示注釈付きメンバ」を読んでいた。** `DistributedProfile.scala:76` の
`no matching overload for constructor QueryInterpreter with arguments (<notype>, Any)`。
`<notype>` は第 1 引数の `val emptyHeapDB = HeapBackend.createEmptyDatabase` です。
`createEmptyDatabase: AnyHeapDatabaseDef` は結果型を**書いている**ので lazy 対象では
なく、その型は `HeapBackend.scala` がシグネチャパスで歩かれたときに初めて入ります。
ところが `memory/DistributedProfile.scala` はコマンドライン順で先です。さらに、
**入れ子テンプレートの親句は外側テンプレートの「シグネチャ相」で型付けされる**ので
（`type_class` は本体の全メンバのシグネチャ → 全メンバの本体、の順）、
`class DistributedQueryInterpreter(...) extends QueryInterpreter(emptyHeapDB, param)`
が `emptyHeapDB` をそこで強制し、`<notype>` のまま `lazy_done` に**恒久キャッシュ**
されていました。nsc は全シンボルに lazy completer を持つので起きません。
`complete_lazy_sig` を、**シグネチャパス中に走った完了が何も決められなかったとき、
診断ごと巻き戻して pending に戻す**ようにしました。ボディパスの時点では書かれた
シグネチャは全部入っているので、そこで正しく決まります。10 行で再現します。

```scala
class QI(db: String, param: Any)
class DP {
  val v = HB.s
  class Sub(param: Any) extends QI(v, param)   // no matching overload … (<notype>, Any)
}
object HB { def s: String = "x" }              // ← DP より後ろにあることが条件
```

**4. `recursive method run needs result type` は 3 のカスケードでは
ありませんでした**（3 を直しても残りました）。同じ順序問題の別の消費者です。
`overridden_ret_type` は「候補のシグネチャを強制しない」と決めてあり
（強制すると slick 155 件 → 307 件になった、とコメントに実測が残っています）、
`memory/QueryInterpreter.scala` の `def run(n: Node): Any` はまだ型が入っていないので
`override def run(n: Node) = …` は借りる先を見つけられず、推論待ちのまま自己再帰を
踏んでいました。**ボディが型付けされる直前にもう一度だけ探索し直す**
（`retry_overridden_ret`）ようにし、あわせて **`complete_lazy_sig` の再入検査を
「型がまだ決まっていないときだけ循環」** に変えました（結果型が既に入った時点で
再帰呼び出しは循環ではありません）。この 2 つが揃って初めて消えます
（片方だけでは数字が動きませんでした——ブリーフの「1 つ直して数字が動かなくても
無関係と結論しないこと」がそのまま当たりました）。

**5. 関数型の lub が `AnyRef` に落ちていた。** `SQLiteProfile.scala:138` の
`value apply is not a member of AnyRef`。`Seq((s: String) => Timestamp…, (s: String) => …String)`
の要素型です。`lub` には「同じクラスで引数だけ違うなら引数を join する」腕が
ありますが、`FunctionN` は `Type::Function` という独自のバリアントなのでそこに
入らず、基底型列を歩いて `AnyRef` になっていました。`Function` 同士・同アリティなら
パラメータは `glb`（反変）、結果は `lub`（共変）で join します。

**6. ワイルドカード型引数は、その型パラメータの宣言境界を持つ。**
`FixRowNumberOrdering.scala:19` の
`no matching overload for (Node, Option[Comprehension[Option[Node]]])Node with arguments (Node, Some[Comprehension[_]])`。
`final case class Comprehension[+Fetch <: Option[Node]]` なので
`Comprehension[_]` は `Comprehension[_$1] forSome { type _$1 <: Option[Node] }` です。
`is_sub_type` の `Class`/`Class` 同一シンボル腕で、**左辺**の裸の `Wildcard` を
その型パラメータの `bound_hi` を上限とする `BoundedWildcard` に読み替えます。
右辺は触りません（右辺のワイルドカードは既に何でも含みます）。`agent/tq` が直した
`(Applied, Wildcard)` とは別の場所です。**緩和のみ**なので、これまで通っていた形が
落ちることはありません。境界が効いていることは `final3_bad.scala` で確認しています
（`ComprB[_]` は `ComprB[Some[NdB]]` ではない。実 scalac も
`type mismatch; found: ComprB[_] required: ComprB[Some[NdB]]` と言います）。

**残件（最小再現と診断まで）**

* `jdbc/SQLiteProfile.scala:183`。
  `no matching overload for (Iterable[U], JdbcActionComponent.RowsPerStatement)…
  with arguments (Iterable[U], RowsPerStatement)` ——両辺は接頭辞を除けば同じ名前です。
  `JdbcActionComponent` は

  ```scala
  type RowsPerStatement >: slick.jdbc.RowsPerStatement.One.type <: slick.jdbc.RowsPerStatement
  ```

  という**境界付き抽象型メンバ**を持ち、`MultipleRowsPerStatementSupport` が
  `override type RowsPerStatement = slick.jdbc.RowsPerStatement` で具体化します。
  `SQLiteProfile` はそれを mixin しているので実 scalac では同一ですが、こちらは
  親の宣言側の抽象型メンバを派生側の具体化を通して as-seen-from できていません。
  この節の 5 つの根とは形が違い、抽象型メンバの精練の一般的な扱いに入るため
  手を付けていません。

**ブリーフの見立てとの差分**

* 「7 件すべて別の根と思って始めてください」——実際は 7 件 5 根で、
  `Shape.scala:397` と `RelationalProfile.scala:82` は同一根の 1 段違いでした。
* 「`DistributedProfile.scala` は `recursive method run` が根で `:76` が
  カスケードかもしれない」——**逆でも同じでもなく、独立した 2 根**でした。
  `:76` を直しても `:91` は残り、別の修正が要りました。
* 「`Shape.scala:397` は型ラムダかもしれない（`agent/probe12` の残件と同じ根か）」
  ——違います。`Predef` の型エイリアスが 1 本無いだけでした。
* 「`FixRowNumberOrdering` は `agent/tq` が直した `(Applied, Wildcard)` の周辺」
  ——隣ですが別の腕（`Class`/`Class` の引数比較）でした。
### コレクション引数まわり 7 件の 7 つの根（`agent/final1`）

slick に残っていた「コレクションを引数に渡すところ」の 7 件を **1 件ずつ最小再現**
したところ、**7 件で 7 つの根**でした（同じ症状も同じファイルも 1 根ではない、という
これまでの観測どおり）。すべて実 scalac 2.13.16 で通ること／落ちることを先に確認して
から直しています。slick は `errors=17 files_with_errors=13` →
**`errors=10 files_with_errors=8`**（`tests/slick_measure.sh`。新規エラーは 0 件。
エラーが消えたファイル: `util/ConstArray.scala`、`jdbc/JdbcModelBuilder.scala`、
`jdbc/JdbcActionComponent.scala`、`compiler/ExpandSums.scala`、
`compiler/MergeToComprehensions.scala`）。

fixture は `tests/fixtures/final1.scala` の 1 ファイルに全ケース（＋異常系
`final1_bad.scala`）、テストは `crates/cli/tests/final1.rs`。修正前の main
（`d7e7767`）ではこの 1 ファイルで 12 件のエラーが出ます。

**1. 自己別名 `self =>` に `apply` を挿せない。**
`final class ConstArray[+T](a: Array[Any], val length: Int) { self => … }` の
`def apply(idx: Int) = self(idx)` が
`value apply is not a member of ConstArray.this.type`。`self` の型は
`C.this.type`（`Type::ThisType`）で、`Select` 側はこれをクラスへ widen して
メンバを引いていましたが、**適用側の `resolve_overload` には `ThisType` の腕が
無く** `_ => None` で止まっていました。クラス自身の型引数を入れた
`Type::Class` に読み替えて `Class` の腕へ委譲します。

**2. 下限しか持たない型パラメータが、implicit 節の手前で確定しない。**
`session.withPreparedInsertStatement(sql, keyColumns.toArray)` が
`ambiguous overload … with arguments (String, Array[R])`。
`ConstArray#toArray[R >: T : ClassTag]: Array[R]` の `R` が未確定のまま
`Array[R]` として残り、`(String, Array[String])` と `(String, Array[Int])` の
両方に適合していました。

nsc は `adaptToImplicitMethod` で、implicit 節を探す**前に**
`inferExprInstance(..., keepNothings = false)` を回します。`Nothing` になる変数は
開いたまま（`take(Array.empty)` が引数の側で決まるのはこれ）ですが、実の下限を
持つ変数は**その下限で確定**します。`solve_lower_bounded_undet` がそれで、下限は
宣言のものではなく**受け手から見た**ものを使います（`R >: T` の `T` は
`ConstArray[String]` では `String`）。

`adapt_implicit_apply` 側にも手当てが要りました。「型パラメータを持つのに
`TypeApply` でない」ものは witness 待ちで抜ける規則があり、`R` が消えた
`(ClassTag[String])Array[String]` までそこで止まっていたためです。ただし
「今の型がパラメータに言及しない」だけでは足りません——`type_mentions_wildcard`
と違って `type_mentions_tparam` は**複合型の中を見ない**ので、slick の
`BaseColumnType[U] = ScalaType[U] with BaseTypedType[U]` は「何にも言及していない」
と読まれ、未代入の `U` で implicit 探索が走ります（fixture `ovl4` が落ちます）。
**宣言の型と今の型を比べて、実際に代入が済んだ場合だけ**通します。

**3. 「引数を型付け中」フラグが、遅延シグネチャ補完に漏れていた。**
`m.Table(namer.qualifiedName, columns, primaryKey, buildForeignKeys(builders), indices)`
の第 4 引数が `((Option[ForeignKey]) => IterableOnce[B])Seq[B]` という**未適用の
メソッド型**になっていました。

`typing_call_args` は「この式は、まだ引数の当たり先が決まっていない引数だ」という
印で、`adapt_implicit_apply` が implicit 節を残す条件に使われます。ところがこれは
**typer のフラグであって式のものではなく**、引数の途中から走る遅延シグネチャ補完が
そのまま引き継いでいました。結果、前方参照された

```scala
final def buildForeignKeys(builders: Builders) =
  mForeignKeys.map(mf => createForeignKeyBuilder(this, mf).buildModel(builders)).flatten
```

の `.flatten` の implicit 節（`A => IterableOnce[B]`）が埋まらず、それが
**メソッドの推論結果型そのもの**になっていました。同じ定義を使用より上に書けば
通る、というのが決め手です。`type_def_body` が本体を型付けする間だけフラグを
落とします。`JdbcModelBuilder.scala:93` の `m.Model(… .map(_.buildModel(builders)))`
はこのカスケードで、一緒に消えました。

**4. 引数が持ち込んだ未確定変数を、join の前に最小化していなかった。**
`tableFields.getOrElse(t.identity, Seq.empty)` が `Seq[AnyRef]` になり、その先の
`f` が `AnyRef` になって
`found: Some[(TableNode, ConstArray[((TypeSymbol, AnyRef), List[AnyRef])])]`。

`getOrElse[V1 >: V]` の `V1` は「宣言された下限 `Vector[TermSymbol]`」と「引数
`Seq.empty` の型」の join です。`Seq.empty` の `A` は未確定変数のままで、
`lub(Vector[TermSymbol], Seq[?A])` は基底型を辿って両者が `Seq` で出会い、引数を
join して `Seq[AnyRef]` を返していました。nsc は上から縛るものが無い変数を下限
（既定では `Nothing`）に最小化してから join するので `Seq[TermSymbol]` です。
`minimize_undet` を `unify_tparam_all` の join と、宣言下限との join の両方に
入れました。

**5. case class でないクラスにコンストラクタパターンを当てていた。**
`case IfThenElse(ConstArray(Library.Not(…), ProductNode(ConstArray(Disc1, map)), …))`
の `map` が `Node` ではなく `Int` に、`disc` が `Array[Any]` になり、
`ProductNode(ConstArray(disc, map))` が `ConstArray[Any]` になっていました。

SLS 8.1.6/8.1.7 では、コンストラクタパターンを持つのは **case class だけ**です。
`ConstArray` は `final class ConstArray[+T](a: Array[Any], val length: Int)` で、
コンパニオンに `unapplySeq` があります。こちらは「`ctor_fields` が空でなく、
引数の数が合えばコンストラクタ」を先に見ていたので、`a: Array[Any]` と
`length: Int` の 2 つを束縛していました。抽出子があるなら抽出子を使い、
`ctor_fields` だけの腕は**抽出子が無いクラス**（それが必要だった場面）に残します。

**6. レシーバが持ち込んだ未確定変数に、期待型が効かない。**
`def sqlOptions(dbType: Option[String]): Set[ColumnOption[_]] =
Set() ++ dbType.map(SqlType(_))` が `Set[SqlType]` になり、**不変**な `Set` が
期待型を受け付けませんでした。`Set()` の `?A` は引数から `SqlType` と読まれた
きりで、`Set[?A]` の `?A` は結果の不変位置にあるのに期待型が上書きしません。
callee 自身の型パラメータには `add_expected_constraints` が同じことをしています
（nsc の `instantiateExpecting`）。レシーバ由来の変数にも、
**不変位置で、かつ引数の解が期待型に適合するときだけ**同じ規則を入れました。

**7. 解くものが何も無い変換探索が、形だけの unify で通っていた。**
6 を直しても
`Set() ++ … ++ (if(!autoInc && !generated) convenientDefault else None)` の鎖は
`Set[ColumnOption[Nothing]]` のままでした。最後の `++` の引数
`Option[Default[_]]` に対し、**`Option.option2Iterable` が
`IterableOnce[ColumnOption[Nothing]]` への view を名乗って**いたためです。それが
通ると単相の `Set#++(IterableOnce[A]): Set[A]`（prelude の広げ役）が適用可能に
なり、型パラメータを持たないので期待型に上書きされる余地も無くなります。

根は `open_conversion_fit` で、**解くべき変数が候補側にも呼び出し側にも残って
いない**とき、それでも `Unify` に判定させていたことです。`Unify` にとって
ワイルドカードは何にでも合うので `Iterable[Default[_]]` が
`IterableOnce[ColumnOption[Nothing]]` に「合って」しまいます。解くものが無い
ときは適合そのものを訊く（`is_sub_type`）ようにしました。実 scalac もこの view
を認めず、`Option[Default[_]]` を `IterableOnce[ColumnOption[Nothing]]` に
渡す 3 つの形（`w2`/`w5`/`x2` 相当）を拒否します。

**ブリーフの見立てとの照合。** 引き継がれた 3 つの仮説のうち、当たったものは
ありません。

* 「`Column$` の件の根は `proto_arg_type` の `!type_mentions_wildcard(p)`。
  `ModuleRef` 経路を見よ」——**違います**。その除外を外し `ModuleRef` 経路に
  「全 alternative が一致する具体的な引数型」を渡すようにしても数字は動きません
  でした（改善そのものは正しいので残してあります）。同じ呼び出しは**ワイルドカードが
  どこにも無くても**失敗します（`Set[ColumnOption[String]]` でも同じ）。根は上の
  6 と 7 です。
* 「`toArray` は下限に落とせばよい」——方向は合っていますが、**下限だけでは
  足りません**。落とすのが implicit 節を探す前だ、というタイミングの方が本体です。
* 「`Table$` の件は `agent/implclause` と同種の根の残り」——**別の根**です。
  implicit 節が残ること自体は同じ症状ですが、原因は `typing_call_args` の
  遅延補完への漏れで、`implclause` が直した 4 つとは無関係です。
* `JdbcModelBuilder.scala:93` は 159 のカスケードでした（これは当たり）。

**ついでに見つかった別件（この節では直していません）**

* 期待型のない `val x = ca.toArray`（`toArray[R >: T : ClassTag]`）は、
  implicit 節を残したまま `(ClassTag[R])Array[R]` が値の型になります。実 scalac は
  `Array[String]` にします。2 を引数位置に限って入れたので、`val` の初期化子は
  そのままです。
* 抽象な `R` の `new Array[R](len)` を含むメソッドを持つクラスは、定数プールに
  `[java/lang/Object` という擬似クラス名を書いてしまい `ClassFormatError` に
  なります（型検査は通り、slick の計測にも出ません）。fixture では
  `Array.tabulate[R]` で避けました。codegen 側の穴です。

  ```scala
  final class Holder[+T](a: Array[Any], val length: Int) {
    def toArray[R >: T : ClassTag]: Array[R] = {
      val ar = new Array[R](length)   // ClassFormatError: Illegal class name "[java/lang/Object"
      ar
    }
  }
  ```

* `(0 until n).map(f).toSeq.toArray[R]` のように**明示型引数**を書いた
  `toArray[R]` は `Array[T]` になり `found: Array[T] required: Array[R]`。
  `agent/setmap` が記録した「明示型引数を書いたメンバ呼び出しが as-seen-from を
  通らない」件と同じ形です。
* `val y = Seq.empty` は `Seq[A]`（`A` は `Seq.empty` の型パラメータ）のままで、
  `val z: Seq[Nothing] = y` が `found: Seq[A]`。未確定変数を値の型に残す設計の
  副作用で、4 の最小化は引数位置だけに入れてあります。
### cats-effect の 3 件——「単体では再現しない」の正体（`agent/final2`）

slick に残っていた cats-effect まわり 3 件（`Resource.ExitCase`、`Ref.Make[F]`、
`cats.effect.IO(fa)`）を直しました。slick は
`errors=17 files_with_errors=13` → **`errors=13 files_with_errors=10`**
（`tests/slick_measure.sh`。エラーが消えたファイル: `basic/BasicBackend.scala`、
`basic/ConcurrencyControl.scala`、`dbio/DBIOAction.scala`。ついでに
`JdbcModelBuilder.scala` の `Column$` 1 件も消えました）。
fixture は `tests/fixtures/f2_cats.scala`（正常系・全ケース 1 ファイル）と
`tests/fixtures/f2_cats_bad.scala`、テストは `crates/cli/tests/final2.rs`。
修正前の main（`d7e7767`）ではこの 1 ファイルで 5 件のエラーが出ます。

3 件のうち 2 件は「slick を丸ごとコンパイルしたときだけ壊れる」と 3 スライス
報告され続けていました。**根はどれも同じ形**です——ある記号が、プログラムが
その名前を書くより先に**別の経路**で記号表に入り、先に入った方の答えが残る。
だから最初にやったのは、その「先に入る経路」を特定して**1 ファイルに畳むこと**
でした（下の「再現手段」）。

#### 1. `Ref.of` の `implicit mk: Ref.Make[F]` が見つからない

`ConcurrencyControl.scala:202`。**これは単体で再現します**（前スライスの
「(a) 暗黙引数の挿入」「(b) 存在型 `GenConcurrent[F, ?]` の implicit スコープ」
という見立ては**どちらも違いました**）。

```scala
def create[F[_]](n: Long)(implicit F: Async[F]): F[Ref[F, Long]] = Ref.of[F, Long](n)
```

`Make[F]` の候補は `Ref.Make` のコンパニオンが継承する
`Ref.MakeInstances#concurrentInstance` / `MakeLowPriorityInstances#syncInstance`
だけです。`SCALA_RS_IMPL_DEBUG`（調査用に一時的に足した trace）で見ると
候補集合が空でした。原因は `Check::load_companion_module` で、
`cats/effect/kernel/Ref$Make$` を **パッケージ** `cats.effect.kernel` に
`Make` という名前で入れていたこと。`SymbolTable::companion_module` は
「そのクラス自身の owner のメンバから同名の module を探す」ので、
`Make` の owner である `Ref` を見にいって何も見つけられません。
`Ref.Make` と**ソースに書けば**別経路でコンパニオンが作られて通る——だから
順序依存に見えていました。修正は 1 行の意図どおりの owner に直しただけです。

```rust
// load_companion_module: 入れ子クラスのコンパニオンは、パッケージではなく
// そのクラスを囲むものに属する。
let owner = {
    let o = self.st.get(class_id).owner;
    if !o.is_none() && self.st.get(o).is_class_like() { o }
    else { crate::classpath::ensure_package(&mut self.st, pkg) }
};
```

#### 2. `type ExitCase is not a member of Resource$`

`BasicBackend.scala:421`。**再現手段はこれです**——同じファイルが
`fs2.Stream` を名前に出すこと。

```scala
def stream(s: fs2.Stream[cats.effect.IO, Int]): Int = 0
def succeeded(e: Resource.ExitCase): Boolean = e == Resource.ExitCase.Succeeded
```

`fs2/Stream.class` を読むと、そのメンバ記述子が
`cats/effect/kernel/Resource$ExitCase` に触れます。入れ子クラスファイル
`Outer$Inner` は「`class Outer` と `object Outer` のどちらが宣言したか」を
何も語らないので、`classpath::java_class_owner` は**常にクラスの方**を答えます。
その結果 `ExitCase` は**トレイト `Resource`** のメンバとして入り、ソースの
`Resource.ExitCase`（`Resource` **オブジェクト**を通る経路）は `Resource$` を
探して見つけられません。`BasicBackend.scala` を単体でコンパイルすると
逆の順序（`Resource$` から先に読む）になるので通っていた、というだけでした。

修正は `classpath::install_java_class_in` に `enter_in_companion_scope` を足し、
「訊いてきた owner が、いま持っている owner のコンパニオン module class なら、
同じ記号をそちらのスコープにも入れる」ようにしたもの。記号は増やしませんし
owner も書き換えません。両方の綴りが**1 つしかないクラス**に届くだけです。

なお、`pickle_supply.rs` の `complete_type_member` はこの `None` を
`tried_types` に**記憶する**ので、一度失敗すると以後ずっと失敗します。
その入口に `SCALA_RS_PICKLE_DEBUG=1` の trace を足しました
（`… : no pickle read -- the class has not been adopted yet`）。
順序依存の「type X is not a member of Y$」はここから始まります。

**前スライスの見立ての訂正**: 「パッケージオブジェクトの val 越しのネスト
クラス」は `agent/implfind` の指摘どおり成り立ちません。ただし「供給の重複」
でもなく、**入れ子クラスの owner がクラスかコンパニオンかを classfile 名から
決められない**ことでした。

#### 3. `cats.effect.IO(fa)` が `no matching overload`

`DBIOAction.scala:237`。これも `fs2.Stream` を同じファイルに書けば単体で
再現します。`IO.apply(thunk: => A): IO[A]` は**by-name 引数**なので、
classfile の総称シグネチャには書けません（`(Lscala/Function0<TA;>;)…`）。
classfile リーダの写しは `apply(Function0[A]): IO[A]` になり、`Future[R]` は
どれにも当たりません。しかも pickle からの補完は
「`lookup_member` が**何も**見つけられなかったときだけ」走るので、
この誤った写しがある限り永久に直りません。scalac はコンパニオンの各メソッドを
**クラス側の static forwarder** としても出すので、`cats/effect/IO` の側にも
同じ erasure の `apply` が載ります（今回はそちらが選ばれていました）。

`Check::retry_module_apply_from_pickle` を足しました。**「no matching overload
を出す直前」でだけ**走り、レシーバのコンパニオン module class に対して
`apply` を pickle から補完し、木を型付けし直します。何も新しく入らなければ
`false` を返すので再帰しません。コンパニオンを先回りして adopt することは
しません（`IO$` の adopt は ~200 メンバの完成を引き起こし、6 行のソースに
分単位かかる——`supply_implicit_members` の doc コメントにあるとおり）。

#### 再現手段（次に同じ形に当たった人へ）

* **順序依存を疑ったら、記号がどこで作られたかを見る。**
  `find_or_stub_java_class` に `std::backtrace::Backtrace::force_capture()` を
  1 行足して slick を丸ごと 1 周させると、`Resource$ExitCase` を作った犯人が
  `fill_java_members`（＝`fs2/Stream.class` を読んだとき）だと 1 回で出ます。
  そこから「その classfile を読ませる 1 行」を書けば単体再現になります。
* **ファイル集合の二分は要りませんでした。** 184 ファイルを削っていくより、
  「誰がその記号を先に作ったか」を直接見る方が速い（1 周 ≒ 90 秒、二分は
  最低でも 8 周）。
* **同一ファイル内では、シグネチャの解決が本体の型付けより先**です。
  だから「先に別のメンバを触っておく」形の warm-up は、同じファイルでも
  2 ファイルでも `Resource.ExitCase` より先には来ません。効くのは
  `parents_pass` 中に classfile を読ませる形（型として名前を書く）だけです。

#### 残件（この節では直していません）

* 暗黙引数節を持つメソッドを**明示的に**書き、期待型を与えた場合に
  暗黙引数が挿入されません。slick には出てきませんが、同じ領域です。

  ```scala
  def a3[F[_]](implicit F: Async[F]): Ref.Make[F] = Ref.Make.concurrentInstance[F]
  // type mismatch; found: (GenConcurrent[F, _])Make[F]  required: Make[F]
  ```

  `implicitly[Ref.Make[F]]` と `Ref.of[F, Long](n)` はどちらも通るので、
  implicit 探索そのものではなく「明示参照に節を適用する」側です。
* `cats.effect.IO` が**項の位置でクラス記号に解決される**ことがあります
  （`IO$` がまだ記号表に無いとき）。3 の修正はその状態からでも動きますが、
  本来は module に解決されるべきで、そこを直せば static forwarder を
  選ぶこと自体が無くなります。
* `IO(1, 2)` を nsc は `too many arguments` で拒否しますが、こちらは
  自動タプル化して `IO[(Int, Int)]` にします（main からある差）。
  → 上の 3 件はすべて次節（`agent/arraygen`）で直しました。

### `Array` の codegen ——「型は通るのに実行時に壊れる」7 件（`agent/arraygen`）

`agent/setmap` が `tests/fixtures/setmap1.scala` で回避していた 3 件を直し、
あわせて `Array` を使う普通のプログラム 8 本を dual-run しました。プローブ 8 本の
うち **6 本が最初の実行で差分を出し**、さらに 4 つの根が出ました。合計 7 件。
fixture は `tests/fixtures/arraygen1.scala`（全ケースを 1 ファイル）、テストは
`crates/cli/tests/arraygen.rs`、プローブは `tests/conform/array_*.scala` の 8 本。
修正前の main（`d7e7767`）では `arraygen1.scala` は **4 件のエラー**で止まり、
その 4 行を消して通しても `VerifyError` → `ClassCastException` →
`ClassFormatError` と順に落ちます。

`Array` は erasure と ABI の継ぎ目そのもので、**7 件のうち 6 件は型検査を
完全に通ります**。「コンパイルできた」は `Array` については何の保証にもなりません。

**1. 明示型引数がジェネリック親のメンバに as-seen-from をかけない。**
`s.map[Int](_.length)`（`s: immutable.HashSet[String]`）が
`value length is not a member of A` ＋ `found: CC[Int] required: HashSet[Int]`
になり、型引数を書かない `s.map(_.length)` は通っていました。`TypeApply` は
オーバーロード集合を型引数の個数で絞ったあと `SymbolTable::get(only).ty`——
**宣言そのままの型**——を土台にしていました。`map` を宣言しているのは
`IterableOps[A, CC, C]` なので、`A` も `CC` も受け手の引数が入っていません。
選択（`type_select`）は同じ仕事をすでに済ませて `overload_member_types` に
入れてあるので、そこから引くようにしました（`Check::member_ty_as_seen_from`）。

**`xs.toArray[R]` は直っていません**（下の「残件」）。`agent/setmap` の README
が「これは as-seen-from の件と**同じ根**」と書いていて、コーディネータ経由で
`agent/final1` からも同じ見立てが来ましたが、**どちらも誤りです**。prelude の
`toArray` は `(implicit ClassTag[A]): Array[A]` と**単相**に宣言されていて
（`prelude_seq.rs` の `add_conversions`、`prelude.rs:3460`）、nsc の
`toArray[B >: A: ClassTag]: Array[B]` ではありません。型パラメータが 0 個なので、
明示型引数は as-seen-from 以前に**代入する先を持ちません**。実際
`s.map[Int](f)` を直しても `xs.toArray[R]` は 1 ミリも動きませんでした。
多相にしてみると、今度は `List(1, 2, 3).toArray`（期待型なし）で `B` が未確定の
まま残り、implicit 節が適用されずに
`value mkString is not a member of (ClassTag[B])Array[B]` になります。
**下限しか持たない型変数を選択の時点で下限に落とす**推論が先に要ります
（`instantiate_leftover_tparams` は `Apply` からしか呼ばれず、しかも
`sig_params` が `ClassTag[B]` を見つけて降ります）。触るのは
`maybe_auto_apply` / `adapt_implicit_apply` で、そこは `agent/final1` が
同時に編集している場所なので、このスライスでは戻しました。

**2. 同じファイルの前の宣言が後の生成を壊す——持ち越されているのは
`scala.Array$` のオーバーロード集合そのものです。** `scala.Array` は `apply` を
**10 個**宣言しています。prelude が手書きするのは
`apply[T](xs: T*)(implicit ClassTag[T]): Array[T]` の 1 つだけで、残り 9 つ
（`apply(x: Int, xs: Int*): Array[Int]` など、プリミティブと `Unit`）は
**`PickleSupply` が要求されたときにだけ**シンボル表に入ります。入れる引き金は
`Array[T](…)` という**明示型引数**で、`type_expr` の `TypeApply` 分岐が
`Module[T]` の形を見て `supply_from_pickle_class(cls, "apply")` を無条件に
呼びます（`SCALA_RS_PICKLE_DEBUG=1` が `scala.Array#apply: supplied 9
overload(s)` と出します）。

つまり **`Array[Any](1, "a")` がファイルのどこかにあるかどうかで、後続の
`Array(3, 1, 2)` が解決するオーバーロードが変わります**。それ自体は nsc と
同じ結論（nsc も `apply(x: Int, xs: Int*)` を選びます）に着くのですが、
gen.rs は `owner == "scala/Array$" && name == "apply"` に対して
**10 個すべてに generic の記述子を書いていました**:

```
invokevirtual scala/Array$.apply:(Lscala/collection/immutable/Seq;Lscala/reflect/ClassTag;)Ljava/lang/Object;
```

`apply(x: Int, xs: Int*)` を選んだ呼び出しはスタックに `int` と `Seq` を
積んでいるので、`Seq` の位置に `int` が来て `VerifyError`。単相の 9 つは
自分の記述子（`method_desc_boxed`）が正しいので、型パラメータの有無で
分けるだけで済みました。

**この「順序が意味を持つ」性質は `Array$` 固有ではありません。**
`PickleSupply::complete` は設計上 lazy かつ additive で、`check.rs` の
`own_decl_when_all_inherited` のコメントが同じ事故（`TreeMap#collect` が
`Map#collect` を先に読んだファイルでだけ `List` を返した）を記録しています。
供給の順序を変えるのではなく、**どのオーバーロードが選ばれても正しい
記述子が出る**ようにするのが直し方です。fixture の `mixedFirst` を
`inferredLater` より前に置いてあるのはこれを踏むためで、動かすとテストが
バグを見なくなります。

**3. `ClassTag` の `classOf` がタプルで `java/lang/Object` に落ちる。**
`Array[(Int, String)](1 -> "one")` は
`Array.apply(seq, ClassTag.apply(classOf[Object]))` を出すので `Object[]` が
でき、呼び出し側が付ける `checkcast [Lscala/Tuple2;` で
`ClassCastException`。`gen_java_class_of` は `Type::Array` を（同じ理由で）
特別扱いしていましたが、`Type::Tuple` / `Type::Function` は `_` に落ちていました。
`Type::Annotated` / `Type::Constant` も剥がすようにしてあります。
`ClassTag` の runtime class は `Array.apply` が**実際に確保する配列の要素型**
なので、`jvm_desc` と食い違ってはいけません。

**4. `f(arr: _*)` が `Array` を包まずに渡す。** 可変長引数は
`scala/collection/immutable/Seq` に erase されるので、
`render(names: _*)`（`names: Array[String]`）は
`[Ljava/lang/String;` をその記述子の下に積んで `VerifyError` でした。
gen.rs には「`f(xs: _*)` はもう列を持っているので包むものは無い」という
前提があり、**`Array` は列ではない**という例外が抜けていました。nsc の javap は
`Predef.copyArrayToImmutableIndexedSeq(names)` を出します（`genericWrapArray`
の `mutable.ArraySeq` は `immutable.Seq` ではないので届きません）。
**Java の可変長引数だけは例外**で、そこは配列そのものが引数です。

**5. `Array[T]` の要素代入がロードできないクラスファイルを出す。**
`def repeat[T: ClassTag](x: T, n: Int)` の中で `a(i) = x` すると
`invokevirtual "[java/lang/Object".update:(ILjava/lang/Object;)V` が出ます。
`[java/lang/Object` は JVM が名前として受け付けないので
**`ClassFormatError` でクラスがロードすらできません**。`new Array[T](n)` は
`ct.newArray(n)` に書き換えられて型が `Any` になるため、`qual.ty` が
`Type::Array` ではなくなり、gen.rs の配列アクセス経路に入らないためでした。
nsc と同じく `ScalaRunTime.array_apply` / `array_update` / `array_clone` を
呼びます（`length` がすでに `array_length` に落ちているのと同じ分岐）。
`def dup[T](a: Array[T]) = a.clone()` も**同じ根**で、こちらは引数として
受け取っただけでも壊れます。`--no-scala-library` には `ClassTag` が無いので、
そちらは今までどおり診断です（`tests/fixtures/arraygen_gate.scala`）。

判定は**要素型ではなく受け手の型**で行っています。要素型が抽象な配列は
この時点でもう `Type::Array` として届かず（`new Array[T](n)` は
`ct.newArray(n)` に書き換わって型が `Any` になり、`a: Array[T]` の
パラメータも erasure を通ると潰れます）、**それが配列用の経路が
そもそも呼ばれなかった理由**です。

**6. `ArrayOps` の `$extension` に引数無しの記述子を書いていた。**
`a :+ x` が
`invokestatic scala/collection/ArrayOps.$colon$plus$extension:(Ljava/lang/Object;)Ljava/lang/Object;`
——**受け手だけの記述子**——を出していました。実物は
`$colon$plus$extension(Object, Object, ClassTag)Object` です。スタックには
配列・要素・`ClassTag` の 3 つが積まれるので、余りが最初の合流点で
`VerifyError: Inconsistent stackmap frames` になります。`head` や `reverse` の
ように受け手しか取らないメンバでは正しかったので、**引数を取るメンバだけが
壊れていました**。pickle から来たシグネチャは nsc が出す erasure そのものなので、
記述子はシンボルから作ります（受け手だけは手書き——`ArrayOps` の
`Array[A]` は `Object` に潰れ、`[Ljava/lang/Object;` ではありません）。

**7. ラムダの `Array` 引数に checkcast が無い。** `g.map(_.length)`
（`g: Array[Array[Int]]`）が `VerifyError: Bad type on operand stack in
arraylength`。ラムダの `apply` は引数を `Object` で受けるので、
`arraylength` / `aaload` / `aastore` の前に cast が要ります。引数を型付き
ローカルへ移す所は `Type::Class` と `Type::Tuple` は cast していて、
`Type::Array` だけ抜けていました（捕捉変数の側の
`emit_from_erased_object` は前から正しく扱っています）。要素型が抽象なら
配列自身も `Object` に潰れているので、そこは cast しません。

**差分プローブ（`Array` 8 本）**

機能チェックリストではなく普通のプログラムとして書き、出力を `println` で
伴わせて dual-run しました。**8 本中 6 本が最初の実行で落ちています**
（`array_matrix` だけは書き直しました。下の残件を踏むためです）。

| プローブ | 形 | 初回の結果 |
|---|---|---|
| `array_histogram` | `new Array[Int]` に数えて `sortBy`/`take` | 一致 |
| `array_matrix` | `Array[Array[Double]]` の積、`ofDim`/`flatMap` | 差分（`flatten`/`transpose`。下の残件） |
| `array_varargs` | `Array[Item]` → `map` → `render(names: _*)` | 差分（4） |
| `array_inplace_sort` | `update` でのバブルソート、`fill`/`tabulate`/`clone` | 差分（`clone` 未実装） |
| `array_log_parse` | `split` → `flatMap` → `groupBy` → `toSeq` | 差分（下の残件 2 つ） |
| `array_classtag_util` | `[T: ClassTag]` の `repeat`/`concat`、`Array.copy` | 差分（5） |
| `array_inventory` | `indexWhere`/`updated`/`:+`/`zipWithIndex`/`partition` | 差分（6） |
| `array_argv_match` | `case Array("add", a, b)` / `rest @ _*`、`grouped` | 一致 |

**残件（最小再現つき・直していません）**

* `Array[Array[T]]` の `flatten` / `transpose`。どちらも
  `A => IterableOnce[B]` / `A => Array[B]` という**view の implicit** を要求
  します。探索が失敗してメソッド型が式に残り、
  `value mkString is not a member of ((Array[Int]) => IterableOnce[B], ClassTag[B])Array[B]`
  という診断になります（**黙って通ってはいません**）。`array_wrap_view` は
  `Array[Int]` 専用かつ `wrapIntArray` 固定なので、`array_wrap_candidates` で
  一般化したうえで `B` を包んだ型から解く必要があります。

  ```scala
  val grid: Array[Array[Int]] = Array(Array(1, 2, 3), Array(4, 5, 6))
  println(grid.flatten.mkString(""))
  println(grid.transpose.map(_.mkString("")).mkString("|"))
  ```

* `Array#flatMap` に**メソッド参照**を渡すと `ambiguous overload`。ラムダ
  （`xs.flatMap(s => parse(s))`）は通り、`List#flatMap(parse)` も通ります。
  `ArrayOps.flatMap` は 2 つあり、prelude では 1 つ目の引数を
  `A => Any` と近似しています。nsc は `A => IterableOnce[B]` なので
  `Option[Int]` の方が `A => BS` より specific だと言えますが、`Any` では
  引き分けます。1 つ目を nsc どおりにすると
  `arr.flatMap(x => Array(...))` が 2 つ目へ回り、そこは view の implicit
  （上の件）を要求するので、この 2 つは一緒に直す必要があります。

  ```scala
  def parse(s: String): Option[Int] = s.toIntOption
  Array("1", "x").flatMap(parse)   // ambiguous overload for flatMap
  ```

* `"a b c".split(" ", 2)`。prelude の `String#split` は 1 引数だけです
  （`Array` ではなく `String` 側の穴）。

* `xs.toArray[R]`（上の 1 参照）。prelude の `toArray` を多相にするだけでは
  `List(1,2,3).toArray` が壊れるので、下限だけを持つ型変数の推論と一緒に。
  `agent/setmap`・`agent/final1` の両方がここを踏んでいます。

  ```scala
  def f[T: ClassTag](xs: Seq[T]): Array[Any] = xs.toArray[Any]
  // found: Array[T]  required: Array[Any]
  ```

なお、コーディネータ経由で `agent/final1` から回ってきた
「クラスのメソッド内の `new Array[R](len)`（`R` は抽象型パラメータ）が
`[java/lang/Object` を定数プールに書いて `ClassFormatError`」は上の **5** と
同じ根で、この節で直っています。回避（`Array.tabulate[R]`）は外して構いません。
正確には壊れるのは `new` ではなく**その配列に触る側**で、`out(i) = …` /
`out(i)` / `a.clone()` の 3 つです（`c.blank[Int](3)` のように作って返すだけなら
前から通っていました）。fixture の `CArr#toArr` と `Main.dup` が両方を留めます。

```scala
class CArr[+T](val xs: Seq[T]) {
  def toArr[R >: T: ClassTag]: Array[R] = {
    val out = new Array[R](xs.length)
    var i = 0
    while (i < xs.length) { out(i) = xs(i); i += 1 }   // ← ここが ClassFormatError だった
    out
  }
}
```

**測定**

* `tests/slick_measure.sh`: `files=184 errors=17 files_with_errors=13`
  → **変化なし**（エラー行も 1 文字違わず同一）。この 7 件はどれも
  「型検査は通るのに実行時に壊れる」ものなので、型エラーを数える指標は
  動きません。**動かないことを確認するのが正しい期待値**です。
* `tests/slick_subset.sh`（`SLICK_SEED_LOG` 付きで 1 回）:
  `subset_files=47 classes=300 verified=300 failed=0` → 変化なし。
* `tests/conform`: 77 本 → **85 本**。

### `agent/lastone` スライスのテスト

`crates/cli/tests/lastone.rs`（4 本）。fixture は `tests/fixtures/lastone.scala`
（1 ファイルに全ケース）と `tests/fixtures/lastone_bad.scala` です。他のエージェント
との衝突を避けるため `e2e.rs` には入れていません。

`lastone.scala` は slick を 1 行も使わずに `SQLiteProfile.scala:183` の形を並べます:
`type RowsPerStatement >: Rps.One.type <: Rps` という**境界付き抽象型メンバ**を、
mixin が**上限まで広げて**具体化する側（`MultiSupport`）と、**下限まで狭めて**
具体化する側（`SingleSupport`）の両方で、内部 trait が
`super.insertAll(value = …, batch = …, rows = if (batch) Rps.One else rows)` を
名前付き引数で呼びます。**修正前の main では
`no matching overload for (U, Boolean, Comp.RowsPerStatement)String` 1 件で落ちます。**
`fixtures_lastone_library_abi` / `fixtures_lastone_private_runtime` が
`--scala-library` と私有ランタイムの両方で `java -Xverify:all` の下に走らせ
（狭める側は `$super$` アクセサの記述子が親と違うので、型検査だけでは
通っても**ロードと実行**で初めて食い違いが出ます）、
`real_scalac_dual_run_lastone` が real scalac 2.13.16 の stdout と 1 文字まで
一致することを見ます。`fixtures_lastone_bad_is_error` は、`this` から見えるように
したことで型メンバが「なんでも通す」ようになっていないことを固定します:
狭い具体化の下で `Rps.All` を渡す形と、何も具体化していない場所で同じことを
する形の**2 件**を拒否します（real scalac 2.13.16 も同じ 2 行で
`found: BadRps.All.type / required: … (which expands to) BadRps.One.type` と
`required: BadOpenProfile.this.Rows` を出します）。同じ fixture に
`class Ops { val / = "div"; val + = "plus"; var % = "mod" }` と
`object Ops { val * = "times" }`（slick の `ast/Library.scala` そのものの形）も
入れてあります。**型検査では捕まらない**——生の `/` はフィールド定義として
書けてしまい、`java` がクラスをロードするときに初めて
`ClassFormatError: Illegal field name "/"` になるので、
`-Xverify:all` で実際に走らせる 3 本が唯一の網です。

### `agent/indy` スライスのテスト

`crates/cli/tests/indy.rs`（8 本）。fixture は `tests/fixtures/indy1.scala`
（私有ランタイムでも動く `Function0` / `Function1` だけ）、`tests/fixtures/indy2.scala`
（`Function2` / `Function3`、`PartialFunction`、ユーザー定義 SAM、by-name、
ノンローカル `return`、`Array` 引数）、`tests/fixtures/indy1_bad.scala` です。
他のエージェントとの衝突を避けるため `e2e.rs` には入れていません。

見ているのは**挙動**と**形**の 2 軸です。

* 挙動: `indy1` を私有ランタイムと実 scala-library の両方で `java -Xverify:all` の
  下に走らせ、`indy2` は実 scalac 2.13.16 の stdout と byte 一致することを見る。
  `invokedynamic` は **`Class.forName(initialize=false)` では link されない**ので、
  ブートストラップが壊れていても検証器は黙っています。**実際に走らせる**この 2 本が
  唯一の網です。
* 形: `indy1` は 10 個のラムダを持つのに閉包 classfile が **0 個**であること、
  `Main$` と `Bump$class` に `$anonfun$` が乗っていること、`javap -v` に
  `BootstrapMethods` と `REF_invokeStatic java/lang/invoke/LambdaMetafactory.metafactory`
  が出ること、`indy2` は逆に **ちょうど 3 個**（`PartialFunction` 2 個 + SAM 1 個）
  出ることを固定します。最後の 1 本は「まだ indy にしていない形」を明示的に
  留めるためのもので、境界を動かすときは**この数を意図的に**動かしてください。

`indy1_bad.scala` は 2 引数リテラルを `Int => Int` に入れる形です。codegen が
link できない call site を組み立てる前に、typer が
`type mismatch; found: (Int, Int) => Int` で止めることを見ます。

