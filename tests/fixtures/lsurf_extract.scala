// The extractor protocol (SLS 8.1.8, nsc's name-based pattern matching):
// `unapply` results with `isEmpty` / `get` that are not `Option`, one
// sub-pattern binding the whole `get` value, product selectors for several,
// a `Unit` scrutinee, implicit clauses after the scrutinee, and an instance
// extractor reached through a trait.
class Opt[A](val a: A) {
  def isEmpty: Boolean = a == null
  def get: A = a
}
object One { def unapply(s: String): Opt[String] = new Opt(if (s.startsWith("x")) s else null) }

class Pair(val _1: Int, val _2: String)
object Two { def unapply(i: Int): Opt[Pair] = new Opt(new Pair(i, "n" + i)) }

object T1 { def unapply(i: Int): Option[Tuple1[String]] = Some(Tuple1("t" + i)) }
class P2[A, B](val _1: A, val _2: B) extends Product2[A, B] { def canEqual(o: Any) = true }
object PP { def unapply(a: Any): Option[Product2[Int, String]] = Some(new P2(1, "2")) }
object FromTuple { def unapply(a: Any): Option[Product2[Int, String]] = Some((7, "t")) }

// `isEmpty` / `get` inherited at the result's own type arguments, and
// overloads that take parameters passed over.
trait T[A, B >: Null] { def isEmpty: A = false.asInstanceOf[A]; def get: B = null }
class Casey1() extends T[Boolean, String]
object Casey1 { def unapply(a: Casey1) = a }
class Casey2(val a: Int) {
  def isEmpty: Boolean = a < 0
  def isEmpty(x: Int): Boolean = ???
  def get: Int = a
  def get(x: Int): String = ???
}
object Casey2 { def unapply(a: Casey2) = a }

final class NonNullChar(val get: Char) extends AnyVal { def isEmpty = get == 0.toChar }
object NN { def unapply(c: Char): NonNullChar = new NonNullChar(if (c == 'z') 0.toChar else c) }

object UnitX { def unapply(u: Unit): Option[Pair] = Some(new Pair(42, "!")) }

class Tag[X](val name: String)
object Tag { implicit val intTag: Tag[Int] = new Tag[Int]("int") }
object Named { def unapply[X](x: X)(implicit t: Tag[X]): Option[String] = Some(t.name + ":" + x) }
object Plus { def unapply(a: Int)(implicit b: Int): Option[Int] = Some(a + b) }
object Dflt {
  def unapply(s: String)(implicit p: Option[String] = None): Some[String] = Some(s + "/" + p)
}

object Main {
  val Date = """(\d{4})-(\d{2})-(\d{2})""".r

  def main(args: Array[String]): Unit = {
    "xyz" match { case One(s) => println("one " + s) }
    "abc" match { case One(s) => println("bad " + s); case _ => println("one miss") }
    3 match { case Two(n, s) => println("two " + n + " " + s) }
    4 match { case Two(p) => println("whole " + p._1 + p._2) }
    5 match { case T1(t) => println("t1 " + t) }
    "" match { case PP(p) => println("pp " + p._1) }
    "" match { case PP(x, y) => val xx: Int = x; val yy: String = y; println("pp2 " + xx + yy) }
    "" match { case FromTuple(x, y) => println("tuple as product " + x + y) }
    val c @ Casey1(got) = new Casey1()
    println("casey1 " + got + " " + (got == c.get))
    new Casey2(7) match { case Casey2(x) => println("casey2 " + (x + 1)) }
    new Casey2(-1) match { case Casey2(x) => println("bad"); case _ => println("casey2 miss") }
    'a' match { case NN(ch) => println("nn " + ch) }
    'z' match { case NN(ch) => println("bad"); case _ => println("nn miss") }
    val UnitX(y) = ()
    println("unit " + y._1 + y._2)
    3 match { case Named(s) => println(s) }
    implicit val k: Int = 10
    4 match { case Plus(n) => println("plus " + n) }
    println(List(1, 2).map { case Plus(n) => n })
    "x" match { case Dflt(s) => println(s) }
    "on 2024-03-15 ok" match {
      case Date.unanchored(yr, _, _) => println("unanchored " + yr)
      case _ => println("no")
    }
  }
}
