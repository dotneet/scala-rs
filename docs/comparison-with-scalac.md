## Comparison with scalac 2.13

scala-rs is a reimplementation of a subset of nsc, not a replacement for it.
This page lists where its output or behaviour deliberately or knowingly
differs from scalac 2.13.16. The gaps themselves are in
[not-implemented.md](not-implemented.md).

- **Scale**: a small part of nsc. It does not implement the whole language
  specification.
- **Library**: `compile` / `run` link against a scala-library 2.13.16 jar when
  one is found (`--scala-library [<jar>]`; without a path it searches
  `SCALA_LIBRARY_JAR`, `/tmp/scala-rs-lib` and the working directory) and
  then emit no class of the library's own names — `StringOps`, `ArrayOps`,
  `RichInt`, the collections and so on are the jar's. Calls use the jar's
  real descriptors: extension methods through `$extension` statics
  (`StringOps.take$extension`, `RichInt.max$extension`), `Predef$` members,
  `NumericRange` for `to` / `until` on `Long` / `Char` / `Byte` / `Short`,
  `UnapplySeqWrapper` for sequence patterns. `--no-scala-library` forces a
  small private runtime instead (see
  [language-support.md](language-support.md#library-modes)); its `Predef`
  helpers (`assert`, `require`, `???`, `->`, `identity`, `locally`,
  `implicitly`, string concatenation) are intrinsics, and many library
  features are unavailable there.
- **object**: as with scalac, a module `Main$` and a static forwarder `Main`
  are emitted, so `java Main` works.
- **Primitives**: arithmetic on `Int` and friends is emitted as JVM
  instructions (`iadd`, …), not as boxed calls.
- **Traits**: a trait is a JVM interface; concrete members are `default`
  methods with a `static m$` beside them, and implementing classes get
  forwarders in linearization order. A trait `val` becomes a getter, nsc's
  mixin setter and `$init$`; `abstract override` becomes `T$$super$m`.
- **Named arguments** are reordered at the call site, with no separate
  rewrite phase; the parser reads `x = e` uniformly as an assignment and the
  typer treats one in argument position as a named argument, as nsc does.
  Omitted defaults call the `{method}$default$N` getter; for constructors the
  default expression is typed at the call site.
- **try**: an exception table and a `StackMapTable`; `finally` is duplicated
  rather than using `jsr`.
- **Lambdas**: a `FunctionN` literal becomes `invokedynamic` +
  `LambdaMetafactory.metafactory` with a `public static $anonfun$N` body.
  nsc uses `altMetafactory` (serializable lambdas with
  `$deserializeLambda$`) and specialized interfaces such as
  `JFunction1$mcII$sp`; scala-rs does neither, and calls through
  `apply(Object)Object`. A `PartialFunction` literal and a SAM type other than
  `FunctionN` become synthetic classes, capturing locals into `$captured$n`
  fields and the enclosing instance into `$outer`, as nsc does for its
  anonymous classes.
- **Phases**: there are no separate mixin or explicitouter phases. After the
  typer come uncurry, local `lazy val` lowering, lambda-lift, capture
  analysis, the pickler, the method-specialization pass and erasure; tail
  calls are turned into loops by the backend
  ([architecture.md](architecture.md)).
- **Method types in the pickle** (`ScalaSignature`). nsc's pickler runs
  before uncurry, so a class file records the source's parameter clauses and
  keeps `def f: T` (a `NullaryMethodType`) apart from `def f(): T`. Our
  uncurry has already joined the clauses when the backend runs, so the shape
  is recorded on the symbol first (`Symbol::pickle_clauses`) and the pickler
  writes one `METHODtpe` per clause: `def makeDatabase[F[_]: Async]()`
  reaches scalac as `[F[_]]()(implicit ev: Async[F])`. A `name$default$N`
  getter with no clause before it stays nullary, as nsc's is. The getter of a
  default in a *later* clause is still pickled flat (see
  [not-implemented.md](not-implemented.md)).
- **sealed**: a non-exhaustive match is a warning, as in scalac, and an error
  under `-Xfatal-warnings`.
- **Case classes** get nsc's synthesized members (`copy` / `copy$default$N` /
  `productPrefix` / `productArity` / `productElement` / `productIterator` /
  `canEqual` / `productElementName` / `hashCode` / `toString` / `equals`, and
  the companion's `toString` / `apply` / `unapply`). Differences: no
  companion `writeReplace`; no static forwarders on the class for the
  companion's `apply` / `unapply` / `tupled` / `curried`; case accessor
  fields are `public final` where nsc's are `private final`, and our own
  synthesized members read them with `getfield` where nsc calls the accessor.
  `hashCode` under the library ABI is nsc's, chosen as
  `SyntheticMethods.chooseHashcode` does: `ScalaRunTime._hashCode(this)` when
  no case accessor has a primitive value type, otherwise the written-out
  `MurmurHash3` mix chain (`productSeed`, `Statics.mix` per field, a value
  class field hashed as an instance, `Statics.finalizeHash`). Under
  `--no-scala-library` it is a 31-fold instead — consistent with `equals`,
  but different numbers, because the private runtime has no
  `scala.runtime.Statics`.
- **AnyVal**: `new C(x)` erases to the underlying value, methods are called as
  `$extension` statics, and the value is boxed with `new C(u)` where a
  reference is needed. scalac puts the `$extension` bodies on the companion
  `C$` and a static forwarder on the class; scala-rs puts the body on the
  class's static and an instance forwarder on the companion. Both entry
  points exist on both sides, so either compiler's callers link
  (`crates/cli/tests/value_class_abi.rs`).
- **unapplySeq**: `List` / `Seq` / `Vector` / `IndexedSeq` / `Array` and user
  extractors, `_*`, and named case-class patterns. Under the library ABI
  `List.unapplySeq` returns `SeqOps` and everything else indexes through
  `UnapplySeqWrapper`'s `$extension` methods, as in nsc.
- **Access diagnostics** follow nsc's `ContextErrors.AccessError`: the
  subject is `underlyingSymbol(sym).fullLocationString`, a module class is
  spelled `object C`, the `from` clause names the enclosing class, and a
  constructor takes `in <owner>`. Two differences remain: the prefix type is
  not package-qualified (we print `object C` where nsc prints
  `object xflags.C`), which is `SymbolTable::display_type`'s doing and shows
  in every diagnostic; and nsc's indented "Access to protected … not
  permitted because …" explanation is not printed.
- **Constructor access**: `private` and `protected` constructors are
  checked at `new` (the prefix is the class being constructed; a
  parent-constructor call is not a `new`). nsc drops inaccessible
  alternatives before overload resolution; we pick first, so where another
  accessible constructor exists the check declines to report rather than
  refuse a program scalac accepts. Rewrites that build a `new` internally
  (`copy`) are not checked as constructor calls. The constructor's
  modifier is pickled, and `private` / `protected` / `private[p]` /
  `protected[p]` boundaries round-trip through class files in both
  directions — our reader honours scalac's `privateWithin`, and scalac honours
  ours (`crates/cli/tests/ctorgaps.rs`, `crates/cli/tests/intrinsicqual.rs`).
  The primary constructor is still emitted `ACC_PUBLIC`, as nsc emits it.
