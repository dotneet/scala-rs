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

`agent/cats2` スライス（cats-effect の summoner が `F.type` を返す件と、文字列補間の `$this`）のフィクスチャは接頭辞 `c2`（`c2_thisinterp` / `c2_thisinterp_bad`）で、同じ理由から `crates/cli/tests/cats2.rs` に置いています。`c2_thisinterp.scala` はクラス・トレイト・`object`・ラムダの中の `s"… $this …"` を通し、私有ランタイムと `--scala-library` の両方で `-Xverify:all` の下に走らせて real scalac 2.13.16 の出力と一致することを見ます。`c2_thisinterp_bad.scala` は、`$this` を特別扱いしたことで `$name` が何でも通るようになっていないこと（`not found: value nosuchvalue`）を固定します。`a_summoner_returning_its_own_parameters_type_crosses_a_jar` は **実 scalac** で `def apply[F[_]](implicit F: TC[F]): F.type = F` という cats-effect 形の summoner と、`val TC = tinyeff.TC` を持つパッケージオブジェクト（`import cats.effect.Async` が通る経路そのもの）を持つ小さなライブラリをコンパイルして jar に固め、`ScalaSignature` だけを通して `TC[G].flatMap(fa)(…)` が解決し `java -Xverify:all` で走ることを見ます。自前の pickle ライタはパラメータを指す `SINGLEtype` を書かないので、この fixture は scalac が書いたものでなければ意味がありません（scalac が無い環境では skip します）。同じテストで、witness の無い `TC[Crate]` は `could not find implicit value of type TC[Crate]` のままであることも見ます。

`agent/cats3` スライス（by-name の仮引数がプロトタイプにならなかった件と、オーバーロードされたメンバの後続の節が宣言から読み直されていた件）のフィクスチャは接頭辞 `c3`（`c3_infer` / `c3_infer_bad`）で、同じ理由から `crates/cli/tests/cats3.rs` に置いています。`c3_infer.scala` は cats を 1 行も使わずに 2 つの根を並べます: `def >>[B](fb: => F[B])(implicit ev: Bind[F]): F[B]` に `good.fold(boom, _ => new Box(()))` を渡す形（期待型が `B = Unit` と言っているので、by-name の仮引数がそのまま引数のプロトタイプになる）と、`Duration` / `FiniteDuration` のように**オーバーロードされた** `tag` の `implicit t: TC[F, _]` が受け手の `F` で読まれる形です。私有ランタイムと `--scala-library` の両方で `-Xverify:all` の下に走らせ、`scalac_agrees_c3_infer_output` で real scalac 2.13.16 の stdout とも一致することを見ます（**修正前の main では 4 件のエラーで落ちます**。うち 2 件は `could not find implicit value of type TC[F, _]`——slick が報告していた `GenTemporal[F, _]` と同じ、受け手ではなく宣言側の `F` です）。`c3_infer_bad.scala` は、プロトタイプが「なんでも通す許可」になっていないこと——期待型**なしで**先に推論された `val` は依然として `type mismatch`——と、別の型構築子のための witness は依然として見つからないこと（`could not find implicit value of type TC[Box, _]`）を固定し、`scalac_agrees_c3_infer_bad_is_rejected` が real scalac も同じ 2 行で同じ 2 件を出すことを見ます。`cats_flat_map_then_and_timeout_to_compile` は Coursier キャッシュに cats-core / cats-kernel / cats-effect{,-kernel,-std} があるときだけ走り、`a >> e.fold(F.raiseError, _ => F.unit)` と `wait0.timeoutTo(timeout, F.raiseError[Unit](…))`——slick の `BasicBackend.scala` と `ConcurrencyControl.scala` そのものの形——が**本物の cats で**通ることを見ます（`scalac_agrees_cats_flat_map_then_and_timeout_to` が同じ 11 行を real scalac にも通します）。`cats_syntax_conversion_completes_its_own_witness` は 3 つ目の根——`trait C3Db[F[_]] { implicit val asyncF: Async[F]; def run(fa: F[Long]) = fa.flatMap(…) }`（slick の `BasicDatabaseDef`）——を**単独のコンパイル単位で**通します。`Async` に触れる行を 1 行足すだけで直す前でも通ってしまうので、単独であることが再現条件です。

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

