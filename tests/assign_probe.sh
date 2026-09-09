#!/bin/zsh
# Two-directional probe over assignment and named arguments.
#
# For each snippet below: does scala-rs accept it, and does real scalac
# 2.13.16? Disagreements are reported in **both** directions, because the
# dangerous half of a mutability defect is the one that produces no error
# message. `agent/varassign` was briefed on eight `reassignment to val
# initBlank` errors and found five *silently accepted* programs with this,
# each of which reached the backend:
#
#   def v: Int = 1; v = 2        ->  putfield C.v:I, a field C has not got
#   object O; O = null           ->  putfield scala/runtime.O:LO$;
#   Nil.length = 2               ->  the same, through a Select
#   an inherited val (classfile) ->  putfield to someone else's private field
#   an inherited var (classfile) ->  accepted, and IllegalAccessError at run time
#
# Usage: tests/assign_probe.sh
# Env:   SCALA_RS  a binary to measure instead of building target/release
#        ASSIGN_PROBE_DIR  where the snippets and logs go
#
# The `a` family is what scalac accepts and we must not reject; the `b` family
# is what scalac rejects and we must not accept; the `c` family is real `var`s,
# which no tightening of the `b` family may break. Everything the probe found
# is a test in `crates/cli/tests/varassign.rs`; this script is how the next
# slice on assignment re-asks the question over a wider set.
#
# What accept/reject cannot see: `s01_inherited_var` *agreed* on both sides at
# the branch point and was still miscompiled -- the store went to the
# superclass's private field and threw `IllegalAccessError` at run time. That
# one needs `varassign_inherited_var_runs`, which executes it. A probe of this
# shape bounds the diagnostics, not the code generation.
#
# At the branch point (`daa19440`) this reports 19 disagreements of 47; after
# `agent/varassign`, one -- `b24_setter_only`, which is recorded as unfixed.
set -u
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
if [[ -z ${SCALA_RS:-} ]]; then
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>/tmp/assign_probe_build.log \
    || { cat /tmp/assign_probe_build.log; exit 1; }
fi
SCALAC=${SCALAC:-/tmp/scala-2.13.16/bin/scalac}
JAR=/tmp/scala-rs-lib/scala-library-2.13.16.jar
for f in $BIN $SCALAC $JAR; do
  [[ -e $f ]] || { print "assign_probe: missing $f" >&2; exit 1 }
done
P=${ASSIGN_PROBE_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/scala-rs-assign-probe-XXXXXX")}
C=$P/cases
rm -rf $C; mkdir -p $C

case_() { print -r -- "$2" > $C/$1.scala }

# --- direction A: named arguments, which are not assignments ---------------
case_ a01_this_named          'class C(a: Int, b: Boolean) { def this() = this(1, b = true) }'
case_ a02_this_all_named      'class C(a: Int, b: Boolean) { def this(x: String) = this(a = 2, b = false) }'
case_ a03_this_reordered      'class C(a: Int, b: Boolean) { def this(x: String) = this(b = false, a = 2) }'
case_ a04_this_overloaded     'class C(a: Int, b: Boolean) { def this(a: Int) = this(a, b = true); def this(x: String) = this(a = 3) }'
case_ a05_this_default        'class C(a: Int, b: Boolean = false) { def this(x: String) = this(a = 7) }'
case_ a06_this_curried        'class C(a: Int)(b: Boolean) { def this() = this(a = 1)(b = true) }'
case_ a07_new_named           'class C(a: Int, b: Boolean)
object M { val c = new C(b = true, a = 1) }'
case_ a08_method_named        'object M { def f(a: Int, b: Boolean) = a; val z = f(b = true, a = 1) }'
case_ a09_parent_named        'class B(a: Int, b: Boolean)
class D extends B(b = true, a = 1)'
case_ a10_this_named_shadow   'class C(a: Int, b: Boolean) { def this(x: Long) = this(a = 1, b = true); def run(): Unit = { val q = a } }'
case_ a11_this_named_var      'class C(var a: Int, b: Boolean) { def this() = this(a = 1, b = true) }'
case_ a12_this_named_repeated 'class C(a: Int, rest: Int*) { def this() = this(a = 4) }'
case_ a13_new_overloaded      'class C(a: Int, b: Boolean) { def this(a: Int) = this(a, b = true) }
object M { val c = new C(a = 3) }'
case_ a14_method_overloaded   'object M { def f(a: Int, b: Boolean) = a; def f(a: Int) = a; val z = f(a = 3) }'

# --- direction B: assignment to something that is not a `var` --------------
case_ b01_local_val        'object M { def f(): Unit = { val x = 1; x = 2 } }'
case_ b02_field_val        'class C { val v = 1; def f(): Unit = { v = 2 } }'
case_ b03_this_field_val   'class C { val v = 1; def f(): Unit = { this.v = 2 } }'
case_ b04_ctor_param       'class C(a: Int) { def f(): Unit = { a = 2 } }'
case_ b05_ctor_val_param   'class C(val a: Int) { def f(): Unit = { a = 2 } }'
case_ b06_method_param     'object M { def f(p: Int): Unit = { p = 1 } }'
case_ b07_lazy_val         'object M { def f(): Unit = { lazy val x = 1; x = 2 } }'
case_ b08_object_val       'object O { val v = 1 }
object M { def f(): Unit = { O.v = 2 } }'
case_ b09_local_def        'object M { def f(): Unit = { def x = 1; x = 2 } }'
case_ b10_member_def       'class C { def v: Int = 1; def f(): Unit = { v = 2 } }'
case_ b11_case_field       'case class P(x: Int)
object M { def f(): Unit = { val p = P(1); p.x = 2 } }'
case_ b12_trait_val        'trait T { val v: Int = 1 }
class D extends T { def f(): Unit = { v = 2 } }'
case_ b13_pattern_val      'object M { def f(): Unit = { val (a, b) = (1, 2); a = 3 } }'
case_ b14_jar_member       'object M { def f(): Unit = { Nil.length = 2 } }'
case_ b15_def_via_select   'trait T { def v: Int }
class D extends T { def v: Int = 1 }
object M { def f(): Unit = { val d = new D; d.v = 2 } }'
case_ b16_final_val        'class C { final val v = 1; def f(): Unit = { v = 2 } }'
case_ b17_implicit_val     'class C { implicit val v: Int = 1; def f(): Unit = { v = 2 } }'
case_ b18_private_this_val 'class C { private[this] val v = 1; def f(): Unit = { v = 2 } }'
case_ b19_for_bound        'object M { def f(): Unit = { for (i <- 1 to 3) { i = 4 } } }'
case_ b20_self_type_val    'trait T { val v: Int = 1 }
trait U { self: T => def f(): Unit = { v = 2 } }'
case_ b21_object_itself    'object O
object M { def f(): Unit = { O = null } }'
case_ b22_param_in_ctor    'class C(a: Int) { a = 3 }'
case_ b23_unknown_name     'object M { def f(): Unit = { nosuch = 2 } }'
# Known disagreement, kept so it cannot be forgotten: scalac reads this as a
# call to the hand-written setter. See docs/not-implemented.md.
case_ b24_setter_only      'class C { val v = 1; def v_=(x: Int): Unit = () }
object M { def f(): Unit = { val c = new C; c.v = 2 } }'

# --- direction C: real `var`s, which must keep compiling -------------------
case_ c01_local_var        'object M { def f(): Unit = { var x = 1; x = 2 } }'
case_ c02_field_var        'class C { var v = 1; def f(): Unit = { v = 2; this.v = 3 } }'
case_ c03_ctor_var_param   'class C(var a: Int) { def f(): Unit = { a = 2 } }'
case_ c04_trait_var        'trait T { var v: Int = 1 }
class D extends T { def f(): Unit = { v = 2 } }'
case_ c05_private_this_var 'class C { private[this] var v = 1; def f(): Unit = { v = 2 } }'
case_ c06_object_var       'object O { var v = 1 }
object M { def f(): Unit = { O.v = 2 } }'

# --- separately compiled: the shapes only a class file can produce ---------
# A `var` inherited from a class file is an accessor pair around a *private*
# field, and a `val` is a getter with no `_=` at all. Neither shape exists in
# a single-file run, which is why no measure had ever shown either.
mkdir -p $P/lib
print -r -- 'class B { var bv: Int = 1; val bc: Int = 2 }
trait T { var tv: Int = 3; val tc: Int = 4 }' > $P/Lib.scala
$SCALAC -classpath $JAR -d $P/lib $P/Lib.scala >/dev/null 2>&1 \
  || { print "assign_probe: scalac could not build the separate-compilation library" >&2; exit 1 }
case_ s01_inherited_var    'class E extends B with T { def g(): Unit = { bv = 5; tv = 6 } }'
case_ s02_inherited_val    'class E2 extends B { def g(): Unit = { bc = 5 } }'
case_ s03_inherited_trait_val 'class E3 extends T { def g(): Unit = { tc = 5 } }'

DIS=0
printf '%-26s %-9s %-9s %s\n' CASE scala-rs scalac VERDICT
for f in $C/*.scala; do
  n=${${f:t}:r}
  rm -rf $P/o1 $P/o2; mkdir -p $P/o1 $P/o2
  # The `s` family is the only one that needs the separately compiled library.
  if [[ $n == s* ]]; then RSCP=(-cp $P/lib); SCCP=$JAR:$P/lib
  else RSCP=(); SCCP=$JAR
  fi
  if $BIN compile $f -d $P/o1 --scala-library $JAR $RSCP > $P/$n.rs.log 2>&1; then r=accept; else r=REJECT; fi
  if $SCALAC -classpath $SCCP -d $P/o2 $f > $P/$n.sc.log 2>&1; then s=accept; else s=REJECT; fi
  if [[ $r == $s ]]; then v=agree; else v="*** DISAGREE ***"; (( DIS++ )); fi
  printf '%-26s %-9s %-9s %s\n' $n $r $s "$v"
done
print "\ncases=$(ls $C | wc -l | tr -d ' ') disagreements=$DIS logs=$P"
# `b24_setter_only` is a recorded, deliberate disagreement; anything else is
# a regression or a new find.
[[ $DIS -le 1 ]] || exit 1
