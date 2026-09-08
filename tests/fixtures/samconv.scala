// SAM conversion onto types that come out of the scala-library jar rather
// than out of this run's own sources: `scala.math.Equiv`, `scala.math.
// Ordering`, `scala.util.hashing.Hashing`. These are the three sites
// typelevel/cats' kernel writes (`Eq.scala:66`, `Order.scala:118`,
// `Hash.scala:81`), all three in the underscore-placeholder spelling.
//
// On the pre-fix binary every function literal below whose expected type is a
// library type is `type mismatch; found: (A, A) => Boolean  required:
// Equiv[A]`. Literals at a *source-declared* SAM trait already worked, which
// is why cases 7-9 pass before and after: the defect was never "SAM
// conversion is missing", it was that a library class reads as having zero
// abstract methods.

import scala.math.{Equiv, Ordering}
import scala.util.hashing.Hashing

trait Eq0[A] {
  def eqv(x: A, y: A): Boolean
  // A concrete member beside the abstract one. This compiler puts a trait's
  // method bodies in a static rather than in a JVM default method, so the
  // anonymous class a SAM literal lowers to has to carry a forwarder for it;
  // without one `neqv` is an `AbstractMethodError` at run time and nothing
  // earlier notices.
  def neqv(x: A, y: A): Boolean = !eqv(x, y)
}

trait Hash0[A] extends Eq0[A] {
  def hash(x: A): Int
}

object Conv {
  // cats `EqToEquivConversion`.
  def toEquiv[A](ev: Eq0[A]): Equiv[A] = ev.eqv(_, _)
  // cats `Order#toOrdering`.
  def toOrdering[A](cmp: (A, A) => Int): Ordering[A] = cmp(_, _)
  // cats `HashToHashingConversion`.
  def toHashing[A](f: A => Int): Hashing[A] = f(_)
}

object Main {
  def main(args: Array[String]): Unit = {
    // 1. The placeholder spelling, at a library type, through a type
    //    parameter.
    val eqInt: Equiv[Int] = Conv.toEquiv(new Eq0[Int] {
      def eqv(x: Int, y: Int): Boolean = x == y
    })
    println(eqInt.equiv(2, 2))
    println(eqInt.equiv(2, 3))

    // 2. The explicit-parameter spelling at the same type.
    val eqStr: Equiv[String] = (x: String, y: String) => x.length == y.length
    println(eqStr.equiv("ab", "cd"))
    println(eqStr.equiv("ab", "c"))

    // 3. Parameters inferred from the SAM's own signature.
    val eqLong: Equiv[Long] = (x, y) => x == y
    println(eqLong.equiv(4L, 4L))

    // 4. `Ordering`, whose sole abstract method is reached only after the
    //    concrete `equiv` it inherits from `Equiv` is known to be an
    //    override. Used where the library asks for one.
    val byLast: Ordering[String] = Conv.toOrdering((a: String, b: String) =>
      a.charAt(a.length - 1).compareTo(b.charAt(b.length - 1))
    )
    println(List("xc", "ya", "zb").sorted(byLast).mkString(","))
    println(byLast.compare("aa", "ab"))
    // The inherited concrete members still work on the converted value.
    println(byLast.equiv("qa", "ra"))
    println(byLast.lt("qa", "rb"))

    // 5. `Hashing`, straight out of the jar with no prelude entry at all.
    val h: Hashing[String] = Conv.toHashing((s: String) => s.length)
    println(h.hash("abcd"))
    val h2: Hashing[Int] = _ + 1
    println(h2.hash(41))

    // 6. Java SAM types, which already worked: still do.
    val r: Runnable = () => println("ran")
    r.run()
    val c: java.util.Comparator[Int] = (a, b) => a - b
    println(c.compare(7, 3))

    // 7. A source-declared SAM trait, and its inherited concrete method.
    val e0: Eq0[Int] = (x, y) => x == y
    println(e0.eqv(5, 5))
    println(e0.neqv(5, 6))

    // 8. A source-declared SAM trait whose abstract method is inherited is
    //    still not a SAM: `Hash0` declares `hash` and inherits `eqv`, so it
    //    has two.
    val hh = new Hash0[Int] {
      def eqv(x: Int, y: Int): Boolean = x == y
      def hash(x: Int): Int = x * 2
    }
    println(hh.hash(3))
    println(hh.neqv(1, 1))

    // 9. A `FunctionN` expected type is never a SAM conversion.
    val f: (Int, Int) => Int = (a, b) => a * b
    println(f(6, 7))
  }
}
