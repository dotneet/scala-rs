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
