# def マクロ設計メモ

Scala 2.13 の **def マクロ**（`def f = macro impl`）を scala-rs で扱うための設計。
最終目標は slick を素通しでコンパイルすることで、slick が使うマクロは 2 つだけである。

- `slick/lifted/ShapedValue.scala`
  `def mapTo[R <: Product with Serializable](implicit rCT: ClassTag[R]): MappedProjection[R] = macro ShapedValue.mapToImpl[R, U]`
- `slick/lifted/TableQuery.scala`
  `def apply[E <: AbstractTable[_]]: TableQuery[E] = macro TableQueryMacroImpl.apply[E]`

この文書は **フェーズ 0（調査と設計）の成果物**である。実装は途中でも、設計は残す。
実現不可能・非現実的な部分は、そうと明記する。

---

## 0. 要約（先に結論）

- 実行モデルは **JVM ブリッジ方式**を選ぶ。マクロ実装を我々の AST 上で解釈実行する案は採らない。
- 根拠は「`scala.reflect.macros.blackbox.Context` の抽象メンバは 72 個しかなく、すべて
  `scala.reflect.api.*` 型を受け渡す普通の JVM インタフェースメソッドである」ことと、
  「`c.universe` に **`scala.reflect.runtime.universe`（scala-reflect.jar 同梱の完全実装）**を
  そのまま差せる」ことである。後者は型レベルで保証されている
  （`scala.reflect.internal.SymbolTable extends scala.reflect.macros.Universe`,
  `scala.reflect.runtime.JavaUniverse extends scala.reflect.internal.SymbolTable`）。
- この設計は**机上の空論ではなく、動く prototype で検証済み**である（§2.3）。
  scalac でコンパイルしたマクロ実装を、Java の `java.lang.reflect.Proxy` で作った Context 越しに
  呼び出し、`reify` / **quasiquote** / `WeakTypeTag` の 3 パターンすべてが期待どおりの
  reflect Tree を返すことを確認した。
- ただし **slick の 2 マクロに到達するまでの距離は非常に長い**。ボトルネックは
  「マクロを展開すること」ではなく「**マクロ実装のソース自体を scala-rs でコンパイルできること**」
  である（§6.2）。特に `mapToImpl` は本体の約 95% が quasiquote である。
- そして quasiquote と `reify` は **JVM ブリッジでは展開できない**。これらは
  scala-reflect.jar に実装 classfile が存在せず、**nsc のコンパイラ内蔵（fast track）マクロ**
  だからである（§6.2 で実証）。つまり quasiquote / reify だけは
  **scala-rs 自身が組み込みとして実装するしかない**。これが最大の残作業である。

---

## 1. nsc が def マクロをどう扱うか

### 1.1 定義側

```scala
def f(x: Int): Int = macro impl
```

- `macro` は def の右辺だけに現れるソフトキーワード。右辺は式ではなく
  **マクロ実装への参照**（`Ident` / `Select` / それらの `TypeApply`）に限られる。
- 型検査後、マクロ def のシンボルには `MACRO` フラグが立つ。値は `1L << 15`
  （`scala.reflect.internal.HasFlags.isMacro` のバイトコードで確認）。ビット 15 は
  **pickle されるフラグ領域**にあるので、そのまま classfile に残り後続の run が読む。
- マクロ def は**戻り値型の省略を許さない**（実装の戻り値から推論できないため）。
- マクロ def は**バイトコードに残らない**。呼び出し側がすべて展開で消えるので、
  実体としてのメソッドは不要である。
- 別コンパイル単位から展開できるように、nsc は「マクロ def → マクロ実装」の対応を
  **pickle 内の `@scala.reflect.macros.internal.macroImpl(...)` アノテーション**として
  classfile に焼き込む。このアノテーションクラスは（reflect ではなく）
  **scala-library.jar にある**ので、マクロ def を含む classfile はユーザの実行時
  クラスパスにあるクラスだけを参照する。

  中身は `Macros$MacroImplBinding` の 6 フィールドで、pickle 上のキー名も確認済み:

  | キー | 内容 |
  | --- | --- |
  | `macroEngine` | `"v7.0 (implemented in Scala 2.11.0-M8)"` 固定。不一致は展開エラー |
  | `isBundle` | 実装が「bundle クラス」のメソッドか（`class B(val c: Context)`） |
  | `isBlackbox` | 実装の `c` の型が blackbox か whitebox か。**展開側が箱を知る唯一の手段** |
  | `className` | 実装を持つクラスのバイナリ名。object なら末尾 `$`（`pkg.Foo$`） |
  | `methodName` | 実装メソッド名 |
  | `signature` | `List[List[Fingerprint]]` — 引数の作り方（下表） |

  `Fingerprint` は `Int` の value class:

  | 値 | 意味 |
  | --- | --- |
  | `Other` = -1 | そのまま渡す（`Context` 自身など） |
  | `LiftedTyped` = -2 | 引数 Tree を `c.Expr[T]` に包む |
  | `LiftedUntyped` = -3 | 生の `c.Tree` を渡す |
  | `Tagged(i)` ≥ 0 | マクロ def の第 i 型パラメータの `WeakTypeTag` を渡す |

  型引数（`macro Impl.impl[A, B]` の `[A, B]`）は名前つきフィールドではなく、
  アノテーション tree の `TypeApply` 構造から復元される。

### 1.2 呼び出し側（展開）

- 展開は **typer フェーズの中**で起きる。マクロ専用フェーズは無い。展開単位は
  **macro application**、つまり `Apply` / `TypeApply` を含めた**一番外側**のノードである
  （`M.f` 単独ではなく `M.f(1)`）。
- 展開結果の Tree は、呼び出し側で**必ず型検査し直される**。nsc はマクロが返した木を
  そのまま信用しない。
- **blackbox**: 展開結果を `Typed(expanded, TypeTree(innerPt))` で宣言型に**明示的に上書き**し、
  **1 回だけ**型検査する（`innerPt` は宣言戻り値型に呼び出し地点の型引数を代入したもの）。
  展開結果自身のより詳しい型は**捨てられる**。この 1 行の ascription から、
  blackbox マクロが「戻り値型を絞れない / 構造型を作れない / 型推論を駆動できない /
  extractor マクロになれない」という制約がすべて出る。
- **whitebox**: 型検査を **3 回**行う。`#0` は `WildcardType` に対して（implicit 無効）で
  展開結果の実際の型を知り、未確定型パラメータを `inferExprInstance` で具体化、
  `#1` を `innerPt`、`#2` を `outerPt` に対して行う。絞られた型は**保持される**。
- 型引数が未確定のときのために **delay 機構**（`delayed` / `undetparams` /
  `hasPendingMacroExpansions`）がある。展開を保留し、推論が進んでから再開する。
- slick の 2 マクロは**どちらも blackbox** なので、当面 whitebox は不要。

### 1.3 実行

- nsc はマクロ実装を **JVM 上で本当に実行する**。専用のクラスローダ
  （`-Ymacro-classpath` またはコンパイル時クラスパス。`ScalaClassLoader.URLClassLoader`、
  ファイル更新時刻でキャッシュ）で実装クラスをロードし、Java リフレクションで呼ぶ。

```
classLoader = URLClassLoader(-Ymacro-classpath ｜ -classpath)
receiver    = isBundle ? ctor(Context).newInstance(c)
                       : ReflectionUtils.staticSingletonInstance(className)   // MODULE$
method      = Class.forName(className).getMethods.filter(_.getName == methodName).head
                                                  // オーバーロードは定義側で禁止済み
invoke      = isBundle ? method.invoke(receiver, others…)
                       : method.invoke(receiver, (c +: others)…)
others      = signature を Fingerprint で解釈して組み立てた Object[]
```

- したがって **マクロ実装は、展開が起きるコンパイル実行より前にコンパイル済みでなければならない**。
  これは特別なチェックではなく `Class.forName` が失敗するだけである。nsc のエラー文言:
  「macro implementation not found ...（最もよくある理由は、マクロ実装を、それを定義した
  のと同じコンパイル実行の中で使おうとしていることです）」。
  同じ**ファイル**内に実装と def を並べるのは可（slick はこの形）。同じ**実行**の中で
  「実装を定義しつつ、その場で展開する」ことはできない。
- 引数の受け渡しは：第 1 引数に `Context`、以降マクロ def の引数それぞれに対応する
  `c.Expr[T]`（または生の `c.Tree`）、末尾に型パラメータぶんの `c.WeakTypeTag[T]`。
- **fast track**: `reify` / quasiquote / `materializeClassTag` / `materializeTypeTag` /
  `StringContext.f` などは classloader を通らず、コンパイラ内蔵の実装に短絡する。
  これが §6.2 の核心である。

### 1.4 シグネチャ規則

マクロ def

```scala
def f[T1, …](a1: A1, …)(b1: B1, …): R = macro impl[T1, …]
```

に対して、実装は

```scala
def impl[T1, …](c: Context)(a1: c.Expr[A1], …)(b1: c.Expr[B1], …)
                (implicit t1: c.WeakTypeTag[T1], …): c.Expr[R]
```

の形でなければならない。`object` の代わりに **bundle** 形式も許される。

```scala
class Bundle(val c: blackbox.Context) {
  def impl[T1, …](a1: c.Expr[A1], …): c.Expr[R]   // Context はコンストラクタ側
}
```

規則（`DefaultMacroCompiler$MacroImplRefCompiler` の検査項目そのまま）:

- `c` は第 1 引数リストの第 1 引数（object 形式）／唯一のコンストラクタ引数（bundle 形式）。
  **その静的型が blackbox / whitebox を決める。**
- 各値引数はメタレベルを 1 段上げる: `Ai` ⇒ `c.Expr[Ai]`。2.11 以降は生の `c.Tree` も可。
- 戻り値も同様: `R` ⇒ `c.Expr[R]`、または `c.Tree`
  （slick の `mapToImpl` は `Tree` を返す）。
- **引数名が def 側と一致**すること。vararg 性も位置ごとに一致すること。
- 型パラメータは 1 対 1 で対応し、末尾の implicit リストに `c.WeakTypeTag[Ti]` を置ける
  （省略可。省略すればタグが来ないだけ）。**それ以外の implicit 引数は禁止**。
- 実装は `public` で、**オーバーロードされていない**こと（実行時の解決が
  `getMethods.filter(name).head` なので）。
- 参照の形が違えば `macro implementation reference has wrong shape` を出す。

---

## 2. 実行モデルの選択

### 2.1 案 A: 我々の AST 上のインタプリタ

マクロ実装の Scala ソースを scala-rs で構文解析し、その AST を Rust 側のインタプリタで
実行する。`scala.reflect` API は Rust 側の型で自前実装する。

- 利点: JVM 不要。コンパイル時依存が増えない。
- 欠点（致命的）:
  - `scala.reflect.api` は巨大である。Tree / Type / Symbol / Name / Constant / Mirror /
    Position / Liftable / Unliftable / TypeTag / Printers / ReificationSupport …。
    slick の 2 マクロだけでも実際に触れる面は §3 のとおり広い。
  - インタプリタは「Scala のサブセットを実行できる処理系」を新規に作るということであり、
    コンパイラ本体とは別に、クロージャ・パターンマッチ・implicit・コレクションまで要る。
  - そして**再実装した結果が本物と一致する保証がない**。マクロは「本物と同じ木を吐く」ことが
    すべてなので、ここがずれると slick は動かない。

**採らない。** 工数が大きいだけでなく、正しさの根拠が持てない。

### 2.2 案 B: JVM ブリッジ（採用）

マクロ実装を JVM 上で本物として実行する。`Context` は我々が用意し、`c.universe` には
**scala-reflect.jar の `scala.reflect.runtime.universe`** を差す。

```
scala-rs (Rust)                      macro engine (JVM)
──────────────                       ──────────────────
呼び出し地点を見つける
  ↓ 展開要求（実装クラス/メソッド、
    引数 Tree、型引数）を直列化
                        ──────→     Context を組み立てる
                                       universe = scala.reflect.runtime.universe
                                       mirror   = runtimeMirror(macro classpath)
                                     引数 Tree を universe の Tree に組み立てる
                                     実装メソッドをリフレクションで呼ぶ
                                       ↑ reify / quasiquote / WeakTypeTag は
                                         本物の実装がそのまま走る
                                     戻ってきた Tree を直列化
                        ←──────
  Tree を scala-rs の AST に変換
  呼び出し地点で型検査し直す
```

決め手は次の 2 点である。

1. `blackbox.Context` の抽象メンバは **72 個**しかなく、すべて `scala.reflect.api.*` を
   受け渡す普通のインタフェースメソッドである。実装は我々が書ける量である。
   （`javap -cp scala-reflect.jar scala.reflect.macros.blackbox.Context` および親トレイト 11 個で確認）
2. `c.universe` に差すべき `scala.reflect.macros.Universe` の**完全実装が既に存在する**。

```
scala.reflect.internal.SymbolTable  extends scala.reflect.macros.Universe
scala.reflect.runtime.JavaUniverse  extends scala.reflect.internal.SymbolTable
scala.reflect.runtime.universe: scala.reflect.api.JavaUniverse (= JavaUniverse の値)
```

nsc は `c.universe` に自分自身（`Global`）を差す。我々は代わりに実行時ユニバースを差す。
`Tree` を組み立てるだけの用途では、この 2 つは同じインタフェースの別実装にすぎない。

### 2.3 prototype による検証（実施済み）

「Java の `java.lang.reflect.Proxy` で `blackbox.Context` を作り、`universe()` に
実行時ユニバースを返す」だけの約 180 行の probe を書き、scalac でコンパイルした
マクロ実装を実際に呼び出した。JDK 17 の `InvocationHandler.invokeDefault` により、
トレイトのデフォルト実装（`weakTypeOf` など）は本物が走る。
コードと再現手順は [`docs/macro-engine-prototype/`](macro-engine-prototype/) にある。

検証したマクロ実装と結果：

| パターン | 実装 | 得られた Tree |
| --- | --- | --- |
| 素の Tree 構築 | `c.Expr[Int](Literal(Constant(42)))` | `Literal(Constant(42))` |
| `reify`（= slick `TableQueryMacroImpl` の形） | `c.universe.reify { Helper.hello(7) }` | `Apply(Select(Ident(Helper), TermName("hello")), List(Literal(Constant(7))))` |
| **quasiquote**（= slick `mapToImpl` の形） | `c.Expr[Int](q"${x.tree} + 1")` | `Apply(Select(Literal(Constant(41)), TermName("$plus")), List(Literal(Constant(1))))` |
| `WeakTypeTag` | `c.Expr[String](Literal(Constant(t.tpe.toString)))` | `Literal(Constant("String"))` |

つまり **reify も quasiquote も、コンパイル済みであれば実行時ユニバース上でそのまま動く**。
これが案 B を採る最大の実証的根拠である。
（ここで動いているのは、scalac が既に脱糖・コンパイルした `Syntactic*` / `TreeCreator`
呼び出しである。**ソースから脱糖する部分は別問題**であり、それが §6.2 である。）

この probe で分かった運用上の注意：

- `reify` が生成する `TreeCreator` は `mirror.staticModule("…")` でシンボルを引く。
  したがって **engine の JVM クラスパスには、コンパイル対象が参照するクラスも載せる必要がある**。
  マクロ実装だけを載せると `ScalaReflectionException: object Helper not found` になる。
- `c.Expr[T](tree)` は `universe.Expr(mirror, FixedMirrorTreeCreator(mirror, tree))(tag)` に
  展開して実装すればよい（`scala.reflect.internal.StdCreators$FixedMirrorTreeCreator`）。

### 2.4 案 B の正直なコスト

- **新しいコンパイル時依存**が 2 つ増える: JVM と `scala-reflect.jar`。
  現在 scala-rs は scala-library.jar すら任意（`--no-scala-library` で私有ランタイム）である。
  マクロは「jar がある時だけ動く機能」になる。jar が無いときは**黙って通さず診断を出す**。
- engine は Rust ではなく **Java で書く**必要がある（Scala のトレイトを Java から実装する）。
  ビルドに `javac` が要る。ビルド済み engine を同梱するか、初回に `javac` するかは別途決める。
- プロセス間の直列化フォーマットを決める必要がある（§4）。

### 2.5 却下した中間案

- **scala-compiler.jar をそのまま呼ぶ**: マクロ展開だけ nsc に委譲する案。これは
  「scalac を呼んでいる」のと変わらず、scala-rs が Scala コンパイラである意味を失う。
  ベンチマークとしても不正である。採らない。
- **展開結果をソース文字列で受け取り、scala-rs のパーサで読み直す**:
  §4 で「表現形式」として部分的に採用する。ただし `showCode` はシンボルを落とすので、
  これ単独では健全でない（同名の別シンボルを取り違える）。§4.3 の限界を参照。

---

## 3. 実装が必要な reflect API の最小サブセット

**採取方法の但し書き**: このマシンに slick のソースチェックアウトは無い。
以下は Coursier キャッシュにあった **コンパイル済み slick 3.4.1**
(`slick_2.13-3.4.1.jar`) を `javap -c -p` で読んで採取した実測値である。
3.4.1 では `mapToImpl` は `Shape.scala`、`TableQueryMacroImpl` は `Query.scala` にあり、
課題文で言及されている 3.5.x の `scala-2/slick/lifted/` 配置とはファイル構成が違う。
API の面としてはほぼ同一と見てよいが、**ソース文言そのものは未確認**である。
確定させるには `git clone https://github.com/slick/slick` が要る。

slick の 2 マクロがバイトコード上で実際に触る面。**engine 側で我々が実装するのは `Context` だけ**であり、
`universe` 側のメンバは scala-reflect.jar の本物がそのまま動く点に注意。
つまり下表は「engine が壊れていないか」を測るチェックリストであって、
「Rust で書き直す一覧」ではない。

### 3.1 Context（我々が実装する 72 メソッド）

slick が実際に使うのは以下だけである。残りは
`UnsupportedOperationException("… is not implemented")` で**明示的に落とす**。

| メンバ | 使う側 | 実装方針 |
| --- | --- | --- |
| `universe` | 両方 | 実行時ユニバースを返す |
| `mirror` | 両方（間接） | `universe.runtimeMirror(macroClassLoader)` |
| `Expr` / `Expr(tree)(tag)` | 両方 | §2.3 のとおり |
| `WeakTypeTag` / `TypeTag` | 両方 | `universe` の同名コンパニオンを返す |
| `weakTypeOf` / `typeOf` / `symbolOf` | 両方 | トレイトの default 実装が走る |
| `prefix` | `mapToImpl` | 呼び出し地点のレシーバ Tree から `Expr` を作る |
| `enclosingPosition` | `mapToImpl` | 呼び出し地点の Span を `Position` に変換 |
| `abort(pos, msg)` | `mapToImpl` | 例外を投げ、Rust 側でエラー診断に変換する |
| `freshName` | quasiquote 経由 | 単調増加カウンタ |

`typecheck` / `inferImplicitValue` / `inferImplicitView` / `parse` / `eval` /
`enclosingClass` などは **slick は使わない**。これらは「呼ばれたら落ちる」でよい。
（`typecheck` と `inferImplicitValue` は本質的に「コンパイラ本体を engine から呼び戻す」
ことを意味し、実装するなら engine → Rust の逆方向 RPC が要る。§6.4 のリスク参照。）

### 3.2 `TableQueryMacroImpl.apply` が触る universe メンバ

`Function` / `ValDef` / `Modifiers` / `Flag.PARAM` / `TermName` / `Ident`（`Symbol` 版と
`Name` 版の両方）/ `Select` / `New` / `TypeTree(tpe)` / `Apply` / `EmptyTree` /
`termNames.CONSTRUCTOR` / `typeOf[Tag]` / `rootMirror` / `reify` の
`TreeCreator`・`TypeCreator`（`internal.reificationSupport.mkIdent` / `mkTypeTree`、
`Mirror.staticModule` / `staticClass`）。
Symbol / Type 面は `WeakTypeTag.tpe` と `Type.typeSymbol` **のみ**。

### 3.3 `ShapedValue.mapToImpl` が触る universe メンバ

本体はほぼ全部 quasiquote（`q` / `tq` / `pq` / `cq`）である。脱糖後は
`internal.reificationSupport.Syntactic*` が 209 箇所：
`SyntacticSelectTerm`(60) / `SyntacticTermIdent`(35) / `SyntacticSelectType`(14) /
`SyntacticFunctionType`(12) / `SyntacticValDef`(11) / `SyntacticApplied`(11) /
`SyntacticAppliedType`(10) / `SyntacticFunction`(8) / `SyntacticTypeIdent`(7) /
`SyntacticEmptyTypeTree`(6) / `SyntacticNew`(4) / `SyntacticDefDef`(4) /
`SyntacticBlock`(3) / `SyntacticPartialFunction`(3) / `SyntacticSingletonType` /
`SyntacticExistentialType` / `SyntacticAssign` / `FlagsRepr` / `freshTermName` /
`freshTypeName` / `mkRefTree`。

直接使う Tree コンストラクタは `TermName`(107) / `TypeName`(37) / `Typed`(15) /
`Modifiers`(13) / `Bind`(5) / `CaseDef`(4) / `EmptyTree`(22) / `noSelfType` /
`NoSymbol` / `This` / `Super` / `TypeDef` / `TypeBoundsTree` / `Constant` /
`symbolOf` / `Liftable`（`liftTypeTag` 26 回ほか）。

Symbol / Type 面は `WeakTypeTag.tpe` / `Type.typeSymbol` / `TypeSymbol.isClass` /
`.asClass.isCaseClass` / `.fullName` / `.name.toTermName` / `.companion` /
`Symbol.info` / `Type.decls.collect` / `Type.member(Name)`。
つまり **case class のフィールド列挙**をする。implicit 探索・アノテーション読みは無い。

**重要**: 上記はすべて scala-reflect.jar の本物が担当する。我々が用意するのは
Context と、Tree の入出力変換だけである。

---

## 4. 我々の AST ↔ reflect Tree の変換

### 4.1 方向

- **入力（Rust → JVM）**: マクロ呼び出しの引数式。型検査済みの scala-rs AST を
  reflect Tree に組み立てる。slick の 2 マクロは引数 Tree の**中身をほとんど見ない**
  （`mapToImpl` は `c.prefix` と型引数、`TableQueryMacroImpl` は型引数だけ）ので、
  最初は「Literal / Ident / Select / Apply / New / Function / Block」程度で足りる。
- **出力（JVM → Rust）**: 展開結果の Tree。こちらは**中身を全部読む**必要がある。

### 4.2 表現形式

`showRaw` 形式（`Apply(Select(Ident(Helper), TermName("hello")), List(Literal(Constant(7))))`）は
prototype で確認したとおりそのまま得られるが、**パースし直すのは Rust 側の手間が大きく、
エスケープ規則も曖昧**である。engine 側で JSON に直列化する方が確実である。

```json
{"t":"Apply",
 "fun":{"t":"Select","qual":{"t":"Ident","name":"Helper","sym":"slick.lifted.TableQuery$"},
        "name":"hello"},
 "args":[{"t":"Literal","const":{"k":"Int","v":7}}]}
```

Tree ノードごとに `t` を持ち、シンボルが解決済みのノードには `sym`（完全修飾名）を添える。
Rust 側は `sym` があればそれを優先して解決し、無ければ名前解決に落とす。

### 4.3 健全性の限界（正直に）

- 展開結果の Tree は、**JVM 側の実行時ユニバースのシンボル**を指している。Rust 側の
  SymbolTable のシンボルとは別物である。`sym` の完全修飾名で突き合わせるのが橋渡しだが、
  ローカル変数・型パラメータ・匿名関数のパラメータのように**完全修飾名を持たないシンボル**は
  名前でしか運べない。ここは変数捕捉（hygiene）を壊しうる。
  nsc も def マクロは hygienic ではない（`freshName` で回避する文化）ので、
  「本物と同程度に不健全」で済ませられる見込みはある。
- `TypeTree(tpe)` のように **Type を埋め込んだ Tree** が返ってきた場合、Type も
  同じ方式で直列化して Rust 側の `Type` に戻す必要がある。slick の両マクロが使うので、
  これは必須である（`TableQueryMacroImpl` は `TypeTree(e.tpe)`）。
- `showCode` した文字列を scala-rs のパーサで読み直す案は、上の `sym` を落とすので
  **一般には不健全**。デバッグ表示にとどめる。

---

## 5. classfile に何を残すか（分離コンパイル）

マクロ def は別コンパイル単位から展開されるので、`ScalaSignature` に
「このメソッドはマクロで、実装は X.y である」ことを残さなければならない。

- nsc: pickle の `SYMANNOT` に `@macroImpl(tree)`（§1.1 の 6 フィールド）を焼き、
  `MACRO`（`1L << 15`）フラグを立てる。マクロ def の本体は `EmptyTree` で、
  **JVM メソッドとしては出力されない**（だから Java からマクロは呼べない）。
  漏れを検出するために RefChecks に `"macro has not been expanded"` チェックがある。
- scala-rs 現状: `crates/backend/src/pickle.rs` は `SYMANNOT` を書ける（`@deprecated` 等で実績あり）が、
  **`MACRO` フラグは意図的に pickle していない**（同ファイル冒頭のコメント）。
  また unpickler 側（`crates/typer/src/classpath.rs` が読む `PickledMethod`）は
  name / param / ret / tparams しか復元しない。
- 必要な作業:
  1. `Symbol` に `macro_impl: Option<MacroBinding>` を持つ（実装済み。
     `crates/typer/src/symbol.rs`）。
  2. pickle 側で、マクロ def に `MACRO` フラグと実装参照を書く。
     nsc 互換にするなら `@macroImpl` の `TREE` 表現、我々だけで閉じるなら
     もっと単純な符号化でもよい。**scalac が我々の classfile を読む**互換テストが
     既にあるので（`scalac_typechecks_against_our_classfiles_if_present`）、
     nsc 互換の形を目指す価値はある。
  3. unpickler 側で復元する。
- **マクロ def はメソッド本体を出さない**（`crates/backend/src/gen.rs`）。

---

## 6. 段階的な実装計画

### フェーズ 1（このブランチの範囲）

1. パーサが `= macro <ref>` を受理する。`TreeKind::MacroRhs { impl_ref }` を作る。**済**
2. `Symbol.macro_impl` / `MacroBinding` を追加する。**済**
3. typer がマクロ def を認識し、
   - 戻り値型の省略を診断する、
   - 実装参照を解決してバインディングを記録する、
   - 呼び出し地点で「展開できない」ことを**明示的に診断する**（黙って通さない）。
4. backend がマクロ def の本体を出さない。
5. fixture（接頭辞 `macro`）と `crates/cli/tests/macros.rs`。

### フェーズ 2: engine と最小の展開

6. Java の macro engine（`Context` の 72 メソッド、JSON 直列化）。
7. Rust 側から engine を起動し、`Literal(Constant(42))` を受け取って
   呼び出し地点に差し込む。`M.f()` が `42` を返す。
8. ただし **フェーズ 2 には前提がある**: マクロ実装のソースを scala-rs でコンパイルできること。
   すなわち §6.2。

### フェーズ 3: マクロ実装をコンパイルできるようにする（本丸）

9. `scala.reflect.macros.blackbox.Context` / `scala.reflect.api.Universe` の
   prelude（`crates/typer/src/prelude_reflect.rs`）。
   `c.Expr[T]` のような**パス依存型**が要る。現状 scala-rs は
   `import c.universe._` で型メンバは入るが**項メンバが入らない**ことを確認済み
   （probe で `Tree` は解決したが `mk` は `not found: value mk` になる）。
10. これらに対する `library_abi` 相当のコード生成。`Literal(Constant(42))` は
    `c.universe().Literal().apply(c.universe().Constant().apply(box(42)))` になる。

### フェーズ 4: 組み込み（fast track）マクロ

11. `reify` の脱糖器。`TableQueryMacroImpl.apply` に必要。
12. quasiquote の脱糖器（§6.2）。`ShapedValue.mapToImpl` に必要。**最大の 1 項目**。

### フェーズ 5: slick の 2 マクロ

13. `TableQueryMacroImpl.apply` を通す（11 が前提）。
14. `ShapedValue.mapToImpl` を通す（12 が前提）。case class のフィールド列挙
    （`Type.decls.collect` / `Type.member`）が engine 越しに動くことの確認も要る。

---

## 6.2 最大の障害: quasiquote と reify は JVM ブリッジで展開できない

展開の実行は §2.3 で解けている。**残る本当の難所は、マクロ実装のソースを
scala-rs がコンパイルできるかどうかである。** そして、その中でも決定的な事実がひとつある。

### 事実

`scala.tools.reflect.FastTrack` の定数プールに、次の名前がそのまま入っている
（`unzip -p scala-compiler.jar 'scala/tools/reflect/FastTrack.class' | strings` で確認）:

```
QuasiquoteClass_api_apply    QuasiquoteClass_api_unapply
ApiUniverseReify
materializeClassTag   materializeTypeTag   materializeWeakTypeTag
StringContext_f   StringContext_s   StringContext_raw
```

そして **scala-reflect.jar 内には pickle 済みの `@macroImpl` バインディングが 1 つも無い**
（`macroEngine` の文字列検索で 0 ヒット）。`Universe.reify` の宣言は `= macro ???` である。

つまり:

> **quasiquote（`q"…"` / `tq"…"` / `pq"…"` / `cq"…"`）と `reify` は、
> scala-reflect.jar に実装 classfile を持たない。実体は scala-compiler.jar の中にあり、
> nsc は classloader を通さず内蔵実装に短絡する（fast track）。**

### 帰結

- **JVM ブリッジ（案 B）は、これらには使えない。** ロードすべき実装クラスが存在しない。
- したがって **scala-rs はこれらを自前の組み込みとして実装するしかない**。
  これは「マクロ展開器を作れば自動的に付いてくる」ものでは**ない**。
- 前提としていた「quasiquote は whitebox マクロなので展開器で解ける」という見立ては
  **誤りだった**。ここで訂正しておく。

### では何を作るのか

幸い、作るべきものの形ははっきりしている。nsc の quasiquote マクロがやっているのは
**「補間文字列を Scala として構文解析し、`internal.reificationSupport.Syntactic*` の
呼び出し列に脱糖する」**ことだけである（§3.3 のバイトコード実測がそれを裏づけている:
`mapToImpl` の本体は `Syntactic*` 呼び出し 209 箇所に脱糖されている）。

したがって scala-rs 側の作業は:

1. `q"…"` の中身を、**穴（`$x` / `${…}`）を許す形で Scala として構文解析する**。
   scala-rs は既に Scala パーサを持っているので、ここは拡張で済む。
2. 解析結果を `Syntactic*` 呼び出しの AST に落とす
   （`SyntacticSelectTerm` / `SyntacticApplied` / `SyntacticValDef` / `SyntacticDefDef` /
   `SyntacticNew` / `SyntacticFunction` / `SyntacticBlock` / `FlagsRepr` / …）。
   §3.3 の一覧が、slick を通すのに必要な最小セットである。
3. その AST を scala-reflect ABI でコード生成する（フェーズ 3 の 10 と同じ仕組み）。

これで**コンパイル済みの `mapToImpl` が得られ**、あとは §2.3 で実証済みの engine が
実行する。§2.3 で quasiquote 版 `qqImpl` が正しく動いたのは、まさにこの経路の後半を
先に確かめたものである。

`reify` も同様に「reify されるブロックを、universe の Tree 構築呼び出し（`TreeCreator` /
`TypeCreator` 生成）へ脱糖する」組み込みが要る。`TableQueryMacroImpl` に必要。

### 規模の正直な見積り

- quasiquote の脱糖器: **本フェーズより大きい**。穴の型（Tree / Name / Type / List / 名前）に
  よる分岐、`..$` / `...$` の展開、パターン側（`unapply`）まで含めると相当量になる。
  ただし slick が使うのは `apply` 側だけで、pattern quasiquote は使わない。
- reify: 中程度。`TableQueryMacroImpl` の使い方は素直な 1 式の reify である。
- どちらも「Rust で書く新規コンポーネント」であり、既存資産の流用はパーサだけである。

### 代替案（採らないが記録する）

「slick の `ShapedValue.scala` だけ scalac でコンパイルして classfile を用意し、
scala-rs は展開だけ担当する」という運用は技術的には可能である。
だが「scala-rs が slick をコンパイルする」というベンチマークの意味を損なうので、
やるなら**その旨を明示した上で**、ベンチマーク成績としては数えない。

## 6.3 whitebox について

slick の 2 マクロは blackbox である。quasiquote / reify も（fast track なので）
whitebox 展開器を必要としない。したがって **whitebox は当面まったく要らない**。
blackbox だけを実装し、whitebox のマクロ def を見つけたら診断して落とす。

## 6.4 リスク一覧

| リスク | 影響 | 緩和 |
| --- | --- | --- |
| `c.typecheck` / `inferImplicitValue` を使うマクロ | engine から Rust の typer を呼び戻す双方向 RPC が要る | slick は使わない。呼ばれたら診断して落とす |
| hygiene（§4.3） | 展開結果が呼び出し地点の変数を捕捉する | nsc も非 hygienic。`freshName` に依存 |
| Type の往復 | `TypeTree(tpe)` が戻せないと `TableQueryMacroImpl` が動かない | Type も JSON 直列化する（必須作業） |
| engine プロセスの起動コスト | 大きなビルドで遅い | 常駐させて複数展開を 1 プロセスで捌く |
| scala-reflect.jar 依存 | jar が無い環境でマクロが使えない | 診断を出して落とす。私有ランタイムでは非対応と明記 |
| `javac` 依存 | ビルド環境が増える | engine をビルド済みで同梱するか、feature で切る |
| 実行時ユニバースと compiler ユニバースの差 | 一部マクロが挙動を変える | nsc の実装クラスは `c.universe` を **`scala.tools.nsc.Global` として宣言**している（公開 API は `macros.Universe`）。API 経由で書かれたマクロは動く（§2.3 で実証）が、`Global` にキャストするマクロは動かない。診断で落とす |
| fast track マクロ（§6.2） | quasiquote / reify を使うマクロが**一切**コンパイルできない | 自前で脱糖器を書くしかない。フェーズ 4 |
| `MacroImplBinding` の pickle 互換 | scalac が我々の classfile を読めなくなる | `macroEngine` 文字列まで含めて nsc 互換の形で書く |

---

## 7. 現状（このブランチで実際に動くところ）

- `= macro <ref>` を**パースできる**。以前の `unimplemented syntax: macros` は出ない。
- マクロ def のシンボルにバインディングを記録する。
- **展開はまだできない**。呼び出し地点で診断を出す。黙って通すことはしない。
- §2.3 の prototype は [`docs/macro-engine-prototype/`](macro-engine-prototype/) にある。
  CI では走らない（scalac と scala-reflect.jar が要る）。走らせ方と、製品版に足りないものは
  そこの README に書いた。フェーズ 2 で `crates/macro-engine/` として正式に取り込む。

### 7.1 quasiquote の**フロントエンド**（`crates/typer/src/quasiquote.rs`）

`q"…"` / `tq"…"` / `pq"…"` / `cq"…"` を**認識して診断する**ところまでは動く。
以前は `value q is not a member of StringContext` という**誤った**診断が出ていた
（`q` は `Quasiquotes.Quasiquote` のメンバであり、欠けているのは展開である）。

- 補間文字列の中身を、穴（`$x` / `${…}` / `..$xs` / `...$xss`）をプレースホルダ名に
  置き換えて**再構成し、scala-rs のパーサで実際に構文解析する**。`..` / `...` は
  直前の part の末尾に現れるので、そこから rank を剥がす。
- パースできなければ `unimplemented syntax: quasiquote q"..." (理由)`。
- パースできれば、残る欠落は reification なので
  `macro expansion is not implemented: cannot expand quasiquote q"..."` を出す。
- **ユーザ定義の `q` 補間子は横取りしない**。通常の custom interpolator として
  型付けを試し、それが失敗したときだけ quasiquote として報告する
  （fixture `quasi.scala` がこれを実行時まで検証している）。

**slick での実測**: `ShapedValue.mapToImpl` の 14 箇所（`q` 12 / `tq` 1 / `pq` 1）が
すべて認識され、しかも **`unimplemented syntax` は 1 件も出ない**。つまり
**scala-rs のパーサは slick が使う quasiquote の中身をすべて構文解析できる**。
残っているのは §6.2 の 2 と 3、すなわち解析結果を `Syntactic*` 呼び出しに落とす
reification と、そのコード生成である。

### 7.2 reflect ABI に向けて塞いだ穴

`q"…"` を展開できても、それが落ちる先（`c.universe` / 実行時ユニバース）を
scala-rs が型検査できなければ意味がない。フェーズ 3 の下地として次を実装した。
いずれも reflect 専用ではない一般の修正である。

1. **pickle が指すネストしたクラス**。pickle は `scala.reflect.api.Names.TermNameExtractor`
   のように、パッケージ区切りとクラス区切りを区別せずドットで書く。実体は
   `scala/reflect/api/Names$TermNameExtractor.class` で、しかも
   **ネストしたクラスファイルは `ScalaSignature` を持たない**（pickle は最上位クラスの
   classfile にまとめて入っている）。`scala_rs_pickle::sym::pickle_files_for` が
   候補ファイルを右から順に生成して両方を解決する。
2. **バイトコード上の親を持たないトレイト**。`scala.reflect.api.Universe` は
   *abstract class* なので、`trait JavaUniverse extends Universe` の classfile は
   `interfaces: 0` になり、継承関係が pickle にしか無い。
   `erased_desc` は、classfile が親を 1 つも宣言していないクラスに限り
   pickle の親で補う（無条件に補うと `Map#map` の erased descriptor が曖昧になる）。
3. **抽象型メンバ**。`type Tree >: Null <: TreeApi` のような宣言は reflect API の
   語彙そのもので、クラスではないので `ensure_class` では解決できない。
   `PickleSupply::abstract_type_member` が `TypeMember` シンボルとして導入する。
   クラス内部から `Constant` のように**修飾なしで**書かれる場合のために、
   レシーバの線形化とその**外側のクラス**まで探す（`self_type_member`）。
4. **引数なし `def` の `apply` 挿入**。`def Literal: LiteralExtractor` に対する
   `Literal(x)` は `Literal.apply(x)` である。これは reflect に限らない一般の欠落で、
   `def mk: Box` に対する `mk("a")` も通らなかった（`insert_apply_on_nullary`）。
5. **package object のメンバのコード生成**。`scala.math.Pi` は
   `scala/math/package$` の `val` だが、typer はそれをパッケージシンボルに畳み込む。
   パッケージには実行時の値が無いので、レシーバが積まれないまま `invokevirtual` が
   出て **`VerifyError` になっていた**（main でも再現する既存バグ）。
   `load_package_object_receiver` が `<pkg>/package$.MODULE$` を積む。
6. **`import <値>._`**。`import c.universe._` / `import scala.reflect.runtime.universe._`
   の形。プレフィクスが値のときはその**型**のメンバを入れる必要があり、さらに
   無修飾の `Literal` は `u.Literal` を意味するので、typer が
   `Select(u, Literal)` に書き戻す（`term_import_prefixes` / `qualify_term_import`）。
   これをしないと backend が `this` をレシーバにして `ClassCastException` になる。

### 7.3 まだ塞がっていない穴（次に要るもの）

**A. 展開先を宣言したクラスで呼ぶこと。済（`agent/reify2`）。** §7.4 の 1。

**B. reification 本体。一部済（`agent/reify2`）。** §7.4 の 2。実装した部分集合と、
まだ落とせない形は §7.4 に列挙してある。

**C. `c.Expr[T]` などのパス依存型。済（`agent/quasi`）。** §7.6。

**D. engine（フェーズ 2）。** A〜C が済んでも、slick の `mapToImpl` を*呼ぶ*には
§2.3 の JVM ブリッジが要る。こちらは prototype で検証済みで、順序としては最後でよい。

### 7.4 宣言クラスでの呼び出しと reification（`agent/reify2` スライス）

§7.3 の A と B。**`scala.reflect.runtime.universe` 上で Tree を組み立てるコードが
実際に走るようになり**、その上で `q"…"` の一部が本当に脱糖されるようになった。

#### 1. 宣言クラスで呼ぶ（A、済）

`Symbol::declaring_class` / `declaring_is_interface` を足した
（`crates/typer/src/symbol.rs`）。`pickle_supply::erased_desc` は 7.2 の 2 で
「classfile が親を 1 つも宣言していないクラスは pickle の親で補う」ようになっていたが、
**見つけた descriptor がどのクラスの宣言かを返していなかった**。そこを
`ErasedDecl { desc, declared_in, declared_by_interface, off_the_bytecode_path }` にし、
`off_the_bytecode_path`（＝pickle の親を辿ってしか届かない、JVM には見えない経路）で
見つけたときだけ宣言クラスをシンボルに記録する。`gen.rs` はそれを invoke のオーナーに使い、
レシーバをそこへ `checkcast` する。**受け手の classfile から届く普通のメンバの
バイトコードは一切変わらない**（既存 fixture が全部それを固定している）。

```
// scala-rs が出すようになったもの（nsc と同形）
invokeinterface scala/reflect/api/Constants.Constant:()Lscala/reflect/api/Constants$ConstantExtractor;
// 以前: invokeinterface scala/reflect/api/JavaUniverse.Constant() → NoSuchMethodError
```

これだけでは `u.Literal(u.Constant(42))` は通らず、道中で 4 つ塞いだ。いずれも
reflect 専用ではない一般の欠落である。

- **入れ子クラスの名前が外側クラスに潰れる**（`pickle_supply::ensure_class`）。
  `pickle_files_for` は「pickle が入っている classfile」も候補に出すので、
  `scala.reflect.api.Constants.Constant`（実体のない抽象型メンバ）が
  `scala/reflect/api/Constants` にマッチして**外側のトレイトそのもの**に解決していた。
  `names_class` で「自分の単純名で終わる候補」だけを採る。
- **複合上界が捨てられる**（`conv_upper_bound`）。reflect API は
  `type Select >: Null <: SelectApi with RefTree` の形で書かれていて、
  `Refined` を変換できず上界ごと落としていた。`Select <: Tree` が導けないので
  `Syntactic*` に渡せるものが何も無かった。
- **上界が受け手の語彙で解決されていた**（`abstract_type_member`）。上界は
  *宣言クラス*の語彙で書かれている（`Ident` の上界の `RefTree` は同じ `Trees` の
  別の抽象型メンバ）。変換の間だけ `self_ty` を宣言クラスに向ける。
- **既定引数ゲッタの規約**。scalac は既定値が先行パラメータを読まないとき
  **nullary の** `$default$n` を出す。呼び出し側はゲッタ自身の arity に合わせる
  （`default_getter_apply`）。これが無いと `SyntacticTermIdent` が供給されない。
- **複合上界が base type sequence に現れない**（`SymbolTable::base_type_seq`）。
  `lub(Ident, Literal)` が `AnyRef` になり `List(ident, literal)` が
  `List[AnyRef]` になっていた。

#### 2. reification（B、部分実装）

`crates/typer/src/reify.rs`。7.1 が構文解析した木を
`<universe>.internal.reificationSupport.Syntactic*` の呼び出し木に落とし、
**普通の式として型検査・コード生成する**。universe は
`import <universe>._` が記録した term import のプレフィクスから採る
（`Check::universe_in_scope`）。

`q"…"` について落とせる形:

| 形 | 落とす先 |
| --- | --- |
| リテラル | `u.Literal(u.Constant(v))` |
| 名前 | `rs.SyntacticTermIdent(u.TermName("n"), false)` |
| `a.b` | `rs.SyntacticSelectTerm(<a>, u.TermName("b"))` |
| `f(a, b)` / `a.b(1)(2)` | `rs.SyntacticApplied(<f>, List(List(<a>, <b>)))` |
| `$x` | 引数の式をそのまま差す |
| `..$xs` | 引数リスト 1 節ぶんとして差す |
| `f()` | `Nil`（`List()` は `A` を解けない。§7.5） |

**落とせない形は必ず診断する**（`unimplemented syntax: quasiquote q"..." (…)` に
どの形かを書く）。このスライス時点の穴（**§7.7 でほとんど埋まった**）: ブロック、
関数リテラル、`new`、`if`、`match`、型注釈、型適用、`this` / `super`、
定義（`val` / `def` / `class`）、`..$` と普通の引数の混在、`tq` / `pq` / `cq` 全体。

検証: `tests/fixtures/reify_qq.scala` を実 scalac 2.13.16 と dual-run して
**出力が完全一致**する（`crates/cli/tests/reify.rs`）。異常系は
`tests/fixtures/reify_qq_bad.scala`。

### 7.5 このスライスのあとに残っているもの

1. **`tq` / `pq` / `cq`。** `mapToImpl` は 3 つとも使う。`tq` は
   `SyntacticAppliedType` / `SyntacticSelectType` / `SyntacticTypeIdent` /
   `SyntacticEmptyTypeTree` あたり、`pq` は `Bind` / `UnApply`、`cq` は `CaseDef`。
2. **`q` の残りの形。** 特に `SyntacticBlock`（`q"""…"""` の複文）、
   `SyntacticNew`、`SyntacticFunction`、`SyntacticValDef` / `SyntacticDefDef`、
   `Typed`（`(x: T)`）。§3.3 の出現回数が優先順位そのものである。
2. **`..$` と普通の引数の混在**（`q"f(a, ..$xs)"`）。連結の静的型を両側とも
   正しく出す必要がある。
3. **期待型からのメソッド型パラメータ推論。** `List()` が `List[A]` のまま
   解けないので `Nil` で回避している。ここが入れば混在の連結も書きやすくなる。
4. **`Liftable`。** `$x` の `x` が `Tree` でないとき（`Int`、`String`、`Name`、
   `Symbol`、`WeakTypeTag`）、nsc は implicit `Liftable` で持ち上げる。
   `mapToImpl` は `$rTag` / `$rCT` / `${c.prefix}` でこれを使う。
   現状は `Tree` でない穴が型エラーになる（黙って通しはしない）。
5. **§7.3 の C（`c.Expr[T]` などパス依存型）と D（engine）。** C は §7.6 で
   済んだ。D（engine）が残っている。

### 7.6 マクロ実装のシグネチャと `import c.universe._`（`agent/quasi` スライス）

§7.3 の C。**scala-reflect.jar が classpath にあれば、マクロ実装のソースが
コンパイルできるようになった。** 中身は「パス依存型」というより、
**jar のクラスの遅延ロードが型名前空間とワイルドカード import に届いていなかった**
という一般の欠落だった。

| 直したもの | どこ |
| --- | --- |
| **`import <値>._` が継承メンバに届かない。** jar のクラスのメンバは名前ごとに pickle から遅延ロードされる。`import scala.reflect.runtime.universe._` が名乗る `JavaUniverse` は `TermName` / `Literal` / `Constant` / `termNames` を**すべて linearization の上の方**（`api.Names` / `Trees` / `Constants` / `StandardNames`）から継承していて、誰もそれを要求していないので import は**何も**持ち込んでいなかった。パス経由（`u.TermName`）は完了処理を通るので動いていた。reify した quasiquote は `u.TermName(...)` を明示的に組むので、この穴に気づかなかった | `Check::expose_unqualified` → `supply_from_pickle_class` |
| **型名前空間。** reflect API は同じ名前を両方の名前空間に置く（`val TermName` と `type TermName`）。値を先に解決すると term が scope に入り、`expose_unqualified` が「もう束縛済み」と見て止まるので、`val n: TermName = TermName("f")` は右辺だけ通って左辺が `not found` だった | `Check::expose_unqualified_type` |
| **jar のクラスの型メンバがそもそも読めない。** `def` の完了しか無かった。`blackbox.Context` は `scala.reflect.macros.Aliases` から `type Tree = universe.Tree` / `type Expr[T] = universe.Expr[T]` / `type WeakTypeTag[T] = …` を継承していて、これが無いとマクロ実装は**自分のシグネチャを書けない** | `PickleSupply::complete_type_member` / `install_type_alias` |
| **refinement 越しの型メンバ。** slick の `mapToImpl` の `c` は `blackbox.Context { type PrefixType = ShapedValue[?, U] }` という**精製型**で、そこから `c.Expr[…]` / `c.Tree` を引く | `Check::project_from_prefix` の `Type::Refined` 枝 |
| **`import <値>._` の親が未ロード。** `universe_in_scope` は「この prefix は `scala.reflect.api.Universe` を継承しているか」で universe を見分けるが、その親リストは pickle にしか無く、まだ誰も読んでいなかった。だから `import c.universe._` を書いた本体の `q"…"` は全部「cannot expand」になっていた | `PickleSupply::ensure_parents` |
| **term import prefix のスコープ。** `import u._` の `u` はそのメソッドのローカルで、次のメソッドには無い。それでも prefix として使われ、**別メソッドのローカルに対する `getfield`** を吐いていた（`NoClassDefFoundError`）。しかも同じ owner の外側の import を追い出していたので、内側を抜けた後は receiver 無しになっていた | `Check::prefix_in_scope`、`remember_term_import_prefix` は置換をやめて追加に |
| **空の `Context` prelude をやめた。** scala-reflect.jar が classpath にあるときだけ本物を読む。無いときは今までどおり空の `Context` を入れ、`value universe is not a member of Context` と**きちんと言う**（`--scala-library` は scala-reflect.jar を含まない） | `prelude_reflect::want_context_stub` |

検証（`crates/cli/tests/quasi.rs`）:

- `tests/fixtures/qq_universe.scala` — 実行して実 scalac 2.13.16 と**出力が完全一致**。
  `showRaw` まで一致するので、**同じ木**を作っている。`java -Xverify:all`。
- `tests/fixtures/qq_ctx.scala` — マクロ実装そのもの。scala-rs と実 scalac の
  **両方**がコンパイルでき、吐いた classfile は JVM にロード・検証される。
  展開には engine（D）が要るので実行はしない。
- `tests/fixtures/qq_ctx_bad.scala` — reify できない形（型注釈・ブロック・`tq`）は
  必ず**その形を名指しして**診断する。`Tree` でない穴も型エラーになる。
- scala-reflect.jar 無しでは空の `Context` の診断が出ることも固定してある。

**slick への効き方（重要）。** `tests/slick_measure.sh` が使う `deps.cp` には
**scala-reflect.jar が入っていない**。slick 本体はこれに依存している
（`build.sbt` の `scala-reflect`）ので、無ければ実 scalac でも
`ShapedValue.scala` / `TableQuery.scala` はコンパイルできない。数字は:

| classpath | errors | ShapedValue | TableQuery |
| --- | --- | --- | --- |
| 既定（scala-reflect 無し） | 327 → **320** | 29 → 29 | 23 → 23 |
| `-cp scala-reflect.jar` を足す | 322 → **294** | 26 → **17** | 21 → **9** |

（前後とも分岐元 `6c6fc7f` で実測。jar を足すだけで 327 → 322 になるのは、
`Context` 以外の `scala.reflect.*` の名前がいくつか解決するため。）

既定の classpath を勝手に変えると他のエージェントの基準値まで動くので、
`deps.cp` は触っていない。**quasiquote の 12 件を減らすには、まず計測の
classpath に scala-reflect.jar を足す必要がある。**

そのうえで残っているもの:

1. **reification の残りの形。** scala-reflect.jar を足した `ShapedValue.scala` の
   残り 17 件のうち **11 件**がこれで、内訳は `Typed`（型注釈、8 件）、
   `SyntacticBlock`（1 件）、`tq`（1 件）、`pq`（1 件）。§7.5 の 1・2 そのもの。
   どれも「展開できない」ではなく**どの形が足りないか**を名指しする診断になった。
2. **`Ident(TermName("x"))` / `New(TypeTree(…))` のオーバーロード。**
   `val Ident: IdentExtractor` と `def Ident(name: String): Ident` の
   オーバーロード集合に対して `apply` 挿入が働かない。`TableQuery` の 2 件。
3. **`symbolOf[T]` / `typeOf[T]`。** 型パラメータが implicit 節にしか現れない
   メンバは `pin_undetermined_tparams` が明示的に断っている（一般の制限）。
4. **wildcard import の遮蔽。** `import c.universe._` は暗黙の `import scala._` を
   遮蔽するはずだが、`Symbol` は `scala.Symbol` に解決されたままになる。
5. **我々の pickle を scalac が読めない。** scala-rs がコンパイルしたマクロ実装を
   実 scalac から `macro` で参照すると `macro implementation has incompatible
   shape: found (c: Context, x: Tree): Tree` になる。パラメータ節がひとつに
   潰れており、パス依存型も残っていない。§5 のフェーズ 2 の作業。

### 7.7 reification の残りの形（`agent/reify2` 第 2 スライス）

§7.6 の 1 と 2。**`tq"…"` / `pq"…"` / `cq"…"` と、`q"…"` の残りの形が
落とせるようになった。** 形はすべて実 scalac 2.13.16 の `-Ymacro-debug-lite`
（nsc 自身の quasiquote マクロが吐く展開が印字される）から読み取り、
`tests/fixtures/qr_forms.scala` が `showRaw` まで実 scalac と突き合わせている
（`java -Xverify:all` で実行、56 行が完全一致）。

#### 落とせるようになった形

| 形 | 落とす先 |
| --- | --- |
| `tq"T"` | `rs.SyntacticTypeIdent(u.TypeName("T"))` |
| `tq"a.b.C"` | `rs.SyntacticSelectType(<a.b を項として>, u.TypeName("C"))` |
| `tq"F[A, B]"` | `rs.SyntacticAppliedType(<F>, List(<A>, <B>))` |
| `tq"A => B"` | `rs.SyntacticFunctionType(List(<A>), <B>)` |
| `tq"(A, B)"` | `rs.SyntacticTupleType(List(<A>, <B>))` |
| `tq"a.b.type"` | `rs.SyntacticSingletonType(<a.b>)` |
| `tq"A#B"` | `rs.SyntacticTypeProjection(<A>, u.TypeName("B"))` |
| `tq"A with B"` | `rs.SyntacticCompoundType(List(<A>, <B>), Nil)` |
| 型の空欄（`val x = e` の型） | `rs.SyntacticEmptyTypeTree.apply()` |
| `q"x: T"` | `u.Typed(<x>, <T>)` |
| `q"f _"` | `u.Typed(<f>, rs.SyntacticFunction(Nil, u.EmptyTree))` |
| `q"f[T](a)"` | `rs.SyntacticTypeApplied(<f>, List(<T>))` の上に `SyntacticApplied` |
| `q"{ a; b }"` / `q"..$stats"` | `rs.SyntacticBlock(List(<a>, <b>))` |
| `q"val v: T = e"` | `rs.SyntacticValDef(u.Modifiers(rs.FlagsRepr(0L)), u.TermName("v"), <T>, <e>)` |
| `q"new C[T](a)(b)"` | `rs.SyntacticNew(Nil, List(rs.SyntacticApplied(<C[T]>, List(<a>, <b>))), u.noSelfType, Nil)` |
| `q"e match { … }"` | `rs.SyntacticMatch(<e>, List(<case>))` |
| `q"{ case p => e }"` | `rs.SyntacticPartialFunction(List(<case>))` |
| `q"(y: T) => e"` | `rs.SyntacticFunction(List(<param>), <e>)` |
| `q"this"` / `q"C.this"` | `u.This(u.TypeName(""))` / `u.This(u.TypeName("C"))` |
| `q"a.b = c"` | `rs.SyntacticAssign(<a.b>, <c>)` |
| `q"if (a) b else c"` | `u.If(<a>, <b>, <c>)` |
| `pq"_"` | `rs.SyntacticTermIdent(u.TermName("_"), false)` |
| `pq"x"`（小文字始まり） | `u.Bind(u.TermName("x"), rs.SyntacticTermIdent(u.TermName("_"), false))` |
| `pq"a.b.C(p)"` | `rs.SyntacticApplied(<a.b.C>, List(List(<p>)))` |
| `pq"x @ p"` / `pq"a \| b"` / `pq"_: T"` | `u.Bind` / `u.Alternative` / `u.Typed` |
| `cq"p if g => e"` | `u.CaseDef(<p>, <g>, <e>)`（ガード無しは `u.EmptyTree`） |
| 演算子名 | `NameTransformer` で符号化（`q"a + b"` は `u.TermName("$plus")`） |
| `q"$x.$n"` | 名前の位置の穴は `TermName` をそのまま差す |

#### パーサが潰してしまう区別を、元のソース文字列で戻す

scala-rs のパーサは nsc が区別する形をいくつか正規化してしまう。reification は
**quasiquote の本文テキストを持ち回って**そこを見分ける（`Reifier::src`）。

- `A => B` は `AppliedTypeTree(Ident("Function1"), …)` になり、**書かれた**
  `Function1[A, B]` と同じ木になる。nsc は前者を `_root_.scala.Function1`、
  後者を裸の `Ident` にする。頭の span のテキストが `Function1` かどうかで決める。
- `(a, b)` は `Apply(Ident("Tuple2"), …)` になり、書かれた `Tuple2(a, b)` と
  同じ木になる。nsc は前者だけ `SyntacticTuple`。同じ判定。
- `q"val v = e"` と `q"{ val v = e }"`。ラッパが足す `{}` と作者の `{}` は
  パース後には区別できないので、**本文が `{` で始まるか**を渡している
  （`unwrap_body` の `braced`）。前者は裸の `SyntacticValDef`、後者は
  `SyntacticBlock`。

#### 落とせない形は今までどおり名指しで診断する

パーサが**情報ごと**捨ててしまい、何を作っても「誰も書いていない木」になる形は
作らない。`tests/fixtures/qr_forms_bad.scala` / `reify_qq_bad.scala` /
`qq_ctx_bad.scala` がそれぞれ診断を固定している。

| 形 | 診断 | 理由 |
| --- | --- | --- |
| `q"a :: b"` | a right-associative operator (`::`) is not reified yet | パースで `b.::(a)` になり、書かれた `b.::(a)` と区別できない。nsc はそのどちらでもなく、左辺を fresh な `val` に束ねた**ブロック**を作る |
| `q"if (a) b"` | an `if` without an `else` is not reified yet | パーサは `else` に `()` を補う。nsc は空ブロックを補う |
| `q"_.get"` | a `_` placeholder function literal is not reified yet | パーサが作るパラメータ名と、nsc の `freshTermName` が違う |
| `tq"=> T"` | a by-name type is not reified yet | nsc のパーサ自身が `tq` の中では拒否する |
| `q"f(a, ..$xs)"` | a `..$` splice mixed with ordinary arguments | 連結の静的型を両側とも正しく出す必要がある（§7.5 の 2） |
| `q"class C"` など定義 | a class definition is not reified yet | `SyntacticClassDef` などが未実装（**§7.8/7.9 で入った**） |
| `q"{ lazy val a = 1 }"` | a modified `val` definition is not reified yet | `Modifiers` のフラグ変換が未実装（**§7.8/7.9 で入った**） |
| `q"{ $x }"` | （診断なし。既知の差） | パーサが `{ e }` を `e` に潰すので、単一の穴だけは nsc の `SyntacticBlock(List(x))` に対して `x` になる。意味は同じだが木は違う |

#### ついでに直した一般の穴

reification が要求しただけで、いずれも reflect 専用ではない。

| 直したもの | どこ |
| --- | --- |
| **オーバーロード集合への `apply` 挿入。** `val Ident: IdentExtractor` と `def Ident(name: String): Ident` は同じ名前のオーバーロード集合で、`Ident(TermName("x"))` はどちらにも当たらず `Ident.apply(...)` である。`Bind` / `This` / `New` も同じ形。§7.6 の 2 で、slick の `TableQuery` のマクロ実装はこれだけで書かれている | `Check::insert_apply_on_nullary` の `Type::Overload` 枝 |
| **同名の型メンバに項の選択が食われる。** reflect API は `type Modifiers` と `def Modifiers(flags: FlagSet)` を両方置く。jar のメンバは名前ごとに遅延ロードされるので、**型メンバが先に入る**（`NoMods` を完了すると入る）と名前はもう「見つからない」ではなくなり、項のオーバーロードは読まれないまま `u.Modifiers(flags)` が `<notype>` の `TypeMember` に解決していた（`value apply is not a member of <notype>`）。§7.6 の `expose_unqualified_type` の鏡像 | `Check::type_select` |
| **`invokeinterface` の `count` がスロット数でない。** `long` / `double` の引数は 2 スロット。`reificationSupport.FlagsRepr(8192L)` が `VerifyError: Inconsistent args count operand in invokeinterface` になっていた | `Assembler::invokeinterface` / `count_param_slots` |
| **抽象型メンバの引数に erasure 適応の `checkcast` が無い。** `type TermName >: Null <: TermNameApi with Name` は `Names$TermNameApi` に、`Name` は `Names$NameApi` に erase され、JVM は両者の関係を知らない。nsc はここで `checkcast` を出す | `gen.rs` の `adapt_type_member_arg` |
| **`NoMods` が `Universe` の宣言。** `scala.reflect.api.Universe` は abstract class で、`JavaUniverse` からの継承は pickle にしかない。`u.NoMods` は `invokevirtual scala/reflect/api/Universe.NoMods()` になり検証に落ちる。reification は同じ値を作る `u.Modifiers(rs.FlagsRepr(0L))` を使う（`Modifiers(flags)` は `Modifiers(flags, typeNames.EMPTY, Nil)`） | `Reifier::mods` |

#### slick への効き方

`tests/slick_measure.sh`（scala-reflect.jar 入り）で `errors=257 → 255`。
数字が動かないのは、**同じ行が別の理由で落ちるようになった**からで、
quasiquote 系の内訳は次のとおり。

| 診断 | before | after |
| --- | --- | --- |
| `unimplemented syntax: quasiquote …`（形が足りない） | 10 | **4** |
| `cannot expand quasiquote …`（reify 自体が無い） | 1 | **0** |
| `TableQuery.scala` のエラー合計 | 11 | **6** |

残り 4 件の内訳は `q"…_.get…"` が 3 件（`_` プレースホルダ）と
`q"""…"""` の中の `type` 定義が 1 件。`ShapedValue.mapToImpl` の 8 つの型注釈は
**形としては通るようになり**、いま落ちているのは `$uTag` / `$rTag` が
`WeakTypeTag` で `Tree` ではないため（§7.5 の 4、`Liftable`）である:

```
error: no matching overload for SyntacticFunctionTypeExtractor
       with arguments (List[TypeTags$WeakTypeTag[U]], TypeTags$WeakTypeTag[R])
```

つまり `mapToImpl` の次の一手は **`Liftable`** であって、形ではない。

#### このスライスのあとに残っているもの

1. **`Liftable`。** `Tree` でない穴（`WeakTypeTag` / `Name` / `Int` / `String` /
   `Symbol`）を implicit で持ち上げる。`ShapedValue` の残りはすべてこれ。
2. **`_` プレースホルダと右結合演算子。** どちらも nsc は `freshTermName` を
   使ったブロックを作る。作るなら同じ形にする必要がある。
3. **`..$` と普通の引数の混在**、および**期待型からの型パラメータ推論**（§7.5）。
4. **定義の quasiquote**（`SyntacticClassDef` / `SyntacticDefDef` / `Modifiers`
   のフラグ変換）。`ShapedValue` の `q"""…"""` 全体はこれが要る。
5. **`reify { … }` と `typeOf[T]` / `symbolOf[T]`。** `reify` も quasiquote と
   同じ fast track マクロで、自前実装が要る。`TableQuery` の残り 6 件のうち
   3 件がこれ。
6. **engine（フェーズ 2）。** マクロを*呼ぶ*ための JVM ブリッジ。

### 7.8 `Liftable`、`symbolOf` / `weakTypeOf`、`reify` の診断（`agent/liftable` スライス）

§7.7 の残り 1 と 5。**`Tree` でない穴が持ち上がるようになった**ので、
`ShapedValue.mapToImpl` の `q"($rModule.tupled) : ($uTag => $rTag)"` 系が
「形が足りない」でも「穴が `Tree` でない」でもなくなった。

#### 1. `Liftable`

nsc は `Tree` でない穴について implicit `Liftable[T]` を探し、
`Liftable.liftX[T](arg)` を差す（`scala/reflect/api/StandardLiftables.scala`）。
scala-rs は **implicit 探索はしない**。穴の引数の型から標準インスタンスを選び、
**そのインスタンスが作るのと同じ木を直接組む**。

型を知るために、reify の前に各引数を**投機的に**型付けする
（クローンを型付けし、診断は巻き戻す。`Check::probe_named_arg_types` と同じ形。
呼び出し地点の木は 1 度しか型付けされない）。分類は `Check::lift_for`、
木の組み立ては `Reifier::lift`（`crates/typer/src/reify.rs` の `Lift`）。

| 穴の型 | nsc | scala-rs が組む木 |
| --- | --- | --- |
| `Tree`（`Trees` の型メンバすべて） | `liftTree` = identity | そのまま差す |
| `Int` / `Long` / `Short` / `Byte` / `Char` / `Float` / `Double` / `Boolean` / `Unit` / `String` | `liftInt` &co | `u.Literal(u.Constant(v))` |
| `Constant` | `liftConstant` | `u.Literal(c)` |
| `Type`（`Types` の型メンバ） | `liftType` | `rs.mkTypeTree(t)` |
| `WeakTypeTag` / `TypeTag` | `liftTypeTag` | `rs.mkTypeTree(tag.tpe)` |
| `Expr[T]` | `liftExpr` | `e.tree` |
| `Symbol`（`Symbols` の型メンバ） | Liftable では**ない**（穴の特別扱い） | `rs.mkRefTree(u.EmptyTree, sym)` |
| `Name`（項の位置） | 穴の特別扱い | `rs.SyntacticTermIdent(n, false)` |
| `Name`（型の位置） | 同上 | `rs.SyntacticTypeIdent(n)` |
| `Name`（パターンの位置） | 同上 | `u.Bind(n, rs.SyntacticTermIdent(u.TermName("_"), false))` |
| `..$xs` の要素が上のどれか | `xs.toList.map(v => liftX(v))` | 同形（`List` のときは `.toList` を付けない） |

`Name` の位置依存は nsc のパーサ由来である。`q"$n"` の穴は識別子の位置に立つので、
`q` なら項識別子、`tq` なら型識別子、`pq` なら変数パターンになる。名前の**枠**
（`q"$x.$n"` の `$n`、`q"val $n = e"` の `$n`）はもともとそのまま差していた。

`Symbol` だけは `Liftable` ではなく穴の特別扱いなので、**`..$` の下では nsc 自身が
断る**（"consider omitting the dots or providing an implicit instance of
`Liftable[Symbol]`"）。scala-rs も同じく断る。

**組まないものは名指しで診断する**:
`a hole of type `X` is not lifted (the Liftable instances scala-rs builds are …)`。
ユーザが書いた `Liftable` は探さないので、それも同じ診断になる（黙って別の木を
作るよりよい）。nsc にはあって scala-rs が組まないのは `liftList` / `liftArray` /
`liftMap` / `liftOption` / `liftEither` / `liftTuple*` / `liftScalaSymbol` で、
いずれも rank 0 の穴の形。

検証: `tests/fixtures/lf2_lift.scala` を実 scalac 2.13.16 と dual-run し、
**`showRaw` が完全一致**する（`TypeTree` は `showRaw` が中身の型を隠すので
`show` も並べて印字する）。29 行。`WeakTypeTag` と `Expr` は materialiser 無しには
実行時に作れないので、`tests/fixtures/lf2_ctx.scala` で**マクロ実装として**
コンパイルし、両コンパイラが通し、classfile が `java -Xverify:all` でロード・検証
されることを見る。異常系は `tests/fixtures/lf2_lift_bad.scala`。

#### 2. `symbolOf[T]` / `weakTypeOf[T]` / `typeOf[T]`

§7.6 の 3。`def symbolOf[T](implicit tag: WeakTypeTag[T]): TypeSymbol` は
型パラメータを**implicit 節にしか**書かず、結果型にも書かない。
`pin_undetermined_tparams`（`crates/typer/src/pickle_supply.rs`）はこの形の
メンバを**丸ごと落として**いたので、`symbolOf` は `not found: value symbolOf`
だった。

落とす理由は「型パラメータが決まらないまま implicit が解けず、typer が黙って
eta 展開する」ことの回避である。しかし *materialiser* の形
——節が implicit だけで、その implicit が当の型パラメータを要求する——は
`classTag[Short]` と同じく**常に明示型引数で呼ばれる**。そこで、この形に限って
メンバを残すようにした。明示型引数が無ければ `T` は `Nothing` になり
「implicit が見つからない」という診断になる（誤ったプログラムにはならない）。

効果:

- **マクロ実装の中では実際に解ける。** `implicit rTag: c.WeakTypeTag[R]` が
  スコープにあるので、`symbolOf[R]` / `weakTypeOf[R]` の implicit はそれで埋まる。
  slick の `ShapedValue.mapToImpl` の `val rSym = symbolOf[R]` がこれ。
- **外では正直な診断に変わる。** `u.typeOf[Int]` は
  `no implicit: could not find implicit value of type TypeTags$TypeTag[Int]`。
  `TypeTag` の materialization（型を `TypeCreator` に reify するコンパイラ内蔵
  マクロ）は未実装で、これが `c.typeOf[HList]` の残る障害である。

#### 3. `reify { … }` の診断

`scala.reflect.api.Universe` の `def reify[T](expr: T): Expr[T] = macro …` は
quasiquote と同じ**コンパイラ内蔵マクロ**で、scala-reflect.jar に実装は無く、
pickle のエントリには消去後の descriptor すら無い。そのため
`value reify is not a member of JavaUniverse` と言っていた——
`value q is not a member of StringContext` と同じ**嘘**である。

`Check::report_internal_universe_macro` が、レシーバが universe のとき
（無修飾なら `import <universe>._` が効いているとき）に

```
macro expansion is not implemented: cannot expand reify { ... }.
`reify` is a compiler-internal macro with no implementation in scala-reflect.jar,
so scala-rs would have to reify the expression itself, the way it does
quasiquotes; see docs/macros.md §6.2.
```

と言う。**式全体の木化は実装していない**（quasiquote と違い、任意の式を
`TreeCreator` の無名クラスに落とす必要がある）。

#### slick への効き方

`tests/slick_measure.sh`（scala-reflect.jar 入り）で `errors=237 → 228`、
`files_with_errors=60 → 60`。内訳:

| ファイル | before | after |
| --- | --- | --- |
| `ShapedValue.scala` | 20 | **10** |
| `TableQuery.scala` | 6 | 7 |

`TableQuery.scala` が 1 増えるのは、`typeOf` が「見つからない」から
「implicit が無い」に変わったことで、同じ行の 2 つめの穴
（`Ident(sym: Symbol)` のオーバーロードが未供給）まで見えるようになったため。
診断は正確になっている。

`ShapedValue.scala` の残り 10 件:

| 診断 | 件数 |
| --- | --- |
| `_` プレースホルダ（`(_.get)`、§7.7 の既知の形） | 3 |
| 持ち上げられない穴（`<error>` / `AnyRef`。下の cascade） | 3 |
| `value collect is not a member of Scopes.MemberScope` | 1 |
| `no implicit: TypeTag[HList]`（materialization 未実装） | 1 |
| マクロ def のシグネチャ検査（`must take blackbox.Context`） | 1 |
| `Shape` の型不一致（quasiquote と無関係） | 1 |

#### このスライスのあとに残っているもの

1. **`TypeTag` / `WeakTypeTag` の materialization。** `c.typeOf[HList]` と
   `implicitly[TypeTag[T]]` はこれが要る。型を `TypeCreator` の無名クラスに
   reify するコンパイラ内蔵マクロで、`reify { … }` と同じ機構になる。
2. **`reify { … }` 本体。** 式全体の木化。
3. **`_` プレースホルダと右結合演算子**（§7.7 の 2）。
4. **定義の quasiquote**（§7.7 の 4）。`ShapedValue` の `q"""…"""` 全体。
5. **universe の入れ子クラスがパス越しに引けない。** `u.WeakTypeTag[T]` /
   `u.TypeTag.Int` は `value TypeTag is not a member of JavaUniverse` になる
   （`c.WeakTypeTag[T]` は `Aliases` の型別名なので通る）。
6. **`c.universe.TermName` が `stable identifier required` になる。**
   `c.universe` は `val` なので安定しているはず。
7. **engine（フェーズ 2）。** マクロを*呼ぶ*ための JVM ブリッジ。
### 7.9 定義の quasiquote（`agent/defquasi` スライス）

§7.7 の残件 4。**`q"class C(...)"` / `q"case class C(...)"` / `q"trait T"` /
`q"object O { ... }"` / `q"def f(...) = ..."` / 修飾つきの `q"lazy val a = 1"`
のような定義が落とせるようになった。** 形はすべて実 scalac 2.13.16 の
`-Ymacro-debug-lite` から読み取り、`tests/fixtures/dq_defs.scala` が
**101 行ぶん `showRaw` まで実 scalac と突き合わせている**
（`java -Xverify:all` で実行、完全一致）。実装は
`crates/typer/src/reify_defs.rs`（`reify.rs` の `#[path]` 子モジュール。
`agent/liftable` と同じファイルを触らないための分割で、`reify.rs` 側の変更は
`mod` 宣言・`stat` の委譲・`term` の 2 アーム・`new_spine` の 1 フックだけ）。

#### 落とせるようになった形

| 形 | 落とす先 |
| --- | --- |
| `q"class C"` | `rs.SyntacticClassDef(mods, name, tparams, ctorMods, paramss, earlyDefs, parents, self, body)` |
| `q"trait T"` | `rs.SyntacticTraitDef(mods, name, tparams, earlyDefs, parents, self, body)` |
| `q"object O"` | `rs.SyntacticObjectDef(mods, name, earlyDefs, parents, self, body)` |
| `q"def f = 1"` | `rs.SyntacticDefDef(mods, name, tparams, paramss, tpt, rhs)` |
| `q"lazy val a = 1"` | `rs.SyntacticValDef(u.Modifiers(rs.FlagsRepr(2147483648L)), …)` |
| `q"var x = 1"` | `rs.SyntacticVarDef(…)`（`MUTABLE` は残す） |
| 末尾の implicit 節 | `rs.ImplicitParams(<残りの節>, <implicit 節>)` |
| 型パラメータ | `u.TypeDef(u.Modifiers(PARAM \| 変位), u.TypeName("T"), Nil, u.TypeBoundsTree(lo, hi))` |
| `q"new C(1) { ..$body }"` | `rs.SyntacticNew(Nil, List(<C(1)>), u.noSelfType, <body>)` |
| `q"super.foo"` | `rs.SyntacticSelectTerm(u.Super(u.This(u.TypeName("")), u.TypeName("")), …)` |
| `q"def f: Unit = {..$xs}"` | 右辺は `rs.SyntacticBlock(<xs>)` |
| 穴 | 名前（`q"class $tname"`）、パラメータリスト（`..$params`）、型パラメータ、親（`extends ..$parents`）、本体（`{ ..$body }`） |

#### `Modifiers` のフラグ変換が肝

`Modifiers` が運ぶのは **`scala.reflect.internal.Flags` のビット**で、
scala-rs のパーサの `Flags` とは**番号が違う**（`PRIVATE` はパーサでビット 0、
nsc でビット 2）。値はすべて `-Ymacro-debug-lite` が印字する
`FlagsRepr(<n>L)` から読み戻した:

| 修飾子 | nsc のビット | 確認に使った形 |
| --- | --- | --- |
| `PROTECTED` / `OVERRIDE` / `PRIVATE` | `1<<0` / `1<<1` / `1<<2` | `protected def f = 1` ほか |
| `ABSTRACT` / `DEFERRED` / `FINAL` | `1<<3` / `1<<4` / `1<<5` | `abstract class C` / `val a: Int` / `final class C` |
| `INTERFACE` / `IMPLICIT` / `SEALED` | `1<<7` / `1<<9` / `1<<10` | `trait T` / `implicit val` / `sealed class C` |
| `CASE` / `MUTABLE` / `PARAM` | `1<<11` / `1<<12` / `1<<13` | `case class C` / `var x = 1` / `def f(x: Int)` |
| `COVARIANT` / `CONTRAVARIANT` | `1<<16` / `1<<17` | `class C[+T]` |
| `LOCAL` | `1<<19` | `private[this] val x = 1` |
| `CASEACCESSOR` | `1<<24` | `case class C(x: Int)` の `x` |
| `TRAIT` ＝ `DEFAULTPARAM` | `1<<25` | `trait T` / `def f(x: Int = 1)` |
| `PARAMACCESSOR` | `1<<29` | クラス・パラメータ |
| `LAZY` | `1<<31` | `lazy val a = 1` |

パラメータのフラグは**クラスか `def` かで違う**。`def` のパラメータは `PARAM`
だけだが、クラス・パラメータは `PARAMACCESSOR` に加えて:

- `case` クラスの**第 1 節**は `CASEACCESSOR`（第 2 節以降は普通の扱い）
- `val` / `var` の無い非 `case` のパラメータは `PRIVATE | LOCAL`（メンバではない）
- `var` は `MUTABLE` かつ `SyntacticVarDef`

そして nsc の**パーサが補う親**も再現する: 親が書かれていなければ
`rs.ScalaDot(u.TypeName("AnyRef"))`、`case` なら書かれた親のうしろに
`rs.ScalaDot(Product)` と `rs.ScalaDot(Serializable)`（`case` のときは `AnyRef`
を補わない）。

#### パーサが潰す区別を、また元のソース文字列で戻す

- **`class C` と `class C {}`。** 本体が空でも、波括弧が書かれていれば nsc の
  body は `List(u.EmptyTree)`、書かれていなければ `List()` である。パーサは
  どちらも `body: []` にするので、定義の span のテキストが `}` で終わるかで決める。
- **`def f = {..$xs}` と `def f = $x`。** パーサは `{ e }` を `e` に潰すので、
  右辺の直前のテキストが `{` で終わるかで `SyntacticBlock` に包むかを決める。
- **手続き構文 `def f() { … }`。** nsc は結果型に `_root_.scala.Unit` を補うが、
  パーサは型を空のままにする。右辺の手前に `=` があるかで見分け、無ければ拒否する。

#### 落とせない形は名指しで診断する（`tests/fixtures/dq_defs_bad.scala`）

| 形 | 診断 | 理由 |
| --- | --- | --- |
| `q"class C { self => … }"` | a self type … | 本体が空のときの `List(EmptyTree)` と区別できない |
| `q"class C extends { val x = 1 } with D"` | an early definition … | nsc の `PRESUPER` はビット 37 で、パーサのフラグ語（32 ビット）に無い |
| `q"private[foo] val x = 1"` | a qualified access modifier (`private[X]`) … | `Modifiers` の名前欄。フラグしか運んでいない |
| `q"def f(x: => Int) = x"` | a by-name parameter … | nsc の型は `_root_.scala.<byname>[T]`、パーサはフラグ |
| `q"def f(x: Int*) = x"` | a repeated parameter (`T*`) … | 同上（`<repeated>`） |
| `q"def f() { 1 }"` | procedure syntax … | 上記 |
| `q"def f()"` | a `def` with neither a result type nor a body … | nsc は `_root_.scala.Unit` を補う |
| `q"{ val (a, b) = e; a }"` | a pattern definition … | パーサが 3 つの定義に脱糖する。nsc は 1 つの `SyntacticPatDef` |
| `q"class C[F[_]]"` | a higher-kinded type parameter … | 入れ子の型パラメータ |
| `q"def f[T: Ordering] = 1"` | a context bound (`T : C`) … | nsc の脱糖はパーサではなく typer |
| `q"case class C(x: Int) extends ..$parents"` | a `case` class whose parents are a `..$` splice … | `Product with Serializable` の連結が要る |
| `q"def f(implicit x: Int)(y: Int) = y"` | an implicit parameter clause that is not the last … | `ImplicitParams` は末尾の 1 節だけ |
| `q"def f = macro Impl.f"` | a `macro` definition … | 右辺が式ではない |
| `q"def f(x: Bar[_]) = x"` | a `_` type argument (an existential) … | nsc は `freshTypeName` で名前を作り、呼び出しの外側のブロックに束ねる |

#### ついでに直した一般の穴

| 直したもの | どこ |
| --- | --- |
| **`{ case class X(…); … }` が部分関数と誤読されていた。** ブロックの先頭の `case` は、次が `class` / `object` なら**修飾子**であって節の始まりではない。ローカルの `case class` を持つブロックが `expected pattern, found class` になっていた | `Parser::parse_block_expr` |

#### slick への効き方

`tests/slick_measure.sh`（scala-reflect.jar 入り）で `errors=237 → 237`。
**数字は動かない。** `ShapedValue.mapToImpl` の 15 行のエラーは
`symbolOf` / `Liftable`（`$uTag` / `$rTag` が `WeakTypeTag`）/
`_` プレースホルダ関数リテラルで落ちており、定義の形はそのどれでもない。
ただし本体の巨大な `q"""…"""`（`case class` ではなく `val` 3 つと、本体つきの
`new … { ..$fpChildren; override def read … }`）は、このスライスで
**`super` と `{..$xs}` の右辺まで通るようになり、残る唯一の障害が
`ProductResultConverter[_, _, _, _]` の `_` 型引数（存在型）になった**。
つまり `ShapedValue` の `q"""…"""` の次の一手は §7.7 と同じ性質の
「nsc が `fresh*Name` を使う形」であり、定義ではない。

#### このスライスのあとに残っているもの（§7.7 の一覧の更新）

1. **`Liftable`**（変わらず。`ShapedValue` の主要因）
2. **`_` プレースホルダ / 右結合演算子 / `_` 型引数（存在型）。** いずれも nsc が
   `freshTermName` / `freshTypeName` を使ったブロックを作る形で、同じ形を作るなら
   `rs.freshTypeName("_$")` を呼ぶブロックごと組む必要がある。
   `ShapedValue` の `q"""…"""` はこれ 1 つで止まっている
3. **`..$` と普通の引数の混在**、および**期待型からの型パラメータ推論**（§7.5）
4. **`q"{ type T = Int }"`**（`SyntacticTypeDef`）
5. **`reify { … }` と `typeOf[T]` / `symbolOf[T]`**
6. **engine（フェーズ 2）**

### 7.10 fresh 名を要する 3 形（`agent/freshname` スライス）

§7.9 の残件 2。**`_` プレースホルダ関数リテラル・`_` 型引数（存在型）・
右結合演算子が落とせるようになった。** この 3 つは、それまでの形と決定的に違う
点が 1 つある：**nsc の展開が 1 個の式ではなく「ブロック」である**。

```scala
// q"_.get" の -Ymacro-debug-lite 出力（universe を u、
// u.internal.reificationSupport を rs と略記）
{
  val nn$macro$1: u.TermName = rs.freshTermName("x$");
  rs.SyntacticFunction(
    List(rs.SyntacticValDef(u.Modifiers(rs.FlagsRepr(2105344L)), nn$macro$1,
                            rs.SyntacticEmptyTypeTree(), u.EmptyTree)),
    rs.SyntacticSelectTerm(rs.SyntacticTermIdent(nn$macro$1, false),
                           u.TermName("get")))
}
```

名前は**実行時に universe のカウンタから引く**（`freshTermName` /
`freshTypeName`）。だから scala-rs も「名前を決め打ちする」のではなく、
**同じ呼び出しをするブロックごと組む**必要がある。実装は `Reifier` に
`Fresh` 状態（`crates/typer/src/reify.rs`）を持たせ、木を組む途中で要求された
束縛を溜め、`reify` が最後にブロックで包む。3 形とも**同じ 1 つのブロック**に
まとめて持ち上げられる（nsc と同じ）。

#### 落とせるようになった形

| 形 | 落とす先 |
| --- | --- |
| `q"_.get"` | `{ val n = rs.freshTermName("x$"); rs.SyntacticFunction(List(rs.SyntacticValDef(mods(PARAM\|SYNTHETIC), n, …)), <本体の `_` は `SyntacticTermIdent(n, false)`>) }` |
| `q"_.foo(_)"` | 同じ。プレースホルダ 1 つにつき fresh 名 1 つ |
| `q"(_: Int).get"` | パラメータの型欄も本体の型注釈も nsc と同じく残る |
| `tq"P[_, _]"` | `{ val a = rs.freshTypeName("_$"); val b = …; rs.SyntacticExistentialType(rs.SyntacticAppliedType(<P>, List(rs.SyntacticTypeIdent(a), rs.SyntacticTypeIdent(b))), List(u.TypeDef(mods(DEFERRED\|SYNTHETIC), a, Nil, u.TypeBoundsTree(…)), …)) }` |
| `tq"P[_ <: Int]"` | 上界・下界は `TypeBoundsTree` に入る |
| `tq"Option[P[_]]"` | 存在型は**直下の引数に `_` を持つ適用**を包む（nsc と同じ入れ子位置） |
| `q"a :: b"` | `{ val n = rs.freshTermName("rassoc$"); rs.SyntacticBlock(List(rs.SyntacticValDef(mods(FINAL\|SYNTHETIC\|ARTIFACT), n, …, <a>), rs.SyntacticApplied(rs.SyntacticSelectTerm(<b>, u.TermName("$colon$colon")), List(List(rs.SyntacticTermIdent(n, false)))))) }` |
| `q"a :: b :: c"` | ブロックが入れ子になる（fresh 名 2 つ） |
| `q"b.::(a)"` | **ブロックにしない。** ドット呼びは普通の選択である |
| `pq"_: R[_, _]"` | 型変数パターン。`u.Bind(u.TypeName("_"), u.EmptyTree)`。fresh 名は要らない |
| `pq"_: R[_ <: Int]"` | 境界つきはパターンの中でも存在型 |

フラグ値はすべて `-Ymacro-debug-lite` の `FlagsRepr(<n>L)` から読み戻した:
`PARAM|SYNTHETIC` = 2105344、`DEFERRED|SYNTHETIC` = 2097168、
`FINAL|SYNTHETIC|ARTIFACT` = 70368746274848（`ARTIFACT` は `1L << 46`）。

#### パーサが潰す区別を、また元のソース文字列で戻す

- **`a :: b` と `b.::(a)`。** パーサは右結合演算子の受け手を右辺にするので
  どちらも `Apply(Select(b, "::"), [a])` になる。nsc はこの 2 つに**違う木**を作る
  （前者はブロック、後者は素の適用）。選択ノードの span のテキストが
  **演算子で始まるか**で見分ける: 中置なら span は演算子から始まり、
  ドット呼びなら被選択子から始まる。
- **プレースホルダのパラメータ。** パーサが作る `x$n` は
  `PARAM | SYNTHETIC`、ソースが書いたパラメータは `PARAM` だけ。この差で
  「名前を作る」のか「fresh 名を引く」のかを決める。
- **パターンの中の `_` 型引数。** 裸の `_` は型変数パターン（`Bind`）、
  境界つきは存在型。`pq` / `case` の下を歩いているかを `Fresh::pat_depth` で
  持ち回る。

#### 落とせない形は名指しで診断する（`tests/fixtures/fn2_fresh_bad.scala`）

| 形 | 診断 | 理由 |
| --- | --- | --- |
| `q"_"` | unbound placeholder parameter | 束縛するものが無い。実 scalac も同じく拒否する |
| `tq"_"` | a `_` type argument (an existential) … | 同上（nsc は "unbound wildcard type"） |

#### 検証: fresh 名をどう突き合わせるか

`tests/fixtures/fn2_fresh.scala` を実 scalac 2.13.16 と dual-run し、32 行を
`showRaw` で比較する（`java -Xverify:all`）。ただし fresh 名の**番号**は
そのままでは一致しない。理由は 2 つあり、どちらも木の違いではない:

1. カウンタは universe ごとにグローバルで、その行より前の全行と共有している。
2. nsc は右から左に名前を配る（`q"_.foo(_)"` は引数側のパラメータを先に採番する）。

そこで `crates/cli/tests/quasi.rs` の `renumber_fresh_names` が、
**1 行ごとに、初出順で 1 から採番し直してから**比較する。これで落ちるのは
上の 2 つだけで、**どの出現がどの束縛を指すか**は落ちない
（`_$1 … _$2` と `_$1 … _$1` は別の文字列のまま）。正規化そのものも
`renumber_fresh_names_keeps_binder_identity` で固定してある。

#### slick への効き方

`tests/slick_measure.sh`（scala-reflect.jar 入り）で
`errors=223 → 220`、`files_with_errors=60 → 60`。内訳:

| ファイル | before | after |
| --- | --- | --- |
| `ShapedValue.scala` | 10 | **7** |
| `TableQuery.scala` | 7 | 7 |

消えた 3 件は `(($rModule.unapply _) : $rTag => Option[$uTag]).andThen(_.get)`
の `_` プレースホルダ（62 / 65 / 68 行）である。`TableQuery.scala` は
`reify { … }` と `TypeTag` の materialization で落ちており、この 3 形とは無関係。

`ShapedValue.scala` の巨大な `q"""…"""`（77 行）は
`ProductResultConverter[_, _, _, _]`（パターン中の型変数パターン）と
`TypeMappingResultConverter[…, _]`（存在型）を**両方とも通るようになった**が、
いま落ちているのは `$f` / `$g` の型が `AnyRef` になる cascade で、その大元は
`rTag.tpe.decls.collect`（`value collect is not a member of MemberScope`）である。
形の問題は残っていない。同じ形を `fn2_fresh.scala` の最後の行が
（穴を持ち上げられるものに替えて）実 scalac と突き合わせている。

#### このスライスのあとに残っているもの（§7.9 の一覧の更新）

1. **`MemberScope#collect` など reflect API のコレクション操作**（`ShapedValue`
   の現在の大元）
2. **`TypeTag` / `WeakTypeTag` の materialization**（`c.typeOf[HList]`、
   `TableQuery` の `typeOf[Tag]`）
3. **`reify { … }` 本体**（式全体の木化。`TableQuery` の残り）
4. **`..$` と普通の引数の混在**、および**期待型からの型パラメータ推論**（§7.5）
5. **`q"{ type T = Int }"`**（`SyntacticTypeDef`）
6. **engine（フェーズ 2）**
### 7.10 `TypeTag` / `WeakTypeTag` の materialization（`agent/typetag` スライス）

§7.8 の残件 1。**`typeOf[T]` / `weakTypeOf[T]` / `typeTag[T]` が単相型について
実際に動くようになった。** `c.typeOf[HList]`（slick の `ShapedValue.mapToImpl`）
と `TableQuery` の `typeOf[Tag]` はこれが無くて止まっていた。

#### nsc が何をしているか（`-Xprint:typer` で実物確認）

`def typeOf[T](implicit ttag: TypeTag[T]): Type` の implicit が見つからないとき、
nsc は「見つからない」と言わない。**コンパイラ内蔵マクロ
`materializeTypeTag[T](u)`** を展開して、その場でタグを**作る**:

```scala
scala.reflect.runtime.`package`.universe.typeOf[String](({
  val $u: reflect.runtime.universe.type = scala.reflect.runtime.`package`.universe;
  val $m: $u.Mirror = $u.runtimeMirror(this.getClass().getClassLoader());
  $u.TypeTag.apply[String]($m, {
    final class $typecreator1 extends TypeCreator {
      def apply[U <: scala.reflect.api.Universe with Singleton](
          $m$untyped: scala.reflect.api.Mirror[U]): U#Type = {
        val $u: U = $m$untyped.universe;
        val $m: $u.Mirror = $m$untyped.asInstanceOf[$u.Mirror];
        $u.internal.reificationSupport.TypeRef(…)   // String はここまで書く
      }
    };
    new $typecreator1()
  })
}: reflect.runtime.universe.TypeTag[String]))
```

マクロ実装の中（`c.typeOf[Hl]`）だと `$u` は `c.universe`、`$m` は
`c.universe.rootMirror` になり、トップレベルのクラスは
`$m.staticClass("Hl").asType.toTypeConstructor` の 1 行で済む。
`Int` のような基本型は `TypeCreator` すら作らず `$u.TypeTag.Int` を使う。

#### scala-rs が組む木

実装は `crates/typer/src/materialize.rs`、入口は
`Check::materialize_tag`（`fill_implicit_params_in` の
`classtag_apply_fallback` と同じ並びのフォールバック。nsc が `ClassTag` を
materialize するのと同じ位置である）。

```text
{
  final class $typecreator1 extends scala.reflect.api.TypeCreator {
    def apply[U <: scala.reflect.api.Universe with Singleton](
        $m$untyped: scala.reflect.api.Mirror[U]): <Types.TypeApi> =
      $m$untyped.staticClass("Foo").asType.toTypeConstructor
  }
  <universe>.TypeTag.apply[Foo](
    <universe>.rootMirror.asInstanceOf[<api.Mirror>], new $typecreator1())
}
```

これは**普通の untyped な scala-rs の木**で、quasiquote の reification と同じく
そのまま `type_expr` に通す。ローカルクラスがブロックの中に立つのは、typer の
`TreeKind::Block` が「まだシンボルの無い `ClassDef` にはその場で namer を回す」
ようにできているからで、implicit 探索の最中に定義を 1 つ生やせる。

universe をどれにするかは `universe_in_scope()`——`import <universe>._` の prefix
——で決める。quasiquote が `q"..."` の universe を決めるのと同じ読み方である。
その import が無ければ materialize せず、今までどおり「no implicit」と言う。

#### nsc と違えた 3 点（**木の一致は要求しない**）

タグの木そのものではなく、**`tag.tpe` の実行結果**（`toString` / `=:=` / `<:<` /
`typeSymbol.fullName`）が実 scalac 2.13.16 と一致することを検証している
（`tests/fixtures/tt_tags.scala`、30 行）。違いは 3 つ:

| | nsc | scala-rs | なぜ |
| --- | --- | --- | --- |
| `$u` / `$m` の束縛 | `val` に束ねてから使う | `apply` の引数を直接選択する | 木が小さい。`tag.tpe` は同じ |
| runtime universe の mirror | `runtimeMirror(getClass.getClassLoader)` | `rootMirror` | `JavaUniverse#runtimeMirror` はまだ供給できない（パラメータの `java.lang.ClassLoader` にシンボルが無く、`ensure_class` が `scala.` 以外の pickle 無しクラスを断る）。root mirror の class loader から見えないクラスでだけ挙動が違い、そのときは `ScalaReflectionException` になる（黙って違う型にはならない） |
| creator の結果型 | `U#Type` と書き、nsc の erasure が `Types$TypeApi` にする | `Types$TypeApi` を直接書く | scala-rs は抽象型メンバを `Object` に erase する（`erasure::erase_ty`）。`TypeCreator.apply` は**抽象**なので、`Object` を返す descriptor は何も override せず、最初の `tag.tpe` が `AbstractMethodError` になる |

mirror の引数に `asInstanceOf` を挟むのも同じ性質の埋め合わせである。
`rootMirror` の型は universe の抽象メンバ `Mirror` で、その上界は pickle 上
`JavaMirror` までしか辿れない（`JavaMirror extends api.Mirror[self.type]` の
親は、singleton 引数が変換できないので `conv_upper_bound` が落とす）。
値は本当に `Mirror` なので、cast は常に成功する `checkcast` になる。

#### 供給側で塞いだ穴

`u.TypeTag.apply` を呼ぶには、その前に 3 つ足りないものがあった（§7.8 の残件 5
がまさにこれ）。

| 直したもの | どこ |
| --- | --- |
| **`TypeTags$TypeTag$` にシンボルが無い。** トレイトの入れ子オブジェクトの classfile は自分の `ScalaSignature` を持たない（pickle は囲む `TypeTags` の中）ので `install_classpath` が読み飛ばす。結果、descriptor `()Lscala/reflect/api/TypeTags$TypeTag$;` は解決できない `Type::Named` のままで、`value apply is not a member of TypeTags$TypeTag$` だった。`ModuleClass` を建て、`apply[T](Mirror, TypeCreator): TypeTag[T]` を**手で**入れる。erased descriptor は書き下す（メソッドシンボルの `jvm_name` が `(` で始まればそれが descriptor になる。pickle 供給と同じ約束）。pickle の署名は `Mirror[TypeTags.this.type]` で、この singleton 引数を scala-rs は綴れない | `materialize::ensure_tag_module` |
| **`TypeTags#typeOf` の implicit パラメータが `Type::Named`。** `install_classpath` が読む pickle サブセットはメンバ型を**単純名**で持つので、`TypeTags$TypeTag` という名前は誰も入れておらず未解決だった。これが `no implicit: could not find implicit value of type TypeTags$TypeTag[Foo]` の正体で、erasure もこの型から descriptor を書くところだった | `materialize::resolve_named_tags` |
| **`TypeTags#TypeTag` のアクセサ自体が無いことがある。** `TypeTags` が classfile として読まれれば `TypeTag()` はメソッド一覧に載るが、pickle 経由（classpath 走査で誰も名指ししなかったとき）だと module メンバは `complete_named` が入れる形に含まれず、アクセサごと無い。descriptor を書いてここで宣言する。さらに `TypeTags` は `JavaUniverse` の**直接の**親ではない（`api.Universe` の親で、そこは pickle にしかない）ので、先に `supply_from_pickle` で祖先を辿らせておく——さもないと**その run の最初の `typeOf[T]` だけ**が「value TypeTag is not a member of JavaUniverse」で落ちた | `materialize::ensure_tag_module` / `Check::materialize_tag` |
| **解決済みの型を型木として差せない。** `TypeTag.apply[T]` の `T` も、cast 先の `api.Mirror` も、使用地点には名前で辿る道が無い（`scala.reflect.api.Mirror` は import されていない）。nsc の `TypeTree(tp)` にあたる目印 `Ident("$resolvedType")` を置き、`tree_to_type` がその `ty` をそのまま返す | `materialize::RESOLVED_TYPE` / `Check::tree_to_type` |

#### 作れる形と、名指しで断る形

`staticClass(<name>)` は**クラスを 1 つ**名指しする呼び出しなので、
scala-rs が組むのは**型引数の無いクラス型**だけである。

作れる: 9 つの基本型 / `Unit` / `String` / `Any` / `AnyVal` / `Nothing` / `Null` /
トップレベルのクラス・トレイト（`Foo`、`scala.math.BigInt`、
`slick.collection.heterogeneous.HList`）。

断る（`tests/fixtures/tt_tags_bad.scala` が固定している）:

| 形 | 診断 | 理由 |
| --- | --- | --- |
| `typeOf[List[Int]]` | a type constructor applied to type arguments | nsc は prefix と symbol と引数から `TypeRef` を組む |
| `typeOf[Nest.Inner]` | a class nested in a class or an object rather than a top-level one | `staticClass` はパッケージしか辿らない。nsc は `selectType` を使う |
| `typeOf[AnyRef]` | which is an alias rather than a class | `java.lang.Object` の別名。`staticClass` は実行時に落ちる |
| `typeOf[T]`（型パラメータ） | an abstract type with no tag in scope | nsc も `No TypeTag available for T` と断る。`WeakTypeTag` は free type を作るが未実装 |
| `typeOf[Main.type]` | a singleton type | |
| 構造的型 / 関数型 / タプル / 配列 | a structural type / whose type arguments would have to be reified too | |

**黙って別の型を作らない**ことが要点である。間違ったタグはコンパイルエラーに
ならず、実行時に「違う `Type`」としてマクロに渡るだけなので、後から見つけるのが
最も難しい種類の欠陥になる。

#### 検証

- `tests/fixtures/tt_tags.scala` — scala-rs と実 scalac 2.13.16 の**両方**で
  コンパイルして実行し、30 行の出力が完全一致する（`java -Xverify:all`）。
  `crates/cli/tests/quasi.rs` の `tt_tags_materialises_type_tags` /
  `tt_tags_matches_real_scalac`。
- `tests/fixtures/tt_ctx.scala` — マクロ実装の中の `c.typeOf[HL]` /
  `c.weakTypeOf[Rep]`（slick の `mapToImpl` の形）。両コンパイラが通し、
  classfile が JVM にロード・検証される（展開には engine が要る）。
- `tests/fixtures/tt_tags_bad.scala` — 断る 7 形がすべて名指しで診断される。

#### slick への効き方

`tests/slick_measure.sh`（scala-reflect.jar 入り）で `errors=223 → 221`、
`files_with_errors=60 → 60`。内訳:

| ファイル | before | after |
| --- | --- | --- |
| `ShapedValue.scala` | 10 | **9** |
| `TableQuery.scala` | 7 | **6** |

どちらも消えたのは `no implicit: could not find implicit value of type
TypeTags$TypeTag[...]` で、`c.typeOf[slick.collection.heterogeneous.HList]` と
`typeOf[Tag]` が**実際に通る**ようになった。ログに `TypeTag` の implicit エラーは
1 件も残っていない。

#### このスライスのあとに残っているもの

1. **型引数のある型のタグ。** `TypeTag[List[Int]]`。nsc の
   `internal.reificationSupport.TypeRef` / `SingleType` / `selectType` を
   creator の本体に組む必要がある。入れ子クラス（`selectType`）も同じ道具立て。
2. **タグの型を名前で書けない。** `implicitly[TypeTag[Foo]]` は
   materialization ではなく `TypeTag` という**型名**が引けないところで落ちる
   （無修飾は `not found: type TypeTag`、パス越しの `u.TypeTag[Foo]` は
   `type TypeTag is not a member of JavaUniverse`）。§7.8 の残件 4・5 のままで、
   `typeTag[Foo]` / `weakTypeTag[Foo]` は同じ implicit を要求するので通る。
3. **`runtimeMirror(getClass.getClassLoader)`。** `java.lang.ClassLoader` に
   シンボルが無く、`ensure_class` が `scala.` 以外の pickle 無しクラスを断るため
   メンバごと供給されない（`parameter cl has an unmappable type`）。
4. **`reify { … }` 本体**（§7.8 の残件 2）。式全体の `TreeCreator` 化。
   materialization と同じ機構の上に載る。
5. **engine（フェーズ 2）。** マクロを*呼ぶ*ための JVM ブリッジ。

### 7.11 engine — マクロ実装を本当に呼ぶ（`agent/engine` スライス）

§6 の**フェーズ 2**。§2.3 の prototype を製品コードにし、
**`def f = macro Impl.m` の呼び出しが実際に展開され、展開後のプログラムが走る**
ようになった。実 scalac 2.13.16 と同じ 2 ファイル・2 回コンパイルの構成で
dual-run し、**プログラム出力が完全一致**する（`crates/cli/tests/engine.rs`）。

#### 形（ブリッジの構成）

engine は **Java 1 ファイル**（`crates/typer/java/ScalaRsMacroEngine.java`）で、
Scala のクラスは**すべてリフレクション経由**で触る。したがって `javac` に
scala-reflect.jar は要らず、リポジトリに classfile も置かない。
`include_str!` でバイナリに埋め込み、初回展開時に

```
$TMPDIR/scala-rs-macro-engine-<ソースの FNV ハッシュ>/
```

へ書き出して `javac` する（ハッシュ付きなので古い classfile が走ることはない）。

- **常駐 1 プロセス／1 コンパイル。** 最初の展開で `java` を起動し、
  以降は 1 行 1 リクエストのパイプで捌く（§6.4 のリスク表「engine プロセスの
  起動コスト」への回答）。`Typer` が落ちるときに `Drop` で kill する。
- **classpath は `binary_path` そのもの**（`-cp` ＋ `--scala-library`）。
  nsc が `-Ymacro-classpath` 既定でコンパイル classpath を使うのと同じで、
  §2.3 で分かった「reify の `staticModule` はコンパイル対象のクラスも要求する」
  という注意もこれで満たされる。
- **`Context` は `java.lang.reflect.Proxy`**（prototype と同じ）。実装したのは
  `universe` / `mirror` / `Expr` / `WeakTypeTag` / `TypeTag` / `TermName` /
  `TypeName` / `freshName` / `abort` と、トレイトの default 実装
  （`invokeDefault`）。それ以外は `UnsupportedOperationException` で落ち、
  Rust 側は**その名前を診断に出す**。
- **直列化は S 式**（JSON ではなく）。両端とも自前パーサが 60 行で書け、
  1 行 1 メッセージでパイプに乗る。§4.2 の JSON 案と情報量は同じである。

```
→ (expand "EgImpl$" "plusImpl" (argss (args (arg expr <tree> (ty "scala.Int")))) (tags))
← (ok (t "Apply" (s0) (t "Select" (s0) (t "Literal" (s0) (c "Int" "41")) (n term "$plus"))
        (l (t "Literal" (s0) (c "Int" "1")))))
```

**戻りの木は engine が汎用に書く**。ノード種別を engine は知らない：
`productPrefix` と `productElement` をそのまま並べ、`Symbol` は
`isStatic` のときだけ完全修飾名を添える。「この形は作れない」と判断するのは
**Rust 側だけ**で、知らない `Prefix` は必ず名指しの診断になる。

#### 展開をどこでやるか

nsc と同じく **typer の中**、**macro application の一番外側**で展開する
（`Check::type_expr` の末尾、`adapt` の**手前**）。「一番外側」は
`typing_callee` という 1 ビットで見分ける：`Apply` / `TypeApply` が callee を
型付ける直前に立て、`type_expr` の入口で `mem::take` する。だから
`M.f` は `M.f(1)` の head としては展開されず、レシーバの中の
`M.g(1).h` は展開される。カリー化されたマクロの内側の `Apply` は
「まだ `Type::Method`」で弾かれる。

blackbox なので、展開結果は**宣言された戻り値型**を期待型として 1 回だけ
型検査し、型はその宣言型に戻す（nsc の `Typed(expanded, TypeTree(innerPt))`）。

**展開できなかったものは 1 件残らず診断になる。** `report_macro_calls` の
掃除は phase 1 のまま残してあり、展開器は失敗の**理由**を span ごとに記録して
そこに載せるだけである:

```
error: macro expansion is not implemented: cannot expand nameOf
       (implementation EgImpl$.nameOfImpl): scala-rs cannot build a type tag for
       `List[Int]`, a type constructor applied to type arguments. See docs/macros.md.
```

#### 2 回コンパイルであることは仕様である

nsc は「マクロ実装は**展開が起きる run より前に**コンパイル済みでなければ
ならない」と決めている（§1.3）。scala-rs も同じで、実装が macro classpath に
無ければ engine が `ClassNotFoundException` を返し、それが
`is not on the macro classpath (nsc requires the implementation to have been
compiled by an earlier run)` という理由になる
（`tests/fixtures/eg_samerun_bad.scala` が固定）。
マクロ **def** の側は現在の run にあってよい（slick もその形）。

#### 通るようになった形

| 形 | 例 | 備考 |
| --- | --- | --- |
| 引数なし | `def const(): Int = macro EgImpl.constImpl` | 展開は `Literal(Constant(42))` |
| `c.Expr[T]` 引数 | `def plus1(x: Int): Int` | 呼び出し地点の木を `Expr` に包んで渡す |
| 生の `c.Tree` 引数 | `def twice(x: Int): Int` | 2.11 以降の形。slick の `mapToImpl` がこれ |
| `c.WeakTypeTag[T]` | `def nameOf[T]: String = macro EgImpl.nameOfImpl[T]` | 型引数は**明示**のときだけ |
| 展開結果の木 | `Literal` / `Ident` / `Select` / `Apply` / `TypeApply` / `Block` / `If` / `Typed` / `This` / `EmptyTree` / `TypeTree` | それ以外は名指しで断る |
| static シンボル | 展開の `Ident(EgHelper)` | `isStatic` なら完全修飾パスに展開して呼び出し地点で解決する |

#### 道中で塞いだ 2 つの一般の穴（どちらも「今まで誰も走らせていなかった」）

| 直したもの | どこ |
| --- | --- |
| **`blackbox.Context` がインタフェースとして立っていなかった。** `prelude_reflect` の placeholder は `Flags::EMPTY` で、scala-reflect.jar がある実行でも**この symbol がそのまま本物として使われる**（`ensure_class` は `find_by_jvm` でこれを返す）。結果、マクロ実装の `c.universe` は `invokevirtual` になり、**実行した瞬間に `IncompatibleClassChangeError`** だった。§7.6 の fixture は「classfile がロード・検証できる」までしか見ていなかったので気づけていない | `prelude_reflect::ctx` |
| **pickle が trait と言っているのに placeholder が class のままだった。** `find_or_stub_java_class` が descriptor から建てた symbol は trait/class を知らない。`give_stub_its_kinds` は型パラメータのある classだけを直していたので、`scala.reflect.macros.Universe` のような **型パラメータの無い** trait は class のままだった | `PickleSupply::give_stub_its_kinds` |

#### 検証

- `tests/fixtures/eg_impl.scala` + `tests/fixtures/eg_use.scala` —
  scala-rs で 2 段コンパイルして実行し、8 行の出力が
  `tests/fixtures/expected/eg_use.txt` と一致する（`java -Xverify:all`）。
  **同じ 2 ファイルを実 scalac 2.13.16 でも 2 段コンパイルして実行し、
  同じ 8 行になることを別テストで固定**している。マクロが「違う木」に
  展開されてもコンパイルは通ってしまうので、**出力の比較だけが
  間違った展開を捕まえられる**。
- `tests/fixtures/eg_samerun_bad.scala` — 同一 run に実装がある場合。
- `tests/fixtures/eg_gaps_bad.scala` — 渡せない引数の形・作れないタグ。

#### このスライスのあとに残っているもの

1. **`c.Expr[T](tree)` が scala-rs でコンパイルできない。** `Context.Expr` の
   オーバーロード（`def Expr[T: WeakTypeTag](tree: Tree): Expr[T]`）に解決せず、
   `universe.Expr.apply` の方に当たる。だから fixture の実装は
   すべて `c.Tree` を返している。**slick の `TableQueryMacroImpl` は
   `c.Expr` を返す**ので、これは必要になる。
2. **推論された型引数がタグにならない。** `M.f[T]` と明示された場合だけ
   タグを作る。呼び出し地点で推論された型引数は typer が木に残さないので、
   いまは名指しで断っている。
3. **引数の木は「書かれた構文」しか運べない。** 型付き木のまま渡す
   （§4.3）のではなく、`Literal` / `Ident` / `Select` / `Apply` / `This` を
   構文として渡して呼び出し地点で型検査し直す。ブロック・関数リテラル・`new`
   などは名指しで断る。slick の `mapToImpl` は `c.prefix` を見るので、
   ここは `prefix` の実装（未実装、`UnsupportedOperationException`）と
   合わせて次の一手になる。
4. **`c.prefix` / `c.enclosingPosition` / `c.typecheck` / `c.inferImplicitValue`。**
   `prefix` は呼び出し地点のレシーバ木、`enclosingPosition` は span の変換で
   でき、`typecheck` / `inferImplicitValue` は engine → Rust の逆方向 RPC が要る
   （§6.4）。slick が使うのは `prefix` / `enclosingPosition` / `abort` までで、
   `abort` は実装済み。
5. **展開結果の `TypeTree` は型引数の無いクラスだけ。** `List[Int]` を
   埋めた木は断る。
6. **whitebox。** 変わらず未実装（§6.3）。
7. **`MACRO` フラグと `@macroImpl` の pickle（§5）。** マクロ def を
   *別 run* から展開することはまだできない。いまは「マクロ def は現在の run、
   実装は前の run」という形だけが通る。slick は 1 ファイルに def と実装を
   並べるので、この形で足りる。

#### slick への効き方

`tests/slick_measure.sh` で `errors=203 → 203`、`files_with_errors=60 → 60`、
`tests/slick_subset.sh` は `204/204` のまま。**数字は動かない。**
slick の `TableQuery.apply` / `ShapedValue.mapTo` の呼び出し地点は
「実装が同じ run にある」ので nsc でも展開できない形であり、
engine が効くのは**slick を classfile として先にコンパイルできてから**である。
このスライスが動かすのは §7.1〜7.10 が積み上げてきた「実装をコンパイルする」
側ではなく、その先の「実装を呼ぶ」側で、slick に効くのは
残件 1（`c.Expr`）と 3〜4（`c.prefix`）が入ってからになる。

### 7.12 `c.Expr[T](tree)` と `c.prefix`（`agent/expr` スライス）

§7.11 の残件 1（`c.Expr`）と 4 の一部（`c.prefix`）。あわせて、
**`c.Expr[F[E]]` が要求する `WeakTypeTag[F[E]]` を組み立てられる**
ようになった。この 3 つが揃うと **slick の `TableQueryMacroImpl.apply` と
同じ形のマクロ**が書けて展開でき、実 scalac 2.13.16 と dual-run で
プログラム出力が一致する（`tests/fixtures/ex_impl.scala` +
`tests/fixtures/ex_use.scala`）。

#### 1. `c.Expr[T](tree)` — 値位置の畳み込みが早すぎた

`scala.reflect.macros.Aliases` は `Expr` を **2 つ**宣言している:

```scala
val Expr: universe.Expr.type                       // 抽出子オブジェクト
def Expr[T: WeakTypeTag](tree: Tree): Expr[T]      // 生成メソッド
```

`c.Expr` の選択は `Type::Overload` で始まるが、`maybe_auto_apply` が
**SLS 6.26.3（値位置ではパラメータを取らない候補だけ残す）** をその場で
適用して `val` の方に潰していた。潰れた結果は `universe.Expr$` という
モジュールなので、続く `[Int]` はモジュール→`apply` のリダイレクトに乗り、
`universe.Expr.apply(Mirror, TreeCreator)` に当たって
`no matching overload` になっていた。

nsc の順序は逆で、**明示型引数はオーバーロードを先に絞る**。そこで:

- 選択が畳み込んだときは、その集合を**生き残ったシンボルの側にも**記録する
  （`overload_member_types` / `overload_groups`。呼び出し側が持っている鍵は
  `found[0]` ではなく畳み込み後のシンボルなので）。
- `TypeApply` は、型引数の個数に合う候補が**ちょうど 1 つ**あり、いま持って
  いるシンボルの型パラメータ数がそれと違うときだけ、そちらへ差し替える
  （`Check::alt_taking_targs`）。集合が本当に 2 つ以上だった場合に限るので、
  `Ordering[String]` のような「候補 1 つ」の従来経路は素通りする。

#### 2. `c.prefix` — 呼び出し地点のレシーバ

`peel_application` が `Apply`/`TypeApply` を剥がした先が `Select` なら、
その `qual` が prefix である。**木だけ**を engine に送り、engine 側は
nsc と同じく `Expr[Nothing](prefixTree)(TypeTag.Nothing)` を作る
（blackbox の `PrefixType` は抽象メンバなので、nsc でも
`c.prefix.staticType` は `Nothing` になる。fixture がこれを固定している）。

運べないレシーバ（`new`、ブロック、レシーバなしの呼び出し）は
**その場ではエラーにしない**。実装が `prefix` を読むかどうかは呼び出し側から
分からないので、**理由の文字列を一緒に送り**、engine は `prefix` が実際に
読まれたときだけその理由を載せて投げる。読まない実装は素通しで展開される。

#### 3. `WeakTypeTag[F[E]]` の組み立て

`c.Expr[ExBox[E]](tree)` は暗黙の `WeakTypeTag[ExBox[E]]` を要求する。
§7.10 の materialiser は `staticClass` 1 回で作れる**単相クラスだけ**だったので、
ここで止まっていた。creator の本体を 3 形の合成に一般化した
（`materialize::TagBody`）:

| 形 | 生成する木 |
| --- | --- |
| 単相クラス | `$m$untyped.staticClass("N").asType.toTypeConstructor`（従来） |
| 型構築子の適用 | `$m$untyped.universe.appliedType($m$untyped.staticClass("N"), List(<各引数>))` |
| 型パラメータ | `<スコープ内のタグ>.in($m$untyped).tpe` |

`appliedType(sym, args)` は nsc が書く
`internal.reificationSupport.TypeRef(thisPrefix(owner), sym, List(…))` の
公開版である（シンボルの `typeConstructor` が `TypeRef(owner.thisType, sym, Nil)`
だから同じ `TypeRef` になる）。型パラメータのタグは**通常の暗黙探索**で
引く。materialisation は探索が失敗した*あと*の代替なので、循環はしない。

作れない形は従来どおり名指しで断る。合成は**再帰する**ので、引数が作れない
`List[Nest.Inner]` は「`Inner`, a class nested in a class or an object」と、
引数の方を名指しする。タプル・関数型（`scala.TupleN` / `scala.FunctionN` への
展開が要る）と、タグの無い型パラメータ（nsc は free type symbol を立てるが
scala-rs はやらない）は引き続き断る。`tests/fixtures/tt_tags_bad.scala` が固定する。

**既知のずれ（1 件）**: `Predef.Map` のような**型別名**を経由した構築子では、
nsc の creator が別名を保つ（`selectType(staticModule("scala.Predef"), "Map")`）のに対し、
scala-rs は別名の指すクラスを `staticClass` する。両者は `=:=` で `typeSymbol` も同じだが、
`toString` が `Map[String,Foo]` と `scala.collection.immutable.Map[String,Foo]` に分かれる。
§7.10 が `Predef.String` について既に記録しているのと同じずれで、`String` では
たまたま表示が一致していただけである。`tt_tags.scala` は `Map` については
`=:=` と `typeSymbol.fullName` を比較している（`toString` ではなく）。

#### 4. 展開結果の `New`

reflect の `new C(args)` は `Apply(Select(New(tpt), termNames.CONSTRUCTOR), args)`、
scala-rs の木では `Apply(New(tpt), args)`。`New` を受け取れるようにし、
`New` の上の `<init>` 選択は畳んで落とす。slick の `TableQueryMacroImpl` が
`New(TypeTree(e.tpe))` を書くので、これが要る。

#### 検証

- `tests/fixtures/ex_impl.scala` + `tests/fixtures/ex_use.scala` — scala-rs で
  2 段コンパイルして実行し、`tests/fixtures/expected/ex_use.txt` と一致する
  （`java -Xverify:all`）。**同じ 2 ファイルを実 scalac 2.13.16 でも 2 段
  コンパイルして実行し、同じ 10 行になることを別テストで固定**している。
  出力には `weakTypeOf[ExBox[E]].toString`（＝合成したタグの型）と
  `c.prefix.staticType.toString` が含まれるので、**タグと prefix の作り方が
  nsc と違えば行が変わる**。
- `tests/fixtures/tt_tags.scala` — マクロの外の materialisation。
  `List[Int]` / `Option[Foo]` / `List[List[Int]]` を追加し、実 scalac と
  `tag.tpe` の文字列まで一致することを固定した（従来は名指しで断っていた）。
- `tests/fixtures/ex_notag_bad.scala` — 合成できないタグ。
- `tests/fixtures/ex_gaps_bad.scala` — 運べないレシーバ 2 種。
  どちらも実 scalac は通るので、scala-rs 側の穴を固定した fixture である。

#### このスライスのあとに残っているもの

1. **`c.prefix` に `This` を作れない。** レシーバを書かずに呼んだマクロは
   nsc なら `This(<囲むクラス>)` が prefix になる。`ex_gaps_bad.scala` が
   名指しで固定している。
2. **引数・レシーバの木は「書かれた構文」のまま**（§7.11 残件 3）。
   `new`・ブロック・関数リテラルは運べない。§4.3 の「型付き木のまま渡す」は
   未実装で、slick の `mapToImpl` は `c.prefix` の**木**しか見ないので
   そこは足りるが、`ShapedValue(...)` のような式をレシーバに書かれると届かない。
3. **展開結果に `Function` / `ValDef` / `Modifiers` を作れない。**
   slick の `TableQueryMacroImpl` は `Function(List(ValDef(…)), …)` を
   `TableQuery.apply[E](cons)` に渡すので、**本物の slick に効かせるには
   これが要る**。いまは名指しで断る。
4. **`reify`。** `TableQueryMacroImpl` の最後の 1 行は `reify { … }` で、
   これは fast track マクロなので JVM ブリッジでは展開できない（§6.2）。
   実装を scala-rs でコンパイルするには自前の reify が要る（§7.8 に診断あり）。
5. 推論された型引数がタグにならない（§7.11 残件 2）、`TypeTree` に型引数を
   埋められない（同 5）、whitebox（同 6）、`@macroImpl` の pickle（同 7）は
   そのまま。

#### slick への効き方

`tests/slick_measure.sh` は `errors=177 → 177`、`files_with_errors=57 → 57`。
`tests/slick_subset.sh` は `38 files / 204 classes / verified=204 failed=0` のまま。
**数字は動かない。** §7.11 に書いたとおり、slick の 2 マクロは
「def と実装が同じ run にある」ので nsc でも展開できない形であり、
段階 D（slick を 2 段コンパイルする実験）には上の残件 3・4 が要る。
このスライスが動かしたのは「slick の 2 マクロと**同じ形**のマクロを
書いて展開できる」ところまでである。

### 7.13 段階 D-1: 展開結果の `Function` / `ValDef`（`agent/staged` スライス）

§7.12 の残件 3。**展開結果に `Function` と `ValDef` を作れるようになった**ので、
slick の `TableQueryMacroImpl.apply` が組む木——

```scala
Function(
  List(ValDef(Modifiers(Flag.PARAM), TermName("tag"),
              Ident(typeOf[Tag].typeSymbol), EmptyTree)),
  Apply(Select(New(TypeTree(e.tpe)), termNames.CONSTRUCTOR),
        List(Ident(TermName("tag")))))
```

——が丸ごと往復し、展開後のプログラムが走る。実 scalac 2.13.16 と 2 段コンパイルで
dual-run し、出力が完全一致する（`tests/fixtures/sd_impl.scala` +
`tests/fixtures/sd_use.scala`）。

#### 1. `Modifiers` は**名前**で運ぶ

`ValDef` を作るには `Modifiers` が要る。engine は `productElement` を
そのまま流すので、これまで `Modifiers` は `(o "Modifiers(PARAM)")` という
`toString` になっていた。

数値（`FlagSet` は `Long`）を送る案は採らない。nsc のビット配置は内部仕様で、
しかも **1 つのビットに 2 つの名前が乗っている**（`BYNAMEPARAM` は `COVARIANT`、
`DEFAULTPARAM` は `TRAIT`）。そこで engine は `universe.Flag` の
**0 引数・戻り値 `long` のメソッドを反射で列挙**し、立っているビットの名前を
すべて書く。名前の付かなかった残りビットは 16 進数で添える。

```
(mods (f "PARAM") (rest "0") "" (l))
```

Rust 側は名前を自分の `Flags` に写す。**表に無い名前と、名前の付かない残りビットは
どちらも診断**である（`the expansion contains a definition marked `DEFERRED`,
a modifier scala-rs cannot rebuild yet`）。黙って落とすと `var` を `val` に、
`lazy val` を正格な `val` に組み替えてしまい、誰も気づかない。
2 つ名前のあるビットは、**この展開器が組む唯一の定義である `ValDef` としての
読み**を採る（`BYNAMEPARAM` / `DEFAULTPARAM`）。`privateWithin` と
アノテーションも運ぶ（アノテーション付きは現状 診断）。

#### 2. 道中で塞いだ 3 つの一般の穴

| 直したもの | どこ | 影響 |
| --- | --- | --- |
| **`import c.universe._` が暗黙の `import scala._` に負けていた。** `expose_unqualified` は「囲むパッケージ → `scala._` → `java.lang._` → root → **wildcard import**」の順で探していた。SLS 2 では明示の import の方が上（`scala._` / `java.lang._` は最外側の wildcard import）。そのため `Function(vparams, body)` は `scala.Function`（`apply` を持たないオブジェクト）に解決し、**slick が書いているマクロ実装をそもそもコンパイルできなかった** | `Check::expose_from_wildcards` | wildcard 段を `scala._` の前に出した。eager に入る名前は現行スコープにあるのでこの経路を通らず、影響は「pickle から遅延で読む名前」に限られる |
| **`scala.Int` と書くと primitive にならなかった。** パスとして書かれた `scala.Int` はパッケージのメンバ探索に当たって `Type::Class` になる。表示は `Int` なのに何とも等しくないので `val x: scala.Int = 1` が `type mismatch; found: 1  required: Int` だった | `check::scala_value_type` | 展開結果の `TypeTree(typeOf[Int])` は完全名で届くので、この経路がそのまま必要 |
| **タプル・関数型・配列のタグが作れなかった**（§7.12 の既知の残件） | `Check::tag_body` | `scala.TupleN` / `scala.FunctionN` / `scala.Array` を名指しして §7.12 の `appliedType` 合成に乗せる。slick の `c.Expr[Tag => E]` がこれを要求する。`tt_tags.scala` が実 scalac と `toString` まで一致することを固定 |

#### 検証

- `tests/fixtures/sd_impl.scala` + `tests/fixtures/sd_use.scala` — scala-rs で
  2 段コンパイルして実行し、`tests/fixtures/expected/sd_use.txt` と一致する
  （`java -Xverify:all`）。**同じ 2 ファイルを実 scalac 2.13.16 でも 2 段
  コンパイルして実行し、同じ 6 行になることを別テストで固定**している。
  パラメータ名を取り違えた `Function`、修飾子を落とした `ValDef` は
  どちらもコンパイルは通ってしまうので、**出力の比較だけが捕まえられる**。
- `tests/fixtures/sd_gaps_bad.scala` — 断る 2 形。
- `tests/fixtures/tt_tags.scala` — タプル・関数型・配列のタグを追加。

#### 3. 引数を取らないマクロの結果を適用する形

`SdUse.adder(20, 22)` で `adder` が**引数を取らない**マクロのとき、
`Apply` はマクロ自身の引数節ではなく**展開結果への適用**である。
展開器は `Apply` を無条件に剥がしていたので
`the implementation takes 0 argument(s) and the call site supplies 2` という
——実 scalac が通す呼び出しに対する——誤った診断を出していた。

マクロ def 自身のパラメータ節の数（シンボルの `Type::Method` の `paramss`）を
数え、多い分は**中に入って**そこで展開する。層は素の `Apply` とは限らず、
関数値の適用は typer が挟む `apply` 選択を通るので、層数を数えて降りるのでは
なく「頭が当のマクロで、節の数がちょうど合う」ノードを探す
（`macro_application_node`）。外側の `Apply` はマクロ def のシンボルを
**持ったまま**なので落とす。残しておくと `report_macro_calls` が
「展開されていないマクロ」を——理由の文字列すら無い形で——報告する。

#### 4. `reify` に足りないもの（D-2 の調査結果）

段階 D-2（自前の `reify`）は**このスライスでは実装していない**。
設計は確定し、**組むべき木が実 scalac 2.13.16 で通ることまで確認した**が、
その手前に scala-rs 側の穴が 3 つ残っている。

`reify { … }` が展開されるべき形は（nsc の `-Xprint:typer` と同じ）:

```scala
{
  final class $treecreator1 extends scala.reflect.api.TreeCreator {
    def apply[U <: scala.reflect.api.Universe with Singleton](
        m: scala.reflect.api.Mirror[U]): U#Tree = {
      val u = m.universe
      u.internal.reificationSupport.SyntacticApplied(…)   // ← §7.1 の reifier
    }
  }
  c.universe.Expr.apply[T](
    c.universe.rootMirror.asInstanceOf[scala.reflect.api.Mirror[c.universe.type]],
    new $treecreator1())
}
```

**この形は実 scalac が受理する**（`u.internal.reificationSupport.Syntactic*` を
パス依存の `U` 越しに呼ぶところも含めて）。つまり
`crates/typer/src/reify.rs` の reifier に universe として
`m.universe` を渡せば、本体はそのまま流用できる——
`crates/typer/src/materialize.rs` の `TypeCreator` 合成が
`TreeCreator` 版のひな型になる。

scala-rs 側で塞がっていない穴は 3 つで、いずれも `reify` 以前の問題である:

| 穴 | 症状 |
| --- | --- |
| universe の**入れ子オブジェクト**がパスからも wildcard import からも引けない | `c.universe.Expr` は `value Expr is not a member of Universe`、`import c.universe._` 下の `Expr` は `not found: value Expr`。`Exprs.Expr` は trait の中の `object` で、`PickleSupply` が供給していない（§7.8 残件 5 と同じ穴） |
| `c.universe` が**安定識別子**として型に書けない | `Mirror[c.universe.type]` が `stable identifier required, but c.universe found`（§7.8 残件 6）。`c.universe` は `val` なので安定のはず。合成側は `RESOLVED_TYPE` で型を直接埋めれば避けられるが、穴自体は残る |
| reify 本体の**衛生性** | nsc の reify は*型付き*の木を作るので、`TableQuery` は `staticModule("slick.lifted.TableQuery")` に解決される。§7.1 の reifier は書かれた名前をそのまま `SyntacticTermIdent` にするので、展開先のスコープで解決される。静的シンボルは `_root_.` 付き完全パスに書き換え、それ以外（ローカル・パラメータ）は**名指しで断る**、というのが設計だが未実装 |

したがって `reify { … }` は §7.8 の診断のままである。

#### このスライスのあとに残っているもの

1. **展開の型引数が「前の run のクラス」でなければならない。**
   タグは `staticClass(<完全名>)` で組むので、engine の mirror が
   解決できるのは**マクロ classpath にあるクラスだけ**である。
   `TableQuery[Coffees]` のように *同じ run* で定義する行クラスは
   まだ渡せない（`sd_gaps_bad.scala` が固定）。nsc はコンパイラ自身の
   universe を使うのでこの制約が無い。**本物の slick の利用側**を
   通すにはここが要る。
2. **`reify`**（上の 4）、**`c.prefix` の `This`**（§7.12 残件 1）、
   **型付き木のまま渡す**（同 2）はそのまま。
3. **`TableQuery.apply[E](cons.splice)` のオーバーロード選択。**
   `TableQuery.apply` は「引数 1 つ」と「引数無し（マクロ）」の 2 つがあり、
   scala-rs は後者を選んでから結果に `(cons.splice)` を適用しようとして
   `value apply is not a member of TableQuery[E]` になる。nsc は前者を選ぶ。
   本物の `TableQuery.scala` を通すのに要る 3 件のうちの 1 つ
   （残り 2 件は `reify` と、マクロと無関係な
   `new BaseTag { base => … }` の自己名 `base` が引けないこと）。

#### slick への効き方

`tests/slick_measure.sh` は `errors=155 → 154`、`files_with_errors=52 → 52`。
`tests/slick_subset.sh` は `38 files / 204 classes / verified=204 failed=0` のまま。
減った 1 件は `TableQuery.scala` の `c.Expr[Tag => E]`（関数型のタグ）である。
残りは §7.12 と同じく、slick の 2 マクロが「def と実装が同じ run にある」形
だからで、段階 D-3 には `reify` が要る。

### 7.14 段階 D-2 の手前: 入れ子 `object` と `<val>.type`（`agent/reifyd` スライス）

§7.13.4 が名指しした 3 つの穴のうち、**1 と 2 を塞いだ**。どちらも `reify`
専用ではなく一般の機能追加で、マクロと無関係なコードにも効く。3（reify 本体の
衛生性）と `reify` の展開そのものは**このスライスでも未実装**であり、診断は
§7.8 のままである。

#### 1. trait の中の `object` が供給されていなかった（§7.8 残件 5）

`trait Exprs { object Expr { … } }` は、インタフェースメソッド
`Expr()Lscala/reflect/api/Exprs$Expr$;` と module 自身の classfile に落ちる。
`PickleSupply::complete_named` は pickle の `Def` と `Val` しか読まないので、
`MemberKind::Module` のエントリは**丸ごと捨てられて**いた。結果、

- `c.universe.Expr` → `value Expr is not a member of Universe`
- `import c.universe._` 下の `Expr` → `not found: value Expr`

という、どちらも**嘘**の診断になっていた（メンバは pickle にある）。

`PickleSupply::install_nested_module` を足した。module class を
`Outer$Name$` の JVM 名で入れ、**`class_sym`（探索を始めた受け手のクラス）**に
0 引数のアクセサを立てる。宣言元の trait に置く案は捨てた:
`Check::qualify_term_import` は「メンバの owner」を import 接頭辞のクラスと
突き合わせて `import u._` 下の裸の名前を `u.name` に戻すのだが、ライブラリ
クラスの pickle 親は 1 段ずつしか繋がらないので、linearisation の遠くにある
trait に置いたアクセサは「この import のもの」と認識されず、
`Main$.Expr()` を吐いて `ClassCastException` になった。`install` と同じ
規約（受け手のクラスに入れる）が正しい。

呼び出し先は `erased_desc` に決めさせる。`api/JavaUniverse` の classfile は
`interfaces: 0` なので、`invokevirtual JavaUniverse.Expr()` は解決しない
（`NoSuchMethodError`）。`declaring_class` / `declaring_is_interface` を
記録し、`checkcast` を挟んでそのクラスを名指しする — nsc と同じ形である。

classfile 由来の**壊れたアクセサは修理する**。`adopt_binary_class` が
`Exprs.class` を読むと descriptor から `def Expr(): Exprs$Expr$` を入れるが、
`Exprs$Expr$` のシンボルは誰も作っていないので戻り値は未解決の
`Type::Named` のまま、`class_sym_of` が `None` を返し
`c.universe.Expr.apply` は `value apply is not a member of Exprs$Expr$` に
なっていた。解決済みの戻り値は**触らない**（精度は足すが、メンバは奪わない）。

`materialize::ensure_tag_module` は「module class があること」を仕事済みの
印にしていたが、この供給が先に module class を作るようになったので、
印を **`apply` があること**に変えた。アクセサの二重登録も、同じ module class
を指すものが既にあれば足さない、という条件に変えてある。

#### 2. `c.universe` が安定識別子として型に書けなかった（§7.8 残件 6）

`Mirror[c.universe.type]` が `stable identifier required, but c.universe
found`。原因は `member_is_stable` ではなく **`Check::term_path_sym`** で、
`SymKind::Term | Module | ModuleClass` しか受けていなかった。pickle から
読んだ `val` は 0 引数の **`SymKind::Method`**（classfile は `val` の
アクセサと素の `def` を区別できない）に `Flags::ACCESSOR` を立てて入るので、
これが落ちていた。`c.universe.Tree` は `path_dependent_type` を通り
`member_is_stable`（こちらは `ACCESSOR` を見る）しか呼ばないので通っていた、
という食い違いである。

`Type::SingleType { sym }` の読み手 3 か所（`class_sym_of` /
`expand_in_type` / `erase_ty`）は `sym.ty` をそのまま見ていたので、
0 引数 `Method` を結果型に開く `SymbolTable::singleton_underlying` を通す。

#### 3. 道中で塞いだ 3 つの一般の穴（いずれも**黙って壊れる**形だった）

| 直したもの | どこ | 症状 |
| --- | --- | --- |
| **メソッドの引数がそのメソッドの「メンバ」に見えていた。** `install` は引数シンボルを method の owner 下に確保するので、`qual.sym` がメソッド（＝適用の被呼側）のとき `lookup_member(qual.sym, name)` がそれを拾う | `Check::type_select` の `qual.sym` フォールバック | `m.staticClass(n).fullName` が `staticClass` の**引数 `fullName`** に解決し、codegen が「所有者クラス＝メソッドの erased descriptor」で `Fieldref` を吐いた。`ClassFormatError: Illegal class name "(Ljava/lang/String;)L…;"` — **コンパイルは無言で成功する** |
| **括弧なし選択で `declaring_class` の `checkcast` が抜けていた。** `Apply` 経路は `checkcast_erased_method_receiver` で入れているのに、`Select` 単独の経路には無かった | `gen::gen_select` の `SymKind::Method` 枝 | `u.Expr` が `JavaUniverse` をスタックに積んだまま `invokevirtual Universe.Expr()`。`VerifyError` |
| **メンバ `object` の受け手が捨てられていた。** 修飾子が 0 引数アクセサ（型が `Type::Method`）だと `class_sym_of` が答えられず、また pickle 親が繋がっていないと `is_owner_compatible` も偽になるので、`load_module_instance` に落ちて**囲む source クラスの `this`** を積んでいた | `gen::gen_module_member_receiver` | `universe.Liftable[String](f)` が `aload_0` を積み `ClassCastException: Main$ cannot be cast to scala.reflect.api.Liftables`。**コンパイルは無言で成功する** |

`gen_receiver` の `TypeApply` / `Typed` も剥がすようにした（`o.P.apply[T](x)`
は関数が `TypeApply` に包まれていて、フォールバック枝が `fun.sym` しか見て
いなかった）。

#### 検証

- `tests/fixtures/rd_nested.scala` — 実行時 universe に対して、パス越しと
  wildcard import 越しの入れ子 `object`（`Expr` / `Liftable`）と
  `Mirror[scala.reflect.runtime.universe.type]` を使い、5 行印字する。
  **実 scalac 2.13.16 でも同じ 5 行**（`tests/fixtures/expected/rd_nested.txt`）。
  受け手を取り違えた member object はコンパイルが通ってしまうので、
  **走らせる以外に捕まえる方法が無い**。
- `tests/fixtures/rd_impl.scala` + `tests/fixtures/rd_use.scala` — **`reify`
  が展開されるべき形を手書きし、実際に展開して走らせる**。下の 4 を参照。
  `rd_impl` は `c.universe.Expr` をパス越しと wildcard import 越しに、
  `Mirror[c.universe.type]` を型引数に使い、`TreeCreator` を 3 つ組む。
  scala-rs で 2 段コンパイルして実行すると 3 行になり、**同じ 2 ファイルを
  実 scalac 2.13.16 で 2 段コンパイルして実行しても同じ 3 行**である
  （`tests/fixtures/expected/rd_use.txt`）。静的シンボルを別の universe で
  解決した creator も、splice を rebase し忘れた creator も**コンパイルは
  通る**ので、出力の比較だけが捕まえられる。

#### 4. `Exprs#Expr.apply` を手書きする

`reify` の展開は最後に `c.universe.Expr.apply[T](mirror, creator)` を呼ぶ。
`Expr` が引けるようになっても、この `apply` は**呼べなかった**:
pickle の署名は

```text
def apply[T](mirror1: Mirror[Universe.this.type], treec: TreeCreator)
            (implicit tag: WeakTypeTag[T]): Expr[T]
```

で、`Universe.this.type` は「完了中のクラス」に対して変換されるのだが、
それは module `Expr$` 自身なので第 1 引数が `Mirror[Expr$]` になり、
どの呼び出しとも合わない（`no matching overload for
(Mirror[Expr$], TreeCreator)(WeakTypeTag[T])Exprs$Expr[T]`）。
`materialize::ensure_tag_module` が `TypeTag.apply` を手書きしているのと
まったく同じ理由なので、同じ扱いにした
（`PickleSupply::install_expr_apply`、erased descriptor も書き下ろし）。
implicit 節はそのまま残してあるので、手書きの
`c.universe.Expr.apply[T](m, creator)` は `WeakTypeTag[T]` を
§7.10 の materialiser から受け取る。

これで **`reify` が組むべき木は、手書きなら丸ごと動く**:
`rd_use.scala` の 3 つのマクロは engine で本当に展開され、
`42 / 42 / true` を印字する。残っているのは
「`reify { … }` からこの木を**自動で組む**こと」だけである。

#### このスライスのあとに残っているもの

1. **`reify { … }` の展開そのもの**（§7.13.4 の穴 3）。木の材料は揃った
   ので、残るのは check.rs 側の合成と**衛生性**である。nsc の展開形
   （`-Xprint:typer` 実測）は

   ```scala
   { val $u: c.universe.type = c.universe
     val $m: $u.Mirror = c.universe.rootMirror
     $u.Expr.apply[T]($m, new $treecreator1())($u.TypeTag.apply[T]($m, new $typecreator2())) }
   ```

   で、creator の本体は `val $u = $m$untyped.universe` の下に §7.1 の
   reifier を置いたもの。衛生性は静的シンボルを
   `$u.internal.reificationSupport.mkIdent($m.staticModule("RdHelper"))` に、
   `splice` を `x.in[$u.type]($m).tree` に落とす——**どちらも
   `rd_impl.scala` で手書きして動くことを確認済み**。ローカルやパラメータは
   名指しで断る、というのが設計で、これが未実装。
   合成側は各識別子が静的シンボルかどうかを知る必要があるので、
   `Check::hole_lifts` と同じ「クローンを投機的に型付けして巻き戻す」形で
   本体を先に解決するのが素直である。
2. **trait の中の入れ子*クラス***（`u.Liftable[Int]` を**型**として書く形）は
   まだ `not found: type Liftable`。今回入れたのは term 側だけである。
3. **`u.Mirror` の上限が読めない。** `Mirrors#Mirror` は
   `type Mirror >: Null <: api.Mirror[self.type]` で、`conv_upper_bound` が
   この上限を落とすので `x.in[u.type](mm)` の `mm` は
   `u.Mirror` ではなく `scala.reflect.api.Mirror[u.type]` に cast して
   渡す必要がある（nsc は前者を書く）。`rd_impl.scala` のコメント参照。
4. §7.13 の残件 1・3（展開の型引数、`TableQuery.apply` のオーバーロード選択）は
   そのまま。

#### slick への効き方

`tests/slick_measure.sh` は `errors=134 → 134`、`files_with_errors=48 → 48`。
`tests/slick_subset.sh` は `38 files / 204 classes / verified=204 failed=0` の
まま。slick の 2 マクロは `reify` が要るところで止まっており、この 2 件は
その手前を通しただけなので数字は動かない。
