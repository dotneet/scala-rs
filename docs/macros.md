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

**A. 展開先を宣言したクラスで呼ぶこと。** `PickleSupply` は補完したメンバを
*レシーバ*のクラスに載せる。ところが `u.Constant()` の実体は
`scala.reflect.api.Constants` インタフェースの宣言であり、`api.JavaUniverse` は
バイトコード上そのインタフェースを実装していない（7.2 の 2 と同じ理由）。
nsc は `checkcast scala/reflect/api/Constants` を挟んでから
`invokeinterface scala/reflect/api/Constants.Constant()` を出す。
我々は `invokeinterface scala/reflect/api/JavaUniverse.Constant()` を出すので
実行時に `NoSuchMethodError` になる。
**必要な作業**: `MemberHit.owner`（宣言クラス）をシンボルに記録し、codegen が
レシーバの静的型がそれを実装していないときに `checkcast` を挿む。
これが済めば `scala.reflect.runtime.universe` 上の Tree 構築が**実行できる**ようになり、
quasiquote の reification をエンドツーエンドで dual-run 検証できるようになる。

**B. reification 本体。** 7.1 が作った構文木を
`internal.reificationSupport.Syntactic*` 呼び出しに落とす（§6.2 の 2）。
穴の rank（`$` / `..$` / `...$`）と、穴の型（Tree / Name / Type / リスト）による
分岐がここに入る。§3.3 の一覧が slick に必要な最小セットである。

**C. `c.Expr[T]` などのパス依存型。** `blackbox.Context` の
`type Expr[T] = universe.Expr[T]`（`scala.reflect.macros.Aliases`）が解決できないと
マクロ実装のシグネチャ自体が型検査できない。現状 `crates/typer/src/prelude_reflect.rs`
は**空の `Context`** を入れており、classpath 上の本物より優先されてしまう。
7.2 の 1〜3 が入った今、**空の prelude をやめて scala-reflect.jar の本物を読む**方が
筋が良い（prelude を外して試すと `Expr` が `Exprs$Expr$` として解決するところまでは
確認済み）。ただし `--scala-library` だけでは scala-reflect.jar は classpath に無いので、
無いときは診断を出して落とすこと。

**D. engine（フェーズ 2）。** A〜C が済んでも、slick の `mapToImpl` を*呼ぶ*には
§2.3 の JVM ブリッジが要る。こちらは prototype で検証済みで、順序としては最後でよい。
