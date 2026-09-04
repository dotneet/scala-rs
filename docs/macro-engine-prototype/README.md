# Macro engine feasibility probe

The experiment described in `docs/macros.md` §2.3, verbatim. **Not production
code.** It does not run in CI.

## What it establishes

One thing only: whether the design in `docs/macros.md` §2.2 — running a def
macro's implementation for real, on the JVM — actually holds together.

- `Context` is built with `java.lang.reflect.Proxy`. `blackbox.Context` has only
  72 abstract members, and every one of them is an ordinary interface method
  passing `scala.reflect.api.*` values around. Default implementations inherited
  from traits (`weakTypeOf` and friends) are delegated to the real ones through
  `InvocationHandler.invokeDefault` (JDK 16+).
- `c.universe` points at **`scala.reflect.runtime.universe`**. That this
  conforms to `scala.reflect.macros.Universe` is guaranteed at the type level
  (`scala.reflect.internal.SymbolTable extends scala.reflect.macros.Universe`).
- Unimplemented `Context` members **fail loudly** with
  `UnsupportedOperationException`. They never quietly return null.

## Result

`M.scala` and `M2.scala` are compiled with scalac, and the probe calls each
implementation.

| Implementation | Uses | Tree returned |
| --- | --- | --- |
| `M.impl` | `Literal(Constant(42))` | `Literal(Constant(42))` |
| `M2.reifyImpl` | `reify` (the shape of slick's `TableQueryMacroImpl`) | `Apply(Select(Ident(Helper), TermName("hello")), List(Literal(Constant(7))))` |
| `M2.qqImpl` | **quasiquote** `q"${x.tree} + 1"` (the shape of slick's `mapToImpl`) | `Apply(Select(Literal(Constant(41)), TermName("$plus")), List(Literal(Constant(1))))` |
| `M2.tagImpl` | `WeakTypeTag` | `Literal(Constant("String"))` |

So **both reify and quasiquotes run as they are on the runtime universe**, as
long as they have already been compiled.

**However**, desugaring a quasiquote or a `reify` *from source* is a separate
problem. Those are fast-track macros with no implementation classfile in
scala-reflect.jar, so the JVM bridge cannot expand them. See `docs/macros.md`
§6.2.

## Running it

Needs scalac 2.13 and scala-reflect.jar (for example under
`/tmp/scala-2.13.16`).

```bash
SCALA=/tmp/scala-2.13.16
CP=$SCALA/lib/scala-reflect.jar:$SCALA/lib/scala-library.jar
mkdir -p /tmp/proto/out /tmp/proto/jout

$SCALA/bin/scalac -d /tmp/proto/out M.scala M2.scala
javac -cp "$CP" -d /tmp/proto/jout Proto.java

# reify looks symbols up through mirror.staticModule, so the macro's own
# classpath has to be on -cp as well.
java -cp /tmp/proto/jout:/tmp/proto/out:"$CP" Proto /tmp/proto/out 'M$'  impl
java -cp /tmp/proto/jout:/tmp/proto/out:"$CP" Proto /tmp/proto/out 'M2$' reifyImpl
java -cp /tmp/proto/jout:/tmp/proto/out:"$CP" Proto /tmp/proto/out 'M2$' qqImpl
java -cp /tmp/proto/jout:/tmp/proto/out:"$CP" Proto /tmp/proto/out 'M2$' tagImpl
```

## What a production version still needs

The gap to close when this is rebuilt as `crates/macro-engine/` in phase 2 of
`docs/macros.md`:

- Argument trees are hard-coded (`Literal(Constant(41))`,
  `WeakTypeTag[String]`). They should arrive serialised from the Rust side.
- The returned tree is only printed with `showRaw`. It should come back as JSON,
  types included, and be turned back into Rust's AST (`docs/macros.md` §4).
- Use real classes rather than `Proxy`, both for the per-start reflection cost
  and so that missing implementations are caught statically.
- `c.prefix`, `c.enclosingPosition`, `c.abort` and `c.freshName` are not
  implemented. slick's `mapToImpl` uses all of them.
