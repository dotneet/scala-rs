# マクロ engine の実現性 probe

`docs/macros.md` §2.3 の実験そのもの。**製品コードではない**。CI では走らない。

## 何を確かめるものか

「def マクロの実装を JVM 上で本当に実行する」という設計（`docs/macros.md` §2.2）が
成立するかどうか、ただ 1 点を確かめる。

- `Context` は `java.lang.reflect.Proxy` で作る。`blackbox.Context` の抽象メンバは
  72 個しかなく、すべて `scala.reflect.api.*` を受け渡す普通のインタフェースメソッドである。
  トレイトの default 実装（`weakTypeOf` など）は JDK 16+ の
  `InvocationHandler.invokeDefault` で本物に委譲する。
- `c.universe` には **`scala.reflect.runtime.universe`** を差す。これが
  `scala.reflect.macros.Universe` に適合するのは型レベルで保証されている
  （`scala.reflect.internal.SymbolTable extends scala.reflect.macros.Universe`）。
- 実装していない `Context` メンバは `UnsupportedOperationException` で**明示的に落とす**。
  黙って null を返したりはしない。

## 結果

`M.scala` / `M2.scala` を scalac でコンパイルし、それぞれの実装を probe から呼ぶ。

| 実装 | 使うもの | 返ってきた Tree |
| --- | --- | --- |
| `M.impl` | `Literal(Constant(42))` | `Literal(Constant(42))` |
| `M2.reifyImpl` | `reify`（slick `TableQueryMacroImpl` の形） | `Apply(Select(Ident(Helper), TermName("hello")), List(Literal(Constant(7))))` |
| `M2.qqImpl` | **quasiquote** `q"${x.tree} + 1"`（slick `mapToImpl` の形） | `Apply(Select(Literal(Constant(41)), TermName("$plus")), List(Literal(Constant(1))))` |
| `M2.tagImpl` | `WeakTypeTag` | `Literal(Constant("String"))` |

つまり **reify も quasiquote も、コンパイル済みでありさえすれば実行時ユニバース上で
そのまま動く**。

**ただし**「ソースから quasiquote / reify を脱糖する」のは別問題である。これらは
scala-reflect.jar に実装 classfile を持たない fast track マクロなので、
JVM ブリッジでは展開できない。詳細は `docs/macros.md` §6.2。

## 走らせ方

scalac 2.13 と scala-reflect.jar が要る（例: `/tmp/scala-2.13.16`）。

```bash
SCALA=/tmp/scala-2.13.16
CP=$SCALA/lib/scala-reflect.jar:$SCALA/lib/scala-library.jar
mkdir -p /tmp/proto/out /tmp/proto/jout

$SCALA/bin/scalac -d /tmp/proto/out M.scala M2.scala
javac -cp "$CP" -d /tmp/proto/jout Proto.java

# reify が mirror.staticModule でシンボルを引くので、
# マクロのクラスパスは -cp にも載せる必要がある。
java -cp /tmp/proto/jout:/tmp/proto/out:"$CP" Proto /tmp/proto/out 'M$'  impl
java -cp /tmp/proto/jout:/tmp/proto/out:"$CP" Proto /tmp/proto/out 'M2$' reifyImpl
java -cp /tmp/proto/jout:/tmp/proto/out:"$CP" Proto /tmp/proto/out 'M2$' qqImpl
java -cp /tmp/proto/jout:/tmp/proto/out:"$CP" Proto /tmp/proto/out 'M2$' tagImpl
```

## 製品版に足りないもの

`docs/macros.md` フェーズ 2 で `crates/macro-engine/` として作り直す際の差分:

- 引数 Tree をハードコード（`Literal(Constant(41))`、`WeakTypeTag[String]`）している。
  実際には Rust 側から直列化して受け取る。
- 戻りの Tree を `showRaw` で印字しているだけ。実際には Type も含めて JSON で返し、
  Rust の AST に戻す（`docs/macros.md` §4）。
- `Proxy` ではなく実クラスにする（起動ごとの反射コストと、実装漏れの静的検出のため）。
- `c.prefix` / `c.enclosingPosition` / `c.abort` / `c.freshName` が未実装。
  slick の `mapToImpl` はこれらを使う。
