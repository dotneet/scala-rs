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
- Early inference of forward members preserves their defining imports, including
  references from anonymous classes passed to parent constructors.
- Explicit type arguments on factory calls use the same inherited override
  filtering as an explicit `.apply`, including loaded collection delegates.
- Partially applied factories infer receiver parameters from arguments and
  expected results before implicit search. Binary result parameters retain
  their declared bounds until call-site inference. Implicit-only methods infer
  result-only lower bounds at the receiver while preserving explicit type arguments.
  Parameterless results infer their lower bounds in value position; explicit
  and omitted apply both retain factory receiver inference until the arguments.
  Selection qualifiers defer that value-position inference, including aliases.
- The target is Java 8 class files (major version 52), with a `StackMapTable`
  (`full_frame`) in the `Code` attribute. Frame types for locals are the erasure
  of the slot's declared type, as in scalac.
- Nested APIs resolve higher-kinded result families through the concrete outer
  profile, including implicit conversions and separate compilation. The
  `retfamily` test compares both ABI modes with scalac and checks both orders
  of a diamond hierarchy.
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
  scope are reported as an ambiguous reference. So is a definition together
  with an `import` clause nested more deeply than it, which precedence alone
  would let the definition win. See
  [docs/gitbucket.md](docs/gitbucket.md) ("Not this cluster: `Database` /
  `DatabaseFactory`") and the `impprio` and `nameamb` tests.

- Object wildcard imports preserve the imported receiver for mutable fields
  and inherited calls, including nested imports and renamed selectors.
  `wildrecv` compares execution byte-for-byte with scalac 2.13.16 in both ABI
  modes and probes acceptance and rejection in both directions.

直接自己末尾呼び出しは `final` / `private` / object / ローカル def でループ化します。
`@tailrec` の未対応形状は診断します。対応範囲と深い再帰・scalac 相互運用テストは
[docs/tailrec.md](docs/tailrec.md) を参照してください。

`Singleton` の型境界は ScalaSignature に保存し、scalac 2.13.16 との
分割コンパイルを両方向で検査しています。正常な継承の JVM 実行と、不正な
境界変更の拒否を確認するテストは `singleton_metadata` です。
[修正の範囲と検証方法](docs/notes/singleton-metadata-parent.md) を参照してください。
外部 trait の `hashCode` / `toString` と組み込み `Any` の上書き関係も、
[scalac 相互運用テスト](docs/notes/inherited-universal-override.md) で検査します。

**Java クラスファイルのメンバ**は、`private` 以外——`public` / `protected` に加えて
**パッケージプライベート（既定アクセス）**——を読み込みます。既定アクセスは Scala の
`private[<パッケージ>]` として記録するので、同一パッケージからは見え、別パッケージからは
実 scalac と同じ行・同じ文言で拒否されます。フィールドの `Signature` 属性も読むため、
`MainNode<K, V> mainnode` のような総称フィールドが消去された生の型で見えることは
なくなりました。Java の**インスタンスフィールド**の読み出しは `getfield`、`import C._`
で名前だけ書いた Java の `static` フィールドは `getstatic` を出します（以前はどちらも
アクセサ呼び出しとして出力していたため、読み出しを 1 つ含むだけでクラスが
`ClassFormatError` でロードできませんでした）。検査は `triemapjava` テスト
（`tests/fixtures/tmj_*.scala` と javac でコンパイルする
`tests/fixtures/java/tmjava/JBase.java`）にあります。

`class C[K, V]` の中で `new C[K, V](…)` と**自分自身を書いた型引数付きで生成**する
メソッドは、戻り値型を書かなくても `C[K, V]` に推論されます。書かれた型引数は
「まだ具体化されていない置き換え用の型パラメータ」と型だけでは区別できないため、
以前はリスト全体（`new C[K, Int]` の `Int` まで）を捨てて値引数から推論し直しており、
値引数が型パラメータに触れない `scala/collection/concurrent/TrieMap.scala` の
`CNode.renewed` などでは `C[Nothing, Nothing]` になっていました。

Type arguments written on an **overloaded** callee now instantiate every
alternative before applicability is weighed, which is SLS 6.26.3 and nsc's
`Infer.inferPolyAlternatives`; they used to arrive only after an alternative
had been picked, so each alternative re-inferred its own instantiation from
the value arguments. A written `new C[…]` is also checked against the class's
declared bounds (nsc's refchecks `checkBounds`) and for having the right
*number* of arguments — only the over-applied direction was reported before.
The tests are `overscore` (`tests/fixtures/ovsc_*.scala`), and
`ovsc_legal.scala` is the guard that the two new rejections do not reach legal
code.

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

型引数を書かない `new Array(n)` は、要素型を**期待型**から取ります。nsc が
`Array` のコンストラクタの型パラメータを `pt` に対して解くのと同じで、
`val a: Array[Int] = new Array(3)` は `newarray int`、`take(new Array(2))` が
`(Array[String])Int` に渡るなら `anewarray java/lang/String` になります。
要素型はどの JVM 配列クラスを確保するかを決めるので、間違えてもコンパイル
エラーにはならず、実行時の `ArrayStoreException` になります。したがって
fixture は**実行**して確かめます（`arrayelem` は両モード、`arrayelem_tag` は
jar モードのみ。期待出力は実 scalac 2.13.16 のもの）。期待型が何も言わない
場合の要素は `Nothing` で、これは `anewarray java/lang/Object` に消去され、
scalac が `ClassTag.Nothing.newArray` で作る配列と同じクラスになります。
要素が `ClassTag` を必要とする抽象型なら、scalac と同じ
`cannot find class tag for element type K` で拒否します（`arrayelem_bad`）。
引数の個数も数えるようになりました。`new Array[Int](10, 10)`（2.10 で消えた
多次元コンストラクタ）と、引数リストを書かない `new Array[Int]` は、以前は
どちらも黙って通り、配列クラスに存在しないコンストラクタへの
`invokespecial` を出していました（`arrayelem_arity_bad`）。

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

Binary inner-class parent constructors retain the enclosing instance named by
imported aliases, including generic aliases, omitted constructor parentheses,
member API objects, forwarded singleton API paths, and API values whose declared
type refers to the enclosing `this`. The `bparent` regression compiles its
library with scalac 2.13.16 and compares client execution under JVM verification.
Qualified companion lookup preserves enclosing lexical bindings; a source-parent
regression also compares execution with scalac in both runtime modes.
Value conversions retry after completing their binary implicit witnesses. The
`proven` fixture checks a bounded Shape witness against scalac, including runtime
output and an incompatible-result rejection. Higher-kinded parameterless results
infer their constructor from singleton underlying types, while narrowing still
requires a valid conversion and subtype evidence.
Abstract implementations without a written `override` use the inherited result
as an expectation and retain narrower inferred results. The `absresult`
fixture checks source and binary parents, overloads, generics and recursion.
The unannotated table projection probe also generates matching SQL.

継承した `lazyZip` などで、JVM の転送メソッドが失った `this.type` を
Scala の宣言情報から保持します。登録先のクラスと実際の宣言元を区別し、
`ArraySeq.lazyZip(...).map(...)` の `BuildFrom` が受け手の型を使うようにします。
`lzorigin` は実 scalac との受理・拒否と実行出力の比較を行います。

値クラスが総称メソッドの境界を通る際は、ブリッジで引数を取り出し、
戻り値を箱に包みます。内部型が参照型や型パラメータの場合も対象です。
匿名クラスで内部表現とブリッジの JVM シグネチャが一致する場合は実装名を
分け、名前付きクラスの同じ衝突は診断します。`vcbridge` は実 scalac と
受理・拒否および実行出力を比較します。

**ユニバーサルトレイト（SLS 5.3.3）と値クラスの本体制限**は nsc の
`Typers.checkEphemeral` をそのまま実装しています（`crates/typer/src/valueclass.rs`）。
nsc ではこれは 1 つの関数で、`where` の語（"value class" か "universal trait
extending from class Any"）だけが呼び出し元によって変わります。

- `trait T extends Any` の本体に書けるのは `def`・型・`import`・ネストした
  クラスだけです。`val` / `var` は
  `field definition is not allowed in universal trait extending from class Any`、
  式文は `this statement is not allowed in ...`、ネストした `object` は
  `implementation restriction: nested object is not allowed in ...` と
  2 行目の `This restriction is planned to be removed in subsequent releases.`
  で拒否します。
- 値クラスの本体（および `def` の本体の**任意の深さ**）にネストした
  `class` / `trait` / `object` を書くことはできません。ただし**匿名クラス**は
  nsc と同じく除外します（`!cd.symbol.isAnonymousClass`、scala/bug#7571）。
  値クラス内の `new I2 { ... }` と `PartialFunction` リテラルは合法です。

`neg/anytrait` と `neg/valueclasses-impl-restrictions` は、scalac の `.check`
ファイルと**行も文言も完全に一致**するようになりました。

ローカルな `class C` と `object C` が**コンパニオンになるのは同じブロックに
書かれたときだけ**です（nsc の `Contexts.lookupSibling`。1 つのメソッドの
2 つのブロックは同じ owner を共有するため、owner では区別できません）。
`Symbol::local_scope` が各ローカル定義のブロックを記録し、
`Checker::companion_scope` が一致を要求します。これがないと
`{ class C { private def x = 0 }; { object C { new C().x } } }` の
`private` が読めてしまいます（`neg/t8002-nested-scope`）。

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

2 つの親から**別々の具体化で**届く基底クラスは、先に届いた方ではなく
**meet（両者の下限）** で読みます。以前は線形化を歩いて最初の具体化を採って
いました（最派生の「到達元」が勝つ、という規則）が、それは nsc の規則ではなく、
足りてもいません。`trait Str[+A] extends LinOps[A, Str[A]] with Iter[A]` は
`IterOps` に `IterOps[A, Str[A]]` と `IterOps[A, Iter[A]]` の 2 通りで到達し、
`Iter` と `LinOps` はどちらも他方の上にはいません。SLS 5.1.2 は最後に書かれた
`Iter` を先に並べるので、`IterOps.tail` は `Iter[A]` と読まれていました——実
scalac 2.13.16 は `Str[A]` と型付けます。nsc は
`BaseTypeSeqs.compoundBaseTypeSeq` で到達したすべての具体化を保持し、
`mergePrefixAndArgs(variants, Variance.Contravariant, _)` で解決します。すなわち
**共変パラメータでは glb（最派生）、反変パラメータでは lub（最汎化）**であり、
「常に最派生を採る」規則は反変側を逆に間違えます。`SymbolTable::base_type_args`
がこの規則になりました。線形化の順序自体は変えていません（`agent/basetypeseq`
がその案を 1551 → 1579 と計測して捨てています）。`@uncheckedVariance` だけが
違う 2 つの到達は同じ型なので 1 つに畳み、注釈のない綴りを残します
（nsc の `=:=` は注釈を見ません）。`Check::base_type_instance` も、対象に到達
しうる親が 2 つ以上あるときはこの merge 済みの答えを読みます。おかげで
「宣言が定義を言い直しているだけか」を、宣言の所有者から定義の所有者への
**すべての経路**を 512 ノードの予算で試して調べる必要がなくなり、親 DAG の深さに
対して指数的なその走査（オーバーロード解決の内側で走っていました）を削除しました。
`tests/fixtures/btmeet_basetypemeet.scala` は共変・反変の両側と
`@uncheckedVariance` の綴りを**実行**して実 scalac 2.13.16 の出力と比較し、
`btmeet_basetypemeet_bad.scala` は scalac と同じ 3 行（23・44・52 行目）で
拒否されることを固定します（`btmeet` テスト）。

**1 つの継承メンバを兄弟の 2 つが override している場合、順序を決めるのは
レシーバの線形化だけです。** `TreeSet[A]` は `Set` 経由で
`IterableFactoryDefaults`（`CC = Set`）を、直接 `SortedSetFactoryDefaults`
（`CC = TreeSet`）を mixin し、どちらも 1 つの `IterableOps.empty` に対して
`override def empty: CC[A @uncheckedVariance]` を書きます。両者は互いの
サブクラスではないので `Check::drop_overridden` の所有者テストは順序を付けられず、
どちらも定義なので宣言／定義テストも効きません。結果、`TreeSet` 内の裸の `empty`
は `<overload Set[A] | TreeSet[A]>` のままでした。nsc の `findMember` は
レシーバの base type sequence を辿って**最初に一致したもの**を採り、それは
`TreeSet` の線形化でより派生側にある `SortedSetFactoryDefaults` の方です。
`Check::drop_sibling_overrides` がこの規則で、`crate::lin::linearize` の順序を
そのまま使います。答えが「対」ではなく「レシーバ」に属することは主張ではなく
計測です——同じ 2 つの trait を mixin 順だけ変えた 2 クラスに対し、実 scalac
2.13.16 はそれぞれ**別の** override を実行します。
規則は狭く保っています。(1) 両方が定義であること（一方が `DEFERRED` なら
nsc は置き換えをやめるため。`agent/liboverload` が「階層が決める」版を計測し、
scalac と乖離することを確認済み）、(2) 所有者が互いに無関係であること
（関係があれば既存の所有者テストの担当）、(3) **両方が override している
メンバが候補集合の中にあること**——`Check::same_signature` は型パラメータを
含む引数を何にでも一致させるので、形だけでは「同じメンバ」とは言えません
（`agent/libanyval`）。さらに `Check::same_member_at` がレシーバの prefix で
両者を substitute してから引数リストを厳密に比較します。これがないと
`trait GA[T] extends GBase[T]` と `trait GB extends GBase[String]` が持つ
**本物のオーバーロード**を消してしまい、`agent/catstail` が slick を 0 →
7 errors にした形と同じになります（実際に計測して確認しました）。
`tests/fixtures/sibover_siblingoverride.scala` は mixin 順を入れ替えた 2 つの
クラス・ライブラリと同じ形・本物のオーバーロード・宣言と定義の同居を**実行**して
実 scalac 2.13.16 の出力と 1 行ずつ比較し、`sibover_siblingoverride_bad.scala` は
scalac と同じ 2 行（35・36 行目）で拒否されることを固定します（`sibover` テスト）。
scala library は 912 → **894 errors**（145 files のまま）、cats・gitbucket・slick
は不変で、slick の 1490 class ファイルは byte 一致です。

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

**空の引数リストが pickle を通るようになりました。** nsc は `def f: T`
（`NullaryMethodType`）と `def f(): T`（引数 0 個の `MethodType`）を別の型として
区別し、呼び出し側にもそれを課します（前者に `f()` と書けば
`T does not take parameters`）。scala-rs は両方を `NullaryMethodType` として書いて
いたため、**自分のソースが書いた括弧のまま呼べないクラスファイル**を出していました
（実 scalac が `println(Keys.empty())` に `Empty does not take parameters`）。
case class 固有の話ではなく、`def f(): T` すべてが対象です。あわせて
`uncurry` が潰す前の**引数節の形**を `Symbol::pickle_clauses` に記録し、pickler は
節ごとに `METHODtpe` を書くようにしました（nsc の pickler は uncurry より前に
走るため）。これで `def makeDatabase[F[_]: Async](): F[…]` が
`[F[_]]()(implicit ev)` として、`def cur(a)(b)` が `(a)(b)` として読まれます。
逆に**過剰修正も禁物**で、引数の無い `name$default$n` ゲッターは nsc と同じく
nullary のままにします（`()` を付けて書くと実 scalac が
`Auto-application to () is deprecated` を出します）。0 フィールド case class の
`copy()` は pickle にあって classfile に無い「幽霊メンバー」だったので、
codegen 側も出すようにしました（実 scalac が `Empty().copy()` を通した先で
`NoSuchMethodError` になっていました）。分割コンパイルの検証は `pickleparams`
テスト（`tests/fixtures/pp_*.scala`）で、正常系は scala-rs がライブラリを、実 scalac
が呼び出し側をコンパイルして**実行**し scalac 同士の出力と一致すること、異常系は
`f()` が同じ行・同じ件数で拒否されることを固定します。

アクセス不能な member の診断は nsc の `ContextErrors.AccessError` と同じ文を組み
立てるようになりました。主語は `underlyingSymbol(sym).fullLocationString`——
`method x in class C`、`variable foo in class Sub2`、`object Inner in object
Outer`——で、module class は `directObjectString` に従って `C$` ではなく `object
C` と綴ります。`from` 側も同じ規則です（`object Use in package xflags`、空パッ
ケージなら `class Q3`）。コンストラクタは nsc と同じく `as a member of <prefix>`
ではなく `in <owner>` になります。`.check` に `cannot be accessed` を含む `neg`
コーパス 26 件で、文言の一致は **T1/T2/T3 = 0 → 3**（`t8002-nested-scope`、
`t3714-neg`、`t3871` が `.check` と 1 バイト違わず一致）。**エラー件数は動きま
せん**——変わるのは診断が何と言うかであって、どのプログラムを拒否するかではあり
ません。残る差は prefix の型がパッケージ修飾されないこと（`object C` に対し nsc は
`object xflags.C`）で、これは `display_type` が**すべての**診断で型を印字する
やり方であって、この診断の問題ではありません。

**`identity` / `locally` / `implicitly` が受け手を捨てなくなりました。**
`gen_apply` は `Predef` の多相組み込みを**名前だけ**で振り分けており、
`gen_predef_poly` は受け手も第 2 引数以降も捨てて
`scala/Predef$.<name>:(Ljava/lang/Object;)Ljava/lang/Object;`——つまり恒等関数——を
出していました。そのためユーザー定義の `identity` / `locally` / `implicitly` は
**自分の引数をそのまま返し**、`mk("m").identity(side("i"), side("j"))` では受け手
`mk("m")` も第 2 引数 `side("j")` も**一度も評価されません**でした。コンパイルは
通り、`-Xverify:all` も通り、実行して初めて分かる種類の誤りです
（`agent/sysout` が直した `println` と同じ根で、同スライスが「独自の dual-run
証拠が要る」として残していたもの）。組み込みは prelude 自身の `Predef` メンバー
だけが持つ `Intrinsic::Identity` / `Locally` / `Implicitly` で判定し、名前だけの
経路はシンボルが解決しなかった呼び出しに限りました。fixture
`tests/fixtures/intrinsicqual_predef.scala` は**実行**し、`--scala-library` と
`--no-scala-library` の両モードで実 scalac 2.13.16 の出力と 1 バイト違わず一致
します。slick の 1490 クラスファイルはこの変更だけでは**バイト単位で不変**です。

**`private` / `protected` なコンストラクタを検査するようになりました。**
`new C(…)` は member selection ではないので `type_select` のアクセス検査が
`<init>` を見ることはなく、さらに primary constructor のシンボルはクラスが書いた
修飾子（`class C private ()` の `ctor_mods`）を**そもそも持っていません**でした。
両方を閉じ、コーパスの `neg/sensitive`、`neg/t4987`、`neg/protected-constructors`
を nsc と同じ行・同じ 1 行目で拒否します。`new` の prefix は構築される型なので、
`class Sub extends Prot("x")`（親コンストラクタ呼び出し）は合法のまま、`Sub` の中に
書いた `new Prot("x")` は実 scalac と同じく拒否されます。合成された `new`
（`v.copy(x = 2)` の下ろし先など）は対象外です——そこでのアクセス判断は `copy`
のものだからです。副作用として `<init>` の修飾子が `ScalaSignature` にも載り、
slick の 1490 クラスのうち **4 つが 1 バイトずつ**変わります（`private` な primary
constructor を持つクラスの pickle されたフラグのみ。バイトコードは不変）。
その 1 バイトが正しいことは、**実 scalac 2.13.16 が我々のクラスファイルを読んで**
以前は通していた `new SepPriv("x")` を拒否するようになったことで裏付けています。
分割コンパイル越し（`neg/t6601`）はまだ通ります——コンストラクタの privacy が
クラスファイルの往復で失われるためで、`pickle_supply.rs` 側の別スライスです。

**`Unit` を返す `Predef` の多相組み込みが、要る値を捨てなくなりました。**
`gen_predef_poly` は結果型が `Unit` のとき自分の結果を必ず `pop` していました。
`identity` / `locally` / `implicitly` は `(Object)Object` に消去されるので、
`Unit` でも参照（`BoxedUnit.UNIT`）を返します。捨ててよいのは**文の位置**だけで、
値が**引数**のときは呼び手のスタックが空になり、`println(identity(()))` は
`VerifyError: Operand stack underflow` でした。nsc と同じく値を残し、`gen_stat`
の文位置の破棄（`discarded_predef_poly`）が落とすようにしました。私有ランタイム
側は同じ食い違いの**裏返し**（消去が引数を box するのに誰も pop せず、
`if (b) identity(()) else side()` が `Inconsistent stackmap frames`）で、同じ述語
で閉じています。fixture `tests/fixtures/unitpop_intrinsic.scala` は両方の位置を
1 つのプログラムに持ち、`-Xverify:all` の下で**実行**して両モードとも実 scalac
2.13.16 の出力と一致します。**コンパイルは通り、実行しなければ分からない**種類の
誤りなので、テストは必ず走らせて出力を比較します。slick の 1490 クラスファイルは
修正前後で**バイト単位で不変**です。

**secondary constructor への `new C(…)` が正しい引数を渡すようになりました。**
`class Sec(val a: Int) { def this(s: String) = … }` に対する `new Sec("abcd")` は
`VerifyError: Bad type on operand stack` でした。**記述子（descriptor）は最初から
正しく**、`gen_new` は typer が選んだコンストラクタから
`invokespecial Sec."<init>":(Ljava/lang/String;)V` を——実 scalac と同じものを——
出しています。誤っていたのはその**引数**で、`erasure::method_param_types` が
「この `new` は引数を何型に合わせるか」という問いにクラスの**最初の `<init>`
メンバ**、すなわち常に primary で答えていました。そのため `String` リテラルが
`Int` に対して消去され、`$unbox` が被さり、`ldc "abcd"; checkcast Integer;
intValue; valueOf` が `String` を宣言するスロットに `Integer` を渡していました。
コンパイルは通り、呼び出し箇所の逆アセンブルを読んでも正しく見え、**実行して
初めて**分かる誤りです。修正は、backend が記述子を作るのに使うのと同じシンボル
（`Apply` が持つ、`pick_ctor_at` が選んだ alternative）を
`method_param_types` に渡すことで、両者が食い違えないようにします。
fixture `tests/fixtures/secondaryctor_new.scala` は**実行**し、両モードで実
scalac 2.13.16 の出力と一致します。逆向き——primary が参照型で secondary が
プリミティブ——も同じ欠陥で、こちらは**クラスファイルから読んだ**クラスで起きます。
コーパスの `run/kmpSliceSearch`（`new scala.util.Random(Integer.parseInt("kmp",
36))`）は `Integer` が `(I)V` に渡されて `VerifyError` でしたが、`fail` → `pass`
になり `.check` と一致します（クラスファイルの差は `Integer.valueOf` 1 命令だけ）。
slick の 1490 クラスファイルは**バイト単位で不変**です——slick の secondary
constructor 4 か所はいずれも primary との差が参照型どうしで、`box_adaptation` が
どちらでも `None` を返すためです（詳細は
[docs/scala-library.md](docs/scala-library.md)）。なお secondary
constructor 側の**デフォルト引数**は未実装のままで、`<init>$default$n` が宣言
されず診断になります（分岐元でも同じ。同ドキュメントに記録）。

**コンストラクタのデフォルト引数が「2 番目以降のパラメータリスト」でも
埋まるようになりました。** `class Curr(a: Int)(b: String = "b" + a)` を
`new Curr(7)()` と呼ぶと、3 引数のディスクリプタに対して 1 引数の
`invokespecial` を出しており、診断なしで通ったうえ実行時に
`VerifyError: Bad type on operand stack` になっていました（黙ったままの
ミスコンパイル）。`new` の引数は JVM のディスクリプタの形に合わせて既に
平坦化されて `fill_defaults_and_implicits` に届くのに、この関数はシンボルから
**平坦化前の** `paramss` を読み直していたため、第 1 クラスだけを見て「足りて
いる」と判断していたのが原因です。あわせて、後続クラスのデフォルトは
先行クラスの引数を**名前で参照してよい唯一の形**（同一クラス内の参照は nsc が
拒否します）なので、その getter `$lessinit$greater$default$n` は先行クラスの
パラメータを取ります。呼び出し側に式を展開して済ませられないため、nsc と同じく
**コンパニオンを合成**して getter を置くようにしました（この形だけ。第 1 クラスの
デフォルトは従来どおり展開で済ませるので、クラスファイルは増えません）。

**「デフォルトを必要としない候補を優先する」nsc の規則を実装しました。**
`class Prefer(n: Int) { def this(k: Int, bump: Int = 5) = ... }` に対する
`new Prefer(1)` は primary で、これまでは両候補を同時に比較して
`ambiguous overload for constructor` になっていました。ただしこの規則だけでは
`new Three(2)("m")()` が「拒否」から「黙って間違った候補」に変わります
（平坦化後の `(2, "m")` を primary がちょうど受け取ってしまう）。nsc は
**第 1 クラスで**コンストラクタを選ぶので、`flatten_curried_new` が畳んだときの
第 1 クラスの長さを報告し、`pick_ctor_at_clause` が候補をその集合に限るように
しました。

**`private[p]` なコンストラクタを、pickle から境界を解決して検査するように
なりました（読み取り側）。** 記録にあった「nsc は `PRIVATE` フラグ＋
`privateWithin` 参照で書く」は誤りで、実際には**フラグは立ちません**
（scalac 2.13.16 が書いた `class Qual private[libp] (...)` の `<init>` は
`flags=0x200`、`PRIVATE`/`PROTECTED` ともに 0）。`Member::private_within` が
境界の単純名を持つようになり、診断は nsc と一語一句同じです。パッケージ内部の
呼び出しは通って**実行**され、slick は `errors=0 classes=1490`／1490 個の
クラスファイルはバイト単位で不変です（過剰拒否なし）。**書き込み側**は未実装で、
scala-rs が書いたクラスファイルには境界が入らないため、自分の reader でも
実 scalac でも通ります（pickle のフォーマット変更が必要。
[docs/not-implemented.md](docs/not-implemented.md) に記録）。

**補助コンストラクタの `this(...)` でも名前付き引数を使えるようになりました。**
`def this() = this(AnyRefMap.exceptionDefault, 16, initBlank = true)` の
`initBlank = true` は**代入ではなく名前付き引数**です。自己委譲の経路だけが
これを知らず、各ペアを `Assign` として型付けしていたため、primary
コンストラクタのパラメータ（補助コンストラクタの本体からは見えます）に対して
`reassignment to val <name>` を出し、残った `Unit` が
`no matching overload for constructor` を追加で出していました。`new C(b = 2, a = 1)`
と `extends B(b = 2, a = 1)` は以前から対応済みで、3 つ目のこの経路だけが
抜けていました。並べ替えられた引数は SLS 6.6.1 のとおり**書かれた順**に評価され、
省略されたデフォルトは平坦な `<init>` ディスクリプタに届きます。あわせて、
名前を宣言しているだけの候補ではなく**適用可能な**候補を選ぶようにしました
（`class C(a: Int, b: Boolean) { def this(a: Int) = … }` に対する `new C(a = 3)`
は `missing argument for parameter b` になっていました）。

**`var` でないものへの代入を拒否するようになりました。** `check_reassignment` は
左辺が `Term` でないものを「解決済みの `x_=` セッタ」とみなして素通しして
いたため、`def v: Int = 1; v = 2` は存在しないフィールドへの `putfield C.v:I` を、
`object O; O = null` は `putfield scala/runtime.O:LO$;` を出していました
（型エラーなし）。危険なのは**エラーメッセージが出ない方の半分**です。
クラスファイルから継承した `var` はアクセサ対 `bv()` / `bv_$eq(int)` と
**private** フィールドとして届くのに、セッタへの書き換えは `Select` 形
（`this.bv = 5`）にしかなく、`bv = 5` は他人の private フィールドへの
`putfield` になり、実 scalac がビルドした親クラスに対して実行時に
`IllegalAccessError` を投げていました。単純名の形も書き換えるようにしています。
検査は `varassign` テスト（実 scalac 2.13.16 との 44 ケースの双方向比較と、
scalac がビルドした親クラスに対する JVM 実行）です。

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
- `tests/assign_probe.sh` — 47 assignment and named-argument snippets, each
  compiled by this compiler *and* by real scalac 2.13.16 and compared on
  accept/reject in **both** directions: what scalac accepts and we reject, and
  what scalac rejects and we accept. The second direction is the one no error
  count can show, and it is where `agent/varassign` found five silently
  miscompiled programs. Reports 19 disagreements of 47 at `daa19440` and one
  after (`b24_setter_only`, recorded in
  [docs/not-implemented.md](docs/not-implemented.md)); exits non-zero above one.
- `tests/expand_fm.py` — expand the seven FreeMarker templates slick's build
  generates Scala sources from, so a measurement covers what sbt would compile.
- `tests/testkit_measure.sh` — the same measurement for `slick-testkit`, slick's
  own test suite, compiled against the class files scala-rs produced for slick.
- `tests/reap_strays.sh` — kill `scala-rs` processes orphaned by a killed test
  run (`--kill`; without it, only reports).

How the fixtures, the dual-run harnesses and the pickle-reader regression tests
are organised is described in [docs/testing.md](docs/testing.md).

`negchecks` テストは、**拒否を増やす**規則（ユニバーサルトレイトと値クラスの
本体制限、ローカルなコンパニオンのスコープ）を両側から固定します。異常系は実
scalac 2.13.16 の出力そのもの——9 件の診断を行と文言で——に合わせ、余分な診断が
1 つも出ないことも検査します。正常系は「違法な形の**合法な隣人**」——`def` だけの
ユニバーサルトレイト、型エイリアスと匿名クラスと `PartialFunction` リテラルを
持つ値クラス、`private` を読むコンパニオン——を**実行**して実 scalac の出力と
比較します。拒否規則は広すぎると動いていたプログラムを壊すので、正常系が
**修正前のバイナリでも同じ出力を出す**ことを確認した上で追加しています。ローカル
コンパニオンの異常系は、行だけでなく実 scalac 2.13.16 の**文そのもの**
（`method x in class C cannot be accessed as a member of C from object C` と
`value y in class D cannot be accessed as a member of D from object D`）を固定
します。

`intrinsicqual` テストは 2 つを固定します。ひとつは `identity` / `locally` /
`implicitly` が受け手と第 2 引数以降を捨てないこと——ユーザー定義の 3 つと
`Predef` の 3 つを 1 本のプログラムに同居させ、出力の接頭辞で見分けたうえで
**実行**し、両モードで実 scalac 2.13.16 の出力と比較します（修正前のバイナリは
jar モードで別の答えを出します）。もうひとつはコンストラクタのアクセス検査で、
異常系（`tests/fixtures/intrinsicqual_ctor_bad.scala` とコーパス 3 件）は実
scalac の行と文で、正常系（`tests/fixtures/intrinsicqual_ctor.scala`——自分の
コンパニオン、`extends` 越しの `protected`、`private[p]`、アクセス可能な別の
オーバーロードを持つクラス）は**実行**して固定します。拒否規則なので、正常系が
**修正前のバイナリでも同じ出力を出す**ことを確認した上で追加しています。

`unitpop` テストは、`Unit` を返す `Predef` の多相組み込みが**値の位置では値を
残し、文の位置では落とす**ことを固定します。片方だけ直すと必ずもう片方が壊れる
（残しすぎればスタックに残骸が出て `Inconsistent stackmap frames`、捨てすぎれば
`Operand stack underflow`）ので、`tests/fixtures/unitpop_intrinsic.scala` は
両方の位置と、非 `Unit` の同じ 3 つを 1 本のプログラムに入れています。文の位置は
どれも直後に分岐を置いてあります——残骸は次の stackmap frame まで生き延びて
初めて見つかるからです。**コンパイルも通り `javap` も通る**種類の誤りなので、
検査は `java -Xverify:all` での**実行**と実 scalac 2.13.16 との出力比較だけです。
両モードで回し、修正前のバイナリでは jar モードが `Operand stack underflow`、
私有ランタイムが `Inconsistent stackmap frames` で落ちることを確認しています。

`secondaryctor` テストは secondary constructor への `new` を固定します。この種の
欠陥はエラー数にもクラスファイル数にも現れず、呼び出し箇所の `javap` すら正しく
見えるので、**11 本すべてが `java -Xverify:all` でプログラムを実行**します。
fixture `tests/fixtures/secondaryctor_new.scala` は、クラス内部・コンパニオン・
無関係なオブジェクトからの呼び出し、消去後の記述子が 1 引数だけ違う 2 つの
secondary、別の secondary へ委譲する secondary、値クラスを取る secondary、
デフォルト引数、そして同じクラスの `new C(primary の引数)` を 1 本のプログラムに
まとめ、実 scalac 2.13.16 の出力と両モードで比較します（修正前のバイナリは両
モードとも `VerifyError`）。答えだけでなく**バイトコード**も固定していて、引数の
前に unbox/box 対が入らないことを検査します——記述子の方を primary に合わせて
「一致させる」修正でも答えは変わらないためです。回帰側は、secondary を持たない
普通の `new`、そして**本来必要な boxing** が消えていないこと（`Any` スロットへの
プリミティブ、プリミティブスロットへの箱）を実行して確かめます。

線形化（SLS 5.1.2）は `linearization` テストで二重に検査します。正常系は深く広い
ダイヤモンド継承の `super` 連鎖を実行し、同じソースを実 scalac 2.13.16 で
コンパイル・実行した出力と直接比較します（`super` の連鎖がそのまま線形化なので、
停止のためのガードが線形化を黙って切り詰めれば出力から trait が 1 つ消えます）。
異常系は閉路のある `extends` グラフが scalac と同じ行・同じ文言で拒否されること、
そして**そもそも停止すること**（60 秒の上限つき）を検査します。

`triemapjava` テストは、`src/library/scala/collection/concurrent/TrieMap.scala`
——ライブラリ計測で最も errors の多かったファイル（46 件）——を出発点にした 4 つの
欠陥を固定します。まず計測が Java 側を見ているかを確かめました：
`tests/scalalib_measure.sh` はライブラリの 32 本の `.java` が生む 33 個の
クラスファイルを `-cp` に置いて回すので、**Java は見えており、除外は不要**でした
（`skipped` は 0 です）。4 つのうち最大のものは Java と無関係で、`class C[K, V]` の
中に書かれた `new C[K, V](…)` の型引数リストが捨てられ、値引数が型パラメータに
触れないため `C[Nothing, Nothing]` に推論されるというものです。`tmj_selfctor.scala`
は `CNode` と同じ形——**どのコンストラクタ引数も `K`/`V` に触れない**——で 4 つの
代入位置を作り、両モードで**実行**して実 scalac 2.13.16 の出力と比較します
（修正前のバイナリは 4 件のエラーを出します）。異常系
`tmj_selfctor_bad.scala` は、書かれたリストを信じることで初めて意味を持つ 2 件
——`Cell[V, K]` を `Cell[K, V]` に代入できないこと、書かれた `[String, V]` に
`k: K` を渡せないこと——を実 scalac と同じ行・同じ文言で拒否します（修正前は
1 件しか出ず、しかも文言が違いました）。残り 3 つは Java 側で、
`tests/fixtures/java/tmjava/JBase.java` を **javac でコンパイルしてクラスファイル
として読ませ**、パッケージプライベートな static を `import JBase._` と修飾付きの
両方で読み、総称フィールドを継承した型で読み、そして**実行**します。異常系
`tmj_java_bad.scala` は同じ static が別パッケージからは拒否されることを、
実 scalac と同じ行・同じ文言で固定します。

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

`neglit` テストは負のリテラル（`val b: Byte = -3`）の狭化変換を固定します。
`-3` はパーサーの時点で `unary_-` の呼び出し（`Apply(Select(Literal(3),
"unary_-"), Nil)`）に脱糖され、そのメソッドの宣言戻り値型は狭められていない
`Int` なので、SLS 6.26.1 の狭化を適用する `Typer::adapt`（正常系の裸のリテラルは
`Type::Constant` のまま届く箇所）にたどり着いた時点で定数性が失われていました。
`negated_int_literal`（`crates/typer/src/check_infer.rs`）は木の形からその定数性を
復元し、既存の一本の狭化ルールに乗せるだけで、狭化ルールを増やしてはいません。
`tests/fixtures/negl_run.scala` は `Byte`/`Short`/`Char` への境界値（`127`/`-128`）
と `Long`/`Float`/`Double` への幅拡張を含め、コンパイルするだけでなく**実行**して
実 scalac 2.13.16 の出力と比較します（狭化が正しい型だが誤った値——符号の欠落や
切り捨て——を生んでも型検査だけでは見つからないため）。`negl_bad_range.scala`
（`-300`）と `negl_bad_boundary.scala`（`128`・`-129`）は範囲の境界を、
`negl_bad_nonconst.scala`（`val n = 3; val b: Byte = -n`）はこの折り畳みが
リテラルの直接の否定だけに絞られていて非定数式には効かないことを、それぞれ
scalac と同じ行で固定します。この欠陥は `src/library` の計測を 1 件も動かしません
（ライブラリは狭化対象への負のリテラルを一度も書かない）——正しさの修正であって、
歩留まりの修正ではありません。

`varargsrecv` テストは、可変長引数を**値として使う**ときの受け手の型を固定します。
`xs: T*` は本体では `scala.collection.immutable.Seq[T]` であり、`T*` は宣言の
書き方にすぎません。`Check::seq_of` はこれを名前 `Seq` の**スコープ検索**で
求めていましたが、nsc の `definitions.SeqClass` は固定シンボルで、`scala.Seq` は
そのエイリアスです。スコープ検索は両方向に誤ります。**違う `Seq` を拾う**方は
無害ではありません——`gen_desc` は可変長引数に必ず
`Lscala/collection/immutable/Seq;` を書くので、`object Main { class Seq[A] { def
tag = "MINE" }; def f(xs: Int*) = xs.tag }` は修正前は**コンパイルが通り**、
実行時に `ClassCastException: ArraySeq$ofInt cannot be cast to Main$Seq` で
落ちていました（診断はどこにも出ません）。**あるのに見つけられない**方は
`src/library` の計測そのもので、`scala/package.scala` の
`type Seq[+A] = …` というエイリアス（`package scala` を書くファイル）と、
`package scala.jdk` のような修飾付き 1 節を書くファイルでは束縛すら無いことの
2 つでした。現在は JVM 名が `scala/collection/immutable/Seq` のクラスを
`scala` パッケージとパス（ソースの `scala.collection.immutable`）から引きます。
正常系 `tests/fixtures/varargsrecv.scala` はプリミティブ・`Unit`・型パラメータ・
`Array` 要素・空呼び出し・`xs: _*` の転送を 1 本に入れ、同じ object の中で名前
`Seq` を自前のクラスとエイリアスに束縛したうえで**実行**し、実 scalac 2.13.16 の
出力と比較します（`expected/varargsrecv.txt` が scalac のものであることもテストが
毎回作り直して確かめます）。記述子と呼び出し側の包み方（`wrapIntArray` /
`wrapUnitArray` / `wrapRefArray`、空なら `Nil`）も `javap -c` で scalac と突き合わせ
ます。異常系は 3 つ——自前の `Seq` のメンバーは受け手に無いこと（scalac と同じ文）、
`*` 引数が節の最後でないこと（`*-parameter must come last`。メソッド・`case class`・
クラスの 3 箇所、scalac と同じ文言と件数。修正前は**黙って通って**おり、
scala/scala 自身の `neg/parstar` はこれで `fail` → `pass` になります）、
`val v: Int*` が拒否されること——です。私有ランタイムには
`scala.collection.immutable.Seq` が無いため、そのモードでは可変長引数を値として
使えず、テストはその**拒否**を固定します（黙って通しません）。

別コンパイルでは型パラメータの上下限と型エイリアスの宣言種別を
`ScalaSignature` に保持します。`crates/cli/tests/existential.rs` は実際の
scalac を読み手にして、正例の JVM 実行と不正な型引数の拒否を検証します。
メソッドの引数節（空リストを含む）は書き手側では nsc と同じ入れ子の
`METHODtpe` として保存されるようになりました。ただし `-cp` のクラスディレクトリを
読む簡易デコーダ（`crates/backend/src/pickle.rs` の `unpickle`）はメンバーを
1 本の平坦な引数リストとして持つため、**自分が出したクラスファイルに対しては**
`T.cur(1)(2)` をまだ受け付けません（実 scalac は受け付けます）。
`E#Elem` の接頭部にも未解決の制限があり、
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
