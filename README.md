# scala-rs

A compiler for a subset of Scala 2.13 (nsc), written in Rust. It reads Scala
sources and emits JVM class files.

It is not a port of scalac's sources. It is an original reimplementation.
Scala 3 syntax and TASTy are out of scope.

## Status

Experimental. This is a subset compiler, not a finished Scala compiler, and it
makes no claim of conformance to the language specification. What exists today:

- The front end carries an AST close to nsc's `Tree`: namer, typer (including
  implicit search), uncurry, lambda-lift and erasure.
- The target is Java 8 class files (major version 52), with a `StackMapTable`
  (`full_frame`) in the `Code` attribute. Frame types for locals are the erasure
  of the slot's declared type, as in scalac.
- Two ABI modes. By default the compiler links against a real
  `scala-library-2.13.x` jar when it can find one; with `--no-scala-library` it
  emits its own private runtime class files (`scala/Option`, `scala/List`,
  `scala/FunctionN`, …) instead. See [Library modes](#library-modes).
- Lambdas are emitted as `invokedynamic` through `LambdaMetafactory`, like nsc
  2.13. `PartialFunction` literals and arities above 22 are still compiled to
  anonymous classes.
- An unqualified name is resolved by SLS 2's four precedence levels —
  definitions of the same compilation unit, explicit imports, wildcard imports,
  then package members of other units — and two bindings of one level in one
  scope are reported as an ambiguous reference. See
  [docs/gitbucket.md](docs/gitbucket.md) ("Not this cluster: `Database` /
  `DatabaseFactory`") and the `impprio` test.

直接自己末尾呼び出しは `final` / `private` / object / ローカル def でループ化します。
`@tailrec` の未対応形状は診断します。対応範囲と深い再帰・scalac 相互運用テストは
[docs/tailrec.md](docs/tailrec.md) を参照してください。

`Singleton` の型境界は ScalaSignature に保存し、scalac 2.13.16 との
分割コンパイルを両方向で検査しています。正常な継承の JVM 実行と、不正な
境界変更の拒否を確認するテストは `singleton_metadata` です。
[修正の範囲と検証方法](docs/notes/singleton-metadata-parent.md) を参照してください。
外部 trait の `hashCode` / `toString` と組み込み `Any` の上書き関係も、
[scalac 相互運用テスト](docs/notes/inherited-universal-override.md) で検査します。

`case class` に合成する `hashCode` は、`--scala-library` では nsc と**同じ値**に
なりました。以前は両モードとも 31 倍で畳んでいたため、`Point(1, "a").hashCode` が
scalac の `-1322997830` に対して `128` になり、自前の `equals` とは整合するものの、
**scala-rs でコンパイルした case class と scalac でコンパイルした case class が
違うハッシュを持つ**ため、両方が入る `HashMap` / `Map` / `Set` で引けませんでした。
jar モードでは nsc の 2 つの本体（primitive なフィールドが 1 つも無ければ
`ScalaRunTime$._hashCode`、あればインラインの `MurmurHash3` mix 列）をそのまま
再現します。`--no-scala-library` では `scala.runtime.Statics` /
`ScalaRunTime$` が存在しないので 31 倍畳みのままで、その値は**設計上異なる**ものとして
別の期待値ファイルに固定してあります。scala-rs が case class を、実 scalac が
それを `HashMap` に入れるプログラムをコンパイルして一緒に走らせる相互運用テストを
含め、検証は `caseabi` テスト（`tests/fixtures/caseabi_*.scala`）にあります。
詳細は [docs/comparison-with-scalac.md](docs/comparison-with-scalac.md) の
「The synthesized members of a `case class`」を参照してください。
companion の `writeReplace` と、class 側の `apply` / `unapply` / `tupled` /
`curried` static forwarder は**未実装のまま**です。

同名メソッドのオーバーライド判定は、SLS 5.1.4 のとおりパラメータ型を不変として
扱います。型パラメータを含む型どうしは以前「同じ」と答えていたため、
`IterableOps.concat[B >: A](suffix: IterableOnce[B])` と
`MapOps.concat[V2 >: V](suffix: IterableOnce[(K, V2)])` のような**オーバーロード**を
オーバーライドと誤認し、scala/scala の `src/library` に scalac が出さない 34 件の
診断を出していました。現在は形（可変長引数か否か、名前渡しか否か、同じクラスの
型引数が確実に異なるか、JVM 名の異なるクラスか、束縛された型変数か）で決着する
場合だけ「別のメソッド」と判定し、決着しない場合は従来どおり黙ります。
バックエンドのブリッジ生成も消去後の記述子だけでは両者を区別できないため、
消去前に確定した「オーバーロードである」関係を `method_overload_pairs` に凍結して
参照します（誤ってブリッジを出すと親の署名で `ClassCastException` になります）。

`AnyVal` とその派生（9 つの値クラスとユーザー定義の値クラス）は
`java.lang.Object` を基底クラスに持ちません。nsc の `AnyClass` は親なしで
定義され、`AnyVal extends Any` だからです。そのため
`class Meters(val n: Int) extends AnyVal { def notify(): String = ... }` は
scalac 2.13.16 が受理し実行します。一方 `trait Univ extends Any` のような
**ユニバーサルトレイト**には別の禁止（nsc の
`clazz.isTrait && !clazz.isSubClass(AnyValClass)`）が働くので、こちらは拒否を
維持します。正常系・異常系の検証は `override` テスト
（`libanyval_overload` ほか 3 件）と [docs/scala-library.md](docs/scala-library.md)
の「an overload is not an override」節を参照してください。

同名メソッドと object の `apply` は両方をオーバーロード候補として比較します。
単一候補の呼び出し結果と曖昧な呼び出しの拒否は、`overload_module` テストで
scalac 2.13.16 と比較します。集合の結合も含む
[修正範囲と検証方法](docs/notes/overload-module-and-set.md) を参照してください。

出力先をクラスパスに含めて object を再コンパイルする場合も、Java 起動用の
`main` 転送メソッドを再生成します。`incremental_forwarder` テストは同じ出力先への
2 回のコンパイルと JVM 実行を scalac と比較し、両方向にコンパイラを入れ替えた
組み合わせも検査します。

暗黙検索は、候補を単一化する前に nsc の `isPlausiblyCompatible` にあたる構造的な
前判定（`Typer::plausibly_inhabits`）で絞り込みます。候補の結果型と要求型が
どちらもクラスで、どちらの親も他方に到達しない場合だけ棄却するので、最後の
適合判定が必ず失敗する候補しか落としません。あわせて、単一化がワイルドカードに
しか解けなかった型引数は「確定した」とは数えません（slick の `tupleNShape` のような
導出規則が、要求型が何も言っていない場面で自分の暗黙引数を探し続けるのを止めます）。
どちらも診断結果を変えず、コストだけを下げます。検証は `implfilter` テストと
[docs/gitbucket.md](docs/gitbucket.md) を参照してください。

失敗した暗黙検索は、そこで報告して止まります。`Type::Error` は
`plausibly_inhabits` の前判定をすべて通過してしまうため、要求型または候補の型が
エラー型だと「何にでも適合する」ことになり、スコープ中の暗黙値が全部候補になって
「ambiguous implicit」を名乗っていました（gitbucket では 1 か所で 32 個の候補を
並べていました）。nsc に合わせて 3 つの規則を入れています。(1) 要求型がすでに
エラー型なら検索を行いません。(2) 自分の型がエラー型の候補は
`ImplicitComputation.survives` と同じく候補から外します（型が解決できなかった
`implicit val` が検索に答えるのは、誤ったプログラムを受理する経路です）。
(3) エラー型の要求に対する「暗黙値が見つからない」診断は出しません —
原因はそれが失敗した場所ですでに報告済みです。

暗黙引数を埋められなかった適用は、nsc の `applyImplicitArgs` と同じく
エラー木にします。見つからなかった証拠だけが決められる型引数（slick の
`map[F, G, T](f)(implicit shape: Shape[…]): Query[G, T, C]` の `T`）が式に
漏れ出し、その上のすべての選択が二重に報告されるのを止めます。ただし
**結果型がまだ呼び出し先の型引数を含んでいる場合に限ります**。scala-rs は
単一パスではなく、同じ式を複数回型付けするため、漏れていない結果まで
エラーにすると「以前は通っていたプログラムが通らなくなる」ことが実際に
起きました（`pos/annotated-original`）。正常系（証拠が見つかる場合に型引数が
正しく決まり JVM 実行結果が scalac と一致すること）と異常系（scalac と同じ
2 行だけを拒否すること）は `implcascade` テスト
（`tests/fixtures/implcascade.scala`、`tests/fixtures/implcascade_bad.scala`）
で比較します。

ワイルドカード import は、まだ pickle を読んでいないクラスに問い合わせません。
問い合わせると `PickleSupply` が拒否を恒久的に記憶してしまい、直後に同じクラスを
読み込む `adopt_binary_class` がその記憶を受け取るため、jar 側の `implicit def` が
「implicit と書かれていない普通のメソッド」として暗黙スコープに残り、決して選ばれ
なくなります（バイトコードに implicit は記録されません）。スキップしたクラスは、
別の理由で読み込まれたあとの同じ import の再走査で改めて供給されます。
scalac がコンパイルした jar に対する検証は `implguard` テスト
（`tests/multi/implicit_wildcard_binary`）を参照してください。

クラスファイルは他人のコードの「取りこぼしのある説明」です。3 点を補います。
(1) 暗黙検索が空振りしたとき、レキシカルスコープの候補だけでなく**コンパニオン
側の候補**（SLS 7.2）の親も pickle から補います。(2) trait から継承した具象
メソッドのために class に書かれる mixin forwarder は引数節が 1 つに潰れている
ため、pickle が「節が 2 つ以上で引数総数が同じ」メンバーを答える場合に限り
捨てます。(3) クラスファイルは 1 度しか読みませんが、その短絡は「今問い合わせた
所有者から見えるか」を確認していませんでした（`object Outer` の入れ子オブジェクトは
JVM 名だけでは class 側と区別できません）。検証は `gbopt` テスト
（`tests/multi/gbopt_binary`）と [docs/gitbucket.md](docs/gitbucket.md) を参照。

パス依存型メンバーは高階のもの（`trait P[M[_]] { type F[_] }` の `P.F`）も
接頭部ごとに区別します。別々の `p` / `q` の `F` は別の型構成子であり、宣言が
持たない名前は従来どおり診断します。呼び出し側が書かなかった暗黙引数節でも
依存メソッド型の置換を行い、型ラムダ（kind-projector の `*` を含む）の本体に
現れる出現まで書き換えます。正常系の JVM 実行と不正例の拒否は `hkpath` テスト
（`tests/fixtures/hkp_member*.scala`）で scalac 2.13.16 と比較します。詳細は
[docs/cats.md](docs/cats.md) の「The same member, higher-kinded」を参照して
ください。

引数の期待型に置かれる「まだ決まっていない」印（`_`）は、制約として読みません。
`f: A => G[F[B]]` のような引数はリテラルを `A => _[T[_]]` という期待型で型付け
しますが、その `_` は外側の呼び出しが未決だと言っているだけなので、内側の呼び出し
が自分の型パラメータをそこから取ると `P.F[T[_]]` のような結果になります。引数が
すでに答えを出している位置、およびこれから型付けする引数が決める位置では、
`_` を含む解を採用しません。逆に、型ラムダ（`({ type L[x] = Kle[F, A, x] })#L`）
の中にしか現れない型パラメータは、期待型のラムダ本体まで降りて解きます。
`Applicative[Kleisli[F, A, *]]` の `A` はこれまで `Nothing` に潰れていました。
正常系の JVM 実行と不正例の拒否は `wcinfer` テスト
（`tests/fixtures/wci_open*.scala`）で scalac 2.13.16 と比較します。詳細は
[docs/cats.md](docs/cats.md) の「The undecided position read as a decision」を
参照してください。この `_` が「こちらが置いた印」なのか「利用者が書いた存在型」
なのかは形では区別できないので（`Type::Wildcard` はどちらでもあります）、
期待型を緩めた引数を型付けしている間だけ印として扱います
（`Typer::relaxed_pt_depth`）。`Cache[(Seq[String], Class[_]), String]` のように
利用者が書いた `_` は、これまでどおり解として読みます。

A higher-kinded type variable is solved by nsc's partial unification
(scala/bug#2712): against a type applied to more arguments than the variable
takes, the leftmost surplus is captured and only the rightmost arguments are
abstracted, so `G[B]` against `IndexedStateT[Eval, S, S, B]` is
`G := IndexedStateT[Eval, S, S, *]`. A partially applied class is already a
type constructor here, so no lambda symbol is invented and two spellings of the
same abstraction compare equal. The same capture reads an invariant or
contravariant position of the expected type, and a literal parameter whose
expected type is still a variable is read off the literal's body
(`s => f(s, a)`), as nsc's `typedFunctionUndoingEtaExpansion` does. The JVM run
and the rejections are compared with scalac 2.13.16 by the `hkunify` tests
(`tests/fixtures/hku_partial*.scala`); see "Inventing the type lambda that was
already there" in [docs/cats.md](docs/cats.md).

暗黙の変換は、**その適用全体**（自身の暗黙パラメータ節を含む）が成立して初めて
候補になります。cats の
`implicit def toFlatMapOps[F[_], A](fa: F[A])(implicit F: FlatMap[F])` は形だけ
見れば 1 引数の適用型すべてに当てはまるので、上の部分単一化で `F := Bag` が解ける
ようになった時点で、`FlatMap` インスタンスを持たない型に対する `bag.flatMap(f)` が
「`FlatMap[Bag]` が見つからない」という診断になっていました（しかも変換を挿す経路が
2 つあるため 2 回）。nsc はこれを報告しません。`inferView` は暗黙引数まで含めて適用を
型付けし、失敗した変換は**候補から外して**探索を続けます。どれも残らなければ、残る
診断は選択自身のもの——`value flatMap is not a member of Bag[Int]`——で、位置も選択
自身の位置です。これは**探索の**規則であって診断の抑制ではないので、
`toFlatMapOps(new Bag(1))` と手で書いた場合の暗黙値不足はこれまでどおりエラーです。
証人がある正常系の JVM 実行、証人のない変換が落ちて**別の**変換が勝つ例（両者は
引数型では優劣が付かず、暗黙節だけが差になります）、および 2 種類の拒否は
`convimpl` テスト（`tests/fixtures/cimpl_*.scala`）で scalac 2.13.16 と比較します。
詳細は [docs/cats.md](docs/cats.md) の「The view that was found and could not be
applied」を参照してください。

継承グラフに閉路がある場合も線形化（SLS 5.1.2）は必ず停止します。以前は再帰の
深さだけを 64 で打ち切っていましたが、深さの上限は再帰木の**大きさ**を抑えません。
親が 2 つある節点が閉路上にあると木は `分岐^64` になり、`trait X extends Y with Z;
trait Y extends Z; trait Z extends X` という 3 行（実際の scalac は 2 秒未満で
拒否します）だけで CPU を 100% 使い切ったまま何時間も返りませんでした。現在は
`crates/typer/src/lin.rs` が再帰の**経路**を持ち、線形化中のクラスに再入した時点で
打ち切るので、どんなシンボルグラフでも停止します。あわせて閉路そのものを
`illegal cyclic reference involving trait X` と診断し（scalac 2.13.16 と同じ行・
同じ文言）、閉路を閉じている親を error 型に置き換えて先へ進みます。置き換えは
nsc と同じ理由で、`SymbolTable::is_sub_type` など親をたどる他の走査も閉路に
出会わなくなります。正しい階層に対する線形化は変えていません（slick の 1490 個の
class file が main と 1 バイトも違いません）。深く広いダイヤモンド継承の `super`
連鎖を実行して実 scalac の出力と比較する検査と、閉路の拒否行の検査は
`linearization` テスト（`tests/fixtures/linterm_*.scala`）にあります。

線形化そのものは SLS 5.1.2 の `+:` の畳み込みで、C3 merge ではありません。SLS が
定めているのは親の線形化に対する右結合の `+:` の畳み込みで、`+:` は**どんな 2 つの
列に対しても**答えを持ちます（共有する祖先の順序が食い違っていても）。C3 merge は
そうではなく、どの先頭も自由でないときに推測するしかありません。以前の実装は
`lists[0][0]` を選んでいたため、**深さの違う 2 経路から届く祖先**の位置を間違え、
`class Wider extends Root with L6 with L5` が scalac の `Wider L5 L3 L6 L4 L1 L2 L0`
に対して `Wider L5 L6 L4 L3 L1 L2 L0` になっていました。畳み込みをそのまま書くと
重複も生じないので、`dedup_keep_last` による後始末（Java の
`class LinkedHashMap extends HashMap implements Map` のために必要でした）も
不要になります。

A library member is read from the pickle on demand, and where two classes in
the receiver's linearization declare the same name with the same *explicit*
parameters, only one copy is kept — nsc's `isAsSpecific` looks through an
implicit clause, so two such declarations are equally specific and supplying
both makes every call ambiguous. The one kept used to be whichever the walk
offered first, and for `SortedMap` that is `MapOps.map[K2, V2](f)` rather than
the `SortedMapOps.map[K2, V2](f)(implicit ordering: Ordering[K2])` that
overloads it: `aSortedMap.map(f)` compiled and returned an **unordered** `Map`,
with no diagnostic anywhere. A declaration that adds an implicit clause to one
it inherits now supersedes it — that clause is the `Ordering` witness, and it
is the only way the result can be the receiver's own sorted collection.
Declarations with the same number of parameters are unaffected. The sorted
collections are run rather than merely compiled in
`tests/fixtures/sm_ordering.scala` (`sortedmap` test), under a reversed
`Ordering[Int]` so that iteration order is the evidence; see `docs/cats.md`.

この形（`class Wider extends Root with L6 with L5`）は
`tests/fixtures/linterm_diamond.scala` に入っていて、`linearization` テストが
実 scalac 2.13.16 の出力と比較します。なお、**先行する親がすでに継承している
mixin** を書いたときにクラス本体の `super` が解決先を誤る欠陥は別にあり、
`docs/not-implemented.md` に根本原因（`base_type_seq` が反復する基底クラスを
最派生でない具体化に解決すること）まで記録してあります。

同じ「深さの上限は再帰木の大きさを抑えない」という誤りが
`SymbolTable::is_sub_type` の親走査にもありました。そして**こちらは閉路を必要と
しません**。

```text
trait A(n) extends A(n-1) with B(n-1)
trait B(n) extends A(n-1) with B(n-1)
```

は閉路のない正当な階層で、実 scalac は約 2 秒でコンパイルします。しかし上まで
到達する経路が `2^n` 本あり、`any()` は `true` でしか短絡しないため、
`false`（オーバーロード解決と implicit 探索が最も多く尋ねる答え）を返すのに
すべての経路をたどっていました。22 段で 4 秒、24 段で 19 秒、26 段で 74 秒と
1 段ごとに倍増します。深さは 26 しかないので上限 200 は一度も発火せず、閉路が
ないので閉路検査も助けになりません。

必要だったのは経路の記録ではなく**メモ**でした。別経路で再到達した同じ問いを
一度だけ答えるようにし、あわせて評価中の同じ問い `(a, b)` への再入は `false`
と答えます（最小不動点。閉路を通らずに得られる `true` はすべて残ります）。キーは
問い全体で、シンボル単体ではありません（`List[Int]` と `List[String]` は別の問い
です）。`subst_tparams_cow` は型引数を**増やす**ことがあり問いの集合が有限とは
限らないので、既存の深さ上限は後詰めとして残してあります。詳細は
`SymbolTable::walk_parents` のコメントを参照してください。

メモは 1 つの最外の問いにつき 256 歩を超えてから初めて有効になります。ほとんどの
問いは数歩で終わり（`String <: CharSequence` は 3 歩）、そこでメモを取ると型検査で
最も熱いアームが遅くなるだけだからです。実測でも遅くなっていません（538 ファイルの
scala-library を交互に min-of-5 で計測して修正前 3877ms / 修正後 3542ms）。答えが
変わっていないことは slick の 1490 個の class file が修正前と 1 バイトも違わない
ことで確かめています。正常系（9 段のダイヤモンドを実行し実 scalac の出力と比較。
オーバーロード解決が走査の `true` に依存する例を含む）と、停止することの検査は
`subtypeterm` テストと `tests/fixtures/subtypeterm_diamond*.scala` にあります。

クラスの**自己別名**越しに読んだ型メンバー（slick の profile cake が書く
`trait Profile { self: Profile => trait API { type ColumnType[T] = self.ColumnType[T] } }`）
は、その接頭部が指すインスタンスまで簡約します。決めるのは**書かれた接頭部**
であって読み手のクラスではありません。`object Jdbc` の中に書いた
`Mem.api.ColumnType[Int]` は `MemType[Int]` のままです。`ColumnType` を抽象の
まま残すプロファイルでは簡約せず、右辺がその出現自身を指す別名（cats の
`Representable#compose`）も簡約しません。型引数を伴わない一階の `p.T` は
まだ簡約しません（本スライス以前からの制限）。正常系の JVM 実行と不正例の拒否は
`hkselfalias` テスト（`tests/fixtures/hkself_member*.scala`）で scalac 2.13.16 と
比較します。詳細は [docs/cats.md](docs/cats.md) の「The self alias the prefix
names」を参照してください。

マクロ実装参照に書かれた**型引数**（`def mapTo[R] = macro ShapedValue.mapToImpl[R, U]`
の `[R, U]`）を読むようになりました。nsc はこれを呼び出し側の型引数と並べません。
`R` はマクロ def 自身の型パラメータなので呼び出し側の型引数から、`U` は所有クラスの
型パラメータなので**レシーバ**（`asSeenFrom`）から取ります。従来は個数を数えて
1 対 1 に並べていたため、個数が合わない `mapTo` は展開できず、個数が偶然合う
`swapped[A, B] = macro Impl.pairImpl[B, A]` は**黙って逆順のタグ**を渡していました
（`swapped[Int, String]` が実 scalac の `R=String U=Int` に対し `R=Int U=String`）。
jar の `@macroImpl` 注釈側とソース側の両方の読み手を直しています。nsc 自身が
「書かれた型」ではなくその**シンボルの型**を読むため `Impl.f[List[R], Int]` が
`List[A]` になる件は、真似も置換もせず理由付きで拒否します。正常系は
`tests/fixtures/mt2_mdef.scala`（実 scalac でコンパイル）と `mt2_use.scala` を
**実行**して実 scalac 2.13.16 の出力とバイト単位で比較し、異常系は
`mt2_bad.scala`（実 scalac は通る 3 例）が名指しで拒否されることを固定します
（`mapto2` テスト）。詳細は [docs/macros.md](docs/macros.md) §7.22 を参照してください。

For what the language subset does and does not cover, see
[docs/language-support.md](docs/language-support.md) and
[docs/not-implemented.md](docs/not-implemented.md).

## Benchmark

The reference workload is [slick](https://github.com/slick/slick): 184 files,
23,337 lines, compiled with `-Xsource:3` against scala-library 2.13.16 plus
slick's dependency jars. Both compilers were measured on the same machine under
the same conditions, back to back.

|                | wall    | CPU     | class files |
| -------------- | ------- | ------- | ----------- |
| scalac 2.13.16 | 12.0 s  | 68.6 s  | 1498        |
| scala-rs       | 1.8 s   | 1.7 s   | 2127        |

Medians of three runs, alternating between the two compilers so that both see
the same machine. The CPU-time gap is the larger one: scalac's wall time is
carried by several threads, while scala-rs runs the compile itself on one
thread and only parallelises writing the class files.

scala-rs emits more class files than scalac for the same sources, so the
comparison is not entirely in its favour: `PartialFunction` literals still
become anonymous classes here.

What that run establishes:

- All 184 slick files typecheck, with 0 errors.
- All 1552 emitted class files load under `java -Xverify:all`. They *load*:
  `Class.forName(initialize = false)` links nothing, so method bodies are not
  verified by that number (see `tests/slick_run.sh`).
- The test suite is 130 test binaries / 1849 tests. 84 of them (the programs in
  `tests/conform/`) are dual-run against real scalac 2.13.16 and required to
  produce byte-identical stdout.

This is one benchmark, not a completeness claim. A large real program compiling
and verifying says nothing about the parts of the specification it happens not
to use.

Methodology, phase breakdown and profiling notes are in
[docs/performance.md](docs/performance.md).

## Build

A Cargo workspace. The CLI crate is `scala-rs-cli`; the binary is `scala-rs`.

```bash
cargo build -p scala-rs-cli --release
```

Or run it straight from the workspace:

```bash
cargo run -p scala-rs-cli -- compile file.scala -d out/
```

The binary lands in `target/release/scala-rs` (or `target/debug/scala-rs`).

## Usage

Compile sources into a directory of class files:

```bash
scala-rs compile file.scala -d out/
scala-rs compile file.scala -d out/ --scala-library /path/to/scala-library-2.13.16.jar
scala-rs compile file.scala -d out/ --no-scala-library
scala-rs compile B.scala -d outB -cp outA --no-scala-library
scala-rs compile file.scala -d out/ -Xsource:3
```

Compile and run the entry point (`main` in an `object Main`). `run` adds the
library jar to `java -cp` when one is in use:

```bash
scala-rs run file.scala
scala-rs run file.scala --scala-library /path/to/scala-library-2.13.16.jar
scala-rs run file.scala -- arg1 arg2
```

The emitted class files are launched by `java` exactly as scalac's are: an
`object` produces a module class `Main$` plus a forwarder `Main` carrying the
static `main`.

```bash
java -cp out Main
java -cp out:scala-library-2.13.16.jar Main
```

### Library modes

`--scala-library [<jar>]` (or the `SCALA_LIBRARY_JAR` environment variable)
links against the **scala-library 2.13 ABI**: `Option`, `List`, `FunctionN`,
`Tuple2`, `Predef$`, the `Rich*` / `StringOps` / `ArrayOps` extension methods,
the collections, `Either`, `scala.util.Try`, `scala.jdk.CollectionConverters`,
and so on come from the jar, and no colliding private class file is emitted.
Members that are not in the hand-written prelude are supplied on demand by
reading the `ScalaSignature` pickle out of the jar's class files. If the path is
omitted, `SCALA_LIBRARY_JAR`, `/tmp/scala-rs-lib` and the current directory are
searched.

`compile` and `run` use an auto-detected jar by default and fall back to the
private runtime when there is none. `--no-scala-library` forces the private
runtime.

### Debug and diagnostic flags

- `--parse` — parse only and dump the AST (no typechecking, no output).
- `--typer` — dump the tree after namer/typer. This is a dump flag, not a stop
  flag: the compile still runs to the end.
- `-Xfatal-warnings` — turn warnings (non-exhaustive match, …) into errors.
- `-Xsource:<version>` — source level: `2.13` (default), `3` or `3-cross`. The
  `3` levels accept the Scala 3 spellings this subset implements (`A & B`
  intersection types). As in nsc, a level below the current major is an error.
- `-language:<feat>` — enable `postfixOps`, `implicitConversions` or `dynamics`.
- `-cp` / `--class-path` — read previously compiled class files, Scala classes
  in jars (through their `ScalaSignature` pickle) and Java `.class` files from
  jars, jmods and the JDK.
- `SCALA_RS_PICKLE_DEBUG=1` — trace which library members were supplied from a
  pickle, and why the others were not.

`scala-rs --help` prints the full list.

## Project layout

| crate                | role                                                                    |
| -------------------- | ----------------------------------------------------------------------- |
| `crates/span`        | source positions and diagnostics                                        |
| `crates/lexer`       | lexing (newline tokens for semicolon inference, interpolation modes)     |
| `crates/parser`      | recursive-descent parser; AST close to nsc's `Tree`                     |
| `crates/pickle`      | reader for nsc `ScalaSignature` pickles, shared by typer and backend     |
| `crates/typer`       | namer, typer, implicit search, uncurry, lambda-lift, erasure             |
| `crates/backend`     | JVM class file emission (major 52, `StackMapTable`) and the private runtime |
| `crates/driver`      | pipeline driver                                                         |
| `crates/cli`         | command line; binary `scala-rs`                                         |

Test data lives outside the crates: `tests/fixtures/` (single-file programs plus
their expected stdout), `tests/multi/` and `tests/conform_multi/` (multi-file
programs), `tests/conform/` (the differential conformance corpus).

## Testing

```bash
cargo test
```

Tests that need external artifacts skip themselves rather than fail when the
artifact is missing:

- Anything comparing against real scalac needs `scalac` 2.13.16 on `PATH`.
- Anything in library mode needs `scala-library-2.13.16.jar`, found through
  `SCALA_LIBRARY_JAR` or `/tmp/scala-rs-lib`.
- The Java-side checks need `java` (and, for a few of them, `jar` and `javap`).

The scripts under `tests/` are measurement harnesses, not part of `cargo test`:

- `tests/bench.sh` — time a full compile of slick's 184 sources and report wall
  and CPU time. `--parse` times parsing only; `REPS=n` repeats.
- `tests/slick_measure.sh` — compile slick's sources and report the error count
  (correctness, not speed). It rebuilds its own toolchain and re-clones slick at
  a pinned revision when pieces are missing.
- `tests/slick_subset.sh` — find the fixpoint of slick files that compile
  cleanly together, emit their class files, and load every one of them with the
  bytecode verifier on.
- `tests/verify_all.sh <dir> [cp...]` — load every class file under a directory
  with `Class.forName(name, true, loader)` and count `VerifyError` /
  `ClassFormatError`. Initialising is the point: it forces linking, and linking
  is what runs the verifier over the method bodies. `slick_subset.sh` passes
  `false` there, so it never verifies a body; this is the check that noticed
  six of slick's 1490 classes could not be loaded at all while every other
  measure was green.
- `tests/slick_run.sh` — build slick twice (scala-rs and real scalac), compile
  the client programs in `tests/slick_progs/` once with real scalac, and run
  that one client binary against each slick build, comparing stdout byte for
  byte. The first harness that asks whether the emitted slick *runs*. Each
  program is executed `RUNS` times (default 3) and the per-program `m/n` is
  printed, so an intermittent failure cannot be averaged away. See
  [docs/notes/running-the-slick-we-compiled.md](docs/notes/running-the-slick-we-compiled.md).
- `tests/expand_fm.py` — expand the seven FreeMarker templates slick's build
  generates Scala sources from, so a measurement covers what sbt would compile.
- `tests/testkit_measure.sh` — the same measurement for `slick-testkit`, slick's
  own test suite, compiled against the class files scala-rs produced for slick.
- `tests/reap_strays.sh` — kill `scala-rs` processes orphaned by a killed test
  run (`--kill`; without it, only reports).

How the fixtures, the dual-run harnesses and the pickle-reader regression tests
are organised is described in [docs/testing.md](docs/testing.md).

線形化（SLS 5.1.2）は `linearization` テストで二重に検査します。正常系は深く広い
ダイヤモンド継承の `super` 連鎖を実行し、同じソースを実 scalac 2.13.16 で
コンパイル・実行した出力と直接比較します（`super` の連鎖がそのまま線形化なので、
停止のためのガードが線形化を黙って切り詰めれば出力から trait が 1 つ消えます）。
異常系は閉路のある `extends` グラフが scalac と同じ行・同じ文言で拒否されること、
そして**そもそも停止すること**（60 秒の上限つき）を検査します。

型の**修飾子**が何も指さない場合（`trait AllOps[A] extends Ops[A] with
Missing.AllOps[A]`）は、`crates/cli/tests/qualfallback.rs` が両方向を固定します。
`tree_to_type` の `TreeKind::Select` は、接頭部の下に名前が見つからないとき単純名へ
フォールバックします（jar の `type` エイリアスはバイトコードに痕跡を残さないため、
Twirl が生成する全テンプレートの親句にある `HtmlFormat.Appendable` はこの経路でしか
解決できません）。このフォールバックが誤記にも効いてしまい、同じ単純名を持つ無関係な
型——cats では定義中の trait 自身——に静かに束縛されていました。`qualifier_names_nothing`
は、修飾子が**何かを指している**ときだけフォールバックを許します。指していなければ
scalac と同じ `not found: value <q>` を同じ行に出します。正常系の fixture は
コンパイルするだけでなく**実行**して出力を実 scalac と比較します（別のエイリアスに
束縛し直されても型検査は通ってしまうため）。

`subtypeterm` テストは `SymbolTable::is_sub_type` の親走査について同じ二重の検査を
します。停止側は 34 段のダイヤモンド継承（経路は `2^34` 本。修正前は 5 時間の外挿）
が 10 秒以内に終わることと、22 段と 30 段の所要時間の比が指数的でないこと。正常系は
`tests/fixtures/subtypeterm_diamond.scala` を実行して実 scalac の出力と比較します
（停止のためのガードが `true` を 1 つ落とせば、診断は出ないままオーバーロード解決が
静かに変わるので、実行して出力を比べる以外に気づく方法がありません）。

別コンパイルでは型パラメータの上下限と型エイリアスの宣言種別を
`ScalaSignature` に保持します。`crates/cli/tests/existential.rs` は実際の
scalac を読み手にして、正例の JVM 実行と不正な型引数の拒否を検証します。
`E#Elem` の接頭部や curried メソッドの引数節には未解決の制限があり、
Slick の逆方向テスト（MODE=a）はまだ通っていません。
現在の数値と検証範囲は [tests/BASELINE.md](tests/BASELINE.md) を参照してください。

## Documentation

- [docs/language-support.md](docs/language-support.md) — the implemented
  language subset, feature by feature.
- [docs/not-implemented.md](docs/not-implemented.md) — what is knowingly
  missing.
- [docs/architecture.md](docs/architecture.md) — crate structure, and how
  library symbols are supplied from `ScalaSignature` pickles.
- [docs/performance.md](docs/performance.md) — benchmark methodology, phase
  breakdown, and the optimisations behind the numbers above.
- [docs/testing.md](docs/testing.md) — test layout and what each suite fixes.
- [docs/comparison-with-scalac.md](docs/comparison-with-scalac.md) — an honest
  diff against scalac 2.13.
- [docs/macros.md](docs/macros.md) — design notes for def macros, and
  [docs/macro-engine-prototype/](docs/macro-engine-prototype/) — the
  feasibility probe behind them (not production code).
- [docs/slick-testkit.md](docs/slick-testkit.md) — compiling slick's own test
  suite: what is measured, the numbers, and what they found.
- [docs/cats.md](docs/cats.md) — where this compiler stands on typelevel/cats,
  the second real-world benchmark.
  引数なしメソッドの戻り値へ挿入する `apply` でも型引数を推論し、
  レシーバの型で具体化した上限・下限を検証します（`catstail3` の実行・拒否テスト）。
- [docs/scala-library.md](docs/scala-library.md) — compiling scala/scala's own
  `src/library` from source: the numbers, and the one root behind them.
- [docs/specialization.md](docs/specialization.md) — `@specialized` はメソッドが
  所有する1型引数の Int/Long 特殊化を実装しています（object・final・private
  メソッド）。クラス特殊化は未完了です。`tests/spec_classfiles.sh` で
  scalac との差を引き続き計測します。
- [docs/notes/](docs/notes/README.md) — development notes: the investigations
  and the reasoning behind individual changes.

## License

Apache-2.0
