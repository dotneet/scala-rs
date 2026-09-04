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

