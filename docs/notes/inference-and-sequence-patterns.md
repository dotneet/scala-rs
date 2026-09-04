# Type inference, argument conformance, and sequence patterns

Two slices from the scala-rs development log. Both are about the typer deciding
what an argument or a pattern actually *is*: sequence patterns and stable-identifier
patterns on the pattern-matching side, and base types and auto-tupling on the
argument-conformance side. Both were found by diffing our output against real
scalac 2.13.16.

### Sequence patterns, `StringOps.map`, and stable-identifier patterns (`agent/seqpat`)

This slice cleared three things: `case Seq(a, b)` was unusable, there was only one
`StringOps.map`, and stable-identifier patterns were stricter than nsc. The fixtures are
`tests/fixtures/seqpat.scala` / `seqpat_map.scala` / `seqpat_ids.scala`
(all byte-for-byte identical to real scalac 2.13.16 on stdout), plus the rejecting side,
`seqpat_bad.scala` / `seqpat_star_bad.scala` / `seqpat_nolib_bad.scala`.
The tests live in `crates/cli/tests/seqpat.rs`.

**1. Only `List`'s companion had an `unapplySeq`.**
I added `unapplySeq[A](x: CC[A]): Option[Seq[A]]` to the companions of
`Seq` / `Vector` / `IndexedSeq` / `Array`
(`crates/typer/src/prelude_seqpat.rs`). On the codegen side, alongside
`gen_unapply_seq_bind` (the **List-specific** head/tail walk that starts from
`checkcast List`) there is now `gen_unapply_wrapper_bind`, and `SeqPatShape` switches
between `scala/collection/SeqFactory$UnapplySeqWrapper$` and
`scala/Array$UnapplySeqWrapper$`. It calls the same
`lengthCompare$extension` / `apply$extension` / `drop$extension` that real scalac's
`javap -p -c` shows, so passing a `Vector` as a `Seq`, or matching the `ArraySeq` that
`"abc".map(_.toString)` returns with `case Seq(a, b, c)`, no longer breaks.

For the record, the README's claim that "`case List(a, b, rest @ _*)` still throws a
`VerifyError` on main" **had already been fixed by the later `41d4bca` (the extractor
checkcast)**. It is pinned down by `listShape` / `caseElems` in `seqpat.scala`.

**1b. Two silently broken things found along the way.**

- **An `Any` scrutinee.** Writing `case Seq(a, b)` / `case List(a, b)` /
  `case Array(a, b)` against an `Any` went straight into the `checkcast` /
  wrapper extension with no type test at all
  (`ClassCastException` / `IllegalArgumentException: Argument is not an
  array`). Like scalac, we now emit the `instanceof` first (for `Array`,
  `ScalaRunTime.isArray(Object, 1)`), and skip it only when the static type
  already guarantees it.
- **A `_: T` sub-pattern.** `case List((s, _: TableNode))` emitted a
  `checkcast TableNode` before binding the elements, so **a non-matching value
  turned into an exception** instead of falling through to the next case. A type
  ascription is a *test*, not a cast, so it is left to `gen_pattern`'s `instanceof`
  (`is_type_test_pat`). Case-class constructor patterns
  (`case Some((s, _: TableNode))`) had the same gap.

**2. The two overloads of `StringOps.map`.** In 2.13, `StringOps` has both
`map(Char => Char): String` and `map[B](Char => B): IndexedSeq[B]`, and their
JVM descriptors differ only in the return type (confirmed with `javap -s`). The right
thing is to carry **two symbols** in the prelude as well (`crates/typer/src/prelude_strmap.rs`);
folding them into one makes `value_extension_desc` build the descriptor from the symbol's
result type, so the `IndexedSeq`-returning one gets called even for `Char => Char`.
Once both were present we got `ambiguous overload`, and the cause was in three places
in overload resolution:

- `is_as_specific_method` did not treat the other alternative's type parameters as
  undetermined. If `B` in `map[B](Char => B)` cannot be pinned to `Char`, each
  alternative comes out "as specific as" the other.
- In the other direction, our own type parameters were not **rigid**. `B` in `Char => B`
  is not `Char`, so we substitute its upper bound (`Any` by default) before comparing.
- `arg_score` scored "function types match as long as the parameter shapes line up".
  That relaxation exists for lambdas whose result type is not known yet, so it now
  demands real conformance **only when both sides are determined** (value discarding for
  `Unit` / `Any` parameters and numeric widening behave as before).

On top of that I added nsc's `Infer.pretypeArgs`. If every overload candidate demands the
same function parameter type, the lambda can be typed before resolution. Without it,
`"abc".map(_.toString)` stays `(<notype>) => <notype>`, is applicable to both, and the
more specific `Char => Char` version wins by mistake.

**3. Typechecking stable-identifier patterns.** nsc does not require conformance, only
that the two types **can be inhabited at the same time**. Open classes can always be
co-inhabited, so `case Ids.other =>` (an `Other`) is legal against an `ST[Int]` scrutinee.
Only `final` classes (`String`, value classes, arrays, objects) and primitives justify
exclusion, and there scalac does emit `type mismatch`
(`stable_pattern_compatible` / `is_final_like`).

**Bonus: the parser was dropping `final` / `abstract` / `sealed`.** `parse_modifiers`,
which reads a class's optional constructor modifiers (`class C private (x: Int)`), skips
newlines, so it **ate the modifiers of the next definition** that followed right after
`class Other`. In other words, `final` / `abstract` / `sealed` / `implicit` disappeared
from every class after the first one in a file (and since `FinalOther` was not final,
the check in point 3 did not fire either). Constructor modifiers sit on the same line as
the class name, so it now checks for `private` / `protected` / `@` **before** skipping
newlines.

| `seqpat.scala` (library dual-run) | Sequence patterns over `Seq` / `List` / `Vector` / `IndexedSeq` / `Array` (fixed length, `_*`, nested, tuple elements, case-class elements), receiving an `ArraySeq` as a `Seq`, an `Any` scrutinee, and `_: T` sub-patterns | `empty` `one 1` `two 3` `many 3 2` `xyz\|w` `q` `ab` `a2` `24` `3` `3` `xy\|z` `4` `k7` `5` `abc` `arr 12` `seq 12` `seq 12` `lst 9` `?` `?` `table a` `plain a` `table b` `plain b` `table c` `plain c` |
| `seqpat_map.scala` (library dual-run) | The two overloads of `StringOps.map` (`Char => Char` gives `String`, anything else gives `IndexedSeq[B]`) | `Ab` `ABC` `ArraySeq(a, b, c)` `ArraySeq(97, 98, 99)` `a-b` `abc` `3` `false,false,true,true,false` |
| `seqpat_ids.scala` (library + private-runtime dual-run) | Stable-identifier patterns (unrelated class / trait / `Any` scrutinees), and the modifiers of a definition that follows a class | `st` `?` `tr` `?` `other` `?` `7` `true` `true` |
| `mc_update.scala` (`crates/cli/tests/mutcoll.rs`, library + private-runtime dual-run) | `f(args) = v` → `f.update(args, v)` (SLS 6.15): arrays, a user class's `update`, a two-argument `update`, a selected receiver (`h.b(1) = 41`), a generic `update`, an `update` returning something other than `Unit`, and using the result of an `apply` as the receiver | `7,0,8` `15` `1:2:hi` `42` `3=x` `10` `5` |
| `mc_maps.scala` (`crates/cli/tests/mutcoll.rs`, library dual-run) | Companion `apply` (zero-argument and varargs) and `empty` for `mutable.Map` / `HashMap` / `LinkedHashMap` / `Set` / `HashSet` / `LinkedHashSet` / `ArrayBuffer` / `ListBuffer` / `Buffer`, plus `m(k) = v`, `update` / `getOrElseUpdate` / `remove` / `contains`, `+=` / `-=` / `++=` / `--=`, and `nested("outer")("inner") = 42` into a nested `Map` | `List((d,4), (e,5))` and 16 more lines |
| `mc_queue.scala` (`crates/cli/tests/mutcoll.rs`, library dual-run) | `mutable.Queue` / `Stack` / `ArrayDeque` / `PriorityQueue` / `TreeSet` / `TreeMap` / `ArraySeq` / `StringBuilder`: companion `apply` (including the zero-argument one) and `empty`, `new X[T]()`, `enqueue` / `dequeue` / `head` / `push` / `pop` / `top` / `append` / `prepend`, the `Growable` / `Shrinkable` operators, and `StringBuilder.newBuilder` | `1` `2` `2` `List(2, 3)` and 33 more lines |
| `mc_maps_bad.scala` (`crates/cli/tests/mutcoll.rs`) | `m("a") = "wrong type"` / `m(1) = 2` are rejected by the desugared `update(String, Int)`; `n(0) = 7` on a class with no `update` gives `value update is not a member of NoUpdate`; `q.enqueue("not an Int")` is rejected on the element type | 4 errors |
| `mc_queue_bad.scala` (`crates/cli/tests/mutcoll.rs`) | When `op=` is not a member of the receiver, we emit **one** error, like nsc (the second line being `Expression does not convert to assignment because receiver is not assignable.`) | 1 error |

The rejecting side is `seqpat_bad.scala` (5 cases involving `final` classes, `String`, and
primitives; real scalac 2.13.16 emits the same 5), `seqpat_star_bad.scala` (`_*` not in
final position), and `seqpat_nolib_bad.scala` (`case Array(…)` under `--no-scala-library`
is diagnosed). Minimal accepting tests live in `seqpat.rs` as well
(`a_seq_pattern_binds_the_scrutinees_element_type` /
`a_star_pattern_takes_the_extractors_own_container` /
`a_user_unapply_seq_is_untouched` /
`string_ops_map_picks_the_alternative_by_the_literals_result` /
`a_stable_id_pattern_only_has_to_be_inhabitable` /
`modifiers_after_a_class_are_not_swallowed` /
`a_constructor_access_modifier_still_parses`).

The measurement went from `files=184 errors=620 files_with_errors=87` to **exactly the
same `errors=620 files_with_errors=87`** (the multiset of errors does not move by a single
entry). slick's own `case Seq((s, _: TableNode))` (`JdbcStatementBuilderComponent.scala`
lines 164-165) is still `found: A required: TermSymbol`. Writing the same shape on its own
compiles (`a_seq_pattern_binds_the_scrutinees_element_type` in
`crates/cli/tests/seqpat.rs`), so on slick's side it is a **cascade from a different error
in the same file**. Just below it, `currentUniqueFrom = from match { … }` hits a separate
gap (carried over from main), which I wrote up in the Remaining list below.


### Argument base types and auto-tupling (`agent/hkinfer`)

Two independent problems around argument conformance, both found by diffing against real scalac.
The tests are in `crates/cli/tests/hkinfer.rs` and the fixture prefix is `hk`.

**1. Type arguments were not being inferred from an argument's base type. This is not specific to higher-kinded types.**
The report was about passing `object OC extends C[Option]` to `def use[F[_]](c: C[F])`, but
**the first-order case fails in exactly the same way**:

```scala
trait D[A]; object OD extends D[Int]
def u[A](d: D[A]): A = ???
u(OD)   // error: no matching overload for (D[A])A with arguments (OD$)
```

What separated the working from the failing case was not the kind arity but **whether the
argument is a singleton type**. `new LC` (a class instance) worked all along; only `OC` / `OD`
(objects) failed. `unify_tparam_all` first rewrites the argument to the parameter's class with
`align_to_param_class` and then unifies, but that `align_to_param_class` and `base_type_instance`
**only accepted `Type::Class`**. The type of an object reference is
`Type::ModuleRef`, so it fell straight through.

nsc's `Types.baseType` reads a singleton type through **whatever it widens to**. I did the same:
`base_type_instance` now also handles `ModuleRef` / `ThisType` / `SingleType` / `Annotated`, and
`align_to_param_class` looks through those three singleton types too. The result of a method
returning `this.type` (`SelfInt.me`), and path types like `val sv: SelfInt.type = SelfInt`,
now go through the same route.

Merely **having** a base type is not enough; as before, we still require its type arguments to
match (`hk_base_bad`: `object OD extends D[Int]` is not a `D[String]`, and since it pins `A` to
`Int`, `two(OD, "s")` does not compile either. Real scalac emits the same 2 errors).

**2. Auto-tupling (SLS 6.6) did not fire when the callee was overloaded.**
`retry_tupled_args` itself had been there all along, but it bailed out whenever the callee was
overloaded, on the theory that "nsc does not tuple into an overload".
`println` is precisely an overloaded method, so `println(1, "a")` did not compile.

Here is the order I confirmed against real scalac.

- **If even one alternative takes the arity you wrote, do not tuple.**
  Given `def c(x: String, y: String)` / `def c(t: (Int, String))`, `c(1, "x")` is
  `type mismatch; found: Int(1) required: String` in scalac too (`hk_tuple_bad`).
- Only when no alternative takes that arity do we pack the arguments into a single tuple and
  retype **exactly once** (`println(1, "a")` → `println((1, "a")): Any`).
- After repacking, ordinary overload resolution runs. Given `def b(x: Any)` /
  `def b(t: (Int, String))`, `b(1, "x")` picks the more specific `b((Int, String))`
  (scalac says `bTup` too).
- If ordinary resolution succeeds first, it still wins, as before. Given `def h(a: Int, b: Int)` /
  `def h(t: (Int, Int))`, `h(1, 2)` is `two-args`.

The decision is made by `some_alt_takes_arity` (`check.rs`), which also counts varargs parameters
and omissible trailing defaults. **We do not expand in the other direction**: given
`def g(a: Int, b: Int)`, `g((1, 2))` is still an error (`hk_tuple_bad`).

It is not limited to two elements. `Tuple3` through `Tuple22` go through the same path, and the
elements are ordinary expressions (`hk_tuple_lib` checks
`println(Red == Red, Red.toString, Custom("a") == Custom("a"))`,
`println(Set(1,2) & Set(2,3), Set(1,2) | Set(3), Set(1,2) diff Set(1))`, and
`println(f.isDefinedAt(1), f.applyOrElse(-1, (_: Int) => "neg"))`, plus 4-, 6- and 22-element
cases, all diffed against real scalac). 23 elements or more is an error, like scalac, because
there is no `Tuple23`.

**We emit no warning.** nsc warns on auto-tupling, but in 2.13.16 that warning comes from
**`-Xlint:adapted-args`**, not from `-deprecation`
(`adapted the argument list to the expected 2-tuple: add additional parens instead`).
scala-rs does not have that lint, so it **accepts without a warning**.

| fixture | what it pins down | expected output |
| --- | --- | --- |
| `hk_base.scala` (`crates/cli/tests/hkinfer.rs`, private-runtime + library dual-run) | Solving type arguments from an argument's base type: objects (first-order `Box[Int]` / higher-kinded `Ctor[IdBox]`), class instances, the result of a method returning `this.type`, and the path type `val sv: SelfInt.type` | `7` `s` `3` `5` `6` `8` |
| `hk_base_lib.scala` (`crates/cli/tests/hkinfer.rs`, library dual-run) | The reported shape verbatim: `object OC extends C[Option]` / `class LC extends C[List]` passed to `def use[F[_]](c: C[F])`, plus the explicit `use[Option](OC)` and the first-order `firstOrder(OD, 42)` | `Some(1)` `List(1)` `Some(1)` `42` |
| `hk_base_bad.scala` (`crates/cli/tests/hkinfer.rs`, rejecting case, both modes) | The base type's type arguments still have to match (`need(OD)` / `two(OD, "s")`). Real scalac emits 2 errors as well | (2 compile errors) |
| `hk_tuple.scala` (`crates/cli/tests/hkinfer.rs`, private-runtime + library dual-run) | The auto-tupling order: a single method (`f` / `s`), a same-arity alternative winning (`h`), and tupling into an overload and then picking the most specific (`a` / `b`) | `1` `3z` `two-args` `aAny` `bTup` |
| `hk_tuple_lib.scala` (`crates/cli/tests/hkinfer.rs`, library dual-run only) | `println(1, "a")` prints `(1,a)`. Same for `Tuple3` / `Tuple4` / `Tuple6` (including elements that use `==`, extension methods, and `PartialFunction` members). The private runtime's `Tuple2` has no `toString` of its own, and writing the parentheses explicitly as `println((1, "a"))` shows the same difference, so this one is jar-only | `(1,a)` `1` `(true,Red,true)` `(3,4)` `(Set(2),Set(1, 2, 3),Set(2))` `(true,neg)` `(1,2,3,4)` `(1,b,3.0,true,c,6)` |
| `hk_tuple_bad.scala` (`crates/cli/tests/hkinfer.rs`, rejecting case, both modes) | What tupling **must not** let through: the reverse expansion of `g((1, 2))`, a non-tuple parameter (`one(1, 2)`), a parameterless method (`zero(1, 2)`), and the case where an alternative takes the same arity (`c(1, "x")`). Real scalac emits the same 4 | (4 compile errors) |

The measurement went from `files=184 errors=518 files_with_errors=80` to **`errors=517
files_with_errors=80`**. The difference in the multiset of errors is **exactly one entry**: what
disappeared is `type mismatch; found: DBIOAction[R, S, Effect with E with E2]
required: DBIOAction[R, S, E]`, and **nothing was added**. slick barely ever passes an
`object` as a type-class witness, and barely ever calls an overload with a mismatched argument
count, so these two fixes do not move slick's numbers much
(both of them came out of diffing against real scalac).

