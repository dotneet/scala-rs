// A value class scrutinee is held *unboxed* -- `w: Wrapped` is an `int` in
// its slot -- but `gen_ctor_fields_pattern` lowered `case Wrapped(x)` to
// `instanceof` / `checkcast` / `getfield` against it as though it were a real
// boxed instance:
//
//   VerifyError: Bad local variable type
//     Type integer (current frame, locals[3]) is not assignable to reference
//
// A box is always a reference, so a scrutinee of *primitive* sort is provably
// the underlying value: the test is vacuous (a value class is final and the
// static type already names it) and the pattern is a plain binding, which is
// what nsc emits for the same source (`iload; istore`, no `instanceof`).
//
// The same file covers `Wrapped.unapply(w)` named explicitly. nsc's is
// `Wrapped$.unapply(int): Option` answering `Some(u)` with the *underlying*
// value boxed, never a `Wrapped`; our call sites already erased the argument
// that way, so what was missing was only the method
// (`NoSuchMethodError: 'scala.Option Wrapped$.unapply(int)'`).
final case class Wrapped(u: Int) extends AnyVal
final case class Millis(ms: Long) extends AnyVal
case class Outer(w: Wrapped, n: Int)

object Main {
  // The scrutinee's static type is the value class: nothing to test.
  def direct(w: Wrapped): Int = w match { case Wrapped(x) => x + 1 }

  // Here it is not, so the box is real and the type test has to stay.
  def fromAny(a: Any): Int = a match {
    case Wrapped(x) => x
    case _          => -1
  }

  // A value class field of an ordinary case class erases to the underlying
  // type too, so the nested pattern is a binding as well.
  def nested(o: Outer): Int = o match { case Outer(Wrapped(x), n) => x + n }

  // A sub-pattern that tests rather than binds.
  def literal(w: Wrapped): String = w match {
    case Wrapped(7) => "seven"
    case Wrapped(_) => "other"
  }

  // `case q @ Wrapped(x)` binds both the value class and its underlying.
  def bound(w: Wrapped): Int = w match { case q @ Wrapped(x) => q.u + x }

  def ascribed(w: Wrapped): Boolean = w match { case _: Wrapped => true }

  def guarded(w: Wrapped): Int = w match {
    case Wrapped(x) if x > 3 => x
    case Wrapped(_)          => 0
  }

  // A two-slot underlying type takes the same path.
  def wide(m: Millis): Long = m match { case Millis(x) => x * 2 }

  // Boxed again: `Option`'s element is an `Object`, so this one really does
  // hold a `Wrapped` instance.
  def inOption(o: Option[Wrapped]): Int = o match {
    case Some(Wrapped(x)) => x
    case None             => 0
  }

  def main(args: Array[String]): Unit = {
    println(direct(Wrapped(7)))
    println(fromAny(Wrapped(3)))
    println(fromAny("z"))
    println(nested(Outer(Wrapped(4), 5)))
    println(literal(Wrapped(7)))
    println(literal(Wrapped(8)))
    println(bound(Wrapped(2)))
    println(ascribed(Wrapped(1)))
    println(guarded(Wrapped(5)))
    println(guarded(Wrapped(1)))
    println(wide(Millis(21L)))
    println(inOption(Some(Wrapped(9))))
    println(inOption(None))

    // The extractor named directly. `.get` rather than the `Option` itself:
    // the private runtime's `Some` has no case-class `toString` yet, which is
    // a separate gap.
    println(Wrapped.unapply(Wrapped(11)).get)
    println(Millis.unapply(Millis(12L)).get)
    println(Wrapped.unapply(Wrapped(13)).isEmpty)
  }
}
