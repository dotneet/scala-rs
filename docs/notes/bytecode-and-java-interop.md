# Bytecode emission and Java interop

These notes collect three investigations into bugs that live below the typer, in the shape of the bytecode we emit and in how we read Java classfiles. They cover JVM verifier errors caused by a wrong operand-stack shape around `Unit`, nested Java interfaces together with interface `static` methods and the linearization rules that decide whether a Java subclass is concrete, and trait method access flags plus boxing at `extends` argument positions. What the three have in common is that the compiler produced a classfile with no diagnostic at all: the failure only appeared when the JVM verified or ran the code.

### Comparison operands of type `Unit`, and `scala.Enumeration` (`agent/uniteq`)

The bug: `() == ()` and other operations on `Unit` values compiled with no diagnostic but blew up in the JVM verifier, and members of `scala.Enumeration` such as `values` and `withName` were invisible. The root causes were that `Unit` boxing had been applied to value positions but not to comparison operands or to the receiver of a member selected on a `Unit` value, and that inherited-member supply from pickles never ran for a user class extending a library class.

Two independent issues. The fixture prefix is `ue`; the tests are in `crates/cli/tests/uniteq.rs`.

#### 1. `() == ()` gives `VerifyError: Operand stack underflow`

```scala
println(() == ())                            // VerifyError (no diagnostic)
val u1 = (); val u2 = (); println(u1 == u2)  // same
```

`agent/unitbox` put `scala/runtime/BoxedUnit` into the **value positions** of `Unit` --
parameters, fields, array elements, type arguments -- but **comparison operands**, and the
**receiver** of a member selected on a `Unit` value, were missed.

A `Unit` expression leaves nothing on the stack. For `() == ()`, erasure only applied `$box`
to the argument side, so exactly one `getstatic BoxedUnit.UNIT` was pushed while
`BoxesRunTime.equals(Object,Object)` pops two. The classfile was written out with no
diagnostic, and the failure appeared only when the JVM verified it.

```
java.lang.VerifyError: Operand stack underflow
  Location: Main$.main([Ljava/lang/String;)V @3: invokestatic
  Reason: Attempt to pop empty stack.
```

`().toString` / `().hashCode` / `().isInstanceOf[T]` / `().asInstanceOf[T]` had the same
shape: the invoke happened without the receiver ever being pushed.

The fixes are all in `crates/backend/src/gen.rs`, listed below. They simply ride on the
existing `adapt_unit_arg` (a `checkcast` if `unit_leaves_boxed_ref`, otherwise a
`getstatic BoxedUnit.UNIT`); no new machinery was added.

| Place | What was fixed |
|---|---|
| `gen_receiver` | Pass the receiver of an `Apply` through `adapt_unit_arg` |
| `gen_select_receiver` | Same for the receiver of an argument-less `Select` (`().toString`) |
| `gen_any_eq` / `gen_eq_ne` | Same for the right-hand operand (`x == ()`) |
| `asInstanceOf` / `isInstanceOf` under `TypeApply` | Push the receiver. `asInstanceOf[Unit]` `pop`s afterwards, so it balances out |
| `emit_any_hash` | Do **not** box `Unit` here. The receiver was already boxed above, so boxing again would push it twice |

`getClass` got fixed along the way. Argument-less `.getClass` was missing from the intrinsic
dispatch and fell through to a plain `Object.getClass`, so `1.getClass` returned
`class java.lang.Integer` instead of nsc's `int` (and `().getClass` would have been
`class scala.runtime.BoxedUnit` instead of `void`). The `Apply` side was already correct, so
both now go through `emit_get_class`.

scalac turns `() == ()` into `true` with a warning
(`comparing values of types Unit and Unit using == will always yield true`).
We emit no warning, but the value agrees.

#### 2. Members of `scala.Enumeration` are missing

```scala
object Color extends Enumeration {
  val Red, Green, Blue = Value
  val Custom = Value(10, "custom")   // no matching overload
}
Color.values                          // value values is not a member of Color$
Color.withName("Green")               // same
```

The cause was that **inherited-member supply was not taking effect**.
`PickleSupply::complete_named` will not read a pickle unless "the receiver's class is
`scala/...` (or something that `adopt_binary_class` took over)". `Color$` is a user class, so
`object Color extends Enumeration` received **nothing** beyond what the prelude had
hand-written (`Value` and `Value.id`).

We added a path to `PickleSupply::complete` that -- only when nothing else was found --
asks the **library-side ancestors** in turn (in linearization order, nearest first)
(`library_ancestors` in `crates/typer/src/pickle_supply.rs`). Members are installed on the
ancestor that declares them, so they also agree with the class the JVM call names. With
that, `values` / `withName` / `apply` / `maxId` and the whole `ValueSet` surface can be read
from the `ScalaSignature` of `scala/Enumeration.class`. Nothing was duplicated by hand.

The only thing added to the prelude is the three overloads of `Value`
(`crates/typer/src/prelude_enum.rs`). `Enumeration` has a **class `Value`** and **four
methods named `Value`** under the same name, and supply only runs when member lookup found
**nothing**, so if the inner class answers to the name, those four overloads are never asked
for. Deleting the prelude's argument-less `Value` does not help either: then
`Value(10, "custom")` resolves the bare name to the class and you get
`value apply is not a member of Value`.

The consecutive numbering in `val Red, Green, Blue = Value` is not compiler machinery. The
library's `Value()` reads and bumps `Enumeration.nextId` at run time, so the existing
handling of multiple assignment -- evaluating the right-hand side once per name -- is enough
to produce 0, 1, 2.

#### Verification

| fixture | What it pins down | Expected output |
| --- | --- | --- |
| `ue_eq.scala` (dual-run in both modes) | `Unit` operands: literal, local, a call returning `Unit`, a `Unit` parameter; `equals` / `hashCode` / `toString` / `isInstanceOf[Unit]` / `asInstanceOf[Unit]` / `getClass`; through `Any`; `Unit` against non-`Unit`; `id(())` erased through a type parameter; conditional and statement positions; `case () =>`; `equals` of a `case class`; a user-defined `equals` | `true` `false` ... `2` |
| `ue_eqlib.scala` (library dual-run only) | `##` (`scala.runtime.Statics`), `Unit` inside `List` / `Set` / `Map` / `Option`, `() -> 1`, a `(Unit, Unit) => Boolean` lambda, `count(_ == ())`. The private runtime has neither `Statics` nor a varargs `List.apply` nor `Set` / `Map` / `Function2`, so this is jar-only | `0` `0` `true` ... `List(())` |
| `ue_eq_bad.scala` (error case) | That boxing does not loosen the typer: `val s: String = ()`, `() eq ()`, and `().length` are all errors (real scalac reports the same 3) | (compile error) |
| `ue_enum.scala` (library dual-run only) | The consecutive numbering of `val Red, Green, Blue = Value`, `Value(i, name)` / `Value(i)` / `Value(name)`, `values` / `withName` / `apply` / `maxId`, `toList` / `filter` / `size` / `contains` on `ValueSet`, `type Weekday = Value`, the stable-identifier pattern `case Color.Red =>`, `Value` being `Ordered`, and the `NoSuchElementException` from `withName` | `(Red,0,10)` `List(Red, Green, Blue, custom)` `true` `Blue` `Color.ValueSet(Red, Green)` ... |
| `ue_enum_bad.scala` (error case) | `withName(1)` / `Value(1, 2)` / `Color.nosuchMember` / `val n: Int = Color.Red` are all errors (real scalac also reports 4; for `Value(1, 2)` it is a `protected` violation on their side and an overload mismatch on ours) | (compile error) |

Since the private runtime has no `scala/Enumeration`, `ue_enum` pins down that
`--no-scala-library` **does produce a diagnostic** (`ue_enum_private_runtime_is_diagnosed`).
The same goes for `ue_eqlib`.

We also inspect the bytecode itself (`ue_eq_pushes_both_operands`). With `javap -p -c` we
check that the two instructions immediately before `BoxesRunTime.equals` are both
`BoxedUnit.UNIT` -- just running the program is not enough, because the output before the
fix **also compiled fine**, and only the verifier noticed.

The slick measurement went from `files=184 errors=327 files_with_errors=64` to
`files=184 errors=322 files_with_errors=64`. The drop comes mainly from inherited-member
supply: `lazyZip` / `toMap` / `compare` now resolve (and now that `lazyZip` gets through,
the `LazyZip.map` behind it has become newly visible).

#### Remaining

- **An unknown parent class passes silently.** `object Bogus extends NoSuchThingHere`
  emits a classfile with no diagnostic **in both modes** (behavior that predates this fix
  and has nothing to do with `Unit` or `Enumeration`). Consequently
  `object Color extends Enumeration` itself is not an error even under
  `--no-scala-library`. `ue_enum` fails on the private runtime only because it uses
  `Value` inside.
- We do not distinguish `Color.Value` and `Weekday.Value` as **different types**. The
  prelude's `Value` is a single class with no prefix (no path dependence), so assignments
  that nsc would reject with `type mismatch` go through. `ue_enum_bad` avoids this shape.
- We do not emit nsc's warning for `Unit` comparisons
  (`comparing values of types Unit and Unit ...`).
- We read `Unit => Boolean` as `() => Boolean` rather than `Function1[Unit, Boolean]`
  (`missing parameter type for expanded function`). That is a separate parser-side issue;
  `ue_eqlib` avoids this shape.
- `##` unconditionally calls `scala.runtime.Statics.anyHash`, so on the private runtime it
  gives `NoClassDefFoundError` (not just for `Unit` -- `1.##` does the same). This is
  another pre-existing hole, and one of the reasons `ue_eqlib` is jar-only.

### Nested Java interfaces and interface statics (`agent/javanest`)

The bug: nested Java generic interfaces such as `java.util.Map.Entry` lost their type
parameters, extending Java collection classes was rejected as needing to be abstract, and
`static` methods on Java interfaces were emitted with the wrong constant-pool tag. The root
causes were a stub symbol that was never reloaded from its own classfile, a C3 merge that
emitted the same class more than once, an abstract-member check that ignored `Object` and
the superclass chain, and `invokestatic` referring to a `Methodref` instead of an
`InterfaceMethodref`.

Two Java-interop issues, plus two cascading issues that hung off the first one.

#### 1. A nested Java generic interface loses its type parameters

```scala
val e: java.util.Map.Entry[String, Int] = it.next()
// error: Entry does not take type parameters
```

`java.util.Map$Entry` is `interface Map<K,V> { interface Entry<K,V> {...} }`, so the
**nested side has its own type parameters** too. That is written in the `Signature`
attribute of `java/util/Map$Entry.class`, and the classfile reader read it correctly
(`crates/typer/src/javaclass.rs`).

The cause was upstream of that. The generic signature of `Map.entrySet()` **names**
`java/util/Map$Entry`, so merely reading `java/util/Map.class` puts `Entry` into the symbol
table as a **stub** with no parent and no type parameters. When the owner is a class,
`complete_binary_member` (`crates/typer/src/check.rs`) just returned as soon as a member was
found, and did not go read `java/util/Map$Entry.class` even when what it found was that
stub. The fix is to apply `ensure_java_loaded` when the member found is a class.

#### 1a. Linearization emitted the same class twice (SLS 5.1.2)

Even after the nested type was fixed, `class Cache extends java.util.LinkedHashMap[String, Int]`
still gave **`class Cache needs to be abstract.`**. The eight members defined by `HashMap`
and `AbstractMap` -- `size` / `isEmpty` / `containsKey` / `put` / `remove` / `putAll` /
`equals` / `hashCode` -- were reported as unimplemented.

Dumping the linearization showed `java/util/Map` appearing **three times**, and the first of
those occurrences came **before** `java/util/HashMap`. The abstract-member check only looks
at `lin[..bi]` -- "only a more derived base can implement it" -- so `HashMap.put` did not
count as an implementation of `Map.put`.

The C3 merge in `crates/typer/src/lin.rs` falls back to `lists[0][0]` when two parents
impose contradictory orders, and that is where it emitted the same class twice. It is
normal for a Java class to `implements` an interface that its own superclass already
implements (`class LinkedHashMap<K,V> extends HashMap<K,V> implements Map<K,V>`), and this
shape hits that fallback every time.

In SLS 5.1.2, `L(C) = C, L(Cn) +: ... +: L(C1)` defines `a +: b` as "drop from `a` anything
already in `b`", so **the later position wins**. We therefore now **keep only the last
occurrence** in the merge result. That is exactly `+:` itself, and it removes the
duplicates too.

```
Before: Cache, LinkedHashMap, Map, HashMap, Serializable, Cloneable, Map, AbstractMap, Map
After:  Cache, LinkedHashMap, HashMap, Serializable, Cloneable, AbstractMap, Map
```

#### 1b. What `Object` and the superclass chain implement (JLS 9.2)

Java interfaces **redeclare** `equals` / `hashCode` as deferred (`java.util.Map`,
`java.util.Map.Entry`, ...), and they redeclare superinterface methods too
(`java.util.List` redeclaring `java.util.Collection.containsAll`).

`full_lin` appends `Object` / `AnyRef` / `Any` at the **end of the sequence** so they are
hidden from the backend's mixin machinery, so they essentially never land in `lin[..bi]`.
Since `Object` really is the ultimate superclass of the class, its concrete members always
count as implementations (`trait T { def hashCode(): Int }; class D extends T` compiles
under scalac too).

For the same reason, a member declared deferred by a **Java interface** is implemented by a
concrete member of a **non-interface base** (i.e. the superclass chain) wherever it sits in
the linearization. Java has no `abstract override`, so an interface cannot cancel out an
implementation further down. Interface-to-interface is out of scope (redeclaring a
superinterface's default method as abstract really does leave it unimplemented).

#### 2. `static` methods on an interface were called via `Methodref`

```scala
val e = java.util.Map.entry("k", 7)
// IncompatibleClassChangeError: Method 'java.util.Map$Entry
//   java.util.Map.entry(...)' must be InterfaceMethodref constant
```

A silent miscompile: **it type-checks and fails at run time**. Per JVMS 4.4.2, methods
declared by an interface (including `static` ones) must be referred to in the constant pool
by `CONSTANT_InterfaceMethodref`. The `invokestatic` instruction itself is correct and
**only the constant's tag** differs, so a disassembly looks identical.

`Assembler::invokestatic_interface` already existed (for `scala/App.$init$`), so we made the
`Flags::STATIC` branch of `invoke_method` (`crates/backend/src/gen.rs`) use it "when the
owner is an interface". All the Java 9+ interface factories (`Map.entry` / `List.of` /
`Map.of` / `List.copyOf` / `Comparator.comparing` ...) go through this. `invokeinterface`
and `invokespecial` were already using `iface_ref`.

#### 3. A discarded erased result was being unboxed

Another silent miscompile, which surfaced once the LRU cache probe started getting through.

```scala
val m = new java.util.HashMap[String, Int]()
m.put("a", 1)   // NullPointerException (real scalac is fine)
```

`java.util.Map[String, Int].put` is `(Object, Object)Object` on the JVM, so the typer wraps
the result in `$unbox`. nsc inserts this adaptation from the **expected type**, so in
statement position (expected type `Unit`) it emits `invokevirtual put; pop` and never
touches the value. Since `put` returns the **previous value**, the first insertion unboxes
`null` and crashes. In `gen_stat`, a discarded `$unbox` now emits its operand directly as a
statement (`map.remove(k)` / `list.set(i, x)` / `buf.remove(0)` have the same shape).

#### Verification

The fixture prefix is `jn`; the tests are in `crates/cli/tests/javanest.rs`. The
success cases are run under `java -Xverify:all` **in both modes (private runtime and jar)**
and diffed against the output of real scalac 2.13.16.

| fixture / test | Contents |
|---|---|
| `jn_nested.scala` | `Map.Entry[K, V]`, the shape where Scala code `implements` it, the wildcard `Entry[_, _]`, and the depth-2 `AbstractMap.SimpleEntry` |
| `jn_static.scala` | `Map.entry` / `List.of` / `Map.of` / `List.copyOf` (interface statics), `Iterator.next` / `CharSequence.length` (default methods, i.e. `invokeinterface`), `Integer.valueOf` / `String.valueOf` (class statics stay `Methodref`) |
| `jn_lru.scala` | The whole probe: an LRU cache over `LinkedHashMap` plus extending `Thread` plus an anonymous `Comparator` plus `Arrays.sort` |
| `jn_nested_bad.scala` (error case) | An implementation of `Map.Entry` that omits `getValue` / `setValue`. Pins down `class Half needs to be abstract.` and that **only** `getValue` / `setValue` are listed (`equals` / `hashCode` / `getKey` are not). Real scalac 2.13.16 lists the same 2 |
| `jn_interface_static_constant_has_the_interface_tag` | Reads the classfile's constant pool directly and pins down that `Map.entry` / `List.of` have tag 11 and `Integer.valueOf` has tag 10. Merely running successfully can miss a wrong tag |
| `jn_extending_java_collections_is_concrete` | Extending `HashMap` / `ArrayList` / `LinkedHashMap` / `LinkedList` / `Thread` (all of which previously said "needs to be abstract") |
| `jn_object_members_implement_deferred_declarations` | That `Object` implements things like `trait T { def hashCode(): Int }` |
| `jn_nested_arity_is_still_checked` | `Map.Entry[String, Int, Long]` is still an error |
| `jn_discarded_erased_result_is_not_unboxed` | That calling `put` / `remove` / `set` in statement position does not crash |

#### Remaining

- `java.util.Arrays.toString(a: Array[Object])` gives
  `no matching overload ...with arguments (Array[AnyRef])`. We map the classfile's
  `[Ljava/lang/Object;` to `Array[Any]`, and since arrays are invariant in Scala,
  `Array[AnyRef]` does not conform. This is a pre-existing difference, independent of
  this fix.
- Calling an interface's generic static **with explicit type arguments** -- as in
  `java.util.function.Function.identity[String]()` or
  `java.util.Comparator.naturalOrder[String]()` -- gives `no matching overload`. That is a
  pre-existing hole on the type-argument application side, separate from the constant-pool
  tag.
- `java.util.Set.of("x")` gives `ambiguous overload` (choosing among the 10 `of` overloads
  including the varargs one). Another pre-existing overload-resolution issue.
- The relaxation of the abstract-member check is limited to "members declared by a Java
  interface". Things declared deferred by a Scala trait still consider only `lin[..bi]` as
  before (because `abstract override` is meaningful there).

### Private methods in traits, and `extends` arguments to a generic parent (`agent/traitpriv`)

The bug: `private` methods in traits were emitted with `ACC_PRIVATE | ACC_ABSTRACT`, which
the JVM rejects with `ClassFormatError`, and primitive constructor arguments passed to a
generic parent in an `extends` clause were not boxed, producing a `VerifyError`. The root
causes were that the `$class`-based trait encoding declared genuine `private` members on the
interface anyway, and that the `super_args` loops lacked the boxing check `gen_new` already
performed.

Two independent issues found by `tests/slick_subset.sh` (a measurement that compiles the
closure of real slick and does `Class.forName` on every emitted classfile with
`-Xverify:all` -- measuring not just "it type-checks" but "the JVM can actually load it"),
plus one more that `agent/javanest` had discovered and localized.

#### 1. `private` methods in traits were emitted as `ACC_PRIVATE | ACC_ABSTRACT`

```
BAD slick.util.ReadAheadIterator : java.lang.ClassFormatError: Method update
    in class slick/util/ReadAheadIterator has illegal modifiers: 0x402
```

`slick/util/ReadAheadIterator.scala` is a perfectly ordinary shape: a
`private[this] def update()` called from other trait members. JVMS 4.6 forbids specifying
`private` and `abstract` together on any method, and interface members are no exception.

We checked how real scalac emits this with a minimal reproduction (`javap -p -v`).

```scala
trait T { private def h = 1; def g = h + 1 }
```

nsc 2.13.16 compiles traits to Java 8 default methods, so `h` sits directly on the interface
as a **genuinely `private` method with a body**, and `g` (a default method written on the
interface itself) calls it with `invokespecial`.

```
private int h();
  flags: (0x0002) ACC_PRIVATE
public default int g();
  flags: (0x0001) ACC_PUBLIC
    0: aload_0
    1: invokespecial #20   // InterfaceMethod h:()I
```

This backend does not use default methods. It takes the old approach (the Scala 2.11 trait
encoding): concrete trait members always go into a helper class named `<Iface>$class` as
`static` methods, the interface declares only the abstract signatures, and forwarders are
grown on the classes that mix the trait in. With that encoding we cannot simply copy nsc's
shape (the body of `$class` is a **different class** from the interface, so it cannot call a
`private` member there directly). Instead we preserve the invariant that nsc's shape
maintains -- code that calls a `private` member is always inside the trait itself. A
**genuine `private` (one the typer did not `access_widened`) no longer appears on the
interface at all** (no abstract declaration and no forwarder); the actual body on `$class` is
made `private static`, and other members of the same `$class` call it with `invokestatic`
(not `invokeinterface`). `access_widened` (where the typer made a `private` member public so
that another class, such as the companion, can read it) stays on the usual
`public abstract` path as before. Its **name**, though, was later brought in line with nsc
by `agent/outer` (`Widened$$secret`; see the section "Four roots of touching the outer class
from an anonymous class").

`is_trait_private_def` in `crates/backend/src/gen.rs` holds the decision and is called from
four places: the loop that declares the interface's abstract methods (`emit_class`), the
access flags on the `$class` side (`emit_trait_impl_method`), the search for "the next
implementation" along the linearization (`next_lin_impl`), and the selection of mix-in
forwarders (`emit_mixin_forwarders`). Without fixing the latter two, the name of a `private`
member could contaminate forwarder selection for a same-named member of another trait, or we
would generate a forwarder to a `private` signature that does not exist.

#### 2. `extends` constructor arguments to a generic parent were not boxed

This is the one `agent/javanest` discovered while running, having also pinpointed the place
to fix (it is not yet written up in the javanest section above). It is fixed here.

```scala
class A1 extends java.util.concurrent.atomic.AtomicReference[Int](1)
// VerifyError: Type integer ... is not assignable to 'java/lang/Object'
```

In expression position, `new AtomicReference[Int](1)` was boxed correctly by `gen_new` (it
compares the post-erasure actual parameter type `Object` against the static type `Int` of
the value on the stack, and calls `emit_box` when it is a primitive). The superclass
constructor call generated by the `extends` clause did not have that same check. There are
two places: the `super_args` loop that builds a `class`'s `<init>`, and the `super_args`
loop that builds the `<init>` for `object ... extends ...(args)`
(`crates/backend/src/gen.rs`, both inside `emit_class` / the module `<init>` builder).

We made `parent_super_ctor` also return the constructor's **declared** parameter types (the
types from `ctor_sym` if there is a Java `<init>`, and otherwise the class's `ctor_fields`)
-- `ctor_param_tys`, factored out of the same computation in `gen_new` into a shared function
-- and put the same check `gen_new` uses
(`is_jvm_primitive(&a.ty) && !is_unit_like(&a.ty) && !is_jvm_primitive(pty)`) into both
`super_args` loops. Both a Java generic parent (`AtomicReference[Int]`) and a
hand-written Scala generic parent (`class Box[T](val v: T)`), both `class` and
`object ... extends`, and all 8 primitives are diffed against the output of real scalac.

#### Verification

The fixture prefix is `tp`; the tests are the new `crates/cli/tests/traitpriv.rs`. The
success cases are run under `java -Xverify:all` **in both modes (private runtime and jar)**
and diffed against the output of real scalac 2.13.16. The three fixtures for issue 1 also
read the classfile directly and pin down the methods' access flags (`javap` can print a
disassembly in which a `private abstract` still looks like an ordinary declaration, so
output comparison alone cannot detect a regression in the shape).

| fixture / test | Contents |
|---|---|
| `tp1.scala` | The exact shape of `ReadAheadIterator` (two `private[this] var`s plus a `private[this] def update()` called from two public members). `tp1_private_method_is_not_abstract_on_the_interface` pins down that `update` does not appear on the interface at all, and that `update` on `$class` is `private static` (neither `abstract` nor `public`) |
| `tp2.scala` | Name collision when two traits each have a `private` method of the same name. `tp2_private_method_gets_no_mixin_forwarder` pins down that the mix-in class has no member named `helper` at all |
| `tp3.scala` | Regression guard for the `access_widened` side: a `private def secret` read from the trait's companion is widened, and `tp3_widened_private_keeps_interface_signature` pins down that it stays `public abstract` on the interface (in contrast with the genuine private in `tp1`) |
| `tp4.scala` | `class ... extends java.util.concurrent.atomic.AtomicReference[Int](1)` (the reported reproduction itself) |
| `tp5.scala` | A hand-written Scala generic parent `Box[T]`, `object ... extends`, and all 8 primitives |

Verify-failure counts from `./tests/slick_subset.sh` (at start -> at completion):

```
at start:      subset_files=38 classes=204 (of 184 sources)  verified=203 failed=1
               BAD slick.util.ReadAheadIterator : ClassFormatError (illegal modifiers 0x402)
at completion: subset_files=38 classes=204 (of 184 sources)  verified=204 failed=0
```

`tests/slick_measure.sh` (the type-check error count) was unchanged at both start and
completion: `files=184 errors=257 files_with_errors=63 classes=0` -- both of these issues are
codegen bugs after type-checking succeeds, and the remaining errors across slick's 184 files
are unrelated pre-existing holes.

#### Remaining

- The 3 items listed under "Remaining" in `agent/javanest`'s README section (the
  `Array[AnyRef]` non-conformance for `Arrays.toString`, explicit type arguments to an
  interface's generic static, and the `Set.of` overload ambiguity) are out of scope for this
  fix.
- The very rare shape where the body of a `private` trait method itself contains `super.X()`
  (and `X` is not an override chain with the same name as the method itself) has not been
  checked for interactions with the existing `needs_super_accessor` heuristic (we did not
  find it in real code).
