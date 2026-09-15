# Source macro types and binary call inference

The compatibility fixes are covered by small Scala 2.13.16 oracle cases:

- A typed `Any => ...` argument contributes an upper constraint, not a final
  solution, while another lambda still has an untyped result. Binary
  `map[R](read: U => R, write: R => Option[U])` calls now infer the reader's
  result instead of prematurely fixing `R` to `Any`.
- An inherited `FunctionN.apply` does not establish that a binary receiver's own
  `apply` has been loaded. Complete that receiver before selecting parameter
  names, types and defaults. The Slick 3.4.1 regression exercises
  `O.Length(32, varying = true)` and an optional mapped projection at runtime.
- Macro type descriptors carry source symbol IDs and their type arguments in
  both directions. The mirror describes class type parameters with their real
  bounds and variance, and uses unprefixed references for those parameters so
  inherited types substitute correctly. Weak method type parameters retain
  their identity instead of being erased or requiring a runtime class tag.
- Binary symbolic type aliases decode their names and recover concrete
  nullary members such as `Self`. Chaining Slick HList values now preserves
  the head and tail types across inherited `::` calls.
- A companion already entered as a binary placeholder still needs its
  implicit declarations. Failed searches also revisit the wanted type's
  scope after loading its base classes. Slick HList projections find the
  base companion's `Shape` witnesses without an explicit companion import.
- An inherited stable accessor replaces its erased classfile forwarder when
  its Scala signature is read. A reexported object no longer becomes an
  overload of two representations of the same singleton.
- Missing implicit arguments can use a binary default getter with the call's
  receiver and preceding arguments. A supplied implicit still wins, and an
  ambiguous search still reports an error rather than taking the default.
- Top-level binary module singleton tags keep their identity in both
  directions. Macro trees can carry `SingletonTypeTree`, and fully qualified
  singleton paths load the selected module before checking stability,
  including paths starting with `_root_`. Slick HList indexing exercises the
  generated casts at runtime.
- Extractor patterns complete binary companions before reading `unapply`, so
  generic extracted values retain their types. The HList `mapTo` macro now
  builds and runs a case-class projection.
- An intersection-valued implicit candidate infers against the requested
  base type, while final conformance still checks the full result. This lets
  Cats infer `E = Throwable` for Future's `attemptTap` from the
  `MonadError[Future, Throwable] with CoflatMap[Future]` instance.

`aliaslookup`, `slickshape` and `gbmapto` compare accepted and rejected programs
with scalac. The source-type macro test checks nested applications, member types
at two instantiations, bounds, covariance, inheritance, typed expansion results,
and an invalid narrowing. Its runtime comparisons use `java -Xverify:all` and
cover both scala-rs stages as well as a scalac-built macro implementation used
by scala-rs. Existing `gbmac`, `macrotransportbatch`, `macrotag` and `macromirror`
fixtures exercise the surrounding protocol.

This does not establish complete macro compatibility. Source and instance
singleton tag transport, higher-kinded source parameter info, and polymorphic
source method reflection still have separate restrictions. Reading a scala-rs-built macro implementation
from scalac still rejects its context-dependent `WeakTypeTag` evidence metadata;
that failure also reproduces at `d3318f35` and remains after integrating
`55d7600f`. The `Mirror[universe.type]` bound failure at `d3318f35` is resolved
by the integrated upstream singleton fixes.
