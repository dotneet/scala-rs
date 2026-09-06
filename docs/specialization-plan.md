# `@specialized`: implementation plan

This note defines the smallest useful specialization slice for Scala 2.13.16.
It is based on the current compiler tree and on classfiles emitted by the real
2.13.16 compiler. It does not claim that accepting the annotation is
specialization: the current tree records the annotation and emits no
specialized ABI.

## Current boundary and evidence

Stage 1 is present in `crates/parser/src/specialization.rs` and
`crates/typer/src/symbol.rs`. `Symbol::specialized` holds the selected
`SpecializedTypes`, and `Symbol::unspecialized` records the member opt-out.
Nothing reads those fields during lowering or code generation. The pickle
writer intentionally drops both annotations in
`crates/backend/src/pickle.rs::pickle_symannot`: publishing an annotation that
claims a specialized member exists would make a separate scalac compilation
link to a member that scala-rs did not emit.

The existing ledger is the authoritative red check. On the baseline in
`tests/BASELINE.md` it reports:

```
tests=37 match=2 differ=26 no_compile=9
specialized classes ($sp): scalac=700 scala-rs=0
```

The two matching tests select no specialization. The ledger compares names
only; it does not verify, load, execute, or compare method descriptors.

The real compiler reports this phase order with `scalac -Xshow-phases`:

```
pickler 7
fields 11
tailcalls 12
specialize 13
explicitouter 14
erasure 15
```

The scala-rs driver currently has no specialize step. In
`crates/driver/src/lib.rs::compile_paths`, `uncurry` and the existing lowering
passes run at lines 315-326, `pickle_all` runs at line 353, generic signatures
are recorded at lines 367-373, and `erase` runs at lines 374-376. The new phase
must be inserted after the source pickle snapshot and before erasure. It must
retain the generic source declarations and add specialized metadata/trees;
replacing the generic declaration would break fallback calls and separate
compilation.

The erasure pass in `crates/typer/src/erasure.rs::erase` rewrites type
parameters to their JVM erasure and records abstract parameter masks used by
bridge generation. Once it has run, a source `T` no longer carries enough
information to decide whether a primitive variant is owed. Specialization
therefore has to substitute a selected primitive while the pre-erasure types
are still available. Existing substitution helpers such as
`symbol::subst_tparams_slice` can be reused; an `Any` substitution is not a
valid implementation of this phase.

## Three small fixtures and primary measurements

The measurements below used `/tmp/scala-2.13.16/bin/scalac`, the
`scala-library-2.13.16.jar` ABI, JDK 17, and both `JAVA_HOME` and `PATH`
pointing at that JDK. The fixture set was deliberately limited to three
shapes:

1. A class type parameter:

   ```scala
   class ClassBox[@specialized(Int, Long) A](val value: A) {
     def get: A = value
   }
   object ClassClient {
     def intBox = new ClassBox[Int](7)
     def longBox = new ClassBox[Long](7L)
     def stringBox = new ClassBox[String]("s")
   }
   ```

   scalac emits `ClassBox.class`, `ClassBox$mcI$sp.class`, and
   `ClassBox$mcJ$sp.class`. The generic `ClassBox<A>` keeps
   `value: Object`, `(Object)V`, `value()Object`, and `get()Object`. It also
   carries `value$mcI$sp()I`, `value$mcJ$sp()J`, `get$mcI$sp()I`,
   `get$mcJ$sp()J`, and `specInstance$()Z`; the generic implementation
   unboxes its `Object` field and returns `false` from `specInstance$`.

   `ClassBox$mcI$sp` extends `ClassBox<Object>`, has a primitive
   `value$mcI$sp:I` field and `(I)V` constructor, and implements `get()I`,
   `get$mcI$sp()I`, and `value()I`. It additionally has boxed
   `get()Object` and `value()Object` methods with
   `ACC_BRIDGE|ACC_SYNTHETIC`, and `specInstance$()Z` returns `true`. Its
   constructor stores the primitive and calls `ClassBox.<init>(Object)` with
   `null`; the specialized accessors are the source of truth for the value.
   The Long sibling is the same shape with `J` descriptors.

   `ClassClient$` calls `(I)V` and `(J)V` on the specialized constructors for
   the first two methods. The String method calls the generic
   `ClassBox.<init>(Object)V`. Thus the fallback is observable in bytecode,
   not just in a classfile name list.

2. A method type parameter:

   ```scala
   object MethodOps {
     def id[@specialized(Int, Long) A](x: A): A = x
     def twice[@specialized(Int) A](x: A): A = x
     def generic[A](x: A): A = x
   }
   class MethodHost {
     def id[@specialized(Int, Long) A](x: A): A = x
   }
   object MethodClient {
     def intId = MethodOps.id(7)
     def longId = MethodOps.id(7L)
     def stringId = MethodOps.id("s")
     def intTwice = MethodOps.twice(7)
     def stringGeneric = MethodOps.generic("s")
   }
   ```

   The generic methods retain descriptor `(Object)Object` and the generic
   `Signature` `<A:Ljava/lang/Object;>(TA;)TA;`. The module method variants are
   `id$mIc$sp(I)I`, `id$mJc$sp(J)J`, and `twice$mIc$sp(I)I`; `MethodHost` has the
   same `$mIc$sp`/`$mJc$sp` names. A method's own type parameter uses the
   `$m<primitive>c$sp` spelling. A member inherited from a specialized class
   type parameter uses `$mc<primitive>$sp`, as `ClassBox.get$mcI$sp` shows.

   `MethodClient$` invokes `id$mIc$sp(I)I` and `id$mJc$sp(J)J` for primitive
   calls, while `id(Object)Object` followed by a `checkcast` handles String.
   The unannotated `generic` method has no specialized sibling.

3. A specialized trait ABI:

   ```scala
   trait IntReadable[@specialized(Int) A] { def read: A }
   class IntBox(value: Int) extends IntReadable[Int] {
     def read: Int = value
   }
   ```

   scalac emits `IntReadable$mcI$sp.class` in addition to
   `IntReadable.class`. The marker interface extends
   `IntReadable<Object>`. The base interface declares `read()Object`, a
   synthetic static helper `read$mcI$sp$(IntReadable)I`, and a public default
   `read$mcI$sp()I`. `IntBox` implements the specialized marker interface and
   has `read()I`, `read$mcI$sp()I`, and a boxed
   `read()Object` bridge with `ACC_BRIDGE|ACC_SYNTHETIC`.

   A Java client that directly constructs `ClassBox$mcI$sp`, invokes
   `MethodOps$.MODULE$.id$mIc$sp(7)`, and calls
   `((IntReadable$mcI$sp) new IntBox(7)).read$mcI$sp()` compiles and runs under
   `java -Xverify:all`, printing `7:7:7`. Against the current scala-rs output,
   `javac` reports four missing symbols: the specialized class, the method
   variant, and the specialized trait interface. This is a binary ABI defect,
   not a performance-only difference.

The same three sources compiled by the current release binary produce 12
classfiles versus scalac's 15 and no `$sp` classfiles. `javap -p -s` confirms
that the generic fallback methods are present but every specialized method,
constructor, marker interface, and primitive bridge above is absent.

## Dependency map in the current tree

The first implementation has to cross these existing boundaries.

| Area | Current code | Required specialization work |
| --- | --- | --- |
| Annotation selection | `parser/src/specialization.rs`; `Symbol::specialized` and `record_specialization` in `typer/src/symbol.rs` | Reuse the selected `SpecializedType` and its tag. Do not re-parse annotation trees in the backend. |
| Phase placement | `driver/src/lib.rs::compile_paths` | Add a pre-erasure pass after `pickle_all`. Source pickles must describe only the generic declarations, as in scalac where specialize is after pickler. |
| Type substitution | `typer/src/symbol.rs::subst_tparams_slice`; `typer/src/erasure.rs` | Clone/retarget selected trees and symbols while their types are still typed. Specialized symbols must retain primitive types until descriptor emission. |
| Generic signatures | `backend/src/sig.rs`, called before `erase` | Keep generic signatures on the base class/method. Record the specialized class's `C<Object>` superclass signature separately; do not manufacture a generic `T` signature for primitive fields/methods. |
| Class emission | `backend/src/gen_class.rs::walk_stats`, `emit_class`, `emit_class_ctor`, `emit_def` | Emit sibling `$mcI$sp`/`$mcJ$sp` classes with primitive fields, constructors, methods, `specInstance$`, and boxed bridges. The base class remains emitted. |
| Method emission | `backend/src/gen_class.rs::emit_def`; `backend/src/gen_desc.rs::jvm_desc` | Emit `$mIc$sp`/`$mJc$sp` siblings in the same owner. Keep the generic `(Object)Object` entry point and select the primitive entry point at statically primitive call sites. |
| Call/new rewriting | typed `Apply`, `TypeApply`, and `New` trees before `erase` | Rewrite only when the selected type argument is a concrete supported primitive. Generic calls and type-variable calls must remain on the fallback entry point. |
| Bridges and traits | `backend/src/gen_trait.rs::emit_erasure_bridges`, trait default/mixin emitters | Add specialization bridges as explicit metadata. Existing erasure bridge matching must not guess a specialized method from two erased `Object` types. Emit the specialized trait marker/default/static helper ABI. |
| Pickle/classpath | `backend/src/pickle.rs::pickle_symannot`; `typer/src/pickle_supply.rs::despecialized` | Keep source annotations out of generated pickles. Preserve `despecialized()` for reading scalac variants: it maps a `$mc...$sp` parent back to the pickled generic parent. Generated variants need classfile `Signature`/marker attributes but no source ScalaSignature. |

`backend/src/gen.rs::Gen::extras` can carry extra emitted classes, but using it
alone is insufficient: the generated class must also participate in the JVM
name index, inner/outer metadata, owner/companion ordering, and classpath
metadata. A variant should therefore be represented by explicit specialization
metadata consumed by the class emitter, with `extras` used only after those
relationships are established.

## First executable slice

The first slice should support one source type parameter at a time and the
`Int` and `Long` selections used above. It should cover top-level/member
classes, object/class methods, and one specialized trait with a concrete
implementing class. It should implement `@unspecialized` as an opt-out for an
individual member. It should preserve all generic fallback declarations.

For each supported selected primitive:

1. Keep the generic class or method symbol and its erased descriptor.
2. Create a specialized sibling with the exact nsc name and primitive
   descriptor. Substitute the selected primitive through fields, parameter
   types, result types, and method bodies before erasure.
3. Rewrite `new C[Int](...)` and calls such as `f[Int](...)` only when the
   type argument is statically that primitive. A String or an unknown type
   must use the generic constructor/method and its boxing behavior.
4. For a specialized class, emit the primitive field/constructor and the
   primitive override/accessor methods, then emit boxed bridge methods for the
   generic JVM entry points. Set `specInstance$` to false on the base and true
   on each variant.
5. For a specialized trait, emit the marker interface and the base interface's
   specialized default/static helper, make the primitive implementation class
   implement the marker interface, and emit its primitive method plus the
   boxed bridge.
6. Keep generated variants out of `pickle_all`; source readers must still read
   the generic ScalaSignature. Classfile `Signature` and synthetic/bridge flags
   must match the measured scalac shape.

The first slice should diagnose unsupported shapes at the specialization phase
with a source location rather than silently treating them as generic. Its
initial boundary is: selected types other than `Int`/`Long`, more than one
specialized class type parameter, local/anonymous specialized classes, value
classes, higher-kinded or dependent bounds, and specialized overload/override
cases without an explicit bridge plan. These diagnostics are temporary phase
boundaries; they must not be counted as successful negative tests, and no
`Any` substitution or relaxed expected output is an acceptable fallback.

## Follow-up phases

After the first slice is stable, extend the same metadata and descriptor path
to the remaining primitive tags (`Byte`, `Short`, `Char`, `Float`, `Double`,
`Boolean`, `Unit`), `AnyRef`, primitive combinations across multiple type
parameters, nested/local classes, and the full specialized override and
overload rules. Add the remaining trait, value-class, bounds, and
`@unspecialized` interactions only after the single-parameter ABI is verified.
The full `tests/spec_classfiles.sh` ledger should remain explicitly red until
the class/method bodies and dispatch behavior are implemented, even if a
subset of its names starts matching.

## Acceptance tests for the first slice

The tests must verify behavior at source, classfile, runtime, and separate
compilation boundaries.

Positive checks:

- `ClassBox[Int]` and `ClassBox[Long]` construct the specialized siblings;
  `ClassBox[String]` constructs the generic class and returns the same result.
- `MethodOps.id` and `MethodHost.id` use the exact `$mIc$sp`/`$mJc$sp`
  descriptors for primitive calls, while String and an unconstrained type
  parameter use `(Object)Object`.
- The base class has the unbox helpers and `specInstance$ == false`; each
  primitive sibling has primitive fields/constructors, boxed bridges, and
  `specInstance$ == true`.
- A concrete `IntReadable[Int]` implementation links through
  `IntReadable$mcI$sp.read$mcI$sp()I` and through the boxed `read()Object`
  view.
- `@unspecialized` suppresses a member variant without suppressing variants
  required by the owning class.
- A Java client compiles against the emitted classfiles and runs with
  `-Xverify:all`; the result is `7:7:7`. A scalac consumer and a scala-rs
  consumer should each link against the other's provider output.

Negative checks:

- Unsupported selected types and unsupported class/method shapes produce the
  specialization diagnostic at the annotation/use site.
- A specialized override whose selected set is narrower than its parent is
  rejected for the specialization reason, matching scalac's
  `spec-overrides` category.
- The generic fallback remains available; a negative test must not pass merely
  because an annotation was ignored or because a call was widened to `Any`.

Required classfile checks are `javap -p -s`, method flags for
`ACC_BRIDGE|ACC_SYNTHETIC`, `javap -c` call-site owners/descriptors, and
`java -Xverify:all` execution. Name-only ledger success is not an acceptance
condition by itself. The first slice is complete only when the three fixture
families compile, execute, and pass both same-language and Java separate
compilation checks with the generic fallback still exercised.
