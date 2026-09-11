// Implicit conversions and implicit classes: which one applies, conversions
// on receivers vs arguments, chained enrichments, and numeric widening
// winning over a user conversion.
import scala.language.implicitConversions
object Main {
  case class Meters(v: Double)
  case class Feet(v: Double)
  implicit def feetToMeters(f: Feet): Meters = Meters(f.v * 0.3048)
  def describe(m: Meters): String = f"${m.v}%.3f m"

  implicit class RichInt2(val i: Int) extends AnyVal { def squared: Int = i * i; def times(f: => Unit): Unit = (1 to i).foreach(_ => f) }
  implicit class Shout(s: String) { def shout: String = s.toUpperCase + "!"; def twice: String = s + s }
  implicit class ShoutMore(s: Shout) { def andMore: String = s.shout + "?" }

  class Wrapper(val n: Int) { def +(o: Wrapper) = new Wrapper(n + o.n); override def toString = s"W($n)" }
  implicit def intToWrapper(i: Int): Wrapper = new Wrapper(i)

  def takesLong(l: Long): String = "long " + l
  implicit def intToString(i: Int): String = "converted-" + i  // must not beat widening
  def takesString(s: String): String = "string " + s

  trait Show[A] { def show(a: A): String }
  implicit val showInt: Show[Int] = (a: Int) => "#" + a
  implicit def showList[A](implicit s: Show[A]): Show[List[A]] = (as: List[A]) => as.map(s.show).mkString("[", ";", "]")
  def show[A](a: A)(implicit s: Show[A]): String = s.show(a)

  def main(args: Array[String]): Unit = {
    println(describe(Feet(10))); println(describe(Meters(1)))
    println(5.squared); var c = 0; 3.times { c += 1 }; println(c)
    println("hey".shout + " " + "ab".twice)
    val w = new Wrapper(1) + 2
    println(w)
    println(takesLong(3)); println(takesString(4))
    println(show(5)); println(show(List(1, 2))); println(show(List(List(3), Nil)))
    val m: Meters = Feet(1)
    println(m.v)
    println(List(Feet(2), Feet(3)).map(f => describe(f)))
    println(1.to(3).map(_.squared))
    val sh: Shout = "implicit"; println(sh.andMore)
  }
}
