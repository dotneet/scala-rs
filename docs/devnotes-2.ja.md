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
`public abstract` の通常経路のままです。ただしその**名前**は後に `agent/outer` が
nsc に合わせました（`Widened$$secret`。§「匿名クラスから外側のクラスを触る
4 つの根」）。

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
* **副産物として見つけた別バグ**: カリー化した `new C(…)(…)`
  （コンストラクタへの直接呼び出し、`copy` 経由ではない）が
  `slick/lifted/SimpleFunction.scala:74` の `new SimpleLiteral(name)(tpe)` で
  `ambiguous overload for apply with arguments (String)` を出していました
  （このスライスの変更の前から存在する症状）。→ `agent/tail4` で修正済み。
  根は「`Apply` 層ごとの独立解決」ではなく**パーサが `New` をチェーンの
  先頭に置いていなかった**ことでした。下の「カリー化した `new C(…)(…)` は
  1 個のコンストラクタ呼び出し」を参照。`try_rewrite_case_copy_curried` が
  `new` 経由の再構築を避けていたのも、そこで直しています。

### カリー化した `new C(…)(…)` は 1 個のコンストラクタ呼び出し（`agent/tail4`）

テストは `crates/cli/tests/tail4.rs`、fixture 接頭辞は `t4` です。

計測は `files=184 errors=177 files_with_errors=57` →
**`files=184 errors=166 files_with_errors=53`**（−11 件 / −4 ファイル）。

| 塊 | before | after |
|---|---|---|
| `value getOrElse is not a member of Product` | 4 件 | **0 件** |
| `value apply is not a member of ConstColumn[T]` / `TypedCase[B, P]` / `ConnectionArbiter$` | 3 件 | **0 件** |
| `ambiguous overload for apply with arguments (String)` | 2 件 | **0 件** |
| `recursive method apply needs result type` | 1 件（同根のカスケード） | **0 件** |
| `type mismatch; found: Option[Product] required: Option[Option[Any]]` ほか `Product` 由来 3 件 | 3 件 | **0 件** |

（新しく到達可能になったエラーが 2 件出ています:
`slick/lifted/Query.scala` の `Shape[…]` / `Tuple2[T, T2]` の型不一致。
これまで手前で落ちていた行が通るようになった結果です。）

`agent/tail3` が「未修正バグ」として残した
`slick/lifted/SimpleFunction.scala:74` の `new SimpleLiteral(name)(tpe)`
（`ambiguous overload for apply with arguments (String)`）を追ったところ、
根は**オーバーロード解決ではなくパーサ**にありました（1）。それが直ったことで
初めて到達可能になった穴が 2 つ（2・3）と、`tail3` の `copy` 書き換えが
`new` を避けたことによる**サイレントな誤コンパイル**が 1 つ（4）出てきます。

もう 1 つ、独立した根として、4 スライスが「slick 184 ファイル全体の状態に
依存する」として縮小に失敗していた `value getOrElse is not a member of
Product` を直しました（5）。slick には依存しておらず、`SymbolTable::lub` が
「クラスは合っているが型引数が違う」候補を素通りしていたのが原因です。

#### 1. 根: `New` がチェーンの**先頭**に付いていなかった

`parse_new`（`crates/parser/src/parse.rs`）は親（`Apply(Apply(C, a), b)`）の
`Apply` を**1 段だけ**分解し、その `fun`（＝`C(a)`）を `New` で包んでいました。
つまり `new C(a)(b)` は `Apply(New(C(a)), b)` になり、`New` の「型」の位置に
**適用式**が入ります。`New` の型付けはそこを普通の式として型付けするので、
`C(a)` は `apply` の探索になります —— コンパニオンが自前の `apply` を持つ
`SimpleLiteral` では `ambiguous overload for apply`、持たないクラスでは
`no matching overload for constructor apply` でした（`tail3` が見た
「`Apply` 層ごとに独立して解決している」という観察は症状の言い換えで、
実際には `New` の位置がずれていただけです）。

`parse_new` はチェーンを最後まで剥がして先頭に `New` を置くようにし
（`new C(a)(b)` → `Apply(Apply(New(C), a), b)`）、`Typer::flatten_curried_new`
（`crates/typer/src/check.rs`）が `extends A(1)(2)` に対して
`type_parent_ctor_app_in` が昔からやっているのと同じことをします ——
先頭が `New` のときに限り引数リストを 1 本に潰す。`pick_ctor` も JVM も
コンストラクタの引数リストは平坦なものとして扱うので、ここが合流点です。

ただし潰すのは**第 1 リストが選ぶコンストラクタが受け取れる分だけ**です。
`class Foo(a: Int) { def apply(b: Int) = … }` の `new Foo(1)(2)` は nsc では
`(new Foo(1)).apply(2)` で、2 リストを潰すと**2 引数の `Foo` を作ってしまう**
——クラスがそういうコンストラクタを持っていれば黙って。どのコンストラクタを
作っているかは第 1 リストの長さが決めるので（`class Ov(a: Int) { def this(a:
Int, b: Int) = … }` の `new Ov(1)(2)` は 1 引数の方）、第 1 節の長さが一致する
候補から総引数数を取り、無ければ「第 1 節がそれ以上長い」候補で代用します
（省略されたデフォルト・implicit の分）。両方 `t4_curried_new.scala` に
入れてあります。

#### 2. コンストラクタの節は `new` に書かれた型引数で読む

`slick/lifted/Case.scala:21` の
`new TypedCase[B, P](ConstArray(cond, res.toNode))(bType, om.liftedType(bType))`
は、宣言が `TypedType[B]` の節に `BaseTypedType[B]` を渡します。この適合は
クラスの型パラメータを `[B, P]` に読み替えたあとでしか成立しませんが、
`new` の経路は `pick_ctor`（型引数を渡さない版）を呼んでいました。
`extends A(1)(2)` は最初から `pick_ctor_at` で型引数を渡しています。同じに
しました。

#### 3. 明示的に書かれた implicit 節を**もう一度探索しない**

コンストラクタの引数は平坦化されて `fill_defaults_and_implicits` に届く一方、
コンストラクタ**シンボル**の `paramss` は 2 節のままなので、第 2 節が
「まだ埋まっていない」と読まれ、ユーザーが書いた引数の**後ろに**探索結果が
追加されていました。`new K[B]("s")(tb)` は型検査を通ったうえで
「2 パラメータのコンストラクタに引数 3 個」というバイトコードになり、
`java -Xverify:all` が `VerifyError: Bad type on operand stack` を出します
——診断ではなく誤コンパイルです。呼び出しが本当に**足りない**とき
（`args.len() < ctor_params.len()`）だけ埋めるようにしました。

#### 4. `copy()(x)` はコンパニオンの `apply` ではなく `new`

`tail3` の `try_rewrite_case_copy_curried` は、カリー化した `new` が壊れて
いたためコンパニオンの `apply` を経由していました。しかし両者が同じメソッド
なのは**コンパニオンが合成のときだけ**です。`emit_module`
（`crates/backend/src/gen.rs`）は、コンパニオンの本体が `apply` を 1 つでも
宣言していると合成 `apply` を出しません。`SimpleLiteral` はまさにそれなので、
`def rebuild = copy()(buildType)` は classfile に無いメソッドへの呼び出しに
コンパイルされていました（`NoSuchMethodError: SimpleLiteral$.apply(String,
Type)`。1 が直って初めて到達できる経路です）。nsc の `copy` はコンストラクタ
呼び出しそのものなので、`new C(…)(…)` を組むように変えました。

#### 5. `lub` が「クラスは合っているが型引数が違う」候補を素通りしていた

`value getOrElse is not a member of Product`（4 件、
`slick/jdbc/PositionedResult.scala`）は `agent/tail1` / `mismatch10` /
`mismatch11` / `tail3` の 4 スライスがいずれも縮小に失敗し、「slick 184
ファイル全体の状態に依存する」と記録していた症状です。実際には slick には
**まったく依存していません**。依存しているのは、その run が
scala-library をどこまで読み込んだかです。

`SymbolTable::lub`（`crates/typer/src/symbol.rs`）は `a` の base type
sequence を歩き、`b` が適合する**最初の**候補を返していました。
`if (rs.wasNull) None else Some(r)` ではその列は `None.type`、
`Option[Nothing]`、そのあとは `Option` 自身の親です。
`Some[Blob] <: Option[Nothing]` は偽（`Blob` は `Nothing` の部分型ではない）
なので次の候補に進みますが、`scala/Option` の classfile は
`implements scala.Product` と書いてあるので、その run のどこかで
`scala/Option` の classfile が読まれていれば `Product` がすでに上界として
並んでおり、そこで止まります。関数はこのあと `b` の列も歩くので、そちらまで
行けば `Option[Blob]` が見つかったはずでした。

素通りしていたのは**クラスは合っているが型引数が違う**候補です。2 つの列は
`Option` で出会っていて、片側が `Nothing`、もう片側が `Blob` だっただけです。
そこで、`b` の列に**同じクラス**の項があれば型引数を join して（`lub` が
自分で持っている「同じクラスなら引数を join する」枝に投げ直すだけ）、
その型で歩きを止めるようにしました。答えは `Option[Blob]` になり、
ライブラリをどこまで読んだかには依存しません。

候補を全部集めて「specificity で順位付け」する版も試しましたが、**間違い**
です: `lub(Circle, Rect)` では `Product` も `Shape` も極小で、
`Product <: Equals` があるぶん `Product` の方が「特殊」に見えてしまいます。
なお nsc の答えは正確には `Option[Blob] with Product with Serializable` で、
交差型を組むところまでは実装していません。

`t4_lub_bases.scala` はこの形をユーザーコードで書き下したもの
（`sealed abstract class Opt[+A] extends Marker` / `case object Nn extends
Opt[Nothing]` / `case class Sm[+A](v: A) extends Opt[A]`）なので、
ライブラリの読み込み状態に依存せず、素の `main` でも
`value get is not a member of Product` として落ちます。

#### 検証

`t4_curried_new.scala` / `t4_lub_bases.scala` は `--scala-library` と `--no-scala-library` の両方で
`-Xverify:all` を通し、real scalac 2.13.16 の標準出力とも突き合わせています
（`crates/cli/tests/tail4.rs`）。修正前の `main` では拒否されることを確認済み
です。`t4_curried_new_bad.scala` は、平坦化が何でも通すようになっていない
こと —— 第 3 引数リスト、第 2 リストの型不一致、埋められない evidence ——
を固定します（nsc 2.13.16 も同じ 3 件を出します）。パーサと `check.rs` の
継ぎ目に触れたので `cargo test --workspace` を回しました。

slick: `errors=177 files_with_errors=57` → `errors=166 files_with_errors=53`。
subset は `38 files / 204 classes / verified=204 failed=0` のままです。

### slick 残 155 件の小さな塊 4 つ（`agent/tail5`）

テストは `crates/cli/tests/tail5.rs`、fixture 接頭辞は `t5` です。

計測は `files=184 errors=155 files_with_errors=52` →
**`files=184 errors=149 files_with_errors=49`**（−6 件 / −3 ファイル）。

ブリーフの推測はどれも一部または全部が外れていました（過去のスライスと同じ
パターンです）。実際に確かめて分かった根は次の 4 つです。

#### 1. 修飾されたコンパニオンへの named arguments

`pkg1.Bar(a = 1, b = "x")`（修飾）は「unimplemented syntax: named
arguments (method parameters not resolved)」でしたが、`Bar(a = 1, b =
"x")`（非修飾）は最初から通っていました。`fun.sym` が違うのが原因です。
非修飾は `apply` メソッドそのものに解決されますが、修飾された方は
**モジュール** `Bar` に解決されます —— `rewrite_receiver_apply` は修飾
されたコンパニオン参照をわざと書き換えません（`scala.Some(1)` の codegen
がそこに依存しています）。モジュールシンボルは自分の `paramss` を持たない
ので、`first_clause_ids` は何も見つけられませんでした。

`named_arg_param_ids` に、`fun.sym` が `Module` のときはそのモジュールの
`apply` メンバーからパラメータ名を読む分岐を足しました。オーバーロードの
callee がすでにやっていることと同じです。fixture: `t5_named_qual(_bad)`。

#### 2. `override def f = ...` は戻り型を継承する

`override def run(n: Node) = n match { case Wrap(x) => run(x) ... }` は
`def run(n: Node): Any = ...` をオーバーライドしていても「recursive
method run needs result type」でした。SLS 6.1 の「オーバーライドする定義
が自分の型を書いていなければ、オーバーライドされるメンバーの型とみなす」
の通りにすると、戻り型は上書き前から分かっているはずです。何もオーバー
ライドしていない同じ形のメソッド（`t5_override_infer_bad.scala`）は、
実 scalac 2.13.16 と同じく引き続きこのエラーになります —— オーバーライド
の場合だけが間違っていました。

`type_def_sig`（`override` 修飾子が付いているときだけ）が
`overridden_ret_type` で祖先を辿り、すでに戻り型が分かっている同名・同引数
のメンバーを探して借ります。借りるのは戻り型だけで、本体は書かれた通りに
検査・推論されます。

直接の修正の裏に、孤立した再現では気付かなかった 2 つの副作用がありました
（slick 全体で計測して初めて見つかったもの）：

- **借りた型は「オーバーライドしたクラスから見た」形に読み替えないと
  いけません。** 最初の版は祖先の宣言をそのまま返していて、非ジェネリック
  なオーバーライドでは正しくても、ジェネリックだと同じ文字が違うシンボル
  を指しているだけの `type mismatch; found: T required: T` を大量に出し
  ました。`subst_as_seen_from`（`bind_found` / `type_select` が継承した
  メンバーに使うのと同じもの）で読み替えるようにしました。
- **型が「分かった」メンバーが、まだ遅延完了待ちのまま残っていました。**
  `register_typed_sig` はパース構文（`: T` が書かれているか）しか見ておらず、
  別の方法で型が確定していても関係なく `pending_sigs` に残していました。
  本体の中の**自己参照**がその途中で自分自身に `complete_lazy_sig` を
  呼んでしまい、シンボルをロックして、いままさに型付け中の本体の複製に
  もう一度 `type_def_body` を再入し、その複製の中の自己参照が今度こそ
  ロックされたシンボルを見つけて偽の循環参照を報告していました。
  `register_typed_sig` は、戻り型がすでに分かっている `DefDef` はもう遅延
  ではないと扱うようにしました。
- **`overridden_ret_type` は最初、まだ未完了の祖先候補をその場で
  `complete_lazy_sig` により強制完了させていました。** これは候補の本体
  （と、その本体がする前方参照）を、**候補の宣言ファイル自身の**トップ
  ダウンパスがまだそのファイルの本当のスコープ（import 込み）を登録して
  いない段階で走らせてしまい、その import 経由でしか見えない名前が
  「owner chain」フォールバックで解決され、でっち上げた無関係な span に
  「not found: value X」を報告していました（slick で計測すると
  `errors=155` が `errors=307` になりました。多くは `not found: value
  Capability` / `DumpInfo` が、実際には正しく import しているファイルの
  中に出るというものでした）。まだ未完了の候補は単に見つからなかったのと
  同じように読み飛ばし、その候補自身のさらに祖先へ探索を続けるだけに
  変えました。それで十分です —— 本当に該当する例はすべて、戻り型が明示的
  に書かれていて何も強制しなくてよい祖先に行き着きます。

fixture: `t5_override_infer(_bad)`。

#### 3. `recv.copy(...)` が `new C(...)` を**名前**で組み立てていた

`try_rewrite_case_copy` は `recv.copy(f = v)` を `new C(...)` に書き換え
ますが、その `new` の型の頭を裸の `Ident { name: "C" }` として組み立てて
いました —— 呼び出し元はすでに `C` の本物の `SymbolId`（レシーバの型に対
する `class_sym_of`）を持っているのに、書き換えた木を型付けするときに
**通常の字句名前解決**でもう一度 `C` を見つけさせていました。別ファイルの
継承チェーンを経由するだけで、`.copy()` を呼ぶファイル自身が単純名で import
していないクラスには、その名前がスコープにある理由がありません。これは
行・列なしの「not found: type C」でした（合成した木は本物の span を持た
ないため）。slick の `slick.jdbc.BaseResultConverter` の `override def
getDumpInfo = super.getDumpInfo.copy(...)` は `slick.util.DumpInfo` を
一度も import しておらず、まさにこれでした。

直し方は、この書き換えが合成する `Ident` にすでに分かっている `SymbolId`
から `sym` / `ty` を直接設定し、`New` を型付けするコードにそれが設定済み
のときは名前で解決し直さずそのまま使わせるようにしただけです。
fixture: `t5_case_copy_qual(_bad)`。

#### 4. SAM（リテラル `FunctionN` ではない）パラメータへの関数リテラル

`case class Builder(sql: String, setParameter: SetParameter[Unit])`
（`SetParameter[-T] extends ((T, PositionedParameters) => Unit)`）に対する
`Builder(sql, (u, pp) => ...)` は、オーバーロード採点の段階でまるっきり
マッチしませんでした。関数リテラルを callee の期待するパラメータ形に
事前型付けする仕組み（nsc の `pretypeArgs`、`agreed_lambda_params`）は
2 個以上の候補を持つ本物の `Overload` でしか動かず、`Builder(...)` は
ケースクラスが合成した `apply` 1 個だけなのでそれに当たらず、
リテラルは `(<notype>, <notype>) => <notype>` のまま採点に回りました。
仮に型付けできていたとしても、`arg_score` の関数パラメータの規則は
リテラルな `scala.FunctionN` しか認識しておらず、それを継承するだけの
トレイトは通りませんでした。slick の `SQLActionBuilder(sql, (u, pp) =>
...)` と `case class SQLActionBuilder(sql: String, setParameter:
SetParameter[Unit])` が同じ形です。

直したのは `arg_score` だけです：クラス型のパラメータが SAM 変換可能
（`SymbolTable::sam_sig`）なら、その抽象メソッドが表す関数型として比較
します。リテラルな `FunctionN` にすでにあった扱いと同じです。型が未確定
のリテラルはパラメータが空いている間はどんな関数形のパラメータにも
マッチする既存の規則があるので、採点自体が SAM を見通せれば別途の事前
型付けは不要でした。`agreed_lambda_params` の事前型付けを単一候補にも
広げる案も試しましたが、これは元に戻しました —— slick 全体で計測すると、
cats-effect の `Async[F].uncancelable[A](body: Poll[F] => F[A]): F[A]`
のような、まだ自分の型パラメータが確定していない単一候補シグネチャにも
事前型付けがかかってしまい、呼び出し自身の推論が `A` を解決する前に
間違った（未確定の）型を先に決めてしまって、`arg_score` 単独の修正より
はるかに多くの退行を起こしたためです（リテラルはどちらにせよ正しく型付け
されます —— 本当の（そしてここでは唯一の）候補が決まったあとに走る
`adapt_args_to_params` が、実際のパラメータ型に対してすべての引数を
もう一度型付けし直します）。fixture: `t5_sam_ctor(_bad)`。

`t5_sam_ctor` は `--scala-library` でのみ検証しています。`SetParameter`
は `Function2` を継承しますが、私有ランタイム（`--no-scala-library`）は
今のところ `scala.Function0` / `scala.Function1` しか出しておらず、これは
名前付き引数にもオーバーライドにも SAM 変換にも関係のない、独立した既存の
穴です（`val f: (Int, Int) => Int = (a, b) => a + b` だけの最小再現でも
`--no-scala-library` の出力が `NoClassDefFoundError: scala/Function2` で
落ちることを確認済み）。このスライスには含めず、別件として切り出しました。

#### 検証

4 本の正常系はすべて `--scala-library` / `--no-scala-library` 両方で
`-Xverify:all` を通し、real scalac 2.13.16 の標準出力とも突き合わせて
います（`t5_sam_ctor` のみ `--scala-library` だけ、上記の理由により）。
4 本とも修正前の `main` では失敗することを確認済みです。4 本の異常系は、
それぞれの直した経路が「何でも通す」ようになっていないこと（未知の
パラメータ名、オーバーライドしていない再帰、SAM パラメータへのアリティ
違反）を固定しており、real scalac 2.13.16 も同じ 4 本を拒否します。
`crates/typer/src/check.rs` と `crates/typer/src/lazysig.rs`（遅延シグネ
チャ補完の継ぎ目）に触れたので、`--test tail5 --test tail3 --test tail4
--test conform --test e2e`（553 本）と、供給の継ぎ目チェックリスト
（`--test overloadshadow --test ambigmap --test setapply --test uniteq
--test integral --test ordsummon --test mutcoll`）を前景で回しました。
全て green です。`cargo fmt --all -- --check` は差分なし、`cargo clippy
--workspace --all-targets --release` の警告は変更前後で完全に同一（新規
警告ゼロ）です。

#### 残件

「quasiquote q"..." (a hole of type `<error>` is not lifted)」（3 件、
`slick/lifted/ShapedValue.scala`）はブリーフの言う通りカスケードでした
——根はクオートやマクロとは無関係で、`slick/lifted/ShapedValue.scala:42`
の `rTag.tpe.decls.collect(...)` が `value collect is not a member of
Scopes.MemberScope` になっていることです。実 scala-reflect の
`ScopeApi`（`javap` で確認）は `scala.collection.Iterable[SymbolApi]` を
継承しているので `collect` はそこにあるはずですが、scala-rs 側のリフレク
ション API プレリュード／pickle 補完がそれを見つけられていません。
`MemberScope` は scala-rs 側で自前定義しておらず、scala-reflect の jar の
pickle をそのまま読んでいるので、これは pickle 補完（`pickle_supply.rs`
／このスライスでは触っていない領域）の別の穴です。「クオートの穴」という
症状表現は誤りで、実装すべきは quasiquote 側ではなくこの `collect` 供給
漏れです。3 件のみで影響範囲が狭く、今回は根の特定までにとどめました。

slick: `errors=155 files_with_errors=52` → `errors=149 files_with_errors=49`。
### cats-effect の summoner（`F.type`）と `$this` 補間（`agent/cats2`）

テストは `crates/cli/tests/cats2.rs`、fixture 接頭辞は `c2` です。

計測は `files=184 errors=155 files_with_errors=52` →
**`files=184 errors=151 files_with_errors=52`**（−4 件）。

ブリーフの仮説は「型射影 `A#B` のメンバ解決が根で、そこから `<notype>` /
`Any` が cats 側にカスケードしている」でしたが、**偽**でした。
`BasicBackend.scala` / `ConcurrencyControl.scala` の塊は型射影とは無関係で、
根は次の 2 つです。

#### 1. 結果型が自分自身のパラメータの `F.type` である summoner

cats-effect の型クラスはコンパニオンの summoner をこう書きます。

```scala
object Async {
  def apply[F[_]](implicit F: Async[F]): F.type = F
}
```

この `F.type` は pickle 上 `SINGLEtype` で、指しているのは**そのメソッド自身の
implicit パラメータ**です。`PickleSupply::conv` はモジュールの singleton
（`p.x.type`）しか読めなかったので、`Async$#apply` は
「unmappable result type」として**まるごと拒否**され、classfile 側の読み
—— 消去済みディスクリプタから作った `apply(x$0: Async[F]): Async[F]` ——
だけが残りました。JVM には implicit という概念が無いので `x$0` は**明示
パラメータ**であり、`adapt_implicit_apply` は implicit 節を埋めず、
`Async[F]` はメソッド型のまま。結果:

```
error: value flatMap is not a member of (Async[F])Async[F]
error: value pure is not a member of (Sync[G])Sync[G]
```

cats-core は同じ summoner を `: Applicative[F]` と書くので、`Applicative[F]`
は通り `Async[F]` は通らない、という非対称が出ていました。
（tail4 が「引数が既に `Any`/`AnyRef` なのでカスケードに見える」と書いた
`>>` の 3 件は**これとは別**で、直した後も 3 件のまま残っています。）

直しは `PickleSupply` に「そのメンバ自身のパラメータを指す `p.type` は、
そのパラメータの宣言型に widen する」1 ルールを足すこと
（`param_singletons`）。`F.type` の指す値の型は `Async[F]` なので、
summoner の結果として選択できるメンバは同じです。

もう 1 つ、`import cats.effect.Async` が実際に通る経路
—— `cats.effect` の package object の `val Async = cats.effect.kernel.Async`
—— では、モジュールクラス `Async$` は `find_or_stub_java_class` が
スタブするだけで **pickle から adopt されない**ので、`complete_named` が
そもそも配ってくれませんでした（`value flatMap is not a member of Async$`）。
`Module[T]` → `Module.apply[T]` のリダイレクトが `apply` を要求する直前に
モジュールクラスを adopt するようにしています（`Check::adopt_cp_module_class`）。
投機的に adopt はしません —— コンパニオンの adopt は全メンバを入れるので。

#### 2. 文字列補間の `$this`

`this` は識別子ではなくキーワードなので、`s"for $this"` は `${this}` と
同じく `this` という式です。`Ident` として読んでいたため項として探され、
slick の `s"No type for symbol $sym found in $this"` が
`not found: value this` になっていました
（`Type.scala` と `BasicBackend.scala` の 2 件）。

#### 検証

`c2_thisinterp.scala` は `--scala-library` と `--no-scala-library` の両方で
`-Xverify:all` を通し、real scalac 2.13.16 の標準出力と一致することを
確認しています。`c2_thisinterp_bad.scala` は、`$name` が何でも通るように
なっていないこと（`not found: value nosuchvalue`）を固定します。

`a_summoner_returning_its_own_parameters_type_crosses_a_jar` は
`F.type` を返す summoner と package object 越しの再エクスポートを持つ
小さなライブラリを**実 scalac** でコンパイルして jar に固め
（自前ライタは `SINGLEtype` を書かないので、fixture は scalac 由来でなければ
意味がない）、jar しか見えないプログラムを通して実行します。
witness の無い `TC[Crate]` は
`could not find implicit value of type TC[Crate]` のままです。

パーサと `pickle_supply.rs` の継ぎ目に触れたので `cargo test --workspace`
（`--release`）を回しました。subset は
`38 files / 204 classes / verified=204 failed=0` のままです。

#### 既知の残件

- `Plain[Box].unit` の形（コンパニオン summoner の結果から、**引数を取らない**
  メンバを選び、型引数が具象クラスのとき）で、クラスの型パラメータが
  as-seen-from されず `F[Unit]` のまま返る。実 cats では再現せず
  （`cats.Applicative[G].unit` は通る）、slick にも現れないので今回の対象外。
- `def apply[F[_]](implicit F: TD[F]): F.type` を**ソース側**で書くと
  `type mismatch; found: F[Int] required: F[Int]` になる。上の直しは pickle
  経路だけで、ソースの `F.type` は別経路。
- 同じファイルに `Async` を明示 implicit パラメータで受け取る定義があると
  `Functor[F].map(x)(f)` が
  `no matching overload for (F[A])((A) => B)F[B]` になる（修正前の `main` でも
  同じ）。cats 側の完了順に依存する別の穴。
- `slick.cats` パッケージがあるせいで `slick.dbio` 内の `cats.effect.IO` が
  `value effect is not a member of <notype>` になる 2 件。根は特定済みで、
  `Check::expose_unqualified` が「囲っているパッケージを owner チェーンで
  全部辿る」ことです。nsc はそうしません: **修飾付きのパッケージ句**
  `package p.q` からは `p` のクラスもサブパッケージも見えず
  （2.13.16 は `-Xsource:3` の有無に関わらず `not found: type Widget` /
  `not found: value cats`）、入れ子の `package p { package q { … } }` からは
  両方見えます。ファイルのパッケージ句が開いたパッケージ（`PackageDef`
  1 つにつき 1 個）だけを辿るように直すと該当 2 件は消えますが、
  今度は `package slick.jdbc` からの**修飾**参照 `slick.ControlsConfig` が
  解決しなくなって差し引き +1 件になったので、今回は戻しました。
  規則自体は正しいので、緩い読みに寄りかかっている別の箇所と一緒に
  ほどく必要があります。
  → `agent/proj` で解決（「型射影 `A#B` のメンバ再読み込みと、`package` 句が
  開くもの」節）。寄りかかっていたのは**デフォルト引数の右辺が呼び出し側の
  スコープで型付けされること**でした。
- tail4 が残した `value database is not a member of BasicBackend.Session`
  （型射影のメンバ再読み込み）も手つかず。
  → `agent/proj` で解決。tail4 の診断は当たっていました。
- cats の `>>`（`no matching overload for (=> F[B])(FlatMap[F])F[B]`）3 件は
  before/after とも 3 件。`Async` / `Deferred` が潰れていたせいではなく、
  `decrementDepth >> releaseIfUnpinned >> …` の左辺が `Any` / `AnyRef` に
  落ちる別の原因（`BasicBackend.scala` は 6 → 5 件、
  `ConcurrencyControl.scala` も 6 → 5 件）。
### slick に残る `type mismatch` 11 件の 8 つの根（`agent/mismatch13`）

テストは `crates/cli/tests/mismatch13.rs`、fixture 接頭辞は `mism13` です。

計測は `files=184 errors=155 files_with_errors=52` →
**`files=184 errors=141 files_with_errors=48`**（−14 件 / −4 ファイル）。
`tests/slick_subset.sh` は `38 files / 204 classes / verified=204 failed=0` の
ままです。`type mismatch` は **11 件 → 2 件**で、残る 2 件はどちらも
`type mismatch` 以外のエラーのカスケードです（末尾「残っているもの」）。

| 塊 | before | after |
|---|---|---|
| `found: Tuple2[T, T2] required: (((T, T2), T2), T2)` ほか `ShapedValue.zip` | 2 件 | **0 件** |
| `found: DBIOAction[R, S, E with Effect] required: DBIOAction[Any, NoStream, Effect]` ほか | 2 件 | **0 件** |
| `found: P required: Rep[Option[QO]]`（`ExtensionMethods.flatten`） | 1 件 | **0 件** |
| `found: Product required: Option[Option[Any]]`（`SQLiteProfile`） | 1 件 | **0 件** |
| `found: Query[G, T, U] required: Query[G, T, C]`（`Query.zipWith`） | 1 件 | **0 件** |
| `found: State[_] required: State[F]`（`ConcurrencyControl.create`） | 1 件 | **0 件** |
| `found: <overload String | <error>> required: String`（`Node.toString`） | 1 件 | **0 件** |
| `not found: type DumpInfo` / `no matching overload for (…)DumpInfo` | 3 件 | **0 件** |
| `no implicit: could not find implicit value of type <:<[…]` | 1 件 | **0 件** |
| `not found: type Mapper` | 1 件 | **0 件** |

引き継いだ診断のうち検証できたのは 1 つだけでした。`tail4` が残した
「`lub` が交差型を組まないので `found: Product required: Option[Option[Any]]`
が 1 件残る」は**場所は合っていて理由が違い**、交差型は要らず、`lub` が
base type sequence の**先頭（＝その型自身）**を見ていなかっただけです（4）。
`JdbcActionComponent` の `E with Effect` は交差型の問題ではなく**ラムダの
結果型の中の変数**（3）、`Query.scala` / `RelationalProfile.scala` /
`Node.scala:636` / `ConcurrencyControl.scala:202` はそれぞれ別の根でした。

#### 1. 置換を 3 回かけていた（`new C[…]` が自分自身のクラスのとき）

`pick_ctor_at`（`crates/typer/src/check.rs`）は、適用可能性を見るために
`flatten` で 1 回、`resolve_overload` から返ってきた結果にもう 1 回、
`subst_tparams(class_id, targs, …)` をかけていました。さらに `new` の側でも
`p = subst_tparams(c, &inferred_args, &p)` で 3 回目です。型引数が
**置換される当の型パラメータを含まない**限りこれは冪等なので、これまで誰も
気づきませんでした。`ShapedValue[T, U]` の中の
`new ShapedValue[(T, T2), (U, U2)](…)` はまさにそれを含みます:
`T` は `(T, T2)` → `((T, T2), T2)` → `(((T, T2), T2), T2)` になり、
`found: Tuple2[T, T2]  required: (((T, T2), T2), T2)` でした。

`pick_ctor_at` の契約を「返す引数型・結果型は `targs` で読んだもの、ちょうど
1 回」に決めました。候補が 1 つのときは `flatten` の結果がそのまま返るので
出口では置換せず、2 つ以上のときだけ置換します（`resolve_overload` は
`Type::Overload` のときだけ候補をシンボルから読み直すため）。`extends` 側と
`new` 側の呼び出し元は、その分の再置換をやめました。

#### 2. `<:<` は implicit の**ビュー**（typer と codegen の両方）

nsc は「候補の型が `From => To` に適合するか」を見るので、`Function1` を
**継承しているクラス**の値もビューです。`scala.<:<` はまさに
`sealed abstract class <:<[-From, +To] extends (From => To)` で、slick の

```scala
def flatten[QO](implicit ev: P <:< Rep[Option[QO]]): Rep[Option[QO]] =
  flatMap[QO](identity(_))
```

（`lifted/ExtensionMethods.scala:210`）はこれだけに頼っています。
`conversion_provides`（`crates/typer/src/implicits.rs`）は構造的な
`Type::Function` と 1 引数メソッドしか見ていなかったので、`Ext` の中の
`r: P` も `identity(_)` の結果も `Rep[Option[QO]]` になれませんでした。
`view_shape` を切り出し、クラス型のときは base type sequence から
`FunctionN` の形を拾います。**引数を取らない** implicit メソッドはビューに
しません（`<:<.refl[A]: A =:= A` が全部の型を自分自身に変換してしまいます）。

codegen 側にも穴がありました。ビューの適用は `Apply { fun: <ev への参照>,
args: [x] }` という木になりますが、`gen_apply`（`crates/backend/src/gen.rs`）は
`fun.ty` が構造的な `Type::Function` のときしか `FunctionN.apply` を出さず、
それ以外は `invoke_method(fun.sym)` に落ちます。`ev` はメソッドではなく
**値**なので、囲んでいる**メソッド**のメンバ呼び出しが出て
`NoClassDefFoundError: direct` になっていました（型検査は通ったうえで）。
`fun.sym` がメソッドでなく、その型のクラスが `FunctionN` を継承していて
引数の数が合うときも `gen_function_apply` に回します。

#### 3. ラムダの**結果**の中にしか出ない型変数

`def h[B](f: Int => Bx[B]): Bx[B]` の `B` は、ラムダの本体だけが決められます。
`p` がちょうど `Type::Function { ret: TypeParam }` のときは既に `Any` に
緩めていましたが、`Bx[B]` のように**1 段中**に入ると `open_to_bounds` が
境界へ開いて `Bx[Any]` になり、不変な `Bx[Int]` はそれに適合しません ——
2 回目の推論が本体から `B` を読む前に、引数が落ちていました。

引数を型付けするときの**期待型だけ**を緩めます。境界ではなく
`Type::Wildcard`（`is_sub_type` が「まだ決まっていない」として扱える形。
`open_to_bounds` が高階パラメータに対して既に使っています）を入れるので、
本体には「`Bx` でなければならない」ことは伝わったままです。`p` 自身は宣言の
ままなので、`solve_open_from_arg` が型付け済みの引数から `B` を読みます。
ワイルドカードがラムダの型に残ると呼び出しの結果まで運ばれる
（`Act[_, _, Effect with _]`）ので、本体の型で貼り直す既存の後始末を
ワイルドカードを含む場合にも広げました。slick の
`DBIOAction.flatMap[R2, S2, E2](f: R => DBIOAction[R2, S2, E2])` がこの形です。

#### 4. `lub` が base type sequence の**先頭**を見ていなかった

`agent/tail4` が「両方の列が同じクラスで出会ったら型引数を join して止まる」
ようにしましたが、`base_type_seq` は**その型自身**を返しません（SLS 3.5.2 では
先頭にあります）。`lub(Some[X], Option[Y])` は 2 つ目の列に `Option` を
見つけられず、`Option[X]` を素通りして `Option` 自身の親 `Product` に着地して
いました。両側の列の先頭にその型を足すだけで、`Option[Option[Any]]` に
なります。`tail4` の記録は「交差型を組んでいないから」でしたが、
`Option[X] with Product with Serializable` は要りません。

#### 5. 継承したメンバは**宣言したクラス**の型パラメータで読む

`type_select`（`crates/typer/src/check.rs`）は `subst_as_seen_from` で親を
たどって正しく読んだあと、**受け手自身の型引数**でもう一度
`subst_tparams(owner, recv_args, …)` をかけていました。位置が揃うのは受け手が
その宣言クラス自身のときだけです。slick の
`BaseJoinQuery[E1, E2, U1, U2, C, B1, B2] <: Query[+E, U, C[_]]` では
`Query` の 3 つのパラメータに join の先頭 3 引数が入り、`Query.map` の
`Query[G, T, C]` が `Query[G, T, U1]` になっていました
（`Query.zipWith`）。しかも 2 回目が効くのは **1 回目が恒等だったとき**
—— `stdJoin` が囲んでいるクラス自身の `C` を書くので `C := C` ——
なので、同じ形を小さく書いても再現しません。クラス型の受け手では
2 回目をやめました（タプル / 関数型は `subst_as_seen_from` が歩けないので
位置による置換を残します）。`extends`・`new` 側も同様です。

#### 6. 明示した型引数は、その引数の**期待型**

`proto_arg_type` は「パラメータがちょうど裸の型パラメータ」のときだけ期待型を
作っていました。`Ref.of[F, State[F]](State(max, min, TreeMap.empty))`
（`basic/ConcurrencyControl.scala:202`）では `[F, State[F]]` を明示しているので
パラメータは既に `State[F]` です。それを渡さないと `State(…)` は期待型なしで
型付けされ、`case class State[F[_]]` の**高階の `F`** はどの引数にも現れない
ので決まらず、`State[_]` になっていました。型引数で決まりきったパラメータは
そのまま期待型として渡します。

#### 7. `copy` の書き換え先を**名前**で綴っていた

`copy(x = 1)` は `{ val t = recv; new C(t.a, 1, …) }` に書き換えられますが、
その `new C` の `C` を**名前の `Ident`**で作っていたので、書き換えが走った
ファイルのスコープで解決されます。slick の

```scala
override def getDumpInfo = super.getDumpInfo.copy(mainInfo = s"idx=$index")
```

（`jdbc/JdbcResultConverter.scala` / `memory/MemoryQueryingProfile.scala`）は
`DumpInfo` を継承したメンバ経由でしか知らず import していないので、
**位置の無い** `not found: type DumpInfo` が出ていました。`crate::materialize`
が既に持っている「解決済みの型」マーカー（nsc の `TypeTree(tp)`）を使って
シンボルそのものを載せます。`tests/multi/mism13_*.scala` の 3 ファイルが
これを再現します。

#### 8. `if` / `match` の分岐の join

`Node.getDumpInfo`（`ast/Node.scala`）の

```scala
val ch = this match {
  case Path(_ :: _ :: _) if !GlobalConfig.dumpPaths => Vector.empty
  case _                                            => childNames.zip(children.toSeq).toVector
}
```

は `Vector[A]`（`Vector.empty` の決まっていない `A`）と
`Vector[(String, Node)]` の join で、引数の join が `AnyRef` まで歩いて
`Vector[AnyRef]` になり、`DumpInfo(…, ch)` が通らず `getDumpInfo` の推論型が
エラーになり、それが `override final def toString` をエラーにし、最後に
`n.toString` が `found: <overload String | <error>>  required: String`
（`Node.scala:636`）でした —— 4 段のカスケードです。

nsc の `solve` は何も縛らなかった変数をその境界で読むので、
`Vector[Nothing]` は `Vector[(String, Node)]` です。`lub_branches` は
join の前にそれをやりますが、条件を 3 つ付けて**取りこぼしだけ**を閉じます:
その型パラメータが**このスコープから名前で引けない**こと（囲んでいる
`def f[T]` の `T` は引けるので開いたまま）、もう一方の分岐がそれを含まない
こと、共変位置にあること。そのうえで**答えは必ず 2 つの分岐型のどちらか**
です（閉じた結果が他方の部分型になったときだけ他方を返し、そうでなければ
従来どおりの `lub`）。`Option.getOrElse` の `[B >: A]` を pickle から読めて
いない別の穴があり、join がより正確になったことでそれが表に出たのを、この
最後の条件が抑えています。

#### 検証

`mism13_lang.scala` は `--scala-library` と `--no-scala-library` の両方で
`-Xverify:all` を通し、real scalac 2.13.16 の標準出力とも突き合わせます
（`expected/mism13_lang.txt`）。`mism13_lib.scala` は `<:<` が jar 側にしか
無いので library モードのみ、私有ランタイムでは `not found: type <:<` を
出します。`mism13_bad.scala` は 6 件を拒否し、nsc 2.13.16 も同じ 6 件を
出します。13 本のテストのうち 10 本は**修正前の `main` で落ちる**ことを
確認済みです。`--release` で `mismatch13` / `mismatch12` / `tail4` /
`buildfrom2` / `conform` / `e2e` / `multifile` と `cargo test --workspace` を
回しました。`cargo clippy --workspace --all-targets` の警告は 70 件のまま
（行番号だけがずれる）で、新規はありません。

**残っているもの**（このスライスでは直していない）:

* `slick/compiler/MergeToComprehensions.scala:218` の
  `found: Some[Tuple2[TableNode, ConstArray[T]]]`。根は 3 行上の
  `tableFields.getOrElse(t.identity, Seq.empty)` が出す
  `no matching overload for (Any, => Vector[TermSymbol])Vector[TermSymbol]`
  です。`Map.getOrElse[V1 >: V](key: K, default: => V1): V1` を
  `prelude_coll.rs` が `V` で単相にモデル化しているので、`Seq.empty` を
  受けられません。5 行で再現します:
  `val m: Map[String, Vector[Int]] = Map.empty; m.getOrElse("k", Seq.empty)`。
* `slick/relational/RelationalProfile.scala:72` の `found: C required:
  CompiledFunction[…]`。同じ行の
  `no implicit: could not find implicit value of type TypedType[Boolean]`
  （`Library.==.column[Boolean](…)`）の下流で、`Compiled.apply[V, C <:
  Compiled[V]](raw: V)(implicit compilable: Compilable[V, C], …): C` の `C` は
  witness だけが決められます。implicit 側が先です。
* `Option.getOrElse[B >: A](default: => B): B` の `B` を pickle から読めて
  いません（シグネチャが `(=> A)A` になり、`Option(1).getOrElse("x")` が
  `no matching overload` です）。ソースで書いた同じ形
  （`def orElseN[B >: A](d: => B): B`）は通るので、穴は unpickler 側です。
* `Ext[P].flatten` を**型引数なしで**呼ぶと `<:<` の witness から `QO` を
  解けません（`e.flatten` は駄目、`e.flatten[Int]` は通る）。`Rp[Option[QO]]`
  のように呼び出し側の型パラメータが**入れ子**にあるときの
  `implicit_solve` の穴で、slick 本体はこの形で呼んでいません。

### slick の `no matching overload` 49 件のうち 14 件（`agent/ovl3`）

テストは `crates/cli/tests/ovl3.rs`、fixture 接頭辞は `o3` です。

計測は `files=184 errors=134 files_with_errors=48` →
**`files=184 errors=120 files_with_errors=41`**（−14 件 / −7 ファイル）。
`no matching overload` は **49 件 → 35 件**になりました。
`tests/slick_subset.sh` は `38 files / 204 classes / verified=204 failed=0`
のままです（`crates/backend` は触っていないので、この数字は
`StringBuilder` のコンストラクタを足す前の 1 回だけ実測しています)。

`no matching overload` は「候補が複数あって選べない」ときのメッセージでは
ありません。**候補が 1 本しか無くても**、その 1 本が引数を受け付けなければ
同じ文が出ます。つまり *prelude が単相にモデル化したシグネチャ* が
「多重定義が足りない」ように見えていた、というのがこの塊の正体でした。
表面の 49 件は 5 つの根に落ちます（うち 1 つは単に足りていない
コンストラクタでした）。

| 根 | before | after |
|---|---|---|
| `Option.getOrElse` / `orElse` / `Map.getOrElse` の `[B >: A]` 欠落 | 7 件 | **0 件** |
| `mutable.HashSet` / `HashMap` が `collection.Set` / `Map` でなかった | 4 件 | **0 件** |
| pickle にしか無い view（`Option.option2Iterable`）が読まれていなかった | 1 件 | **0 件** |
| 同一シグネチャの prelude 宣言と pickle 宣言が `ambiguous` になっていた | 0 件 | **0 件**（2 の副作用として出たものを解消） |
| `new StringBuilder(Int, String)` が prelude に無かった | 1 件 | **0 件** |

#### 1. `[B >: A]` の欠落（`crates/typer/src/prelude_ovl3.rs`）

nsc は `def getOrElse[B >: A](default: => B): B` です。prelude は
`prelude_either.rs` で `(=> A)A`、`prelude_coll.rs` で
`getOrElse[V1 >: V]` を「`V` で単相にモデル化」（コメントにそう書いてある）
していました。だから `(o: Option[Sub]).getOrElse(base)` は
`no matching overload for (=> Sub)Sub with arguments (Base)` でした。

`Typer::infer_method_tparams_in` は既に「引数から解いた型と下限の lub を
取る」（`prelude_lowbound.rs` が `List.::` でそれを使っている）ので、
**下限を宣言するだけ**が修正の全部です。`B` / `V1` は型パラメータなので
erasure は変わらず、私有ランタイム・実 jar のどちらの ABI にも影響しません。
対象は `Option.getOrElse` / `Option.orElse` /
`immutable.Map.getOrElse` / `mutable.Map.getOrElse`。

slick 側の該当箇所は `EmulateOuterJoins.scala:78`、`CreateAggregates.scala:54`、
`MergeToComprehensions.scala:215`、`H2Profile.scala:71`、
`MySQLProfile.scala:94`、`SQLServerProfile.scala:112`、
`JdbcModelBuilder.scala:253` の 7 件です（`mismatch13` が残件として挙げていた
`m.getOrElse("k", Seq.empty)` もこれです）。

#### 2. `mutable.HashSet` / `HashMap` の親（`prelude_ovl3::install_hierarchy`）

`prelude_hier.rs` の辺の表に `mutable/Set` → `collection/Set` はあっても、
`mutable/HashSet` → `mutable/Set` がありませんでした。`add_hash_set` /
`add_hash_map`（prelude.rs）が `&[Type::AnyRef]` で作ったままだったからです。
slick は `mutable.HashSet.empty[TypeSymbol]` を
`def containsSymbol(tss: scala.collection.Set[TypeSymbol])` に渡すので、
`Util.scala:72` / `ExpandSums.scala:323` / `ExpandTables.scala:73` /
`ExpandTables.scala:82` が落ちていました。`LinkedHashSet` / `LinkedHashMap`
も同じ形なので一緒に入れています。

#### 3. 同一シグネチャの重複（`resolve_overload`、`crates/typer/src/check.rs`）

2 の辺を入れた副作用で、`mutable.HashMap` が `getOrElse` を**2 経路**で
見るようになりました――prelude の `mutable.Map` 宣言と、jar から取り込んだ
`collection.MapOps` の pickle 宣言です。どちらも
`(K, => V1)V1` で、nsc なら 1 個のシンボルです。
`resolve_overload` には既に「同じシグネチャは 1 つの候補」という
`winners.dedup_by(|a, b| a.1 == b.1 && a.2 == b.2)` がありましたが、
2 つの `V1` は**別のシンボル**なので `==` が成立しませんでした。
片方の型パラメータをもう片方に読み替えてから比べる
（`canonical_sig`）ようにして、意図どおりの比較にしています。
先に来た候補（＝レシーバに近い方）を残すので、辺を入れる前と同じ側が
選ばれます。

#### 4. pickle にしか無い view を読むタイミング（`check.rs`）

`Seq("a") ++ anOption` は `option2Iterable` が要ります。この implicit は
prelude には無く、`warm_pickled_implicits` が pickle から供給します
（コメントに `Option.option2Iterable` と名指しで書いてある）。ところが
適用可能性の判定（`arg_conforms` → `search_conversion`）は `&self` で走るので
**classfile を読めません**。結果、同じファイルの先行行が `Option` のメンバを
選択して（`search_extension` 経由で）暖めていたときだけ通る、という
再現性の無い挙動になっていました。

`resolve_overload` が `None` を返したときと、`adapt` が変換探索に降りるとき
――どちらも既に失敗が確定している場所――で、**その型自身の**コンパニオンを
暖めてから一度だけ問い直します。クラスごとに 1 回だけなので費用は有界です。

基底クラスまで暖めないのは意図的です。コンパニオンの pickle を読むと
そのコンパニオンの pickled parent が張られ、コレクションではそれが
`IterableFactory.Delegate` などの**prelude が手で書いている**ファクトリ系に
なります。`mutable.Set[T]` の実装型スコープを全部暖めたら `Iterable$` /
`Seq$` / `Set$` に `Delegate` が付き、その `apply[A](A*): CC[A]` が prelude の
`apply` と並んで、`mutable.Set[TypeSymbol]()` が `Set[A]` になりました
（実測でこの退行が出たので、型自身のクラスで止めています）。

#### 5. `new StringBuilder(initCapacity, initValue)`

`prelude_text.rs` のコンストラクタ表は `()` / `(Int)` / `(String)` だけで、
`TableDump.scala:50` の `new StringBuilder(s.length, "")` が落ちていました。
`library_abi` 限定です ── `--no-scala-library` では
`scala.collection.mutable.StringBuilder` は `java.lang.StringBuilder` に
落ちるので、`(int, String)` コンストラクタがそもそも存在しません。

#### 検証

`o3.scala` は `--scala-library` と `--no-scala-library` の両方で
`-Xverify:all` を通し、real scalac 2.13.16 の標準出力とも突き合わせます
（`expected/o3.txt`）。`o3_lib.scala` は `mutable.HashSet` /
`collection.Set` のメンバが jar 側にしか無いので library モードのみで、
私有ランタイムでは `value size is not a member of Set[String]` を出します
（`expected/o3_lib.txt`）。どちらも `mutable.HashMap` の `getOrElse` を
実際に**走らせる**ので、3 の重複解消が別のシンボルを選んで壊していたら
`-Xverify:all` か出力比較で落ちます。`o3_bad.scala` は
`Option[Int].getOrElse("no")` が `Any` になることを見ます（lub まで広がる
だけで、`Int` にはならない）。nsc 2.13.16 も同じ行を拒否します。
7 本のテストのうち 6 本は**修正前の `main` で落ちる**ことを確認済みです
（残る 1 本は `--no-scala-library` での診断を見る否定テストで、修正前も
コンパイルは失敗します）。
`--release` で `overloadshadow` / `ambigmap` / `setapply` / `uniteq` /
`integral` / `ordsummon` / `mutcoll` / `conform` / `ovl2` / `ovl3` /
`mismatch13` / `buildfrom2` / `lowbound` / `e2e` と
`cargo test --workspace` を回しました。`cargo clippy --workspace
--all-targets` の警告（78 件）はどれもこのスライスが触っていない場所で、
新規はありません。

**残っているもの**（`no matching overload` 35 件、このスライスでは直して
いない）:

* `java.util.Arrays.copyOf[Any](a: Array[AnyRef], n)`（`ConstArray.scala:314`
  / `516`、2 件）。nsc は Java シグネチャ中の `Object` を `ObjectTpeJava` と
  して読み、`Any` とも `AnyRef` とも適合させるので**呼び出しは通り**、
  結果を `Array[Any]` に代入するところだけが
  `found: Array[Any] required: Array[Any]` になります（scalac で確認済み）。
  こちらは `Array` の不変性でそのまま落ちています。
* `Array[T]` → `IterableOnce[T]` の暗黙変換（`Predef.genericWrapArray` /
  `wrapRefArray`）が view として登録されていない。`Map() ++ anArray`
  (`JdbcTypesComponent.scala:526`)、`TupleSupport.buildTuple(anArray)`
  (`ResultConverter.scala:58`)、`val xs: IndexedSeq[Any] = anArray` の 3 形。
  backend 側には `emit_array_wrap_to_iterable_ops` が既にあります。
* `Set() ++ anOption` (`JdbcModelBuilder.scala:280`)。1 の続きで、
  `Set.++` / `Seq.++` の `[B >: A]` も単相のままです。ここは
  `prelude_buildfrom` と噛み合うので別スライス向き。
* `RefId[E <: AnyRef]` は不変なので `errors.contains(RefId(n1))` は
  **期待型から `RefId.apply` の `E` を決める**必要があります
  (`VerifyTypes.scala:38` / `41`)。引数を期待型なしで型付けしてから
  多重定義を選ぶ順序に手を入れる話で、影響範囲が広いので触っていません。
* `allTSyms -- referenced.map(_._1)`（`PruneProjections.scala:14`）。
  `.toSet` が返す `immutable.HashSet` に pickle から載った `map` の型
  パラメータが解けず、引数が `HashSet[A]` のままになります。
* `ConfigFactory` の `c.root.asScala`（`GlobalConfig.scala:71` / `78`、2 件）。
  `ConfigObject` が実装している `java.util.Map<String, ConfigValue>` の
  型引数を読めておらず、`Map[AnyRef, AnyRef]` になります。
* `expansions(tsym)` / `expansions contains tsym`（`ExpandTables.scala:25`）。
  `scala.collection.Map` は `prelude_hier` の LINKS が作る**メンバの無い**
  スタブで、`apply` / `contains` は jar の pickle 頼みです。
* cats の `>>`（`BasicBackend.scala:329` / `432` / `434`、3 件）と
  `DBIOAction.scala` の `<:<` を `Function1` として渡す 3 件は未調査です。
### 引数の中の関数リテラル・基底型・Java の `Object`（`agent/mismatch14`）

テストは `crates/cli/tests/mismatch14.rs`、fixture 接頭辞は `mism14` です。

計測は `files=184 errors=115 files_with_errors=41` →
**`files=184 errors=106 files_with_errors=41`**（−9 件）。

`agent/ovl3` が根拠付きで残した 2 つ（`Arrays.copyOf[Any]` 2 件、
`ConfigObject.asScala` 2 件）と、`type mismatch` の `Node.Self` 2 件、
`JdbcBackend` の `(Statement) => Unit` 2 件（＋道連れの
`missing parameter type for expanded function` 1 件）が、次の 4 つの根に
落ちました。消えた 9 件以外に**増えた診断はありません**（差分は無名クラスの
連番だけ）。

| 根 | before | after |
|---|---|---|
| 単相の callee が引数に期待型を渡していなかった | 3 件 | **0 件** |
| 変換の型引数を受け手の**基底型**から解いていなかった | 2 件 | **0 件** |
| Java の型引数 `Any` を `scala.Any` として代入していた | 2 件 | **0 件** |
| 継承した結果型の抽象型メンバをサブクラスで読み直していなかった | 2 件 | **0 件** |

#### 1. 引数の中の関数リテラルに期待型が届いていなかった

```scala
def take(f: Statement => Unit): Int = 1
take(if (cond) { s => si(s) } else { s => si(s); si(s) })
```

`else` 側の `s` が `<notype>` のまま `si(s)` を呼び、
`no matching overload for (Statement)Unit with arguments (<notype>)` が
2 本（本体の文の数だけ）出ていました。`then` 側が通っていたのは**偶然**で、
`section_param_types` が「本体が呼び出し 1 個ならその callee の署名から
パラメータ型を拾う」規則を持っているからです。2 文の本体にはその拾い先が
ありません。

真犯人は `Typer::proto_arg_type` の先頭にあった
「型パラメータを持たない callee には prototype を出さない」でした。nsc は
すべての引数をパラメータ型に対して型付けします（`Typers.typedArg`）。
ただし丸ごと真似すると影響が大きいので、**関数型・`FunctionN`・SAM のいずれかで、
しかも型パラメータもワイルドカードも含まない**パラメータに限りました。
まだ解けていない型パラメータを prototype にしてはいけないのは
`agreed_lambda_params` のコメントが実測付きで書いているとおりです
（cats の `uncancelable[A]` を先に固定して slick が 155→232 になった件）。

同じ穴が 2 つの別経路にもありました。

* **多重定義**（`Type::Overload`）: 全候補が同じ関数型パラメータを要求する
  ときだけ prototype を出します（`agreed_function_param`）。
* **コンストラクタ**（`new C(…)`、および `C(…)` から回るケース）:
  クラスに型パラメータが無く、アリティが一次コンストラクタと一致するときだけ、
  そのフィールド型を prototype にします。
* **コンパニオンの `apply`**: `rewrite_receiver_apply` は
  `Obj(args)` を `Obj.apply(args)` に**書き換えません**（codegen の都合。
  `named_arg_param_ids` のコメント参照）。したがって callee の型は
  `Type::ModuleRef` で、パラメータはその `apply` にあります。ここで
  `AbstractFunctionN.apply` を**継承**しているのを忘れると候補が
  `(String, (Statement) => Unit, Int)SP` と `(T1, T2, T3)R` になって
  「全候補が一致」に届かないので、モジュールクラス越しの as-seen-from を
  通してから比べます。slick の
  `JdbcBackend.StatementParameters(…, if (…) … else { s => …; … }, …)` が
  ちょうどこの形です。

prototype は**制約ではなくヒント**です。メソッド経路が既にそうしているように、
prototype 付きで型付けした引数が文句を言った（あるいは結果がパラメータに
適合しなかった）ら、診断ごと捨てて prototype 無しで型付けし直します。
コンストラクタ経路にこの巻き戻しを入れ忘れると、slick の
`new StructValue(…, xs.toMap)` が新しく落ちました:
`StructValue` の第 2 パラメータは `TermSymbol => Int` で、`Map <: Function1`
を通して `toMap` の `K` / `V` を解く経路が無いからです。prototype 無しで
型付ければ `Map[TermSymbol, Int]` で、そのまま適合します。

#### 2. 変換の型引数は受け手の基底型から解く

`Typer::conv_targs` は変換のパラメータと受け手を**同じ位置どうし**で
突き合わせていました。`java.util.Map[K, V]` に対して受け手が
`ConfigObject`（型引数を 1 つも持たない）だと zip する相手が無く、
`K` も `V` も `AnyRef` に落ちます。`base_type_instance` で
`java.util.Map[String, ConfigValue]` に直してから解きます。純 Scala でも
同じことが起きていました（`class Sub extends Base[String, Int]` に対する
`implicit class Ops[A, B](b: Base[A, B])` の `sub.firstOf` が `AnyRef`）。

#### 3. Java の型パラメータに書いた `Any` は `Object`

nsc は Java シグネチャ中の `Object` を `ObjectTpeJava` として読みます。
`<T> T[] copyOf(T[], int)` を `copyOf[Any](…)` と呼ぶと `T` は
`scala.Any` ではなく `Object` に固定されるので、`Array[AnyRef]` は通り、
結果を `Array[Any]` に代入するところだけが落ちます（実 scalac で確認:
`found: Array[Any] required: Array[Any]` という一見不可解な文面は、
`found` 側が `Array[Object]` だからです）。scala-rs は `TypeApply` で
明示された `Any` を、`JAVA` フラグの立った callee の `Object` 上限の
型パラメータに限って `AnyRef` に読み替えます（`java_object_targs`）。
`Array` の不変性は変わらないので `copyOf[Any](Array[String], 3)` は
拒否したままです（`mism14_bad.scala`）。

#### 4. 継承した結果型の抽象型メンバ

```scala
trait Node { type Self >: this.type <: Node; def rebuild(ch: …): Self }
case class StructNode(…) extends Node {
  type Self = StructNode
  override def rebuild(ch: …) = StructNode(…)   // found: StructNode required: Node.Self
}
```

結果型を書かないオーバーライドは `overridden_ret_type` が親の宣言から
取ってきますが、`subst_as_seen_from` が置換するのは型**パラメータ**だけで、
抽象型**メンバ**はそのままでした。nsc は宣言を `StructNode.this.type` から
見るので `Self` は `StructNode` です。取ってきた結果型の型メンバを、
その名前でサブクラス自身が持っている具象エイリアスに置き換えます
（`own_type_members`）。slick の `ast/Node.scala` の `StructNode` /
`Filter` がこれです。

#### 検証

`mism14.scala` は `--scala-library` と `--no-scala-library` の両方で
`-Xverify:all` を通し、real scalac 2.13.16 の標準出力とも突き合わせます。
`mism14_lib.scala` は `scala.jdk.CollectionConverters` が jar 側にしか
無いので library モードのみで、私有ランタイムでは
`value asScala is not a member of Names` を出します。`mism14_bad.scala` は
`Array` の不変性が残っていることを、診断文面ごと固定します
（修正前の文面は `(Array[Any], Int)Array[Any]` だったので、この否定テストも
修正前の `main` では落ちます）。7 本すべて修正前の `main` で落ちることを
確認済みです。`cargo test --workspace` に加えて、`--release` で
`overloadshadow` / `ambigmap` / `setapply` / `uniteq` / `integral` /
`ordsummon` / `mutcoll` / `conform` / `e2e` / `mismatch14` / `ovl3` /
`mismatch13` / `buildfrom2` を回しています。

**残っているもの**（このスライスでは直していない）:

* `RelationalProfile.scala:82` の `missing parameter type for expanded
  function` は別件です（`mp.genericFastPath { … }` の側）。
* `agent/ovl3` が挙げた残件のうち、`Array[T]` → `IterableOnce[T]` の view、
  `Set.++` / `Seq.++` の `[B >: A]`、`RefId[E <: AnyRef]` を期待型から解く件、
  `collection.Map` のメンバ無しスタブ、cats の `>>` は手つかずです。

### 型射影 `A#B` のメンバ再読み込みと、`package` 句が開くもの（`agent/proj`）

テストは `crates/cli/tests/proj.rs`、fixture 接頭辞は `pj` です。

計測は `files=184 errors=134 files_with_errors=48` →
**`files=184 errors=129 files_with_errors=48`**（−5 件）。
`tests/slick_subset.sh`（着手時 `38 files / 204 classes / verified=204
failed=0`）は**回していません**: このスライスは typer 側で、backend の変更は
`pickle.rs` の 1 箇所（射影の as-seen-from ビューを素の親として書く）だけです。
subset の検証は `Class.forName(initialize=false)` によるバイトコード検証で、
`ScalaSignature` を読まないので、pickle の変更を検出できません。代わりに
下の「検証」のとおり `jarpickle` / `e2e` / `multifile` を回し、classfile の
バイト差分で pickle 側の効果を直接見ています。

`agent/tail4` の診断（「`HeapBackend#BasicActionContext` の `session` を射影先の
prefix で読み直せていない」）は**当たっていました**。`agent/cats2` の診断
（「`value effect is not a member of <notype>` の根は `expose_unqualified` が
囲いパッケージを owner チェーンで全部辿ること」）も**当たっていました**。
外れていたのは「その規則を入れると `slick.ControlsConfig` が解決しなくなる」の
**理由**の方で、パッケージ解決の話ではありませんでした（下の 2 を参照）。

#### 1. `A#B` は前置型を落としていた

`project_from_prefix`（`crates/typer/src/check.rs`）は射影に素の
`Type::Class` で答えます。`Type::Class` に前置型を書く場所はないので、
`A#B` を作った瞬間に「`A` 越しに見ている」という事実が消え、あとの選択は
**`B` の owner の宣言**でメンバーを読みます。slick の

```scala
def run(ctx: HeapBackend#BasicActionContext): R = f(ctx.session)
```

で `session: Session` を宣言しているのは `BasicBackend` で、そこでは
`type Session >: Null <: BasicSessionDef` は抽象です。`type Session =
HeapSessionDef` と言っているのは `HeapBackend` の方なので、結果は
`value database is not a member of BasicBackend.Session` でした。

直しは、射影が**前置型の決めたぶんだけ**を型だけの refinement として
持ち歩くこと（`Checker::projected_class_type` /
`projection_refinements`）。`B` の**字句上の**囲いクラス（とその祖先）が
抽象のままにしている型メンバーの名前を集め、前置型のクラスがそれに具体的な
定義を与えていれば `type S = Sess` として貼ります。メンバーの読み出しは
`expand_in_type` / `subst_as_seen_from` が refinement を既に読むので、
これで `ctx.session` は `Sess` になります。

**ただし refinement は制約でもあります。** 最初の版はそのまま貼ったので、
`type Session = JdbcSessionDef` というエイリアス経由で得た素の
`JdbcSessionDef` を `JdbcBackend#JdbcSessionDef` の引数に渡す
—— slick が至るところでやっていること —— が通らなくなり、
`JdbcActionComponent` / `JdbcProfile` / `StreamingInvokerAction` に
**新規 8 件**（`no matching overload for (JdbcSessionDef { type Database[_]
= …; type Session = … })R with arguments (JdbcSessionDef)`）が出ました。
134 → 138 です。

そこで refinement に `symbol::AS_SEEN_FROM_MARK`（`<asSeenFrom>`。Scala の
識別子にはなり得ない名前）という decl を 1 つ足して**印**とし、
`SymbolTable::is_sub_type` と `display_type` はその印のある refinement を
素の親として読むようにしました。as-seen-from は「どう見えるか」であって
「何を要求するか」ではない、という区別をそのまま置いたものです。型だけの
refinement は erasure が親へ落とすので、classfile 側は素の `B` のままです。
pickle も同じで、`backend/src/pickle.rs` は印のある refinement を素の親として
書きます（`<asSeenFrom>` という名前をシグネチャに出さないため）。
`A#B#C` は印を剥がして親から射影し直し、決まったぶんを持ち越します。

#### 2. `package p.q` は `p` を開かない

`expose_unqualified` は未解決の名前を **owner チェーンを遡って**探して
いました。nsc はそうしません（2.13.16 で実測、`-Xsource:3` の有無に
無関係）:

| 書き方 | `p` のクラス | `p` のサブパッケージ |
|---|---|---|
| `package p.q`（修飾付き） | 見えない | 見えない |
| `package p { package q { … } }`（入れ子） | 見える | 見える（トップレベルの同名を隠す） |

slick は自前の `slick.cats` パッケージを持っているので、緩い読みでは
`package slick.*` のすべてのファイルから `cats` が `slick.cats` に解決され、
`slick/dbio/DBIOAction.scala` の `cats.effect.IO` が
`value effect is not a member of <notype>` になっていました（2 件）。

`Checker::open_packages` は、いま型付けているファイルの `package` 句が
開いたパッケージ（namer が `PackageDef` ごとに 1 つ記録します）を内側から
順に返し、**最後にルート**を返します。ルートを walk に残すのが要点です:
`package slick.jdbc` からの修飾参照 `slick.ControlsConfig` の頭の `slick` は
ルートのメンバーとして解決します。`agent/cats2` はここでルートも一緒に
落としてしまい、差し引き +1 になっていました。

##### 残した最終フォールバックと、その本当の理由

正しい規則だけにすると `slick/jdbc/DatabaseConfig.scala` で
`not found: value ControlsConfig` が出ます。原因を
`not_found_error` に仕掛けたトラップのバックトレースで押さえました:

```
Typer::not_found_error ← type_expr ← type_apply_in ← type_apply
  ← type_expr ← fill_defaults_and_implicits ← type_apply_in ← …
```

**デフォルト引数の右辺が、呼び出し側のスコープで型付けされています。**
`default_getter_apply` が `f$default$n` ゲッターを見つけられないとき、
`fill_defaults_and_implicits`（`type_default_rhs_here` 経由）も名前付き引数の
経路も、保存してある**未型付けの木**をそのまま引数に差し込み、それが呼び出し
場所で型付けされます。`slick/basic/DatabaseConfig.scala` は
`import slick.{ControlsConfig, SlickException}` と
`classLoader: ClassLoader = ClassLoaderUtil.defaultClassLoader` を書いていて、
`package slick.jdbc` の呼び出し側はその import を持っていません。緩い
パッケージ walk がそれを覆い隠していただけです（同じ穴は修正前の `main` でも
`not found: value ClassLoaderUtil` として顔を出しています）。

そこで、正しい規則を先に走らせ、**何も見つからなかったときだけ**開いていない
中間パッケージを見る `expose_from_unopened_packages` を残しました。解決できて
いたものは全部解決できたまま、変わるのは**優先順位**だけ —— そして優先順位が
まさに `slick.cats` 問題の中身でした。デフォルト引数がつねに自分のゲッターの
呼び出しになるか、書かれたスコープで型付けされるようになったら、この
フォールバックは消せます（コメントにもそう書いてあります）。

#### 検証

`pj_projmember.scala` / `pj_pkgscope.scala` は `--scala-library` と
`--no-scala-library` の両方で `-Xverify:all` を通し、real scalac 2.13.16 の
標準出力と一致します。修飾付きパッケージ句は多ファイルが要るので
`a_qualified_package_clause_does_not_open_its_parent` が 5 本書き出して確かめ、
同じプログラムを実 scalac にも通します。`pj_projmember_bad.scala` は 3 件を
拒否し、nsc 2.13.16 も同じ 3 件を出します。8 本のうち 4 本は**修正前の `main`
で落ちる**ことを確認済みです。パッケージ解決と型射影という継ぎ目に触れたので
`--release` で `proj` / `cats2` / `tail4` / `tmember` / `conform` / `e2e` /
`pkgalias` / `imports` / `multifile` / `jarpickle` を回しました。
`cargo clippy --workspace --all-targets` は前後とも 78 件で新規ゼロです。

`pickle.rs` の 1 行は、`pj_projmember.scala` を有り／無しでコンパイルして
`Main.class` / `Main$.class` の**バイトが変わる**ことで効いていることを
確認しました（`def db(ctx: Sub#Ctx)` の引数型が、無しでは
`<refinement>` を被った `REFINEDTPE` になります）。scalac に読み直させる
黒箱テストは書けません —— **入れ子クラスの owner を空パッケージとして
pickle している**別の穴があり、射影と無関係な
`object Holder { class Inner(val n: Int) }` +
`object Api { def take(i: Holder.Inner) }` を書き出して scalac に `-cp` で
読ませるだけで `Symbol 'type <empty>.Inner' is missing from the classpath` /
`type Inner is not a member of object Holder` になります（下の残件）。

#### 既知の残件

* `cats.effect.IO` が見えるようになった結果、`DBIOAction.scala:237` の
  `cats.effect.IO(fa)` が `no matching overload for IO$ with arguments
  (Future[R])` になります（2 件が 1 件に変わっただけで、正味 −1）。`IO$` の
  `apply[A](thunk: => A): IO[A]` を jar の pickle から供給できていない別の穴で、
  `agent/cats2` が `Async$` について直したのと同じ族です。
* cats の `>>`（`no matching overload for (=> F[B])(FlatMap[F])F[B]`）3 件は
  before/after とも 3 件で、**型射影とは無関係**でした（`agent/cats2` の記録が
  正しい）。左辺が `Any` / `AnyRef` に落ちる別の原因です。
* （**`agent/tail6` で 1 件解決**。残り 2 件は下記のとおり根が別）
  `value map is not a member of Any` 3 件は手つかずです。射影ともパッケージとも
  無関係で、3 つとも根が違います。1 つだけ根を特定しました:
  `DatabaseUrlDataSource.scala:31` の `findFirstMatchIn(url).map(…)` は
  **`prelude_regex.rs` が `("findFirstMatchIn", vec![Type::String], Type::Any)`
  と宣言している**のがそのままです。6 行で再現します
  （`val re = "a(b)c".r; re.findFirstMatchIn("abc").map(_ => "")`）。直すなら
  `Option[Regex.Match]` にするだけでなく、パラメータも実 ABI どおり
  `CharSequence` にする必要があります（同ファイルの `unapplySeq` のコメント
  参照。いまの `String` のままだと descriptor が合わずリンクしません）。
  残る 2 件は `RewriteJoins.scala:139` の `foundRefs.filter(…)` と
  `JdbcActionComponent.scala:162` の `prit` で、根は追っていません。
* デフォルト引数の右辺が呼び出し側で型付けされる件（上の 2）は根の特定まで。
  直せば `expose_from_unopened_packages` と、修正前から出ている
  `not found: value ClassLoaderUtil` の両方が消えるはずです。
  → **`agent/tail6` で解決**（予想どおり両方消えました）。
* 射影が持ち歩けるのは**抽象型メンバー**だけです。ジェネリックな外側
  （`C[Int]#Inner` の `T` → `Int`）は `RefineDecl` が名前で照合する仕組みの
  都合で持ち歩けません。slick には現れません。
* 前置型が jar 由来のクラスのとき、その型メンバーは要求されるまで読まれない
  ので、決められるはずのものを取りこぼすことがあります（slick の該当箇所は
  すべてソース側です）。
* **入れ子クラスを pickle するとき owner が空パッケージになります**（新規に
  見つけた別の穴、このスライスの変更とは無関係）。2 行で再現します:
  `object Holder { class Inner(val n: Int) }` /
  `object Api { def take(i: Holder.Inner): Int = i.n }` を scala-rs で
  コンパイルし、その classfile を `-cp` に置いて scalac に
  `Api.take` を呼ばせると `type Inner is not a member of object Holder` と
  `Symbol 'type <empty>.Inner' is missing from the classpath` が出ます。
  `--scala-library` モードのみ確認。scala-rs 同士では `-cp` 経由でも通るので、
  実害は「scalac に読ませたとき」に限られます。
### reflect API の入れ子 `object` と `<val>.type`（`agent/reifyd`）

`docs/macros.md` §7.13.4 が「自前 `reify` の手前に残る 3 つの穴」として名指し
した 1 と 2 を塞ぎました。どちらも `reify` 専用ではなく**一般の機能追加**です。

* **trait の中の `object` を pickle から供給する。** `trait Exprs { object
  Expr { … } }` は `Expr()Lscala/reflect/api/Exprs$Expr$;` というインタフェース
  メソッドと module の classfile に落ちますが、`PickleSupply::complete_named`
  は pickle の `Def` と `Val` しか読まないので `MemberKind::Module` を丸ごと
  捨てていました。そのため `c.universe.Expr` は `value Expr is not a member of
  Universe`、`import c.universe._` 下の `Expr` は `not found: value Expr` と、
  どちらも**嘘の診断**でした。アクセサは探索を始めた受け手のクラスに立て、
  呼び出し先は `erased_desc` に決めさせます（`api/JavaUniverse` の classfile は
  `interfaces: 0` なので `invokevirtual JavaUniverse.Expr()` は解決しません）。
  classfile から読んだだけの壊れたアクセサ（戻り値が未解決の `Type::Named`）は
  修理します。
* **`c.universe` を安定識別子として型に書けるようにする。**
  `Mirror[c.universe.type]` が `stable identifier required, but c.universe
  found` でした。`Check::term_path_sym` が `Term | Module | ModuleClass` しか
  受けておらず、pickle から読んだ `val`（classfile では 0 引数の `def` と
  区別できないので `SymKind::Method` + `Flags::ACCESSOR`）が落ちていたためです。
  `Type::SingleType` の読み手は `SymbolTable::singleton_underlying` で
  0 引数 `Method` を結果型に開きます。

道中で、**コンパイルは無言で成功して実行時に落ちる**穴を 3 つ直しました。

* メソッドの**引数シンボル**がそのメソッドの「メンバ」として見えていました
  （`Check::type_select` の `qual.sym` フォールバック）。
  `m.staticClass(n).fullName` が `staticClass` の引数 `fullName` に解決し、
  codegen が「所有者クラス＝メソッドの erased descriptor」で `Fieldref` を
  吐いて `ClassFormatError: Illegal class name "(Ljava/lang/String;)L…;"`。
* 括弧なしのメンバ選択で `declaring_class` への `checkcast` が抜けていました
  （`Apply` 経路にはある）。`u.Expr` が `VerifyError`。
* メンバ `object` の受け手が捨てられ、囲む source クラスの `this` が積まれて
  いました（`gen_module_member_receiver`）。`universe.Liftable[String](f)` が
  `ClassCastException: Main$ cannot be cast to scala.reflect.api.Liftables`。

さらに **`Exprs#Expr.apply` を手書き**しました。`reify` の展開は最後に
`c.universe.Expr.apply[T](mirror, creator)` を呼びますが、pickle の署名は
第 1 引数が `Mirror[Universe.this.type]` で、この `this.type` は完了中の
クラス（module `Expr$` 自身）に対して変換されるため `Mirror[Expr$]` になり、
どの呼び出しとも合いませんでした。`ensure_tag_module` が `TypeTag.apply` を
手書きしているのと同じ理由なので、同じ扱い（`install_expr_apply`、erased
descriptor も書き下ろし）にしています。implicit 節は残してあるので、
`WeakTypeTag[T]` は既存の materialiser が埋めます。

これで **`reify` が組むべき木は、手書きなら丸ごと動きます**。

fixture は 2 組です。

* `tests/fixtures/rd_nested.scala` — 実行時 universe に対して、パス越しと
  wildcard import 越しの入れ子 `object` と
  `Mirror[scala.reflect.runtime.universe.type]` を使い 5 行印字。
  **実 scalac 2.13.16 と一致**します。
* `tests/fixtures/rd_impl.scala` + `tests/fixtures/rd_use.scala` —
  `reify { 42 }` / `reify { RdHelper.twice(x.splice) }` が展開されるべき形を
  `TreeCreator` で手書きし、**engine で実際に展開して走らせます**
  （静的シンボルは `mirror.staticModule`、splice は `Expr.in` 経由）。
  同じ 2 ファイルを実 scalac でも 2 段コンパイルして実行し、
  `42 / 42 / true` が一致することを別テストで固定しています。受け手や
  universe を取り違えた creator はコンパイルが通ってしまうので、出力の比較
  だけが捕まえられます。

テストは `crates/cli/tests/engine.rs` に追記した 4 本です。

`tests/slick_measure.sh` は `errors=134 → 134`、`files_with_errors=48 → 48`、
`tests/slick_subset.sh` は `38 files / 204 classes / verified=204 failed=0` で
変わりません。slick の 2 マクロは `reify` が要るところで止まっており、この
スライスはその手前を通しただけです。

#### 残件

* **`reify { … }` の展開そのものは未実装**で、診断は `docs/macros.md` §7.8 の
  ままです。木の材料は揃ったので、残るのは合成と**衛生性**（静的シンボルを
  `mkIdent(mirror.staticModule(...))` に、`splice` を `x.in(m).tree` に落とし、
  ローカルは名指しで断る）です。nsc の展開形は `docs/macros.md` §7.14 に実測
  で記録してあります。
* trait の中の入れ子***クラス***を**型**として書く形（`u.Liftable[Int]`）は
  まだ `not found: type Liftable` です。今回入れたのは term 側だけです。
* `u.Mirror` の上限（`api.Mirror[self.type]`）を pickle から読めないので、
  creator の中では `scala.reflect.api.Mirror[u.type]` に cast する必要が
  あります（nsc は `u.Mirror` と書きます）。

### `reify { … }` の展開（`agent/reifybody`）

`agent/reifyd` が「手書きなら丸ごと動く」ところまで通した木を、**コンパイラが
自動で組む**ようにしました（[`docs/macros.md`](docs/macros.md) §7.15）。
`reify { … }` は `crates/typer/src/reify_expand.rs` が

```text
{ final class $treecreator1 extends scala.reflect.api.TreeCreator {
    def apply[U <: scala.reflect.api.Universe with Singleton](
        $m$untyped: scala.reflect.api.Mirror[U]): <Trees.TreeApi> = {
      val $u = $m$untyped.universe
      val $m = $m$untyped.asInstanceOf[scala.reflect.api.Mirror[$u.type]]
      <本体を universe 呼び出しに落としたもの>
    }
  }
  <universe>.Expr.apply[T](
    <universe>.rootMirror.asInstanceOf[<api.Mirror>], new $treecreator1()) }
```

に展開します。本体の lowering は quasiquote と同じ `crates/typer/src/reify.rs`
ですが、**衛生性のぶんだけ違います**（`Reifier::in_reify`）。

* 静的 `object` は `$u.internal.reificationSupport.mkIdent($m.staticModule("..."))`。
  書かれた名前ではなく**シンボル**で解決するので、展開先のスコープに同じ名前が
  あっても意味は変わりません。
* `x.splice` は `x.in[$u.type]($m).tree`。creator が渡された mirror に
  rebase するので、周りの木と同じ universe に属します。
* **型引数**は `mkTypeTree(...)`。中身は `TypeTag` を組むのと同じ材料
  （`crate::materialize::TagBody`）で、単相クラスは `$m.staticClass(...)`、
  型構築子は `appliedType`、型パラメータは**スコープのタグ**から
  `tag.in[$u.type]($m).tpe` です。最後のものが slick の
  `reify { TableQuery.apply[E](cons.splice) }` に要ります。
* **ローカル・パラメータ・`this`・ブロック・型注釈・タグの無い型引数は
  名指しで断ります。** nsc は
  ローカルを *free term* にして展開に持ち回りますが、scala-rs はそれを組めません。
  裸の名前で組めばコンパイルも実行も通り、**呼び出し先にたまたま在る名前**を
  指してしまう——reification が防ぐためにある、まさにそのバグです。

各識別子が何かは `Check::reify_refs` が**クローンを投機的に型付けして巻き戻す**
形で決めます（`hole_lifts` と同じ）。型は `Expr.apply[T]` の `T` を得るために
本体全体を 1 度だけ投機型付けし、`WeakTypeTag[T]` は §7.10 の materialiser が
埋めます。`c.universe.reify { … }`（`import c.universe._` 無し）でも
materialiser が universe を見つけられるよう、展開を型付けしている間だけ
その universe を import prefix として積みます。

`Typer` はソース文字列を持っていませんでした（quasiquote は自分で組んだ文字列を
`Reifier` に渡していた）。`reify` の本体は**実ファイルのテキスト**なので、
`typecheck_units_src` を足して driver から渡しています。`Reifier` が
`A => B` と `Function1[A, B]`、`(a, b)` と `Tuple2(a, b)` を区別するのに要ります。

fixture は 1 組 + 異常系 1 本です。

* `tests/fixtures/rb_impl.scala` + `tests/fixtures/rb_use.scala` —
  リテラル 4 種 / 静的 `object` への適用 / `.splice`（1 つ・2 つ・`String`・
  `Boolean`）/ `c.universe.reify` / 型引数（`Int` と、タグから解く型パラメータ
  1 つ・2 つ）を macro 実装として書き、2 段コンパイルして **16 行印字**します。
  同じ 2 ファイルを実 scalac 2.13.16 で 2 段コンパイルして実行しても
  **同じ 16 行**です。最後の 2 行は splice を副作用つきの式で埋めたもので、
  木が splice を落としたり 2 回組んだりしたら数が変わります。
* `tests/fixtures/rb_bad.scala` — 断る 5 形（パラメータ / ローカル / 型注釈 /
  ブロック / タグの無い型引数）。実 scalac は 5 つとも通すので、これは
  **未実装の告白**です。

テストは `crates/cli/tests/engine.rs` に追記した 3 本
（`rb_reify_expands_and_runs` / `rb_reify_matches_real_scalac` /
`rb_reify_gaps_are_named`）です。

`tests/slick_measure.sh` は `errors=115 → 113`、`files_with_errors=41 → 41`。
slick の `TableQueryMacroImpl` の `reify { TableQuery.apply[E](cons.splice) }`
は**展開できるようになり**、`cannot expand reify` と、その巻き添えだった
`cannot expand apply` の 2 件が消えました。`crates/backend/` を触っていないので
`tests/slick_subset.sh` は省略しています。

#### 残件

* 同じ行に残る `value apply is not a member of TableQuery[E]` は §7.13 の残件
  （`TableQuery.apply` のオーバーロード選択）で、reify とは別件です。
* 呼び出し側で**推論された型引数**はまだマクロに渡らない（§7.13 の残件 1）ので、
  `rb_use.scala` は `RbUse.idOf[Int](5)` と型引数を書き下ろしています。
* ローカル・パラメータの *free term*、ブロック、関数リテラル、`this`、型注釈は
  いずれも未実装（名指しで診断）。
### デフォルト引数が型付けされる場所、`Regex` の実 ABI、default 付き implicit 引数（`agent/tail6`）

`agent/proj` が「根は特定したが直していない」と残した 3 件を扱いました。
`tests/slick_measure.sh` は **`errors=115 → 110`、`files_with_errors=41 → 39`**
（新規エラーは 0）。codegen（`crates/backend/`）は触っていないので
`tests/slick_subset.sh` は省略しています。

#### 1. デフォルト引数の右辺は**書かれたスコープ**で型付けする

`f$default$n` getter を呼べないデフォルト——とくに primary constructor の
もの（nsc はコンパニオンに getter を出しますが、こちらは合成していません）
——は、namer が保存した木を引数リストに差し込み、**呼び出しがある場所で**
型付けしていました。その結果:

* 名前が**呼び出し側のスコープ**で解決される。slick の
  `class DriverDataSource(…, classLoader: ClassLoader =
  ClassLoaderUtil.defaultClassLoader)` は `import slick.util.ClassLoaderUtil`
  の下に書かれていますが、`slick/jdbc/DatabaseConfig.scala` の
  `new DriverDataSource(…, driverObject = driver)` はその import を持たず
  `not found: value ClassLoaderUtil`。
* しかも span は**定義側の**もののまま、file index は呼び出し側なので、
  キャレットが無関係な行（`DatabaseConfig.scala:48` の `new DriverDataSource`）
  に立っていました。これが「呼び出し側で型付けしている」証拠です。
* 同名の別シンボルにも化けます。`BasicBackend.scala:69` の
  `actionListener: ActionListener[F] = defaultActionLogger[F]` は
  `HeapBackend.scala:52` で再型付けされて `F` が **HeapBackend の** `F` に
  なり、`found: ActionListener[F]  required: ActionListener[F]`。

`Checker::record_default_scope` が定義時のスコープスタック・owner・
`this_class`・unit を憶え、`type_default_rhs_here` がそれを差し戻して型付け
します。型付け済みの木は `NodeId::PRETYPED_DEFAULT` を持ち、`type_expr` は
それを見たら**再型付けせず** `adapt` だけします（名前付き引数の経路では
呼び出し側の引数ループがもう一度型を付けにくるため）。

コンストラクタのデフォルトでは、憶えるスコープから**クラス自身のメンバ
スコープを外し**、owner もクラスの外に出します。`new C(1)` の時点で
インスタンスは無いので、フィールドも先行する ctor 引数も名指しできません
——これは nsc も同じで、`class Pair(a: Int, b: Int = a)` は実 scalac 2.13.16
でも `not found: value a` です（`val a` でも同じ）。外さずに残すと `a` が
**フィールド**に解決し、差し込まれた木が呼び出し側の `this` からそれを
読んで実行時 `ClassCastException` になりました。

これで `agent/proj` が「消す条件」をコメントに書き残した最終フォールバック
**`Checker::expose_from_unopened_packages` を削除**しました。
副作用として `crates/cli/tests/multifile.rs` の
`enclosing_package_names_are_visible` が落ちます。この fixture
（`tests/multi/pkg_inner.scala`）は `package top.inner` から `top.Helper` を
無修飾で見ており、**実 scalac 2.13.16 はこれを拒否します**（`-Xsource:3` の
有無に関わらず `not found: value Helper`）。緩いフォールバックだけが通して
いた形なので、fixture を入れ子綴り `package top { package inner { … } }` に
直しました（nsc が受理する形。修飾綴りの方は `crates/cli/tests/proj.rs` が
固定しています）。

#### 2. default 付きの implicit 引数

implicit 探索が空振りしたとき、その引数に default があれば nsc は
**default を使います**（`missing implicit` を出すのは default が無いとき
だけ）。slick の `ScalaBaseType` はそれ前提で書かれていて——

```scala
def apply[T](implicit classTag: ClassTag[T], ordering: scala.math.Ordering[T] = null)
```

——`ScalaBaseType[T]` が `could not find implicit value of type Ordering[T]`
になっていました（`JdbcTypesComponent.scala` の 2 か所）。
`Checker::implicit_param_default` を `fill_implicit_params_in` の
フォールバック列（`ClassTag` / view / `TypeTag` の隣）に足しました。
default の本体は 1 と同じく**書かれたスコープ**で型付けします。

#### 3. `prelude_regex` が jar のシグネチャを覆い隠していた

`prelude_regex.rs` は `unapplySeq` のほかに `findAllIn` /
`findFirstMatchIn` / `replaceAllIn` / `replaceFirstIn` / `split` を
「pickle が無いときのフォールバック」として宣言していました。ところが
**jar のメンバは誰かが要求するまで `lookup_member` に見えない**ので、
install 時のガード `is_empty()` は常に真——つまり**フォールバックが常に
本番**でした。結果は 2 つ:

* `findAllIn` / `findFirstMatchIn` の結果型が `Any`。
  `MysqlCustomProperties.findFirstMatchIn(url).map(…)` が
  `value map is not a member of Any`（`DatabaseUrlDataSource.scala:31`）。
* 使える結果型を持っていた方も、パラメータが `String`。実 ABI は
  `CharSequence` なので descriptor が合わず、コンパイルは通って実行時に
  `NoSuchMethodError: Regex.replaceAllIn(String, String)`。

**5 つとも削除**しました。pickle は 5 つとも実シグネチャで供給できます
（`unapplySeq` だけは供給されないので残します）。これで供給できない名前は
「`Regex` のメンバではない」と診断されます——嘘の型を黙って与えるより
正直です。

`value map is not a member of Any` の残り 2 件
（`RewriteJoins.scala:139` の `foundRefs.filter(…)` と
`JdbcActionComponent.scala:162` の `prit`）は**同種ではありません**。
`agent/proj` の記録どおり 3 件は根が別々でした。

#### 4. jar 由来の implicit 候補は、親を読むまで自分の型にしか合わない

`class C[F[_]](implicit F: Async[F])` の下で `implicitly[Sync[F]]` が
`could not find implicit value of type Sync[F]`。`Async` の親リストは
プログラムが**名前を書いただけ**のクラスでは空のままで、implicit 探索は
不変借用の下で走るので自分では完了させられません。同じファイルの前の行で
`Async[F]` を型として書くだけで通るようになる——スコープ規則ではなく
補完漏れの形です。`Checker::warm_implicit_candidates`（探索が空振りした
**あとだけ**走る）を足しました。標準ライブラリのクラスは対象外です:
親を classfile から足し直すと `mutable.HashSet` の階層が書き換わり、slick に
`containsSymbol(Set[A])` のオーバーロードエラーが 2 件増えました
（`warm_own_scope_once` のコメントが警告しているのと同じ罠）。

#### fixture とテスト

* `tests/fixtures/t6_defaults.scala`（+ `expected/`）——
  default の右辺が定義スコープで解決されること（positional / 名前付きの
  `new`、通常メソッド、default 付き implicit 引数の 4 経路）。両モード。
* `tests/fixtures/t6_defaults_bad.scala` —— 定義スコープに無い名前は
  エラー（`Hidden`）、コンストラクタのデフォルトからは先行 ctor 引数も
  見えない（`a`）。**実 scalac も同じ 2 件を出す**ことを別テストで固定。
* `tests/fixtures/t6_regex.scala`（+ `expected/`）—— `Regex` の 7 メソッドと
  `unapplySeq`。jar モードのみ（private ランタイムには `Regex` が無いので
  診断されることを固定）。
* cats-effect の jar が Coursier キャッシュにあるときだけ走る
  `an_implicit_from_a_jar_answers_for_its_supertypes`。

テストは新ファイル `crates/cli/tests/tail6.rs` の 9 本です。
**修正前の main では 5 本が落ちる**ことを確認しています。

#### 残件

* **`GenTemporal[F, _]` 2 件は残しました**（`ConcurrencyControl.scala` の
  `wait.timeoutTo(timeout, …)`）。4 の修正で
  `implicitly[GenTemporal[F, Throwable]]` は通るようになりましたが、
  `timeoutTo[B >: A, E](…)(implicit F: GenTemporal[F, E])` の `E` は
  implicit 節にしか現れない型パラメータで、探索に届く前に **`Type::Wildcard`
  に潰されて**います（`GenTemporal[F, _]`）。`timeoutTo[Unit, Throwable]` と
  **明示的に型引数を書いても同じ**なので、`solve_implicit_only_tparams` /
  `adapt_implicit_apply` より手前——`cats.effect.syntax` の暗黙変換で得た
  `GenTemporalOps_` の `Select` を型付けする段階——で潰れています。
  `Wildcard` は変数の同一性を消すので、どの候補も合わせられません。
* `Ordering[Null]`（`Type.scala:395`、`new ScalaBaseType[Null]`）は default
  ではなく実際に `Ordering[Null]` を要求する呼び出しで、別の根です。
* コンストラクタのデフォルトに対する nsc のコンパニオン getter
  （`C$default$n`）は依然として未合成です。上に書いたとおり nsc でも
  先行 ctor 引数は参照できないので観測できる差は無いはずですが、
  分離コンパイルで jar 越しにデフォルトを補うことはできません。

