# JVM codegen: outer references, arrays, lambdas

Development notes for the slices whose bugs are invisible to the type checker:
reaching an enclosing class from an anonymous class, lambda, or local class;
emitting `Array` operations that survive `-Xverify:all` and class loading; and
the two test slices (`agent/lastone`, `agent/indy`) whose whole point is that
only *running* the program catches the difference. "It compiled" is not a
guarantee for any of this.

---

### Four roots for touching the enclosing class from an anonymous class (`agent/outer`)

Four errors left on main, all in the shape "touch something belonging to the
enclosing class from the body of an anonymous class, local class, or lambda":
two that fail under `java -Xverify:all`, one that gives an `IllegalAccessError`,
and one that quietly **called a different method**. Each was verified against
real scalac 2.13.16 (`/tmp/scala-2.13.16/bin/scalac`) and `javap -p -c` before
being fixed. Tests were appended to `crates/cli/tests/outer.rs`; the fixture is
`tests/fixtures/outer1.scala` (all cases in one file).

**1. Reading `$outer` inside `<init>` does not pass verification** (`VerifyError`).

```scala
class Outer(val n: Int) {
  def mk(): Base = new Base("tag" + n) { def describe = tag + "/" + n }
}
```

The anonymous class's **parent-constructor argument** reads the outer instance.
scala-rs placed the assignment to `$outer` **after** the super call and emitted
`aload_0; getfield $outer` inside the argument. JVMS §4.10.1.9 requires a
`getfield`'s operand to conform to `class(FieldClass)`, so a `getfield` on
`uninitializedThis` is rejected **regardless of whether the field has been
assigned** (only `putfield` is allowed, and only for a field declared by the
current class). Real scalac's `javap` looks like this:

```
public C$Outer$$anon$1(C$Outer);
   0: aload_1
   1: ifnonnull 6
   4: aconst_null
   5: athrow
   6: aload_0
   7: aload_1
   8: putfield  $outer            ← before the super call
  ...
  26: aload_1                     ← the argument is read from <init>'s parameter
  27: invokevirtual C$Outer.n:()I
  31: invokespecial C$Base."<init>"
```

So nsc does **both** of: (a) assign `$outer` before the super call, and (b) even
so, read from `<init>`'s parameter (local 1) rather than from `$outer` inside the
super argument. (b) is the mandatory part; (a) exists so that a method called
virtually back from the parent's `<init>` can see `$outer`. Both were adopted
(`EmitCtx::presuper_outer` and `start_outer_walk`; only the **first hop** of the
three places that walk the `$outer` chain — `load_owner_instance` /
`load_self_alias_instance` / `load_qualified_this` — is swapped out).

**2. A `private` member needs renaming the moment it crosses a classfile**
(`IllegalAccessError`). The brief's reading was right, except that **the root
was not "it is not renamed" but that we could not even tell it had crossed**.

Scala's `private` is **lexical**: anonymous classes, local classes, lambda
bodies and companions all live inside the owner's scope, so even
`private[this]` is nameable. The JVM's `ACC_PRIVATE` is per classfile, so all of
these are `IllegalAccessError` at runtime. scala-rs already had `access_widened`
(which drops `ACC_PRIVATE`), but it was raised in only **two places** in
check.rs — reads through a companion (`note_companion_access`) and the one path
that reads a `private[this]` unqualified (added by `agent/dbio`). Consequently
all of these slipped through:

| Shape | Result on main |
|---|---|
| `C1.this.a` (**qualified** `this`) | `IllegalAccessError` |
| `private val` / `private def` from an anonymous class | `IllegalAccessError` |
| a `private` member from a lambda body | `IllegalAccessError` |

The third is specific to scala-rs. nsc lowers lambdas to `invokedynamic` plus a
**static method in the same class** (`$anonfun$viaLambda$1`), so nothing crosses;
scala-rs lowers them to anonymous classes, so they do. Real scalac leaves `a` as
`private final int a` (per `javap -p`) for

```scala
class C { private[this] val a = 1; def viaLambda = List(0).map(_ => a).head }
```

And nsc does not merely publish a crossed member — it **renames** it
(`Symbol.makeNotPrivate` → `nme.expandedName`): the owner's full name with `$`
separators, plus `$$`, plus the name. Measured:

| What was written | The name scalac 2.13.16 emits |
|---|---|
| `object A { class Outer { private[this] val secret } }` | `public final int A$Outer$$secret` |
| `private val pUsed` | `private final int B$Outer$$pUsed` + `public int B$Outer$$pUsed()` |
| `private var w` | `private int H$C$$w` + `H$C$$w()` / `H$C$$w_$eq()` |
| `object O1 { private[this] val c }` | `public static final int D$O1$$c` |
| `trait T1 { private[this] val b }` | `public abstract int D$T1$$b()` |
| `package pkgj.sub; class R { private[this] val a }` | `public final int pkgj$sub$R$$a` |
| a `private[this] val ptUnused` **never read across a class** | `private final int ptUnused` (**no rename**) |

The rename is not cosmetic. `private[this]` is not inherited, so

```scala
class P { private[this] def y = 2; def mk() = new AnyRef { override def toString = "" + y }.toString }
class Q extends P { def y = 9 }          // legal
```

gives **`2`** for `new Q().mk()` under real scalac. But "publish without
renaming" makes `P.y` public so that `Q.y` **overrides** it, and scala-rs on main
printed **`9`** — a silent miscompilation that was happening on the very
`private[this]` where `access_widened` was already in effect.

So a new pass, `crates/typer/src/expand_private.rs`, was added at the same
position as nsc's `superaccessors` — **before the pickler**, which in scala-rs
means immediately after `mark_anon_captures`. It walks the unit carrying "the
class the code actually lands in", and when a reference to a `private` member
comes from a class other than its owner, it renames both the symbol name and the
name in the tree together and raises `access_widened` (`_$eq` is kept outside
the expansion, so `Outer$$w_$eq`). `private[pkg]` still carries `Flags::PRIVATE`,
so it is excluded via `private_within` (it is correct for it to be emitted
public, and renaming it would make it unreachable from other files). Because
this runs before the pickler, **the renamed name goes into the pickle too**, so
there is no disagreement with the classfile, exactly as in nsc. Separate
compilation (emitting `sep1.scala` and then compiling `sep2.scala` with `-cp`)
was confirmed to produce the same output as real scalac. scala-rs renames
**more widely** than nsc does, by exactly the amount that lambdas become classes,
but declaration and reference are renamed together so it is closed, and a
`private` member cannot be named from another file, so nothing leaks.

The existing `tp3` test (a companion reading a trait's `private def`) pinned
"stays public abstract under the name `secret`", but real scalac's `javap -p`
shows `public default int Widened$$secret()`. **The test's expectation was the
thing that differed from nsc**, so the name was aligned with real scalac and "the
source name is not emitted" was added.

**3. Assignment to an outer `var` did not walk the receiver** (`VerifyError`).

```scala
class C3 { private[this] var d = 4
  def mk(): Any = new AnyRef { override def toString = { d = d + 1; "" + d } } }
```

The reading side (`gen_ident`) walked `$outer`, but only the `Ident` arm of
`gen_assign` was still using `load_this`, pushing the anonymous class itself as
the `putfield` receiver (`Type 'D$C3$$anon$4' is not assignable to 'D$C3'`). It
was aligned with the reading side's `load_owner_instance` /
`load_self_alias_instance`.

**4. `$outer` for an anonymous class created inside a lambda** (`VerifyError`).

```scala
class C4 { private[this] val e = 5
  def mk(): Any = { val f = () => new AnyRef { override def toString = "" + e }; f() } }
```

`collect_free`, which decides whether the lambda class needs an `$outer`,
counted "the locals that class captures" in its `New` arm, but not "**that
class's `<init>` requires an outer instance**". The anonymous class's body is a
`ClassDef`, so this walk does not descend into it; the lambda looked as if it did
not use `this`, and `load_this` pushed `aload_0` (i.e. the lambda itself) with no
`$outer` present. The rule "if the target of the `New` has an
`outer_field_class`, the lambda needs an outer instance too" was added.

The measurements are identical before and after. `tests/slick_measure.sh` is
**`files=184 errors=65 files_with_errors=34 classes=0` → the same**, and since
codegen (`crates/backend/`) was touched, `tests/slick_subset.sh` with
`SLICK_SEED_LOG` is **`subset_files=38 classes=204 verified=204 failed=0` → the
same**. The baselines were re-measured in this worktree with a binary whose
`crates/*/src` was reverted to main, rather than trusting the README's numbers.
The type-checking numbers not moving is expected: `expand_private_names` only
runs when `has_errors` is false (`crates/driver/src/lib.rs`), and while
`classes=0` nothing reaches the backend.

**Remaining** (found in this shape, not fixed):

* nsc creates **no accessor** for a `private[this] val` (just the field).
  scala-rs still emits `Outer$$secret()`. Renaming means there is no collision,
  but it is a superfluous method.
* nsc renames a `private val` but keeps **the field private**, making only the
  accessor public. scala-rs makes the field public as well.
* nsc makes `object O { private[this] val c }` a `static final` field; scala-rs
  keeps it an instance field (the rename agrees).
* The direction where **scala-rs reads real scalac's classfiles via `-cp`** is
  broken independently of this shape. Even a class with no `private` members at
  all gives `VerifyError: Operand stack underflow`, so it is a separate,
  pre-existing issue (scala-rs-to-scala-rs separate compilation works).

---

### `Array` codegen — seven cases of "types fine, breaks at runtime" (`agent/arraygen`)

The three cases `agent/setmap` had been working around in
`tests/fixtures/setmap1.scala` were fixed, and eight ordinary programs using
`Array` were dual-run as probes. **Six of the eight differed on the very first
run**, yielding four more roots — seven in total. **Six of the seven type-check
completely**, which is the point: for `Array`, "it compiled" guarantees nothing.

The fixture is `tests/fixtures/arraygen1.scala` (all cases in one file); the
tests are in `crates/cli/tests/arraygen.rs`; the probes are the eight
`tests/conform/array_*.scala`. On main before the fix (`d7e7767`)
`arraygen1.scala` stops with **four errors**, and deleting those four lines to
get it through then fails in sequence with `VerifyError` → `ClassCastException`
→ `ClassFormatError`.

`Array` is the seam between erasure and the ABI itself.

**1. Explicit type arguments did not apply as-seen-from to a generic parent's
member.** `s.map[Int](_.length)` (with `s: immutable.HashSet[String]`) gave
`value length is not a member of A` plus `found: CC[Int] required: HashSet[Int]`,
while `s.map(_.length)` without the type argument worked. After narrowing the
overload set by type-argument count, `TypeApply` was building on
`SymbolTable::get(only).ty` — **the declared type verbatim**. `map` is declared
on `IterableOps[A, CC, C]`, so neither `A` nor `CC` has the receiver's arguments
substituted. Selection (`type_select`) has already done that work and stored it
in `overload_member_types`, so it now reads from there
(`Check::member_ty_as_seen_from`).

**`xs.toArray[R]` is not fixed** (see "Remaining"). `agent/setmap`'s README said
"this is **the same root** as the as-seen-from item", and the same reading came
through the coordinator from `agent/final1`, but **both are wrong**. The
prelude's `toArray` is declared **monomorphically** as
`(implicit ClassTag[A]): Array[A]` (`prelude_seq.rs`'s `add_conversions`,
`prelude.rs:3460`), not nsc's `toArray[B >: A: ClassTag]: Array[B]`. With zero
type parameters, an explicit type argument has **nowhere to be substituted**,
as-seen-from or not. Indeed, fixing `s.map[Int](f)` did not move `xs.toArray[R]`
one millimetre. Making it polymorphic then leaves `B` undetermined for
`List(1, 2, 3).toArray` (no expected type), so the implicit clause is not applied
and you get
`value mkString is not a member of (ClassTag[B])Array[B]`. **Inference that drops
a variable with only a lower bound to that bound at selection time** is needed
first (`instantiate_leftover_tparams` is only called from `Apply`, and it
descends because `sig_params` finds `ClassTag[B]`). What would have to change is
`maybe_auto_apply` / `adapt_implicit_apply`, and `agent/final1` was editing that
same place concurrently, so it was reverted for this slice.

**2. An earlier declaration in the same file breaks later code generation — what
carries over is `scala.Array$`'s overload set itself.** `scala.Array` declares
**ten** `apply`s. The prelude hand-writes exactly one,
`apply[T](xs: T*)(implicit ClassTag[T]): Array[T]`; the other nine (the
primitives and `Unit`, e.g. `apply(x: Int, xs: Int*): Array[Int]`) enter the
symbol table **only when `PickleSupply` is asked**. The trigger is an **explicit
type argument** `Array[T](…)`: `type_expr`'s `TypeApply` branch sees the
`Module[T]` shape and calls `supply_from_pickle_class(cls, "apply")`
unconditionally (`SCALA_RS_PICKLE_DEBUG=1` prints
`scala.Array#apply: supplied 9 overload(s)`).

In other words, **whether `Array[Any](1, "a")` appears anywhere in the file
changes which overload a later `Array(3, 1, 2)` resolves to**. That in itself
reaches the same conclusion as nsc (nsc also picks
`apply(x: Int, xs: Int*)`), but gen.rs was writing **the generic descriptor for
all ten** whenever `owner == "scala/Array$" && name == "apply"`:

```
invokevirtual scala/Array$.apply:(Lscala/collection/immutable/Seq;Lscala/reflect/ClassTag;)Ljava/lang/Object;
```

A call that chose `apply(x: Int, xs: Int*)` has pushed an `int` and a `Seq`, so
an `int` lands where the `Seq` is expected and you get a `VerifyError`. The nine
monomorphic ones have correct descriptors of their own
(`method_desc_boxed`), so it was enough to branch on the presence of type
parameters.

**This "order matters" property is not specific to `Array$`.**
`PickleSupply::complete` is lazy and additive by design, and the comment on
`own_decl_when_all_inherited` in `check.rs` records the same accident
(`TreeMap#collect` returned a `List` only in files where `Map#collect` had been
read first). The fix is not to change the order of supply but to make sure the
**descriptor is correct whichever overload is chosen**. The fixture places
`mixedFirst` before `inferredLater` precisely to trip this, so moving it makes
the test stop seeing the bug.

**3. `ClassTag`'s `classOf` fell to `java/lang/Object` for tuples.**
`Array[(Int, String)](1 -> "one")` emitted
`Array.apply(seq, ClassTag.apply(classOf[Object]))`, producing an `Object[]`, and
the caller's `checkcast [Lscala/Tuple2;` gave a `ClassCastException`.
`gen_java_class_of` special-cased `Type::Array` (for the same reason), but
`Type::Tuple` / `Type::Function` fell through to `_`. `Type::Annotated` /
`Type::Constant` are now stripped as well. A `ClassTag`'s runtime class is **the
element type of the array `Array.apply` will actually allocate**, so it must not
disagree with `jvm_desc`.

**4. `f(arr: _*)` passed the `Array` without wrapping it.** Varargs erase to
`scala/collection/immutable/Seq`, so `render(names: _*)` (with
`names: Array[String]`) pushed a `[Ljava/lang/String;` under that descriptor and
gave a `VerifyError`. gen.rs assumed "`f(xs: _*)` already has a sequence, so
there is nothing to wrap", and the exception **`Array` is not a sequence** was
missing. nsc's javap shows `Predef.copyArrayToImmutableIndexedSeq(names)`
(`genericWrapArray`'s `mutable.ArraySeq` is not an `immutable.Seq` and does not
reach it). **Java varargs are the one exception**, where the array itself is the
argument.

**5. Element assignment on an `Array[T]` produced an unloadable classfile.**
Doing `a(i) = x` inside `def repeat[T: ClassTag](x: T, n: Int)` emitted
`invokevirtual "[java/lang/Object".update:(ILjava/lang/Object;)V`.
`[java/lang/Object` is not an acceptable name to the JVM, so **the class cannot
even be loaded — `ClassFormatError`**. The cause is that `new Array[T](n)` is
rewritten to `ct.newArray(n)`, whose type is `Any`, so `qual.ty` is no longer a
`Type::Array` and gen.rs's array-access path is not entered. It now calls
`ScalaRunTime.array_apply` / `array_update` / `array_clone` as nsc does (the same
branch as `length` already lowering to `array_length`).
`def dup[T](a: Array[T]) = a.clone()` has **the same root**, and there it breaks
merely from receiving the array as a parameter. `--no-scala-library` has no
`ClassTag`, so that path is still a diagnostic as before
(`tests/fixtures/arraygen_gate.scala`).

The decision is made on **the receiver's type, not the element type**. An array
with an abstract element type no longer arrives as a `Type::Array` at this point
(`new Array[T](n)` becomes `ct.newArray(n)` with type `Any`, and an `a: Array[T]`
parameter collapses through erasure), and **that is precisely why the array path
was never invoked**.

**6. `ArrayOps`'s `$extension` methods were emitted with receiver-only
descriptors.** `a :+ x` emitted
`invokestatic scala/collection/ArrayOps.$colon$plus$extension:(Ljava/lang/Object;)Ljava/lang/Object;`
— **the receiver alone** — whereas the real one is
`$colon$plus$extension(Object, Object, ClassTag)Object`. Three things are pushed
(array, element, `ClassTag`), so the leftovers gave
`VerifyError: Inconsistent stackmap frames` at the first merge point. It was
correct for members taking only a receiver, such as `head` or `reverse`, so
**only members that take arguments were broken**. Signatures coming from the
pickle are exactly nsc's erasure, so the descriptor is now built from the symbol
(with the receiver alone hand-written — `ArrayOps`'s `Array[A]` collapses to
`Object`, not `[Ljava/lang/Object;`).

**7. No checkcast on an `Array` argument of a lambda.** `g.map(_.length)` (with
`g: Array[Array[Int]]`) gave
`VerifyError: Bad type on operand stack in arraylength`. A lambda's `apply`
receives its arguments as `Object`, so a cast is needed before `arraylength` /
`aaload` / `aastore`. The place that moves arguments into typed locals cast
`Type::Class` and `Type::Tuple` but was missing `Type::Array` (the captured-
variable side, `emit_from_erased_object`, had always handled it). If the element
type is abstract, the array itself has collapsed to `Object`, so no cast is
emitted there.

**Differential probes (eight `Array` programs)**

They were written as ordinary programs rather than feature checklists, with
`println` output, and dual-run. **Six of the eight failed on the first run**
(only `array_matrix` was rewritten, to trip the remaining item below).

| Probe | Shape | First result |
|---|---|---|
| `array_histogram` | count into a `new Array[Int]`, then `sortBy`/`take` | match |
| `array_matrix` | `Array[Array[Double]]` product, `ofDim`/`flatMap` | differs (`flatten`/`transpose`; see Remaining) |
| `array_varargs` | `Array[Item]` → `map` → `render(names: _*)` | differs (root 4) |
| `array_inplace_sort` | bubble sort via `update`, `fill`/`tabulate`/`clone` | differs (`clone` unimplemented) |
| `array_log_parse` | `split` → `flatMap` → `groupBy` → `toSeq` | differs (two items in Remaining) |
| `array_classtag_util` | `repeat`/`concat` with `[T: ClassTag]`, `Array.copy` | differs (root 5) |
| `array_inventory` | `indexWhere`/`updated`/`:+`/`zipWithIndex`/`partition` | differs (root 6) |
| `array_argv_match` | `case Array("add", a, b)` / `rest @ _*`, `grouped` | match |

**Remaining (with minimal reproductions; not fixed)**

* `flatten` / `transpose` on an `Array[Array[T]]`. Both require a **view
  implicit**, `A => IterableOnce[B]` / `A => Array[B]`. The search fails, the
  method type survives in the expression, and the diagnostic is
  `value mkString is not a member of ((Array[Int]) => IterableOnce[B], ClassTag[B])Array[B]`
  (**it does not silently succeed**). `array_wrap_view` is `Array[Int]`-only and
  hardcodes `wrapIntArray`, so it needs generalising via `array_wrap_candidates`
  and then solving `B` from the wrapped type.

  ```scala
  val grid: Array[Array[Int]] = Array(Array(1, 2, 3), Array(4, 5, 6))
  println(grid.flatten.mkString(""))
  println(grid.transpose.map(_.mkString("")).mkString("|"))
  ```

* Passing a **method reference** to `Array#flatMap` gives `ambiguous overload`.
  The lambda form (`xs.flatMap(s => parse(s))`) works, and `List#flatMap(parse)`
  works. There are two `ArrayOps.flatMap`s, and the prelude approximates the
  first one's argument as `A => Any`. nsc has `A => IterableOnce[B]`, which lets
  it say `Option[Int]` is more specific than `A => BS`; with `Any` it is a draw.
  Making the first one match nsc sends `arr.flatMap(x => Array(...))` to the
  second, which requires the view implicit (the item above), so the two have to
  be fixed together.

  ```scala
  def parse(s: String): Option[Int] = s.toIntOption
  Array("1", "x").flatMap(parse)   // ambiguous overload for flatMap
  ```

* `"a b c".split(" ", 2)`. The prelude's `String#split` takes one argument only
  (a hole on the `String` side, not `Array`).

* `xs.toArray[R]` (see root 1 above). Making the prelude's `toArray` polymorphic
  alone breaks `List(1,2,3).toArray`, so it has to go together with inference for
  variables that have only a lower bound. Both `agent/setmap` and `agent/final1`
  have tripped over this.

  ```scala
  def f[T: ClassTag](xs: Seq[T]): Array[Any] = xs.toArray[Any]
  // found: Array[T]  required: Array[Any]
  ```

For the record, the item relayed through the coordinator from `agent/final1` —
"`new Array[R](len)` (with `R` an abstract type parameter) inside a class method
writes `[java/lang/Object` into the constant pool and gives a
`ClassFormatError`" — has the same root as **5** above and is fixed here. The
workaround (`Array.tabulate[R]`) can be removed. Precisely, what breaks is not
the `new` but **whatever touches the array**: the three of `out(i) = …`, `out(i)`,
and `a.clone()` (merely creating and returning one, as in `c.blank[Int](3)`,
always worked). The fixture's `CArr#toArr` and `Main.dup` pin both.

```scala
class CArr[+T](val xs: Seq[T]) {
  def toArr[R >: T: ClassTag]: Array[R] = {
    val out = new Array[R](xs.length)
    var i = 0
    while (i < xs.length) { out(i) = xs(i); i += 1 }   // ← this is what gave ClassFormatError
    out
  }
}
```

**Measurements**

* `tests/slick_measure.sh`: `files=184 errors=17 files_with_errors=13`
  → **unchanged** (the error lines are identical, character for character). All
  seven of these are "type-checks but breaks at runtime", so a metric that counts
  type errors does not move. **Confirming that it does not move is the correct
  expectation.**
* `tests/slick_subset.sh` (once, with `SLICK_SEED_LOG`):
  `subset_files=47 classes=300 verified=300 failed=0` → unchanged.
* `tests/conform`: 77 → **85**.

---

### Tests for the `agent/lastone` slice

`crates/cli/tests/lastone.rs` (four tests). The fixtures are
`tests/fixtures/lastone.scala` (all cases in one file) and
`tests/fixtures/lastone_bad.scala`. They are not in `e2e.rs`, to avoid conflicts
with other agents.

`lastone.scala` reproduces the shape of `SQLiteProfile.scala:183` without using a
single line of slick: a **bounded abstract type member**
`type RowsPerStatement >: Rps.One.type <: Rps`, concretised both by a mixin that
**widens it to the upper bound** (`MultiSupport`) and by one that **narrows it to
the lower bound** (`SingleSupport`), with an inner trait calling
`super.insertAll(value = …, batch = …, rows = if (batch) Rps.One else rows)` with
named arguments. **On main before the fix it fails with one
`no matching overload for (U, Boolean, Comp.RowsPerStatement)String`.**
`fixtures_lastone_library_abi` / `fixtures_lastone_private_runtime` run it under
`java -Xverify:all` in both `--scala-library` and the private runtime (for the
narrowing side, the `$super$` accessor's descriptor differs from the parent's, so
the discrepancy only appears at **load and run** time even when type checking
passes), and `real_scalac_dual_run_lastone` checks that stdout matches real
scalac 2.13.16 to the character. `fixtures_lastone_bad_is_error` pins that making
the type member visible from `this` did not turn it into an "accepts anything":
it rejects **two** shapes — passing `Rps.All` under the narrow concretisation,
and doing the same where nothing is concretised (real scalac 2.13.16 emits the
same two lines,
`found: BadRps.All.type / required: … (which expands to) BadRps.One.type` and
`required: BadOpenProfile.this.Rows`). The same fixture also contains
`class Ops { val / = "div"; val + = "plus"; var % = "mod" }` and
`object Ops { val * = "times" }` (the exact shape of slick's
`ast/Library.scala`). **Type checking does not catch this** — a bare `/` is
writable as a field definition, and only when `java` loads the class do you get
`ClassFormatError: Illegal field name "/"`, so the three tests that actually run
under `-Xverify:all` are the only net.

---

### Tests for the `agent/indy` slice

`crates/cli/tests/indy.rs` (eight tests). The fixtures are
`tests/fixtures/indy1.scala` (only `Function0` / `Function1`, so it runs on the
private runtime too), `tests/fixtures/indy2.scala` (`Function2` / `Function3`,
`PartialFunction`, a user-defined SAM, by-name, non-local `return`, `Array`
arguments), and `tests/fixtures/indy1_bad.scala`. They are not in `e2e.rs`, to
avoid conflicts with other agents.

Two axes are checked: **behaviour** and **shape**.

* Behaviour: `indy1` is run under `java -Xverify:all` on both the private runtime
  and the real scala-library, and `indy2` is checked byte-for-byte against real
  scalac 2.13.16's stdout. `invokedynamic` **is not linked under
  `Class.forName(initialize=false)`**, so the verifier stays silent even when the
  bootstrap is broken. These two tests, which **actually run**, are the only net.
* Shape: `indy1` pins that despite having ten lambdas it produces **zero** closure
  classfiles, that `$anonfun$` methods sit on `Main$` and `Bump$class`, and that
  `javap -v` shows `BootstrapMethods` and
  `REF_invokeStatic java/lang/invoke/LambdaMetafactory.metafactory`; `indy2`
  conversely pins **exactly three** (two `PartialFunction` plus one SAM). That
  last one exists to pin down "the shapes not yet lowered to indy", so when you
  move the boundary, **move this number deliberately**.

`indy1_bad.scala` puts a two-argument literal where an `Int => Int` is wanted. It
checks that the typer stops with `type mismatch; found: (Int, Int) => Int` before
codegen can assemble a call site it cannot link.
