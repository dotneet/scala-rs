# `@specialized`: implementation plan

This note defines the smallest useful Scala 2.13.16 specialization slice. It
is based on the current compiler tree and on classfiles emitted by the real
2.13.16 compiler with JDK 17. Accepting and retaining an annotation is not
specialization: a useful slice must emit the primitive method ABI, select it at
typed call sites, and remain consumable by scalac.

## Current boundary and evidence

The parser and typer already record `@specialized` selections in
`crates/parser/src/specialization.rs` and `crates/typer/src/symbol.rs`.
`Symbol::specialized` holds the selected `SpecializedTypes`, and
`Symbol::unspecialized` records a member opt-out. Before the method slice,
nothing used those fields during lowering or code generation. The baseline
ledger in `tests/BASELINE.md` remains the reference red check; it reports:

```
tests=37 match=2 differ=26 no_compile=9
specialized classes ($sp): scalac=700 scala-rs=0
```

That ledger compares names only. It does not verify descriptors, bytecode
owners, execution, or separate compilation, so a matching name cannot be the
acceptance condition for this work.

The real compiler reports this order with `scalac -Xshow-phases`:

```
pickler 7
fields 11
tailcalls 12
specialize 13
explicitouter 14
erasure 15
```

The specialization pass therefore works while typed method bodies still carry
their type parameters and before JVM erasure. The driver runs the method slice
after `pickle_all` and before `erase`: the generic source declarations are
pickled first, then primitive method symbols and trees are appended for class
emission. The generic declaration remains available for fallback and separate
compilation.

The erasure pass in `crates/typer/src/erasure.rs::erase` rewrites type
parameters to JVM erasures and records masks used by bridge generation. After
that point a source `A` no longer carries enough information to decide whether
an `Int` or `Long` entry is required. Substitution must happen before erasure;
substituting `Any` or widening the expected result is not an implementation of
specialization.

## Three small fixtures and primary measurements

The measurements used `/tmp/scala-2.13.16/bin/scalac`, the 2.13.16
`scala-library.jar`, JDK 17, and both `JAVA_HOME` and `PATH` pointing at that
JDK. The fixture set was limited to three shapes so that the ABI boundary is
observable without running the full workspace or corpus.

1. A method-owned type parameter:

   ```scala
   object MethodOps {
     def id[@specialized(Int, Long) A](x: A): A = x
     def twice[@specialized(Int) A](x: A): A = x
     def generic[A](x: A): A = x
   }
   object MethodClient {
     def intId: Int = MethodOps.id(7)
     def longId: Long = MethodOps.id(7L)
     def stringId: String = MethodOps.id("s")
     def intTwice: Int = MethodOps.twice(7)
     def stringGeneric: String = MethodOps.generic("s")
   }
   ```

   scalac emits the generic `(Object)Object` entries and the exact primitive
   siblings `id$mIc$sp(I)I`, `id$mJc$sp(J)J`, and `twice$mIc$sp(I)I`.
   `MethodClient$` invokes the primitive siblings for `Int` and `Long`, while
   the String calls use the generic method and a cast. The unannotated
   `generic` method has no sibling.

2. A method on a non-final class:

   ```scala
   class MethodHost {
     def id[@specialized(Int, Long) A](x: A): A = x
   }
   ```

   This is a dispatch boundary, not a first-slice positive case. A method that
   can be overridden needs coordinated generic and primitive entries,
   override propagation, and bridges. The first implementation consequently
   specializes module methods and methods that are explicitly `final` or
   `private`; an ordinary class method remains generic until override dispatch
   is implemented and tested.

3. Class and trait observations for the next phase:

   ```scala
   class ClassBox[@specialized(Int, Long) A](val value: A) {
     def get: A = value
   }
   trait IntReadable[@specialized(Int) A] { def read: A }
   ```

   scalac emits `ClassBox$mcI$sp` and `ClassBox$mcJ$sp` siblings, primitive
   fields and constructors, boxed bridges, and `specInstance$`. For the
   trait it emits `IntReadable$mcI$sp`, specialized default/static helpers,
   and implementation bridges. These are real ABI requirements, but class and
   trait specialization are deliberately next phases rather than hidden
   promises of the method slice.

Before this slice, the scala-rs output for the first and third shapes had no
`$sp` variants. The method slice closes the method-owned gap; the class and
trait gaps remain expected red checks until their own ABI work lands.

## Dependency map in the current tree

| Area | Current code | Required method-slice work |
| --- | --- | --- |
| Annotation selection | `parser/src/specialization.rs`; `Symbol::specialized` and `record_specialization` in `typer/src/symbol.rs` | Reuse the selected primitive tag. Do not re-parse annotation trees in the backend. |
| Phase placement | `driver/src/lib.rs::compile_paths` | Run after `pickle_all` and before erasure. Keep the generic source declaration and append variants for code generation. |
| Type substitution | `typer/src/symbol.rs::subst_tparams_slice`; typed tree types | Clone a method body with the selected `Int` or `Long` type, preserving primitive parameter and result types until descriptor emission. |
| Method symbols | `typer/src/symbol.rs` | Track original-to-variant ownership, exact `$mIc$sp`/`$mJc$sp` names, selected type, and JVM method type. |
| Method emission | `backend/src/gen_class.rs::emit_def`; `backend/src/gen_desc.rs::jvm_desc` | Emit the primitive sibling in the same owner while retaining the generic `(Object)Object` entry. |
| Call rewriting | typed `Apply` and `TypeApply` trees before `erase` | Select a variant only for a statically supported primitive argument. Preserve generic calls, boxing, and generic fallback for String or unknown types. |
| Source pickle | `backend/src/pickle.rs::pickle_typesym` and symbol annotations | Preserve method type-parameter `@specialized` metadata and the nsc `SPECIALIZED` flag. Synthetic variants are post-pickle implementation entries, not source declarations. |
| Generic signatures | `backend/src/sig.rs` | Keep the generic method signature on the base entry. Primitive siblings have primitive descriptors and no fabricated generic type-parameter signature. |
| Dispatch boundary | method flags and owner kind | Start with module, `final`, and `private` methods. Do not rewrite an override-capable method until both entries and override dispatch are specified. |
| Class and trait ABI | `backend/src/gen_class.rs`, `backend/src/gen_trait.rs` | Follow-up work for `$mcI$sp` classes, marker interfaces, default/static helpers, and bridges. It is outside this slice. |
| Pickle reader | `typer/src/pickle_supply.rs::despecialized` | Retain the existing reader mapping for scalac-produced `$mc...$sp` parents; generated method variants must not masquerade as source declarations. |

`backend/src/gen.rs::Gen::extras` alone is insufficient for future class
variants: generated classes must participate in the JVM name index,
owner/companion ordering, and classpath metadata. That is why method variants
are explicit symbols in their existing owner, while class and trait variants
need a later explicit metadata design.

## Source pickle and consumer metadata

Scala source pickle and generated JVM entries are separate contracts. The
generic method declaration must remain in the source `ScalaSignature`, while a
scalac consumer needs the method type parameter to carry both its
`@specialized(Int, Long)` symbol annotation and the nsc `SPECIALIZED` symbol
flag. The annotation records the allowed selections; the flag tells the
consumer that a specialized entry is available. Synthetic `$mIc$sp` and
`$mJc$sp` symbols are emitted after the source pickle and are not serialized
as additional source declarations.

This distinction was checked with the method fixture. Against scalac output,
`javap -c` on a separate scalac consumer shows:

```
Consumer$.i: bipush 7; invokevirtual MethodOps$.id$mIc$sp:(I)I
Consumer$.j: ldc2_w 7; invokevirtual MethodOps$.id$mJc$sp:(J)J
Consumer$.s: invokevirtual MethodOps$.id:(Object)Object; checkcast String
```

Against scala-rs output, the same scalac consumer selects the same primitive
owners and descriptors after the method type parameter is pickled with the
annotation and `SPECIALIZED` flag. A controlled provider with methods named
`id` and `id$mIc$sp` but without this metadata caused scalac to box the call
and invoke generic `id(Object)Object`; matching the spelling alone is not an
ABI implementation.

The current class and trait source annotations remain outside this protection
until their variants and dispatch ABI exist. Dropping those annotations is
safer than publishing metadata that promises absent entries. It must not be
confused with dropping method-owned specialization metadata: the method slice
preserves that metadata so a consumer can select the entries that are actually
emitted.

## First executable slice

The first slice is method-owned specialization for one type parameter and the
`Int` and `Long` selections. It is intentionally limited to module methods and
methods marked `final` or `private`; these boundaries avoid claiming a correct
override ABI. It must support `@unspecialized` as a member opt-out and retain
all generic fallback declarations.

For each supported selection:

1. Keep the original generic method symbol, descriptor, body, and source
   pickle entry.
2. Create a sibling in the same owner with the exact nsc name
   `$mIc$sp` for `Int` or `$mJc$sp` for `Long`. Substitute the selected
   primitive through parameter types, result types, and the method body before
   erasure.
3. Rewrite a typed `f[Int](...)`, `f[Long](...)`, or inferred primitive call
   only when its selected type is statically supported. String, type-variable,
   and unsupported selections continue through the generic entry and its
   boxing behavior.
4. Preserve method type-parameter `@specialized` metadata and the nsc
   `SPECIALIZED` flag in the generic source pickle. Do not add synthetic
   variants to the source pickle.
5. Keep a method that can be overridden on the generic path until its
   specialized override and bridge rules are implemented. A shape being out
   of this slice is not a reason to reject otherwise legal source or to
   pretend that generic fallback is specialization.

The implementation therefore does not introduce a blanket unsupported
diagnostic. Unsupported selected primitive tags, class/trait type parameters,
and override-capable methods retain existing generic behavior. This is a
temporary capability boundary, with explicit positive and negative tests, and
not a completion claim for those shapes.

## Follow-up phases

The next phase should add class-owned type parameters and the
`$mcI$sp`/`$mcJ$sp` class ABI: primitive fields and constructors,
`specInstance$`, boxed bridges, and constructor/callsite rewriting. The phase
after that should add specialized trait marker interfaces, default/static
helpers, implementation bridges, and override dispatch. Both phases need
source-pickle and separate-compilation checks like the method slice.

Only after those boundaries are verified should the compiler extend the same
metadata and descriptor path to the remaining primitive tags (`Byte`, `Short`,
`Char`, `Float`, `Double`, `Boolean`, `Unit`), `AnyRef`, multiple type
parameters, nested/local classes, value classes, bounds, and the complete
`@unspecialized` interaction. The full `tests/spec_classfiles.sh` ledger can
remain red for class and trait cases while the method slice is accepted.

## Acceptance tests for the first slice

The first slice is complete only when the method fixture compiles, executes,
and passes same-language and Java/scalac separate-compilation checks while the
generic fallback is still exercised.

Positive checks:

- A module method with `@specialized(Int, Long)` emits the generic entry plus
  exact `$mIc$sp(I)I` and `$mJc$sp(J)J` entries. An `Int` or `Long` call uses
  the primitive entry; a String call uses `(Object)Object` and a cast.
- A method with `@specialized(Int)` emits only `$mIc$sp(I)I`. An unannotated
  generic method emits no sibling.
- The same method body returns the correct primitive values at runtime; the
  variant uses primitive load/return bytecode rather than an `Any` or boxed
  implementation.
- `@unspecialized` suppresses that member's variant while leaving its generic
  method callable.
- A `final` or `private` method follows the same ABI. An ordinary
  override-capable class method remains generic until the override phase; a
  regression test must cover this boundary.
- A scalac consumer compiled against scala-rs output selects
  `$mIc$sp`/`$mJc$sp` for primitive calls and generic `id(Object)Object` for
  String. A scala-rs consumer compiled against scalac output makes the same
  selections.
- A Java client can call the exact generated descriptors and the runtime
  fixture passes `java -Xverify:all`.

Negative and preservation checks:

- `-no-specialization` emits no method variants and preserves generic calls.
- Unsupported selections and class/trait-owned parameters are not claimed as
  implemented: they remain on the existing generic path with no fabricated
  specialized metadata.
- An override-capable method is not rewritten to a sibling that its subclass
  cannot implement. Once override specialization is added, tests must require
  identical behavior through both generic and primitive dispatch entries.
- A controlled provider with a same-named method but missing specialization
  metadata must stay a negative ABI case; name matching alone must not make a
  scalac consumer select a primitive entry.

Required evidence is `javap -p -s`, `javap -c` callsite owner/descriptors,
method flags where applicable, source-pickle inspection, and
`java -Xverify:all` execution. Name-only ledger success is insufficient.
