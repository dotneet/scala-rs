// Same name, different parameter type: an *overload*, never an override.
//
// SLS 5.1.4 makes parameter types invariant under overriding, so each pair
// below is two independent methods. Real scalac 2.13.16 compiles this file and
// prints `tests/fixtures/expected/libanyval_overload.txt`; scala-rs rejected
// every one of them, because its override matcher answered "same type" for any
// pair of types mentioning a type parameter. That is the shape scala/scala's
// own `src/library` is written in -- `Map.concat` against `IterableOps.concat`,
// `Buffer.prepend` against its varargs sibling, `<:<.compose` against
// `Function1.compose`, `ArrayBuilder.addAll` against `Growable.addAll`,
// `Spliterator.OfInt.tryAdvance` against `Consumer`'s -- 34 errors in all.
//
// The result types deliberately erase differently from the ones they overload.
// That is not decoration: it is what keeps `MapOps.concat` legal in the real
// library (`CC[K, V2]` erases to `Object`, `IterableOps.concat`'s `CC[B]` to
// `Iterable`), and without it scalac reports a *name clash after erasure*
// instead -- a different rule, and one this fixture is not about.

trait Bag[+A] { def tag: String }
trait Arr[A] { def tag: String }

trait Ops[A] {
  // `final`, so a spurious match reports "cannot override final member".
  final def pp[B >: A](xs: Bag[B]): Bag[B] = xs
  // concrete and not final, so a spurious match reports
  // "`override` modifier required to override concrete member".
  def cat[B >: A](xs: Bag[B]): Bag[B] = xs
  def addAll(xs: Bag[A]): Bag[A] = xs
}

/** Same head class `Bag`, argument `V2` against `(K, V2)`. A bound type
  * variable is not a tuple containing itself, whatever `V2` is instantiated
  * to, so neither `pp` nor `cat` overrides anything. */
class Table[K, V] extends Ops[(K, V)] {
  def pp[V2 >: V](xs: Bag[(K, V2)]): String = "Table.pp"
  def cat[V2 >: V](xs: Bag[(K, V2)]): String = "Table.cat"
  // `Arr[(K, V)]` and `Bag[(K, V)]` are unrelated classes.
  def addAll(xs: Arr[(K, V)]): String = "Table.addAll"
}

/** A repeated parameter is `scala.<repeated>[A]`, a different constructor from
  * `A` itself -- so the abstract `prep(x: A)` is not the final `prep(xs: A*)`. */
trait Buf[A] {
  def prep(x: A): String
  final def prep(xs: A*): String = "Buf.prep*"
}
class SBuf extends Buf[String] {
  def prep(x: String): String = "SBuf.prep"
}

/** `Partial[B, C]` is a *subtype* of `B => C`, not the same type, so this
  * `andThen` overloads `Function1.andThen[A](g: R => A)` rather than
  * overriding it. `scala.<:<` is written exactly this way. */
trait Partial[A, B] extends Function1[A, B] {
  def andThen[C](k: Partial[B, C]): String = "Partial.andThen"
}
class Doubler extends Partial[Int, Int] {
  def apply(x: Int): Int = x * 2
}

/** Two unrelated classes in the same argument position: `Sink` is not
  * `Int => Unit`. */
trait Sink { def accept(x: Int): Unit }
trait Gen[T] {
  def tryAdv(c: T => Unit): String = "Gen.tryAdv"
}
class IntGen extends Gen[Int] {
  def tryAdv(c: Sink): String = "IntGen.tryAdv"
}

/** `Any` has no parents at all -- nsc's `AnyClass` is
  * `enterNewClass(ScalaPackageClass, tpnme.Any, Nil, ABSTRACT)` -- and
  * `AnyVal extends Any`, so `java.lang.Object` is not a base class of a value
  * class and its `final` members are not in scope to be overridden. scalac
  * 2.13.16 compiles and runs the two definitions below; scala-rs called them
  * "cannot override final member", which is also why
  * `src/library/scala/AnyVal.scala` could not write its own bare
  * `def getClass(): Class[_ <: AnyVal] = null`.
  *
  * Note the exemption is `isSubClass(AnyValClass)`, not "extends Any": a
  * *universal trait* writing `def notify()` is still rejected, by nsc's
  * separate "trait cannot redefine final method from class AnyRef". */
class Meters(val n: Int) extends AnyVal {
  def notify(): String = "Meters.notify"
  def wait(): String = "Meters.wait"
}

object Main {
  def main(args: Array[String]): Unit = {
    val bag = new Bag[(Int, String)] { def tag = "bag" }
    val arr = new Arr[(Int, String)] { def tag = "arr" }
    val t = new Table[Int, String]
    println(t.pp(bag))
    println((t: Ops[(Int, String)]).pp(bag).tag)
    println(t.cat(bag))
    println((t: Ops[(Int, String)]).cat(bag).tag)
    println(t.addAll(arr))
    // Upcast: scala-rs drops an inherited *generic* member from the overload
    // set as soon as the subclass writes the same name, so `t.addAll(bag)` is
    // rejected. That is a member-supply defect this slice does not touch --
    // it predates it and reproduces with a pair the old matcher already
    // treated as an overload -- so the call is written at `Ops`.
    println((t: Ops[(Int, String)]).addAll(bag).tag)

    val b = new SBuf
    println(b.prep("x"))
    println((b: Buf[String]).prep("x", "y")) // upcast: same member-supply gap

    val d = new Doubler
    // The overload is what this file is about; `Function1.andThen` itself is
    // not called, because the private runtime does not supply it.
    println(d.andThen(new Partial[Int, Int] { def apply(x: Int): Int = x }))
    println(d.apply(4))

    val g = new IntGen
    println(g.tryAdv(new Sink { def accept(x: Int): Unit = () }))
    println((g: Gen[Int]).tryAdv((x: Int) => ()))

    val m = new Meters(3)
    println(m.notify())
    println(m.wait())
  }
}
