# Code generation, erasure, and StackMapTable frames

Six slices from the scala-rs development log. What they have in common is that
the typer was happy: every bug here produced a classfile that either failed
verification at class-load time, threw at run time, or silently did nothing at
all. None of them emitted a diagnostic, so the only way to notice was to run the
program and diff the output against real scalac 2.13.16.

### Declarations inside a method body (local trait / class / object, `agent/localtrait`)

`trait` / `class` / `object` can also be written **inside a method body (or a block, an `if`
branch, a lambda)**. Two mechanisms that worked correctly for top-level declarations were
missing here entirely. Both are the kind of gap where **the code typechecks but fails at run
time, or silently becomes wrong code**.

**1. Concrete-member collection only walked inside templates.**
`collect_trait_impls` only traversed the direct children of `PackageDef` / `ClassDef` /
`ModuleDef`, so a `trait` inside a method body was never registered, and **not a single**
trait body or mixin forwarder **was emitted**.

```scala
def main(a: Array[String]): Unit = {
  trait L { val v: String; lazy val w = v + "!"; def plain = v + "?" }
  class LC extends L { val v = "x" }
  println(new LC().plain)   // AbstractMethodError
}
```

Under `javap -p`, `Main$LC` had only `v()` (both `plain()` and `w()` were still the
interface's abstract declarations). That is why plain `def`s were dropped too, not just
`lazy val`s. I switched collection over to the generic child-node walk
(`for_each_term_child`) so a declaration is picked up wherever it sits. Since it now rides
the same path as top level, linearization, `super` accessors, `abstract override`, mixin
setters for trait `val`s, and `lazy val` duplication all keep working as they are.

**2. Local declarations carried no index.** A local name is unique only within a single
method. nsc numbers them `Main$Same$1` / `Main$Same$2`, but we were emitting a classfile
called `Main$Same` for both, so **the one emitted later silently overwrote the earlier
one** (`dupA()` printed `dupB`). `jvm_for_current` now looks at whether reaching the class
crossed a term (a method, or a `val` initializer), and appends `$N` only when it did. The
companion of a `case class` reuses the index the class drew (drawing a separate one makes
`Main$P$1` and `Main$P$2$` disagree).

**When a local trait captures an enclosing local.** A trait has no constructor, so captured
values cannot be turned into constructor arguments the way a local `class` does.
nsc creates a trait accessor per capture (`outerVal$1()`) and has the implementation class carry it.
We do the same thing:

- `anon_capture` **propagates a trait's captures to every class that mixes it in**
  (riding the existing "a local class's captures become constructor arguments plus fields" machinery),
- the interface declares an abstract accessor per capture,
- the implementation class implements that accessor from its own capture field,
- and a trait's `default` method bodies and `$init$` `invokeinterface` through the receiver at entry
  and drop the result into an ordinary local slot (`emit_trait_capture_prologue`).

Accessor names are built from the captured symbol's ID (`n$4492`). Numbering by position
would collide when two traits that capture different locals of the same name are mixed
into one class. Captured `var`s ride the existing `scala.runtime.*Ref` boxing.

**A local class implementing a *top-level* trait already worked**, and `lt1.scala` keeps it
from regressing. The reverse (a top-level class implementing a local trait) cannot be
written at all, since a local trait is not visible outside its scope (though **we currently
still fail to reject `Main.Local`** — see "local declarations are visible outside their
scope" under Remaining. That is a pre-existing gap on the name-resolution side, separate
from local-declaration indices).

| fixture | what it pins down | expected output |
| --- | --- | --- |
| `lt1.scala` (`crates/cli/tests/localtrait.rs`, private runtime and library dual-run) | `val` / `lazy val` / `def` on a local trait, calls through the interface, a local class implementing a top-level trait, `new T {}` and `new C with T`, declarations inside a block, in an `if` branch, in a lambda body, in a `match` case, in a `while` body, and in a `try` block | `x?` `x!` `F` `x?` `x!` `top:lc` `q` `q` `blockT` `ifU` `lam3` `mm` `w0` `w1` `y` |
| `lt2.scala` (same as above) | stacking and linearization of local traits (`B with C` / `C with B`), `abstract override`, a local trait inheriting a local trait, `override` and `super`, a local trait inheriting a top-level trait, a local trait taking type parameters, self types | `C(B(A))` `B(C(A))` `mid(late)` `ab` `a` `Over.m/T.m` `T.label` `top/L` `top` `box:7` `7` `hi g` |
| `lt3.scala` (same as above) | captures by a local trait: `val` / method parameter / `var`, capture on the right-hand side of a trait `val`, captures from an inherited trait | `cap42s` `cap42s` `p7` `1` `2` `13` `base!/base` `base!` `hio` |
| `lt4.scala` (same as above) | two methods declaring a `trait` / `class` / `object` with the same name, an `if` branch shadowing the name of an enclosing local class | `Aaoa` `Bbob` `P1` `Q2` |
| `lt1_bad.scala` (same as above, rejecting case) | the same checks as at top level apply to a local mixin (`illegal inheritance; superclass Other is not a subclass of the superclass Sup`). Real scalac reports the same single error | (1 compile error) |

There are `javap` comparison tests as well (`local_trait_gets_mixin_forwarders_and_impl_class`
/ `same_named_local_declarations_get_separate_classfiles`
/ `local_trait_captures_go_through_an_accessor`
/ `implementing_class_members_match_scalac`). **Missing or extra methods slip past run output
alone** (stdout still matches when a forwarder nobody calls is missing), so the last of these
runs real scalac when `/tmp/scala-2.13.16/bin/scalac` is present and checks that the
implementation class's set of public methods **contains** nsc's. Before comparing, it
normalizes the two spots where our notation differs from nsc's only in spelling (dropping the
local index, `Main$L$1$_setter_$fixed_$eq` = `Main$L$_setter_$fixed_$eq`,
and dropping the owner encoding on `super` accessors, `B$$super$name` = `Main$B$$super$name`).

The slick measurement is `files=184 errors=411 files_with_errors=72`, **the same before and after**
(these are bugs that typecheck, so the error count never moved to begin with).

### `Unit` arguments and `scala.runtime.BoxedUnit` (`agent/unitbox`)

`Unit` becomes `V` **only as a method's return type**. In the places where a value is
actually stored — **parameters, fields, array elements, type arguments** — nsc erases it
to `scala/runtime/BoxedUnit`, and the single value `()` is the `BoxedUnit.UNIT`
singleton. Writing `V` there is not just "different from nsc", it is
**invalid as a descriptor**, and the whole class fails to load.

```
java.lang.ClassFormatError: Method "f" in class Main has illegal signature
  "(V)Ljava/lang/String;"
```

`def f(x: Unit)`, `class C(val u: Unit)`, `var w: Unit`,
`case class K(k: Unit, …)` and `Array[Unit]` were all failing this way.

**What `javap -v -p` showed about real scalac 2.13.16** (confirmed by compiling `Main.scala`
as-is):

- `def f(x: Unit): String` is `(Lscala/runtime/BoxedUnit;)Ljava/lang/String;`.
- `f(())` pushes `getstatic scala/runtime/BoxedUnit.UNIT`.
- `f(g())` (with `def g(): Unit`) emits `invokevirtual g:()V` and **then**
  `getstatic UNIT`. A `V` call leaves nothing behind, so the argument is built right here.
- `val u: Unit = ()` **takes a slot** (the `LocalVariableTable` has
  `u Lscala/runtime/BoxedUnit;`).
- The constructor of `class C(val u: Unit)` is `(Lscala/runtime/BoxedUnit;)V`.
  For `var w: Unit` the field is `Lscala/runtime/BoxedUnit;`, the getter is
  `getfield; pop; return` (returning `V`), and the setter is `w_$eq(BoxedUnit)`.
- For `case class K(k: Unit)`, `apply(BoxedUnit)` / `copy(BoxedUnit)` /
  `unapply` returning `Option<BoxedUnit>`.
- `List((), ())` builds an `anewarray scala/runtime/BoxedUnit` and passes it to
  `ScalaRunTime.wrapUnitArray`. `Array[Unit]` is
  `[Lscala/runtime/BoxedUnit;` (`Array[Nothing]` is the one exception, being
  `[Ljava/lang/Object;`). A `Nothing` parameter is `Lscala/runtime/Nothing$;`.
- `val any: Any = ()` is `getstatic UNIT; astore`. The reason `println` prints `()` is
  `BoxedUnit.toString`.
- A `Unit` expression normally leaves nothing on the stack, but using `def id[A](a: A): A`
  as `id(())` makes the call `(Object)Object`, which **does leave a reference**, so where
  the result is discarded nsc emits a `pop` as well (`invokevirtual id; pop`).
- `x.asInstanceOf[Unit]` is an expression of type `Unit`, so it leaves nothing (nsc drops
  the cast itself and builds `UNIT` at the point of use).
  `x.isInstanceOf[Unit]` is `instanceof scala/runtime/BoxedUnit`.

The implementation splits into three parts.

1. **Descriptors**: I split `jvm_desc_val` (value position) from `jvm_desc` (result
   position) (`crates/backend/src/gen.rs`). Method parameters, fields and array
   elements all use the former.
2. **Slots**: a `Unit` parameter really is passed on the JVM, so it occupies one
   slot (`Frame::alloc_param`). Without it, **every parameter behind it shifts**.
   The symbol itself keeps its void sort, so reading it pushes nothing and it is
   still treated like any other `Unit` expression. Since the value can only ever be
   `BoxedUnit.UNIT`, we rebuild it wherever it is needed
   (`fill_boxed_unit_slot` / `adapt_unit_arg`). Synthetic methods that only forward
   (forwarders, bridges, setters, and a `case class`'s `apply`/`copy`) do the
   opposite — they pass along whatever they were handed — so they use `jvm_slot_sort`
   (i.e. `Unit` is a `Ref`). A reference left behind at a discard position counts only
   for **methods defined in this compilation unit** (`unit_stat_leaves_ref`):
   `Using.resource` / `Breaks.catchBreak` / `ArrayOps` each have a dedicated emitter
   that has already dropped the value, so popping a second time would drain the stack.
3. **The private runtime**: `crates/backend/src/runtime.rs` now emits
   `scala/runtime/BoxedUnit` (`UNIT` / `TYPE` / `equals` by identity /
   `hashCode` of 0 / `toString` of `"()"`) and `scala/runtime/Nothing$`
   (an abstract class extending `Throwable`; it is needed because the verifier
   resolves the classes of parameters even for methods that can never be called).
   With that,
   `emit_box(Unit)` becomes `getstatic UNIT` in **both modes**,
   `println(x: Any)` prints `()` even under `--no-scala-library`, and
   `case () =>` no longer matches `null` (a leftover from `agent/patbind`).
   Every place that branched on `library_abi` for `Unit`'s boxed form is gone.

| fixture | what it pins down | expected output |
| --- | --- | --- |
| `ub_param.scala` (`crates/cli/tests/unitbox.rs`, dual-run in both modes) | `Unit` parameters: `f(())`, `f(g())`, `middle(Int, Unit, String)` (the arguments after the `Unit` do not shift), two `Unit`s in a row, a constructor `val u: Unit`, a method on a class, a `Nothing` parameter (`never(scala.runtime.Nothing$)`) | `got` `got` `s1` `two` `()` `()` `42` `x7` |
| `ub_field.scala` (`crates/cli/tests/unitbox.rs`, dual-run in both modes) | `Unit` fields: `val`/`var`/`lazy val` in a class, an `object` and a trait, getter/setter, a local `var`, assignment to `Any` | `()` ×12 |
| `ub_case.scala` (`crates/cli/tests/unitbox.rs`, dual-run in both modes) | `case class K(k: Unit, n: Int)`: `toString` / `equals` / `hashCode` / `copy` / `productElement` / the companion's `apply` and the erased `apply(Object,Object)` bridge / pattern extraction | `K((),3)` `()` `3` `K((),4)` `true` `false` `2` `()` `3` `true` `U(())` `matched` `()` `3` |
| `ub_mixin.scala` (`crates/cli/tests/unitbox.rs`, dual-run in both modes) | `Unit` members across a trait / abstract class / value class: interface methods, the mixin forwarder, the interface's `default` method, the erasure bridge, the setter of an abstract `var`, an `Int => Unit` lambda | `()` ×4 `m` `d` `m` `d` `()` `sub` `3` `()` |
| `ub_call.scala` (`crates/cli/tests/unitbox.rs`, dual-run in both modes) | arguments that do not go through the ordinary call path: `this(…)` delegation, a trait's `$init$`, default arguments, named arguments, a second parameter list, a by-name `Unit`, a method taking two `Unit`s in a row, the bodies of `try`/`catch` and `match`, recursion | `9` `()` `()` `0` `7` `()` `()` `iv` `d1` `d3` `d4` `n5` `c6` `by` |
| `ub_super.scala` (`crates/cli/tests/unitbox.rs`, dual-run in both modes) | `Unit` arguments to a **super constructor** (`class D extends B((), 5)`, `case object Asc extends Dir(())`), abstract members of a trait, a `def` inside a method | `D5` `()` `5` `()` `E` `l2` |
| `ub_boxed.scala` (`crates/cli/tests/unitbox.rs`, dual-run in both modes) | `()` is `BoxedUnit.UNIT`, not `null`: `id(())`, `String.valueOf(())`, `== ()`, `case () =>` not matching `null`, `toString` / `hashCode`, `pop`ping an `id(())` in discard position (so the stack height matches on the loop's back edge), `asInstanceOf[Unit]` / `isInstanceOf[Unit]` | `()` `()` `true` `false` `unit` `null` `other` `()` `()` `0` `2` `()` `2` `true` `false` |
| `ub_typearg.scala` (`crates/cli/tests/unitbox.rs`, library dual-run only) | `Unit` in type-argument position: `List[Unit]` / `Array[Unit]` (`[Lscala/runtime/BoxedUnit;`) / `Option[Unit]` / `Seq[Unit]` / `Tuple2` / `Map[String, Unit]` / `Set[Unit]` / `PartialFunction[Int, Unit]` / `Unit*` varargs / a lambda whose result is `Unit` / `(Unit, Int) => String`. The private runtime has neither varargs `List.apply` / `Array.apply` nor `Map` / `Set` / `Function2`, so this is jar-only | `3` `()` `true` `2` `List((), ())` `2` `()` `Some(())` `()` `()` `((),1)` `List((), ())` `2` `()` `Map(a -> ())` `Set(())` `Some(())` `()` `()` `f1` |
| `ub_sepdef.scala` + `ub_sepuse.scala` (`crates/cli/tests/unitbox.rs`, separate compilation across `-cp`) | using `Unit` members from another compilation unit: reading a classfile's `Lscala/runtime/BoxedUnit;` back as `Unit` (the `apply` and pattern extraction of `case class LK(k: Unit, n: Int)`, and `class LC(val u: Unit)`). The class names deliberately start with `L` (see below) | `libgot` `s1` `LK((),2)` `()` `()` `()` `m` `()` `2` |
| `ub_param_bad.scala` (`crates/cli/tests/unitbox.rs`, rejecting case) | erasure does not loosen the typer: `g(())` against `def g(s: String)` is an error (real scalac too: `type mismatch; found: Unit required: String`) | (compile error) |

The descriptors themselves are inspected with `javap -p` as well
(`ub_param_descriptors_use_boxed_unit` / `ub_typearg_array_descriptor`).
Running is not enough — a `(V)` class cannot be loaded at all, so there is no way to
tell it apart from "it happened to work". We also check that the private runtime really
does emit `scala/runtime/BoxedUnit` and `scala/runtime/Nothing$`
(`private_runtime_emits_boxed_unit` /
`private_runtime_emits_nothing_class`).

Separate compilation also tripped two gaps unrelated to `Unit`, which I fixed.

- **When building the class named by a `StackMapTable` frame from a descriptor,
  `trim_start_matches('L')` was eating every leading `L`**
  (`vtype_from_desc` in `crates/backend/src/code.rs`). In the default package,
  `LK` is `LLK;`, so it became `K` and failed with `NoClassDefFoundError: K`.
  I changed it to `strip_prefix`. The class names in `ub_sepdef.scala` start with
  `L` precisely to trip this regression.
- A `var` from another compilation unit looks like a `val` when read from `-cp`
  (`reassignment to val w`). It does not depend on the field type, so this one
  sits under Remaining.

The measurement is `files=184 errors=411 files_with_errors=72` → **unchanged at
`errors=411 files_with_errors=72`**. slick stops at typechecking and emits no
classfile at all (`classes=0`), so for a slice that only fixes the backend, the
right outcome is for the numbers not to move. What moved is **whether the code we
emit can be loaded by the JVM**, not how many files get through.

The two value positions this slice missed — **the operands of `==` / `!=`**, and the
**receiver** of a member selected on a `Unit` value — were closed in
the "`Unit` comparison operands and `scala.Enumeration`" work
(`agent/uniteq`). The `== ()` in `ub_boxed.scala` only ever had an `Any`-shaped
receiver, so `() == ()` is not exercised here.


### Blocks that end in a definition, op-assign precedence, nested arrays (`agent/stmtval`)

Four basic forms were broken. They are independent of each other, with separate root causes.

**1. A block whose body ends in a definition gives a `VerifyError`.**

```scala
object Main { def main(a: Array[String]): Unit = { val v = 1 } }
// java.lang.VerifyError: Operand stack underflow
//   Location: Main$.main([Ljava/lang/String;)V @2: pop
```

nsc's `TreeBuilder.makeBlock` builds `Block(stats, ())` when the last statement is
**a definition rather than a term** (you can see it with `scalac -Xprint:parser`).

```
def main(a: Array[String]): Unit = {
  val v = 1;
  ()
}
```

Our `block_from_stats` returned the statement as-is when there was only one, and made the
last statement the block's value when there were several, so `{ val v = 1 }` stayed
**a bare `ValDef`**. The block's type became the definition's type (`Int` here), and
`pop_if_value` in `emit_body_return` then **popped a value that had never been pushed**.
This happens whichever of `val` / `var` / `def` / `class` / `object` / `import` / `type`
the block ends with, so **it affects every method**. The fix was just adding the same
branch nsc has to `block_from_stats` in `crates/parser/src/parse.rs`
(`stat_is_definition`). The empty block `{}` was already
`Literal(())`.

**2. `n += i + x` resolves to string concatenation.**

```
error: no matching overload for (String)String with arguments (Int)
```

nsc's `precedence` returns **0, the lowest**, for `isOpAssignmentName` (a name that ends in
`=`, does not start with `=`, is not `!=` / `<=` / `>=`, and starts with an operator
character). We only looked at the first character, so `+=` got the same 8 as `+`, and
`n += i + x` was being read as **`(n += i) + x`**. The left side `n += i` is `Unit`, so
from there `any2stringadd` → `String.+` gets picked and you get that error.
Nothing breaks when no operator follows, as in `n += 1`, so it only reproduced with a
compound expression. All I did was add `is_op_assignment_name` to `op_precedence` in
`crates/parser/src/ast.rs`. `var s = "a"; s += 1` (`String + Any`)
still goes through.

**3. `new Array[Array[Int]](n)` emitted `anewarray java/lang/Object`.**

The operand of `anewarray` is an **internal name**, and when the element is an array type
it is that descriptor itself (`[I`). scalac emits
`anewarray "[I"`. `emit_newarray` was collapsing everything other than `String` / `Class` /
`ModuleRef` down to `java/lang/Object`, so
`Array[Array[Int]]` became `[Ljava/lang/Object;` and the first `arr(i)(j)` failed with
`VerifyError: Bad type on operand stack in iaload`.
Now that the internal name is built from `jvm_desc`, `[Lscala/Tuple2;` for
`Array[(Int, Int)]` and `[Lscala/Function1;` for `Array[Int => Int]` also come out the
same as scalac's (the handling of `Unit` / `Nothing` elements is unchanged).

**4. The type argument of `Array.ofDim[T](n1, …)` was not instantiated, plus `arr(i) += x`.**

```
error: type mismatch; found: 5  required: T
error: value += is not a member of T
```

`scala.Array$.ofDim` is five overloads, for 1 through 5 dimensions, and **every one of
them takes a single type parameter**. `TypeApply` can only narrow a reference when there
is exactly one candidate taking that many type parameters, so it could not narrow `ofDim`,
and the explicit `[Double]` **never reached anywhere** (the result stayed
`Array[Array[T]]`). Now, once the overload is settled by the value arguments, the type
arguments as written are applied to the chosen candidate
(SLS 6.26.3, `pending_targs`).

The same call has two follow-ons. In code generation, `peel_fun` sees straight through the
`TypeApply` and reads the symbol of **the `Select` underneath**, so unless the resolution
result is propagated there too, it ends up calling the 1-dimensional
`ofDim(I, ClassTag)Object`. And the JVM return type of the 2-dimensional `ofDim` is
`[Ljava/lang/Object;`, so just like `Ljava/lang/Object;` it needs
a **narrowing `checkcast "[[D"`** (scalac emits one as well).
I added `erased_array_return` to `maybe_unbox_erased_result`.

`arr(0) += 1` was a separate gap. nsc's `convertToAssignment` goes into `mkUpdate` when
the receiver is `t.apply(i)`, and builds `t.update(i, t.apply(i) op x)`.
The table and the index are evaluated **exactly once**, via `gen.evalOnce`
(a pure reference is duplicated; anything else is bound to a `val ev$…`). We had no such
branch and were failing with "receiver is not assignable". The receiver of an
**ordinary method call such as `bar` is out of scope** (nsc reports
`UnexpectedTreeAssignmentConversionError` for those too). Rewriting `t(i)` into
`t.apply(i)` is something our typer does as well, so `index_table` picks out
only that shape.

| fixture | what it pins down | expected output |
| --- | --- | --- |
| `sv_block.scala` (`crates/cli/tests/stmtval.rs`, private runtime and library dual-run) | blocks whose last statement is a definition: `val` / `var` / `def` / `import` / `class` / `object` / `type`, the empty block `{}`, both branches of an `if`, a `while` body, the bodies of `try` / `match`, a pattern `val`, nested blocks, a lambda body. A block ending in a term keeps its value | `valLast` `nested` `42` `done` |
| `sv_opassign.scala` (same as above) | `+=` `-=` `*=` `/=` `%=` `<<=` `\|=` `&=` `^=` with a compound right-hand side (`i + x`, `f(x) + g(y)`, `(a + b) * c`, an `if`, an alphabetic operator). `var s = "a"; s += 1` still goes through as `String + Any` | `3` `0` `12` `4` `1` `13` `9` `20` `6` `18` `8` `11` `3` `1` `4.5` `a1` `a1bc` `3` |
| `sv_array.scala` (same as above) | the element types and `getClass.getName` of `new Array[Array[Int]]` / `Array[Array[String]]` / `Array[Array[Array[Int]]]`. One dimension (`Int` / `String` / `Object`), `Array[(Int, Int)]`, `Array[Int => Int]` | `2` `10` `[[I` `y` `[[Ljava.lang.String;` `7` `[[[I` `9` `[I` `s` `[Ljava.lang.String;` `[Ljava.lang.Object;` `1,2` `[Lscala.Tuple2;` `2` `[Lscala.Function1;` |
| `sv_update.scala` (same as above) | `t(i) op= x`: arrays (`Int` / `Double` / `String`), nested `nested(0)(1) += 3`, a class with its own `apply`/`update`, a compound right-hand side, `evalOnce` (the table and the index each run exactly once) | `6` `12` `3` `2.0` `ab` `7` `15` `1` `2` |
| `sv_ofdim.scala` (`crates/cli/tests/stmtval.rs`, library dual-run only; the private runtime has no `ofDim`, so it also checks that a diagnostic is emitted) | `Array.ofDim[T]` in 1 through 5 dimensions × `Int` / `Double` / `String` / a user class. Also `val g: Array[Array[Int]] = Array.ofDim[Int](2, 3)`, `Array.fill(3)(0)` and `Array(1, 2, 3)`, which already worked | `7` `[I` `7` `[[I` `7` `[[[I` `7` `[[[[I` `7` `[[[[[I` `2.0` `[D` `6.0` `[[D` `2.5` `[[[D` `ab` `[Ljava.lang.String;` `z` `[[Ljava.lang.String;` `Cell(3)` `[LCell;` `Cell(4)` `[[LCell;` `0,0,0;0,0,9` `2` `[I` `1,2,3` |
| `sv_lib.scala` (`crates/cli/tests/stmtval.rs`, library dual-run only) | the same four items in shapes only the real scala-library can back up: the element type of `Array[List[Int]]`, `n += i max x`, `var lst ++= List(…)`, a `foreach` lambda whose body ends in a definition | `List(1, 2)` `[Lscala.collection.immutable.List;` `2` `3` `List(1, 2, 3)` `6` `done` |
| `sv_bad.scala` (`crates/cli/tests/stmtval.rs`, rejecting case) | op-assign to an immutable receiver keeps nsc's `convertToAssignment` diagnostics as they are (`value += is not a member of Int` plus `Expression does not convert to assignment because receiver is not assignable.`). Until the precedence fix, this came out as an `any2stringadd` error instead | (2 compile errors) |
| `lf_frame.scala` (`crates/cli/tests/loopframe.rs`) | the minimal form `var c: Option[Int] = Some(1); while (c.isDefined) { c = None }`. On top of running it, **the `StackMapTable` from `javap -v` is matched against real scalac's** (scalac has just the one `class scala/Option`; it also checks we have not fallen back to `java/lang/Object` / `scala/Some` / `scala/None$`). Both modes | `done` |
| `lf_loopvar.scala` (same as above, library dual-run only; the private runtime has no varargs `List.apply`) | assorted locals that span a loop: `while` / `do while` / nested loops / `List` → `Nil` / becoming a different class several times in one iteration / `if`, `match`, `try` and `finally` branches inside the loop / a reference inside a handler / a reference after leaving the loop / the desugaring of `for` / a loop inside a lambda / a `Unit` method containing a `while` / a pattern binding inside the loop / a declared type that is a trait (so the frame type is an interface) / a `var` captured by a lambda / the other arm being `Nothing`. It also checks the frame keeps `scala/Option` and `scala/collection/immutable/List` | `None`×3 `List()`×2 `None`×2 `List(0)` `Some(1)` `true` `Some(3)` `true` `1` `6` `9` `List(2, 1, 0)` `12` `true` `None` `List()` |
| `lf_loopany.scala` (same as above) | forms where the declared class becomes `java/lang/Object`: a `var a: Any` moving between a primitive and a reference inside a loop (also checking the frame is not pinned to `java/lang/Integer`), reassigning an array local, a loop with primitives only, a `null` initial value. Both modes | `2` `2` `6` `z1` |
| `lf_trystack.scala` (same as above) | `try` at a position where the operand stack is not empty: `println(try …)`, as a second argument, `new Box(try …)` (an uninitialized reference), with a primitive already pushed, a form that actually throws, an argument position inside a loop, and with `finally`. Both modes | `w0` `w1` `y` `pq` `a` `n=3!` `boom` `ktrue`×2 `kfalse` `true` `fin f` |
| `lf_ctorframe.scala` (same as above) | the type of `this` after the super-constructor call: subclass constructors whose bodies hold branches, loops and `try`, and a super-constructor argument that is a `try` (an uninitialized `this` sits on the stack). It also checks that the frame of `C.<init>` is `C` and not `B`. Both modes | `b` `pos` `neg` `zero` `3` `g1` `d2` |
| `lf_loopvar_bad.scala` (same as above, rejecting case) | putting a `String` into a `var c: Option[Int]` in a loop body. Frame merges use the declared type, so this does not silently widen to `Any`; it is a `type mismatch` | (compile error) |



### Loop-head frames and `try` on the operand stack (`agent/loopframe`)

Three cases that typecheck but **fail at class-load time**. The first two, which were
assumed to share a root cause, turned out to have **separate causes**. The third is a pre-existing gap found while chasing them.
All three are pinned down by `crates/cli/tests/loopframe.rs` and the `lf_*` fixtures.

#### 1. The frame type of a local that spans a loop is its declared type

```scala
var c: Option[Int] = Some(1)
while (c.isDefined) { c = None }
```

was giving `VerifyError: Bad type on operand stack`. The slot holds
`scala/Some` at entry and `scala/None$` on the backward branch, so the merge of two
unrelated classes becomes `java/lang/Object` in this assembler. That is a correct
frame, but it is **too loose** for the
`invokevirtual scala/Option.isDefined` that reads that slot.

Running `javap -v -c` on real scalac 2.13.16 spells out the answer.

```text
  StackMapTable: number_of_entries = 2
    frame_type = 252 /* append */
      offset_delta = 12
      locals = [ class scala/Option ]
  LocalVariableTable:
     Start  Length  Slot  Name   Signature
        12      23     2     c   Lscala/Option;
```

`class scala/Option` — the same as in `LocalVariableTable`: the **erasure of the slot's declared type**.
scalac computes nothing like a least upper bound of `Some` and `None$`. A local has one type
for its whole lifetime, and every frame just repeats it. The declared type is an upper
bound of everything the source could ever write there, so it can be computed without
a class hierarchy and never widens more than necessary. We adopted the same rule
(`declare_local_ty` → `Assembler::set_local_class`).

**Doing it only at merges is not enough.** This assembler writes frames in a single forward
pass, so a frame it finished writing before seeing the backward branch keeps the entry type.

```scala
var a: Any = 1
while (i < 2) { a = if (i == 0) "s" else 2; i += 1 }
```

did merge correctly to `java/lang/Object` at the loop head, but the frame emitted earlier
inside the condition still said `java/lang/Integer`, giving
`VerifyError: Inconsistent stackmap frames`. The point is to align to the declared
class **on every write**, and `java/lang/Object` is treated as a declared class like any
other (as the actual declared type of `var a: Any`).

#### 2. `try` at a position where the operand stack is not empty

```scala
println(try { "y" } catch { case _: Throwable => "no" })
two("p", try { "q" } catch { case _: Throwable => "no" })
new Box(try { "a" } catch { case _: Throwable => "b" })
```

The JVM discards the operand stack when entering an exception handler (JVMS 4.10.1.6).
Whatever was pushed before the `try` — the `Predef$` receiver, an argument evaluated
earlier, the **uninitialized** reference left behind by `new` — is gone on the catch
side, so at the merge point after the `try` the stack depth was n on one side and 0 on
the other: `VerifyError: Inconsistent stackmap frames`. `println` passed in jar mode
only because that one case evaluated the argument first and used `swap`;
`two("p", try …)` was failing in both modes.

Looking at `javap -c`, scalac lifts the `try` in its `LiftTry` phase into a synthetic
method `private static final java.lang.String liftedTree1$1()` and calls that from the
argument position. We instead **spill the pushed values into locals** for the duration
of the protected region (`spill_operand_stack` / `restore_operand_stack`). That avoids
adding a method, and the uninitialized reference from `new` works as-is too — the
verifier does allow `uninitialized(Address)` to be stored in a local.

#### 3. `this` after the super constructor call (a pre-existing gap found while chasing the two above)

```scala
class B(val s: String)
class C(n: Int) extends B("b") {
  val sign: String = if (n > 0) "pos" else "neg"
}
```

was giving `VerifyError: Bad type on operand stack in putfield` /
`Type 'B' … is not assignable to 'C'`. Per JVMS 4.10.1.9,
`invokespecial <init>` replaces `uninitializedThis` with the type of the **class being
verified**, but we were replacing it with the **class that was called** (i.e. the parent `B`).
Every constructor that needs a frame after the super constructor call — one with a branch,
a loop, or a `try` in its body — was failing this way
(`Assembler::initialize`; the fixture is `lf_ctorframe.scala`).

Measurement: `files=184 errors=346 files_with_errors=64` → **unchanged** (the diagnostics are
word-for-word the same as well). slick stops at typechecking and emits not a single classfile
(`classes=0`), so it is exactly right that the numbers do not move for a slice that only fixed
the backend (same as `agent/unitbox`). What did move is **whether the code we emit can be
loaded by the JVM**.

#### Remaining

- **Frames are still `full_frame` only.** scalac compresses them to `append` / `same` /
  `same_locals_1_stack_item`, so for the same content our classfiles come out
  larger. The verifier accepts either, so it is not a correctness problem,
  but it does mean we cannot compare `javap -v` output line by line
  (which is why `crates/cli/tests/loopframe.rs` matches on the set of
  "classes that appear in frames").
- **`lf_loopvar.scala` is jar-only.** The private runtime has no varargs
  `List.apply` (`value apply is not a member of List$`), a pre-existing gap
  unrelated to the frame story. The same goes for `Option`'s `toString` not being
  the case-class one (`lf_trystack.scala` avoids that and runs in
  both modes).


### Factory method return types and erasure to `…Ops` (`agent/fillconcat`)

It started from a single case that typechecks but fails at run time.

```scala
object Main { def main(a: Array[String]): Unit = println(List.fill(2)(5) ::: List(9)) }
```

```
java.lang.VerifyError: Bad type on operand stack
  Reason: Type 'scala/collection/SeqOps' (current frame, stack[1]) is not
          assignable to 'scala/collection/immutable/List'
```

**The return type of `List.fill` is not wrong.** Looking at the jar with `javap -p`,
`List$` has

```
public scala.collection.SeqOps fill(int, scala.Function0);
public java.lang.Object       fill(int, scala.Function0);
```

these two, and real scalac 2.13.16 calls **the former** as well
(because it is `StrictOptimizedSeqFactory[+CC[_] <: SeqOps[…]]`, `CC[A]` erases to its upper
bound `SeqOps`). The only difference was **the single instruction that comes next**.

```
# scalac
invokevirtual List$.fill:(ILscala/Function0;)Lscala/collection/SeqOps;
checkcast     scala/collection/immutable/List      ← this was missing
```

**The root cause** is the test in `maybe_unbox_erased_result` in
`crates/backend/src/gen.rs`. The rule that adds a `checkcast` when the descriptor's
return type is wider than the erasure of the result type was there from the start, but the
condition was "**can the result type reach the declared return type through the prelude's inheritance relation?**". `crates/typer/src/prelude_hier.rs`
**deliberately** leaves the `…Ops` traits such as `SeqOps` / `IterableOps` out of the
hierarchy (the comment says they have no members, so they only make it longer), so
`List <: SeqOps` can never be shown and the `checkcast` was never emitted.

So we turned it into **a decidable question in the opposite direction**: "can the declared
return type be **shown to conform** to the type we want?" — if it cannot be shown, we emit a
`checkcast`. It is a one-sided test, where `false` means "cannot be shown", not "does not
conform". An extra `checkcast` costs three wasted bytes, whereas a missing `checkcast` turns
the whole method into a `VerifyError`.

This was neither about `List.fill` nor about `:::`. `:::` is
**right-associative**, so `List.fill(2)(5) ::: List(9)` is `List(9).:::(List.fill(2)(5))`,
and the factory's result is not the receiver but the **argument**. What was actually broken:

| Form | Before the fix | After the fix |
|---|---|---|
| `List.fill(2)(5) ::: List(9)` | VerifyError | OK |
| `List.tabulate(3)(i => i) ::: List(9)` | VerifyError | OK |
| `List.concat(List(1), List(2)) ::: List(9)` | VerifyError | OK |
| `List.fill(2)(5).head` / `.reverse` | VerifyError | OK |
| `Vector.fill(2)(5).length` | VerifyError | OK |
| `val xs = Vector.tabulate(5)(i => i * i); xs.updated(0, 99)` | VerifyError (the local's frame type was `SeqOps`) | OK |
| `ArrayBuffer.fill(2)(5).size` / `ListBuffer.fill(2)(5).size` | VerifyError | OK |
| `List.iterate` / `List.empty` / `List.unfold` | OK all along | OK |
| `List.fill(2)(5) ++ List(9)` / `.length` / `.map` / `match` | OK all along | OK |
| `Seq.fill` / `Set.fill` / `IndexedSeq.fill` / `Iterator.fill` / `Array.fill` | OK all along | OK |
| `TreeMap(…) - key` (declared as `Map`, result type `TreeMap`) | OK all along (existing rule) | OK |

`++` and `.length` were passing because `gen.rs` writes an explicit descriptor for them
through a different path, not because `fill` was somehow special.

**Fixtures** (all library dual-run only. The private runtime has no `IterableFactory`,
so `crates/cli/tests/fillconcat.rs` also checks that diagnostics appear under
`--no-scala-library`):

| fixture | what it looks at | expectation |
|---|---|---|
| `fc_factory.scala` | uses `fill` / `tabulate` / `concat` / `iterate` / `empty` / `unfold` of `List` / `Vector` / `Seq` / `Set` / `ArrayBuffer` / `ListBuffer` as the argument of `:::`, as its receiver, via a `val`, with a type annotation, and as the scrutinee of a `match` | `List(5, 5, 9)` and others, 22 lines |
| `fc_ops.scala` | `TreeMap - key` / `TreeSet` / `SortedSet` / `SortedMap`, mutable buffers, `LazyList` / `Queue` / `Iterator`, `sum` / `sorted` / `zip` / `toArray` | `1` `2` `List(1, 2, 3)` and others, 25 lines |
| `fc_local.scala` | binding a factory result to a `val` and then calling several methods on it (this is the only shape that failed; using it once and throwing it away did not fail). Also widening to `Seq[Int]` and going through a `def`'s return value | `Vector(0, 1, 4, 9, 16)` and others, 18 lines |
| `fc_factory_bad.scala` (error case) | that the added `checkcast` does not swallow type errors (putting a `List[Int]` into a `Vector[Int]`, a `List[Int]` into a `List[String]`, `::: Vector(9)`) | 3 compile errors |

Measurement: `files=184 errors=346 files_with_errors=64` → **unchanged**. slick stops at
typechecking and emits not a single classfile (`classes=0`), so it is exactly right that
the numbers do not move for a slice that fixed backend codegen.
What moved is **whether the code we emit gets through the JVM**, not how many files get through.

#### Remaining

- ~~**`List.range` / `Vector.range` / `Seq.range` have no `Integral[Int]`**~~
  → resolved in the next section, `agent/integral`. The test in `fillconcat.rs` has been
  rewritten as `range_resolves_the_integral` (the shape that now passes).
- `Array.range(0, 3)` is a different overload that does not take an `Integral`, so it passes.


### Expression statements in a template body (`agent/ctorstmt`)

`class A { println("ctorA") }` typechecked, produced a classfile, and ran
**without printing anything**. Statements in a template body other than `val` / `var` / `def` (bare expression statements) were going **nowhere at all**:
not into the primary constructor, not into a trait's `$init$`, not into module initialization.
No diagnostic is produced, so the only way to notice is the difference in what it runs.

Per SLS 5.1 / 5.3, statements in a template body are part of the template's **initializer**.

- **class**: run inside the primary constructor, **interleaved in declaration order** with `val` / `var` initializations
- **trait**: go into `$init$` and run in linearization order at mixin time
- **object**: run exactly once during module initialization (when `MODULE$` is created)

The cause was three places in the backend that narrowed the template body to just `ValDef`
(`crates/backend/src/gen.rs`).

| Place | Before the fix | After the fix |
|---|---|---|
| `emit_class_ctor` | `body.filter(ValDef)` | `template_init_stats(body)` (`ValDef` plus bare statements in source order) |
| `emit_module_init` | same as above | same as above |
| `emit_trait_init` | `trait_vals` (`val` only) | `trait_inits` (`val` plus bare statements in source order) |

`ValDef` is handled as before with `gen_expr` plus `putfield` (a mixin setter for a trait),
and bare statements are emitted with `gen_stat`. `gen_stat` is the existing path that discards
a value-producing expression, so generating `if` / `match` / `try` **in statement position**
(`expectedType = UNIT`) comes along for free.

`trait_vals` is also used to generate accessors and mixin forwarders, and there we only want the
`val`s, so the sequence that `$init$` actually runs lives in a separate map, `trait_inits`.
The decision of whether the implementing class calls `$init$`,
was switched over to `trait_inits` too. Without that, a **trait whose body is only statements**,
like `trait T1 { note("T1") }`, gets no `$init$` generated in the first place.

The `extends App` / `DelayedInit` path was already picking up bare statements via
`is_delayed_ctor_stat`, so it is unchanged.

#### A statement on the line after `val x: T` was being swallowed into the type (parser)

We also fixed the parser-side variant of the same "the statement disappears" phenomenon.

```scala
trait A {
  val p: String
  println("x")     // ← was being read as the infix type String println "x"
}
```

`parse_compound_type` was skipping newlines **unconditionally** while looking for `with` and
the refinement `{`, so it also consumed the NEWLINE that ends the statement, and the identifier
on the next line was eaten as an infix type constructor. nsc's `newLineOptWhenFollowedBy(LBRACE)` only skips
"when what follows the newline really is `{` (or `with`)", so we added the same
`newline_opt_when_followed_by` (`crates/parser/src/parse.rs`).
Before the fix you got either an unrelated diagnostic, `not found: type +`, or — for a `val`
with a right-hand side — the statement silently disappearing.

#### Verification

The fixture prefix is `cs` and the test is `crates/cli/tests/ctorstmt.rs`.
Every one of them runs `java -Xverify:all` in **both modes, the private runtime and the jar**,
and is compared against the output of real scalac 2.13.16.

| fixture | contents | expectation |
|---|---|---|
| `cs.scala` | statements in a class / trait / mixin / object, statements interleaved with `val`s (in both a class and a trait), a trait whose body is only statements, a statement after an abstract `val`, a `var` from the body updated by a later statement, and touching `O.v` twice while module initialization still runs once | `A;T1;T2;B;` and others. Exact match with real scalac |
| `cs_forms.scala` | early `require` / `assert`, `if` / `match` / `try` / `while` / lambdas, a `case class` body, a local class, anonymous classes (`new AnyRef { … }` and `new Greeter { … }` implementing an abstract member), a member `object` on the `$outer` path | Exact match with real scalac |
| `cs_bad.scala` (error case) | `notAMethod(1)` as a statement in a class body, `n.noSuchMember` as a statement in a trait body | 2 errors (real scalac also reports 2 on the same 2 lines) |

The shape of real scalac's output as read with `javap -p -c` is locked in too. `Main$B()` goes
`invokespecial Main$A.<init>` → `T1.$init$` → `T2.$init$` → `Main$.note`, in that order, and
that is the same as our output, `$init$` on the interface included.

#### Remaining

- For module initialization, scalac folds a static `object` into `<clinit>` and writes to a
  static field. We put it in `<init>` and write to an instance field (a difference that
  predates `agent/ctorstmt`). Both run exactly once as module initialization,
  so there is no observable difference.
- `require(cond, msg)` in the private runtime does not prefix the exception message with
  `requirement failed: ` (jar mode and real scalac do).
  That is a pre-existing difference independent of this fix, and `cs_forms` is written so it
  does not depend on the body of the message.
- Whether **statements** can be written inside an early definition (`new { val x = 1 } with T`)
  is untouched. nsc does not allow statements in an early definition block, and
  we keep the existing path that emits only `val`s pre-super.


### Branch offsets and `code_length` (`agent/ms`, 2026-09-05)

Three casts in `crates/backend/src/code.rs` were narrowing a class file format
width without checking it, so a program too big in either of two ways compiled
to a class file that nothing complained about until it ran:

| cast | what it was doing |
|---|---|
| `(rel as i16)` in `finish` | a branch over more than 32767 bytes wrapped, so a conditional jumped to a negative offset |
| `self.bytes.len() as u16`, then `frames.retain(\|off\| off < end)` | for a method over 64 KB `end` wrapped, and nearly every stack map frame was discarded |
| `(delta as u16)` in `encode_stack_map` | the same wrap in a frame's `offset_delta` |

They look like one bug and are two, with different right answers.

**A branch that does not reach is our problem to solve, not to report.**
JVMS 6.5 gives every branch a signed 16-bit offset; only `goto`/`jsr` have a
wide form. nsc compiles `scala/test/files/run/t10594.scala` -- 8273 calls
inside an `if` -- to a perfectly ordinary 33109-byte method, because ASM
rewrites the branch it cannot encode:

```
   12: ifne          20
   15: goto_w        33109
   20: ...
```

We wrote `ifeq -7611` and died with `VerifyError: Expecting a stackmap frame at
branch target -7611`. `Assembler::widen_jumps` now does the same rewrite: a
`goto` becomes `goto_w`, a conditional becomes its inverse over a `goto_w`, and
the choice runs to a fixpoint because growing the code can put a further branch
out of range.

Two details are not obvious from the shape:

* **The fall-through of the inverted branch is a new branch target**, and JVMS
  4.7.4 wants a frame on it. The state there is only known while the branch is
  being emitted, so `jump` records it in `cond_frames` and the rewrite promotes
  the entries it needs. Using the *target's* frame instead would be unsound:
  that frame is the merge of every path arriving there, so it can be wider than
  the fall-through's own state, and the code after it may need the precision.
  When a label already sits on the fall-through, its merged frame wins -- other
  predecessors are relying on it.
* **Each rewrite grows the code by a multiple of four**, padded with `nop`s,
  so that the alignment padding of a `tableswitch`/`lookupswitch` behind it
  never has to change size (which would feed back into the fixpoint). The
  `nop`s go *before* the widened instruction: JVMS 4.10.1 wants a frame on the
  instruction after an unconditional branch, and padding placed behind the
  `goto_w` puts unreachable `nop`s there ("Expecting a stack map frame ...
  @23: nop").

**A method over 64 KB is not our problem to solve.** JVMS 4.7.3 requires
`code_length < 65536`, so no encoding of the method exists; nsc says

```
error: Error while emitting Big
Method too large: Big.big ()V
```

and writes nothing for the class. `ClassBuilder::add_code` now records the same
message in `EmittedClass::format_errors`, and the driver reports it and drops
the class instead of writing one. Note the shape of the old failure: the
`code_length` field itself is a `u4`, so the over-long value was written out
faithfully and `javap` read the file back without a murmur -- it was the
`u16` offsets *inside* it that had wrapped.

Because our call sequence is longer than nsc's (`aload_0; checkcast; invoke` is
7 bytes where nsc emits `aload_0; invoke` in 4), we reach that limit at about
57% of the source size nsc does. That is a codegen-quality gap, not a
correctness one, and it is the reason a method nsc accepts can now be rejected
here.

#### Verification

`crates/cli/tests/ms_bigmethod.rs` generates its sources rather than checking
them in -- the smallest program that reaches either limit is tens of thousands
of statements. It pins the t10594 shape (compiled, run under `-Xverify:all`,
stdout compared with real scalac 2.13.16), a backward `goto` out of range with
a `lookupswitch` behind it, and the `Method too large` diagnostic together with
the absence of a class file. `crates/backend/src/code.rs`'s own tests pin the
byte-level shape of each rewrite, including a cascade where widening one branch
is what pushes another out of range.
